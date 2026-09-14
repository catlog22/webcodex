//! Runtime handlers for explicit Workflow Session tool calls.

use super::session_context::{
    session_authority_denied_result, session_lifecycle_denied_result, session_message_error_result,
    session_project_mismatch_result, unknown_session_result,
    workflow_session_authority_fingerprint, SessionProjectMismatch,
};
use super::todo;
use super::tool_inputs::SessionMode;
use super::{
    session_projects, sessions, RecoveryKind, SessionToolCall, ToolCall, ToolResult, ToolRuntime,
};
use crate::auth::AuthContext;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

impl ToolRuntime {
    pub(crate) async fn dispatch_session_tool(
        &self,
        call: ToolCall,
        auth: Option<&AuthContext>,
        transport: sessions::SessionTransport,
    ) -> ToolResult {
        match call {
            ToolCall::StartSession {
                project,
                title,
                mode,
                deny_write_tools,
                deny_shell_tools,
                execution_context,
            } => {
                self.start_session_tool(
                    project,
                    title,
                    mode,
                    deny_write_tools,
                    deny_shell_tools,
                    execution_context,
                    auth,
                )
                .await
            }
            ToolCall::SessionSummary { session_id, limit } => {
                self.session_summary_tool(session_id, limit, auth).await
            }
            ToolCall::UpdateSessionContext {
                project,
                session_id,
                execution_context,
            } => {
                self.update_session_context_tool(
                    project,
                    session_id,
                    execution_context,
                    auth,
                    transport,
                )
                .await
            }
            ToolCall::CloseSession { session_id } => {
                self.close_session_tool(session_id, auth).await
            }
            ToolCall::ValidationSummary {
                project,
                session_id,
                limit,
            } => {
                self.validation_summary_tool(project, session_id, limit, auth)
                    .await
            }
            ToolCall::PostSessionMessage {
                session_id,
                kind,
                message,
                tags,
                reply_to,
                priority,
                requires_ack,
            } => {
                self.post_session_message_tool(
                    session_id,
                    kind,
                    message,
                    tags,
                    reply_to,
                    priority,
                    requires_ack,
                    auth,
                )
                .await
            }
            ToolCall::ListSessionMessages {
                session_id,
                kind,
                status,
                message_id,
                reply_to,
                limit,
            } => {
                self.list_session_messages_tool(
                    session_id, kind, status, message_id, reply_to, limit, auth,
                )
                .await
            }
            ToolCall::GetSessionAssignment {
                session_id,
                message_id,
            } => {
                self.get_session_assignment_tool(session_id, message_id, auth)
                    .await
            }
            ToolCall::ObserveSessionMessages {
                session_id,
                after_observation_token,
                wait_secs,
                limit,
            } => {
                self.observe_session_messages_tool(
                    session_id,
                    after_observation_token,
                    wait_secs,
                    limit,
                    auth,
                )
                .await
            }
            ToolCall::ResolveSessionMessage {
                session_id,
                message_id,
                resolution,
            } => {
                self.resolve_session_message_tool(session_id, message_id, resolution, auth)
                    .await
            }
            ToolCall::CompleteSessionMessage {
                session_id,
                message_id,
                answer,
                completion_key,
                tags,
                expected_assignment_fence,
                priority,
                trusted_recording_session_id,
            } => {
                self.complete_session_message_tool(
                    session_id,
                    message_id,
                    answer,
                    completion_key,
                    expected_assignment_fence,
                    tags,
                    priority,
                    trusted_recording_session_id,
                    auth,
                )
                .await
            }
            ToolCall::SessionDiscussionSummary { session_id, limit } => {
                self.session_discussion_summary_tool(session_id, limit, auth)
                    .await
            }
            _ => unreachable!("non-session tool routed to session dispatcher"),
        }
    }

    pub(crate) async fn start_session_tool(
        &self,
        project: Option<String>,
        title: Option<String>,
        mode: SessionMode,
        deny_write_tools: bool,
        deny_shell_tools: bool,
        execution_context: Option<sessions::SessionExecutionContext>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        let execution_context = match execution_context
            .map(sessions::SessionExecutionContext::validated)
            .transpose()
        {
            Ok(context) => context,
            Err(error) => return invalid_execution_context_result(error),
        };
        let resolved = match project {
            Some(project_input) => match self
                .resolve_project_input_for_auth(&project_input, auth)
                .await
            {
                Ok(resolved) => Some(resolved),
                Err(err) => return err.into_tool_result(),
            },
            None => None,
        };
        // Best-effort load of project-local instruction files (AGENTS.md,
        // CLAUDE.md, ...). Any read failure is swallowed and never fails
        // start_session. `null` when no project was provided.
        let project_instructions = match &resolved {
            Some(resolved) => Some(self.load_project_instructions(&resolved.config).await),
            None => None,
        };
        if resolved.is_none() && auth.is_some_and(AuthContext::is_open_anonymous) {
            return ToolResult::err_with_output(
                "session_owner_identity_unavailable",
                json!({
                    "error_kind": "session_owner_identity_unavailable",
                    "tool_name": "start_session",
                    "state_changed": false,
                }),
            );
        }
        let owner_authority_fingerprint = match workflow_session_authority_fingerprint(auth) {
            Ok(fingerprint) => fingerprint,
            Err(_) => {
                return ToolResult::err_with_output(
                    "session_authority_identity_unavailable",
                    json!({
                        "error_kind": "session_authority_identity_unavailable",
                        "tool_name": "start_session",
                        "state_changed": false,
                    }),
                );
            }
        };
        let options = sessions::SessionCreateOptions::new(
            resolved
                .as_ref()
                .map(|resolved| resolved.resolved_id.clone()),
            title,
            mode,
            sessions::SessionGuards::effective(
                mode,
                sessions::SessionGuards {
                    deny_write_tools,
                    deny_shell_tools,
                },
            ),
        )
        .with_owner_authority_fingerprint(Some(owner_authority_fingerprint))
        .with_project_instructions(project_instructions.clone())
        .with_execution_context(execution_context.unwrap_or_default());
        let summary = match self.sessions.start_session_with_options(options) {
            Ok(summary) => summary,
            Err(error) => {
                return invalid_execution_context_result(error);
            }
        };
        ToolResult::ok(json!({
            "success": true,
            "session_id": summary.session_id,
            "project": summary.project,
            "project_input": resolved.as_ref().map(|resolved| resolved.input.clone()),
            "resolved_project": resolved.as_ref().map(|resolved| resolved.resolved_id.clone()),
            "title": summary.title,
            "mode": summary.mode,
            "guards": summary.guards,
            "execution_context": summary.execution_context,
            "lifecycle": summary.lifecycle,
            "created_at": summary.created_at,
            "project_instructions": project_instructions,
        }))
    }

    pub(crate) async fn update_session_context_tool(
        &self,
        project: String,
        session_id: String,
        execution_context: sessions::SessionExecutionContext,
        auth: Option<&AuthContext>,
        transport: sessions::SessionTransport,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "update_session_context", auth)
            .await
        {
            return result;
        }
        let resolved = match self.resolve_project_input_for_auth(&project, auth).await {
            Ok(resolved) => resolved,
            Err(err) => return err.into_tool_result(),
        };
        let Some(summary) = self.sessions.summary(&session_id, None) else {
            return unknown_session_result(&session_id);
        };
        if summary.project.as_deref() != Some(resolved.resolved_id.as_str()) {
            let mismatch = SessionProjectMismatch {
                session_project: summary.project.unwrap_or_else(|| "<unscoped>".to_string()),
                request_project: resolved.resolved_id,
            };
            return session_project_mismatch_result(
                &session_id,
                "update_session_context",
                &mismatch,
            );
        }
        if !summary.lifecycle.allows_mutation() {
            return session_lifecycle_denied_result(
                &session_id,
                "update_session_context",
                sessions::SessionLifecycleDenial {
                    lifecycle: summary.lifecycle,
                },
            );
        }
        match self
            .sessions
            .update_execution_context(&session_id, execution_context, transport)
        {
            Ok(outcome) => ToolResult::ok(json!({
                "success": true,
                "session_id": outcome.summary.session_id,
                "project": outcome.summary.project,
                "title": outcome.summary.title,
                "mode": outcome.summary.mode,
                "guards": outcome.summary.guards,
                "lifecycle": outcome.summary.lifecycle,
                "execution_context": outcome.summary.execution_context,
                "previous_execution_context": outcome.previous_execution_context,
                "changed": outcome.changed,
                "created_at": outcome.summary.created_at,
                "updated_at": outcome.summary.updated_at,
            })),
            Err(sessions::SessionExecutionContextUpdateError::UnknownSession) => {
                unknown_session_result(&session_id)
            }
            Err(sessions::SessionExecutionContextUpdateError::SessionNotActive { lifecycle }) => {
                session_lifecycle_denied_result(
                    &session_id,
                    "update_session_context",
                    sessions::SessionLifecycleDenial { lifecycle },
                )
            }
            Err(sessions::SessionExecutionContextUpdateError::SessionHasNoProject) => {
                ToolResult::err_with_output(
                    "session_execution_context_requires_project",
                    json!({
                        "error_kind": "session_execution_context_requires_project",
                        "failure_kind": "invalid_session_state",
                        "session_id": session_id,
                        "state_changed": false,
                    }),
                )
            }
            Err(sessions::SessionExecutionContextUpdateError::InvalidExecutionContext(error)) => {
                invalid_execution_context_result(error)
            }
        }
    }

    pub(crate) async fn session_summary_tool(
        &self,
        session_id: String,
        limit: Option<usize>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "session_summary", auth)
            .await
        {
            return result;
        }
        match self.sessions.summary(&session_id, limit) {
            Some(summary) => ToolResult::ok(
                serde_json::to_value(summary)
                    .unwrap_or_else(|_| json!({"session_id": session_id, "events": []})),
            ),
            None => unknown_session_result(&session_id),
        }
    }

    pub(crate) async fn close_session_tool(
        &self,
        session_id: String,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "close_session", auth)
            .await
        {
            return result;
        }
        match self.sessions.close_session(&session_id) {
            Ok(outcome) => {
                // Commit the lifecycle transition first so an in-flight open
                // cannot escape cleanup by reserving after the initial scan.
                let persistent_shells_closed =
                    self.close_persistent_shells_for_session(&session_id).await;
                ToolResult::ok(json!({
                    "success": true,
                    "session_id": outcome.summary.session_id,
                    "lifecycle": outcome.summary.lifecycle,
                    "already_closed": outcome.already_closed,
                    "persistent_shells_closed": persistent_shells_closed,
                    "updated_at": outcome.summary.updated_at,
                }))
            }
            Err(sessions::SessionCloseError::UnknownSession) => unknown_session_result(&session_id),
        }
    }

    pub(crate) async fn authorize_session_target(
        &self,
        session_id: &str,
        tool_name: &str,
        auth: Option<&AuthContext>,
    ) -> Result<Option<super::project_resolution::ResolvedProject>, ToolResult> {
        let Some((project, owner_authority_fingerprint)) =
            self.sessions.session_target_authority(session_id)
        else {
            return Err(unknown_session_result(session_id));
        };
        if let Some(project) = project {
            // Project authorization and immutable creation-time Session authority
            // are independent checks. Local/dev has no credential project check,
            // but still has the same canonical authority fence as every caller.
            let resolved = if let Some(auth) = auth {
                let resolved = match self
                    .resolve_project_input_for_auth(&project, Some(auth))
                    .await
                {
                    Ok(resolved) => resolved,
                    Err(err) => return Err(err.into_tool_result()),
                };
                if resolved.resolved_id != project {
                    return Err(session_project_mismatch_result(
                        session_id,
                        tool_name,
                        &SessionProjectMismatch {
                            session_project: project,
                            request_project: resolved.resolved_id,
                        },
                    ));
                }
                Some(resolved)
            } else {
                None
            };
            let caller_fingerprint = workflow_session_authority_fingerprint(auth)
                .map_err(|_| session_authority_denied_result(session_id, tool_name))?;
            #[cfg(test)]
            let synthetic_test_fixture = owner_authority_fingerprint
                == sessions::TEST_ONLY_PROJECT_SESSION_AUTHORITY_FINGERPRINT;
            #[cfg(not(test))]
            let synthetic_test_fixture = false;
            if !synthetic_test_fixture && owner_authority_fingerprint != caller_fingerprint {
                return Err(session_authority_denied_result(session_id, tool_name));
            }
            return Ok(resolved);
        }

        #[cfg(test)]
        if owner_authority_fingerprint == sessions::TEST_ONLY_PROJECT_SESSION_AUTHORITY_FINGERPRINT
        {
            return Ok(None);
        }

        if auth.is_some_and(AuthContext::is_open_anonymous) {
            return Err(session_authority_denied_result(session_id, tool_name));
        }
        let caller_fingerprint = workflow_session_authority_fingerprint(auth)
            .map_err(|_| session_authority_denied_result(session_id, tool_name))?;
        if owner_authority_fingerprint != caller_fingerprint {
            return Err(session_authority_denied_result(session_id, tool_name));
        }
        Ok(None)
    }

    fn trusted_collaboration_author_session(
        &self,
        trusted_recording_session_id: Option<String>,
    ) -> Option<String> {
        // The kernel injects this private field only after explicitly authorizing
        // the recording Session. No auth/window inference is permitted here.
        trusted_recording_session_id
            .filter(|recording_session_id| self.sessions.contains_session(recording_session_id))
    }

    fn completion_key_fingerprint(
        completion_key: String,
    ) -> Result<String, sessions::SessionMessageError> {
        let completion_key = completion_key.trim();
        if completion_key.is_empty()
            || completion_key.chars().count() > sessions::MAX_MESSAGE_COMPLETION_KEY_CHARS
        {
            return Err(sessions::SessionMessageError::InvalidInput(
                "completion_key must contain 1..=128 characters".to_string(),
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(b"webcodex.session-message-completion.v1\0");
        hasher.update(completion_key.as_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }

    pub(crate) async fn post_session_message_tool(
        &self,
        session_id: String,
        kind: sessions::SessionMessageKind,
        message: String,
        tags: Vec<String>,
        reply_to: Option<String>,
        priority: sessions::SessionMessagePriority,
        requires_ack: bool,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "post_session_message", auth)
            .await
        {
            return result;
        }
        match self.sessions.post_message_with_ack(
            sessions::PostSessionMessageInput {
                session_id: session_id.clone(),
                kind,
                message,
                tags,
                reply_to,
                priority,
            },
            requires_ack,
        ) {
            Ok(message) => ToolResult::ok(json!({
                "success": true,
                "session_id": session_id,
                "message_id": message.message_id,
                "message": message,
            })),
            Err(err) => session_message_error_result(&session_id, None, err),
        }
    }

    pub(crate) async fn list_session_messages_tool(
        &self,
        session_id: String,
        kind: Option<sessions::SessionMessageKind>,
        status: Option<sessions::SessionMessageStatus>,
        message_id: Option<String>,
        reply_to: Option<String>,
        limit: Option<usize>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "list_session_messages", auth)
            .await
        {
            return result;
        }
        match self.sessions.list_messages(
            &session_id,
            sessions::ListSessionMessagesFilter {
                kind,
                status,
                message_id,
                reply_to,
                limit,
            },
        ) {
            Ok(messages) => ToolResult::ok(json!({
                "success": true,
                "session_id": session_id,
                "messages": messages,
            })),
            Err(err) => session_message_error_result(&session_id, None, err),
        }
    }

    pub(crate) async fn get_session_assignment_tool(
        &self,
        session_id: String,
        message_id: String,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "get_session_assignment", auth)
            .await
        {
            return result;
        }
        match self.sessions.get_assignment(&session_id, &message_id) {
            Ok(snapshot) => ToolResult::ok(json!({
                "success": true,
                "session_id": session_id,
                "message_id": snapshot.todo.message_id,
                "todo": snapshot.todo,
                "direct_replies": snapshot.direct_replies,
                "assignment_fence": snapshot.assignment_fence,
            })),
            Err(err) => session_message_error_result(&session_id, Some(&message_id), err),
        }
    }

    pub(crate) async fn observe_session_messages_tool(
        &self,
        session_id: String,
        after_observation_token: Option<String>,
        wait_secs: Option<u64>,
        limit: Option<usize>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "observe_session_messages", auth)
            .await
        {
            return result;
        }
        if wait_secs.is_some() && after_observation_token.is_none() {
            return invalid_session_message_observation_request(
                &session_id,
                "wait_secs requires after_observation_token",
            );
        }
        if wait_secs == Some(0) {
            return invalid_session_message_observation_request(
                &session_id,
                "wait_secs must be at least 1",
            );
        }
        if limit == Some(0) {
            return invalid_session_message_observation_request(
                &session_id,
                "limit must be at least 1",
            );
        }
        let wait_secs = wait_secs.map(|wait_secs| wait_secs.min(60));
        let limit = limit.map(|limit| limit.min(sessions::MAX_MESSAGE_LIST_LIMIT));
        match self
            .sessions
            .observe_messages(
                &session_id,
                after_observation_token.as_deref(),
                wait_secs,
                limit,
            )
            .await
        {
            Ok(observation) => ToolResult::ok(json!({
                "success": true,
                "session_id": session_id,
                "messages": observation.messages,
                "observation_token": observation.observation_token,
                "changed": observation.changed,
                "wait_outcome": observation.wait_outcome,
                "waited_ms": observation.waited_ms,
                "history_lost": observation.history_lost,
                "has_more": observation.has_more,
            })),
            Err(err) => session_message_observation_error_result(&session_id, err),
        }
    }

    pub(crate) async fn resolve_session_message_tool(
        &self,
        session_id: String,
        message_id: String,
        resolution: Option<String>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "resolve_session_message", auth)
            .await
        {
            return result;
        }
        match self
            .sessions
            .resolve_message(&session_id, &message_id, resolution)
        {
            Ok(message) => ToolResult::ok(json!({
                "success": true,
                "session_id": session_id,
                "message_id": message.message_id,
                "message": message,
            })),
            Err(err) => session_message_error_result(&session_id, Some(&message_id), err),
        }
    }

    pub(crate) async fn complete_session_message_tool(
        &self,
        session_id: String,
        message_id: String,
        answer: String,
        completion_key: String,
        expected_assignment_fence: String,
        tags: Vec<String>,
        priority: sessions::SessionMessagePriority,
        trusted_recording_session_id: Option<String>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "complete_session_message", auth)
            .await
        {
            return result;
        }
        let completion_id = match Self::completion_key_fingerprint(completion_key) {
            Ok(completion_id) => completion_id,
            Err(err) => return session_message_error_result(&session_id, Some(&message_id), err),
        };
        let author_session_id =
            self.trusted_collaboration_author_session(trusted_recording_session_id);
        match self
            .sessions
            .complete_message(sessions::CompleteSessionMessageInput {
                session_id: session_id.clone(),
                message_id: message_id.clone(),
                answer,
                tags,
                priority,
                completion_id: completion_id.clone(),
                author_session_id,
                expected_assignment_fence,
            }) {
            Ok(outcome) => ToolResult::ok(json!({
                "success": true,
                "session_id": session_id,
                "message_id": outcome.todo.message_id,
                "answer_message_id": outcome.answer.message_id,
                "completion_id": completion_id,
                "replayed": outcome.replayed,
                "todo": outcome.todo,
                "answer": outcome.answer,
            })),
            Err(err) => session_message_error_result(&session_id, Some(&message_id), err),
        }
    }

    pub(crate) async fn session_discussion_summary_tool(
        &self,
        session_id: String,
        limit: Option<usize>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if let Err(result) = self
            .authorize_session_target(&session_id, "session_discussion_summary", auth)
            .await
        {
            return result;
        }
        match self.sessions.discussion_summary(&session_id, limit) {
            Ok(summary) => ToolResult::ok(json!({
                "success": true,
                "session_id": session_id,
                "counts": summary.counts,
                "open_guidance": summary.open_guidance,
                "open_questions": summary.open_questions,
                "open_risks": summary.open_risks,
                "open_todos": summary.open_todos,
                "high_priority_open_todos": summary.high_priority_open_todos,
                "recent_answers": summary.recent_answers,
                "recent_completions": summary.recent_completions,
                "recent_progress": summary.recent_progress,
                "recent_decisions": summary.recent_decisions,
            })),
            Err(err) => session_message_error_result(&session_id, None, err),
        }
    }
}

fn invalid_session_message_observation_request(session_id: &str, message: &str) -> ToolResult {
    ToolResult::err_with_output(
        message,
        json!({
            "error_kind": "invalid_session_message_observation_request",
            "failure_kind": "invalid_arguments",
            "session_id": session_id,
            "state_changed": false,
        }),
    )
    .with_recovery(RecoveryKind::FixInput, None)
}

fn session_message_observation_error_result(
    session_id: &str,
    error: sessions::SessionMessageObservationError,
) -> ToolResult {
    match error {
        sessions::SessionMessageObservationError::UnknownSession => {
            unknown_session_result(session_id)
        }
        sessions::SessionMessageObservationError::MalformedToken
        | sessions::SessionMessageObservationError::OversizedToken
        | sessions::SessionMessageObservationError::WrongSession
        | sessions::SessionMessageObservationError::FutureRevision => ToolResult::err_with_output(
            "invalid_session_message_observation_token",
            json!({
                "error_kind": "invalid_session_message_observation_token",
                "failure_kind": "invalid_arguments",
                "session_id": session_id,
                "state_changed": false,
            }),
        )
        .with_recovery(RecoveryKind::FixInput, None),
        sessions::SessionMessageObservationError::InvalidObservationState => {
            ToolResult::err_with_output(
                "invalid_message_observation_state",
                json!({
                    "error_kind": "invalid_message_observation_state",
                    "session_id": session_id,
                    "state_changed": false,
                }),
            )
            .with_recovery(RecoveryKind::NoAction, None)
        }
    }
}

fn invalid_execution_context_result(error: String) -> ToolResult {
    ToolResult::err_with_output(
        error,
        json!({
            "error_kind": "invalid_execution_context",
            "failure_kind": "invalid_arguments",
            "field": "execution_context",
            "state_changed": false,
        }),
    )
}

impl ToolRuntime {
    /// Dispatcher for the consolidated `session` tool surface.
    pub(crate) async fn consolidated_session_tool(
        &self,
        call: SessionToolCall,
        auth: Option<&AuthContext>,
        transport: sessions::SessionTransport,
    ) -> ToolResult {
        match call {
            SessionToolCall::Start {
                project,
                projects,
                title,
                mode,
                execution_context,
            } => {
                self.consolidated_session_start(
                    project,
                    projects,
                    title,
                    mode,
                    execution_context,
                    auth,
                )
                .await
            }
            SessionToolCall::Get { session_id } => self.consolidated_session_get(&session_id),
            SessionToolCall::List { project, limit } => {
                self.consolidated_session_list(project, limit, auth).await
            }
            SessionToolCall::AttachProject {
                session_id,
                project,
                expected_revision,
            } => {
                self.consolidated_session_attach(&session_id, &project, expected_revision, auth)
                    .await
            }
            SessionToolCall::DetachProject {
                session_id,
                project,
                expected_revision,
            } => {
                self.consolidated_session_detach(&session_id, &project, expected_revision, auth)
                    .await
            }
            SessionToolCall::UpdateContext {
                project,
                session_id,
                execution_context,
            } => {
                self.update_session_context_tool(
                    project,
                    session_id,
                    execution_context,
                    auth,
                    transport,
                )
                .await
            }
            SessionToolCall::Compact { session_id, note } => {
                self.consolidated_session_compact(&session_id, note, auth)
                    .await
            }
            #[cfg(feature = "workspace-checkpoints")]
            SessionToolCall::Restore {
                session_id,
                project,
                checkpoint_id,
                confirm,
            } => {
                self.consolidated_session_restore(
                    &session_id,
                    project.as_deref(),
                    &checkpoint_id,
                    confirm,
                    auth,
                )
                .await
            }
            #[cfg(not(feature = "workspace-checkpoints"))]
            SessionToolCall::Restore { session_id, .. } => ToolResult::err_with_output(
                "session restore requires the workspace-checkpoints feature",
                json!({
                    "error_kind": "workspace_checkpoints_unavailable",
                    "session_id": session_id,
                    "state_changed": false,
                }),
            ),
            SessionToolCall::Summary { session_id, limit } => {
                self.session_summary_tool(session_id, limit, auth).await
            }
            SessionToolCall::Close { session_id } => {
                self.close_session_tool(session_id, auth).await
            }
        }
    }

    async fn consolidated_session_start(
        &self,
        project: Option<String>,
        projects: Option<Vec<String>>,
        title: Option<String>,
        mode: Option<SessionMode>,
        execution_context: Option<sessions::SessionExecutionContext>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        if project.is_some() && projects.is_some() {
            return ToolResult::err_with_output(
                "invalid_arguments: project and projects are mutually exclusive",
                json!({
                    "error_kind": "invalid_arguments",
                    "failure_kind": "invalid_arguments",
                    "field": "projects",
                    "state_changed": false,
                }),
            );
        }
        let (canonical_input, attached_inputs): (Option<String>, Vec<String>) =
            match (&project, &projects) {
                (Some(input), None) => (Some(input.clone()), Vec::new()),
                (None, Some(inputs)) => {
                    let inputs = match validate_project_list(inputs) {
                        Ok(inputs) => inputs,
                        Err(message) => {
                            return ToolResult::err_with_output(
                                format!("invalid_arguments: {message}"),
                                json!({
                                    "error_kind": "invalid_arguments",
                                    "failure_kind": "invalid_arguments",
                                    "field": "projects",
                                    "state_changed": false,
                                }),
                            );
                        }
                    };
                    let mut inputs = inputs.into_iter();
                    let canonical = inputs.next().expect("validated non-empty");
                    (Some(canonical), inputs.collect::<Vec<_>>())
                }
                // Project-less Sessions stay legal (same semantics as the legacy
                // start_session tool); compact requires a canonical project.
                (None, None) => (None, Vec::new()),
                (Some(_), Some(_)) => unreachable!("mutually exclusive checked above"),
            };
        // Every workspace input is independently resolved and authorized before
        // the session is created; the canonical project is re-resolved inside
        // start_session_tool with identical semantics.
        let mut attached_resolved = Vec::with_capacity(attached_inputs.len());
        for input in &attached_inputs {
            match self.resolve_project_input_for_auth(input, auth).await {
                Ok(resolved) => attached_resolved.push(resolved.resolved_id),
                Err(err) => return err.into_tool_result(),
            }
        }
        let result = self
            .start_session_tool(
                canonical_input,
                title,
                mode.unwrap_or_default(),
                false,
                false,
                execution_context,
                auth,
            )
            .await;
        if !result.success {
            return result;
        }
        let session_id = result
            .output
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut output = result.output;
        output["action"] = json!("start");
        match self
            .session_projects
            .initialize(&session_id, attached_resolved)
        {
            Ok(record) => {
                output["projects"] = projects_json(&self.sessions, &record);
                output["attached_revision"] = json!(record.revision);
            }
            Err(error) => {
                output["attached_error"] = json!(error.message());
                output["projects"] = projects_json(
                    &self.sessions,
                    &session_projects::SessionProjectsRecord {
                        version: 1,
                        session_id: session_id.clone(),
                        revision: 0,
                        attached: Vec::new(),
                    },
                );
            }
        }
        ToolResult {
            success: true,
            output,
            ..result
        }
    }

    fn consolidated_session_get(&self, session_id: &str) -> ToolResult {
        let Some(summary) = self.sessions.summary(session_id, None) else {
            return unknown_session_result(session_id);
        };
        let record = self.session_projects.load(session_id).unwrap_or_else(|_| {
            session_projects::SessionProjectsRecord {
                version: 1,
                session_id: session_id.to_string(),
                revision: 0,
                attached: Vec::new(),
            }
        });
        ToolResult::ok(json!({
            "success": true,
            "action": "get",
            "session_id": summary.session_id,
            "project": summary.project,
            "title": summary.title,
            "mode": summary.mode,
            "guards": summary.guards,
            "lifecycle": summary.lifecycle,
            "execution_context": summary.execution_context,
            "created_at": summary.created_at,
            "updated_at": summary.updated_at,
            "projects": projects_json(&self.sessions, &record),
            "revision": record.revision,
        }))
    }

    async fn consolidated_session_list(
        &self,
        project: Option<String>,
        limit: Option<usize>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        let Some(project) = project else {
            return ToolResult::err_with_output(
                "invalid_arguments: list requires an explicit project filter",
                json!({
                    "error_kind": "invalid_arguments",
                    "failure_kind": "invalid_arguments",
                    "field": "project",
                    "state_changed": false,
                }),
            );
        };
        let resolved = match self.resolve_project_input_for_auth(&project, auth).await {
            Ok(resolved) => resolved,
            Err(err) => return err.into_tool_result(),
        };
        let list = self.workflow_sessions_console_list(&resolved.resolved_id, limit);
        ToolResult::ok(json!({
            "success": true,
            "action": "list",
            "project": resolved.resolved_id,
            "total": list.total,
            "returned": list.returned,
            "truncated": list.truncated,
            "sessions": list.sessions,
        }))
    }

    pub(crate) async fn consolidated_session_attach(
        &self,
        session_id: &str,
        project: &str,
        expected_revision: u64,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        let Some(canonical) = self.require_session_project(session_id) else {
            return unknown_session_result(session_id);
        };
        let resolved = match self.resolve_project_input_for_auth(project, auth).await {
            Ok(resolved) => resolved,
            Err(err) => return err.into_tool_result(),
        };
        if canonical.as_deref() == Some(resolved.resolved_id.as_str()) {
            return ToolResult::err_with_output(
                format!("invalid_project: {project} is the canonical session project"),
                json!({
                    "error_kind": "invalid_project",
                    "session_id": session_id,
                    "state_changed": false,
                }),
            );
        }
        match self.session_projects.attach(
            session_id,
            resolved.resolved_id.clone(),
            expected_revision,
        ) {
            Ok(record) => ToolResult::ok(json!({
                "success": true,
                "action": "attach_project",
                "session_id": session_id,
                "project": resolved.resolved_id,
                "projects": projects_json(&self.sessions, &record),
                "revision": record.revision,
            })),
            Err(error) => error.into_tool_result(),
        }
    }

    pub(crate) async fn consolidated_session_detach(
        &self,
        session_id: &str,
        project: &str,
        expected_revision: u64,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        let Some(canonical) = self.require_session_project(session_id) else {
            return unknown_session_result(session_id);
        };
        let resolved = match self.resolve_project_input_for_auth(project, auth).await {
            Ok(resolved) => resolved,
            Err(err) => return err.into_tool_result(),
        };
        if canonical.as_deref() == Some(resolved.resolved_id.as_str()) {
            return ToolResult::err_with_output(
                format!("invalid_project: cannot detach the canonical session project {project}"),
                json!({
                    "error_kind": "invalid_project",
                    "session_id": session_id,
                    "state_changed": false,
                }),
            );
        }
        match self
            .session_projects
            .detach(session_id, &resolved.resolved_id, expected_revision)
        {
            Ok(record) => ToolResult::ok(json!({
                "success": true,
                "action": "detach_project",
                "session_id": session_id,
                "project": resolved.resolved_id,
                "projects": projects_json(&self.sessions, &record),
                "revision": record.revision,
            })),
            Err(error) => error.into_tool_result(),
        }
    }

    fn require_session_project(&self, session_id: &str) -> Option<Option<String>> {
        if !self.sessions.contains_session(session_id) {
            return None;
        }
        self.sessions.session_project(session_id)
    }
}

fn validate_project_list(inputs: &[String]) -> Result<Vec<String>, String> {
    if inputs.is_empty() {
        return Err("projects must not be empty".to_string());
    }
    if inputs.len() > 8 {
        return Err("projects accepts at most 8 entries".to_string());
    }
    let mut seen = std::collections::BTreeSet::new();
    for input in inputs {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err("projects entries must be non-empty".to_string());
        }
        if !seen.insert(trimmed.to_string()) {
            return Err(format!("duplicate projects entry '{trimmed}'"));
        }
    }
    Ok(inputs
        .iter()
        .map(|input| input.trim().to_string())
        .collect())
}

fn projects_json(
    sessions: &sessions::SessionStore,
    record: &session_projects::SessionProjectsRecord,
) -> Value {
    let canonical = sessions.session_project(&record.session_id).unwrap_or(None);
    let mut projects: Vec<Value> = Vec::with_capacity(record.attached.len() + 1);
    if let Some(canonical) = canonical.as_deref() {
        projects.push(json!({"project": canonical, "canonical": true}));
    }
    for attached in &record.attached {
        if Some(attached.project.as_str()) == canonical.as_deref() {
            continue;
        }
        projects.push(json!({"project": attached.project, "canonical": false}));
    }
    json!(projects)
}

impl ToolRuntime {
    /// `session compact`: deterministically distill session state into one
    /// Memory (knowhow) entry tagged as compaction output.
    pub(crate) async fn consolidated_session_compact(
        &self,
        session_id: &str,
        note: Option<String>,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        let Some(summary) = self.sessions.summary(session_id, None) else {
            return unknown_session_result(session_id);
        };
        let Some(canonical) = summary.project.clone() else {
            return ToolResult::err_with_output(
                "session_execution_context_requires_project",
                json!({
                    "error_kind": "session_has_no_project",
                    "session_id": session_id,
                    "state_changed": false,
                    "message": "compact deposits its output into project Memory; the session needs a canonical project",
                }),
            );
        };
        let todos = self
            .todo_store
            .load(session_id)
            .unwrap_or_else(|_| todo::TodoSessionRecord {
                version: 1,
                session_id: session_id.to_string(),
                revision: 0,
                todos: Vec::new(),
            });
        let open_todos: Vec<Value> = todos
            .todos
            .iter()
            .filter(|todo| {
                !matches!(
                    todo.status,
                    todo::TodoStatus::Resolved | todo::TodoStatus::Cancelled
                )
            })
            .map(|todo| {
                json!({
                    "todo_id": todo.todo_id,
                    "subject": todo.subject,
                    "status": todo.status.as_str(),
                    "assignee": todo.assignee,
                })
            })
            .collect();
        let note = note.unwrap_or_default();
        if webcodex_core::sensitive_text::secret_like_value(&note) {
            return ToolResult::err_with_output(
                "compact note looks like a secret; refusing to persist it",
                json!({
                    "error_kind": "secret_like_input_rejected",
                    "session_id": session_id,
                    "state_changed": false,
                }),
            );
        }
        let body = json!({
            "session_id": session_id,
            "compacted_at_unix_ms": now_unix_millis(),
            "lifecycle": summary.lifecycle,
            "execution_context": summary.execution_context,
            "counts": summary.counts,
            "open_todos": open_todos,
            "note": note,
        });
        let body_string = serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string());
        let summary_line = format!(
            "Compaction of session {session_id}: {} open todo(s), lifecycle {:?}",
            open_todos.len(),
            summary.lifecycle
        );
        let resolved = match self.resolve_project_input_for_auth(&canonical, auth).await {
            Ok(resolved) => resolved,
            Err(err) => return err.into_tool_result(),
        };
        let memory_key = format!("session_compact.{session_id}");
        let result = self.memory_set(
            &resolved,
            memory_key.clone(),
            summary_line,
            Some(body_string),
            None,
            Some(false),
            Some(vec!["compaction".to_string(), "session".to_string()]),
            None,
            auth,
        );
        if !result.success {
            return result;
        }
        let mut output = result.output;
        output["action"] = json!("compact");
        output["session_id"] = json!(session_id);
        output["compact"] = body;
        ToolResult {
            success: true,
            output,
            ..result
        }
    }

    /// `session restore`: re-apply one exact checkpoint to a session workspace.
    /// The checkpoint may live under the canonical project or any attached one.
    #[cfg(feature = "workspace-checkpoints")]
    pub(crate) async fn consolidated_session_restore(
        &self,
        session_id: &str,
        project: Option<&str>,
        checkpoint_id: &str,
        confirm: bool,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        let Some(summary) = self.sessions.summary(session_id, None) else {
            return unknown_session_result(session_id);
        };
        let mut candidates: Vec<String> = Vec::new();
        if let Some(project) = project {
            match self.resolve_project_input_for_auth(project, auth).await {
                Ok(resolved) => candidates.push(resolved.resolved_id),
                Err(err) => return err.into_tool_result(),
            }
        } else {
            if let Some(canonical) = summary.project.clone() {
                candidates.push(canonical);
            }
            if let Ok(record) = self.session_projects.load(session_id) {
                for attached in record.attached {
                    if !candidates.contains(&attached.project) {
                        candidates.push(attached.project);
                    }
                }
            }
        }
        for candidate in &candidates {
            if self.checkpoint_store.load(candidate, checkpoint_id).is_ok() {
                let result = self
                    .workspace_checkpoint_restore(
                        candidate.clone(),
                        checkpoint_id.to_string(),
                        confirm,
                    )
                    .await;
                if !result.success {
                    return result;
                }
                let mut output = result.output;
                output["action"] = json!("restore");
                output["session_id"] = json!(session_id);
                return ToolResult {
                    success: true,
                    output,
                    ..result
                };
            }
        }
        ToolResult::err_with_output(
            format!("unknown_checkpoint_id: {checkpoint_id}"),
            json!({
                "error_kind": "unknown_checkpoint_id",
                "session_id": session_id,
                "checkpoint_id": checkpoint_id,
                "state_changed": false,
            }),
        )
        .with_recovery(RecoveryKind::FixInput, None)
    }
}

fn now_unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
