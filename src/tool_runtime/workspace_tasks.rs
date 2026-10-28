//! CLI、Desktop 与现有资源工具共享的工作区待办服务。
use super::kernel::check_runtime_tool_scope;
use super::permissions::{add_permission_to_result, permission_execution_denied_result};
use super::{ToolResult, ToolRuntime};
use crate::auth::{AuthContext, AuthKind};
use serde_json::json;
use webcodex_core::workspace_task::{
    WorkspaceTaskOperation, WorkspaceTaskRequest, WorkspaceTaskUpdate,
};

fn error(kind: &str, message: &str) -> ToolResult {
    let mut output = json!({"error_kind":kind});
    // 提交阶段的存储错误不能证明未写入，调用方需读取后再决定是否重试。
    if kind != "workspace_task_storage_unavailable" {
        output["state_changed"] = json!(false);
    }
    ToolResult::err_with_output(message, output)
}

impl ToolRuntime {
    pub(crate) async fn workspace_task_request(
        &self,
        request: WorkspaceTaskRequest,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        self.workspace_task_request_inner(request, auth, true).await
    }

    pub(super) async fn workspace_task_request_after_governance(
        &self,
        request: WorkspaceTaskRequest,
        auth: Option<&AuthContext>,
    ) -> ToolResult {
        self.workspace_task_request_inner(request, auth, false)
            .await
    }

    async fn workspace_task_request_inner(
        &self,
        request: WorkspaceTaskRequest,
        auth: Option<&AuthContext>,
        evaluate_permissions: bool,
    ) -> ToolResult {
        if check_runtime_tool_scope(auth, "list_projects").is_err() {
            return error("insufficient_scope", "Project read access is required");
        }
        let resolved = match self
            .resolve_project_input_for_auth(&request.project, auth)
            .await
        {
            Ok(project) => project,
            Err(cause) => return cause.into_tool_result(),
        };
        let Some(root) = resolved.root_fingerprint.as_deref() else {
            return error(
                "workspace_task_identity_unavailable",
                "A verified workspace root identity is required",
            );
        };
        let expected_root = match &request.operation {
            WorkspaceTaskOperation::Create {
                expected_root_fingerprint,
                ..
            }
            | WorkspaceTaskOperation::Update {
                expected_root_fingerprint,
                ..
            }
            | WorkspaceTaskOperation::Complete {
                expected_root_fingerprint,
                ..
            } => Some(expected_root_fingerprint),
            _ => None,
        };
        let permission = if let Some(expected) = expected_root {
            if expected != root {
                return error(
                    "workspace_task_target_changed",
                    "The workspace identity changed; refresh before editing",
                );
            }
            if auth.is_some_and(|a| a.kind == AuthKind::OpenAnonymous)
                || check_runtime_tool_scope(auth, "edit_project_files").is_err()
            {
                return error("insufficient_scope", "Project write access is required");
            }
            if evaluate_permissions {
                let decision = self.permission_evaluator.evaluate_resolved_required(
                    "workspace_task_write",
                    Some(&resolved.resolved_id),
                    "write",
                );
                if matches!(decision.status.as_str(), "denied" | "pending") {
                    let mut result = permission_execution_denied_result(&decision);
                    add_permission_to_result(&mut result, &decision);
                    result.output["state_changed"] = json!(false);
                    return result;
                }
                Some(decision)
            } else {
                None
            }
        } else {
            None
        };
        let Some(db) = self.communication_db.as_ref() else {
            return error(
                "workspace_task_storage_unavailable",
                "Workspace task storage is unavailable",
            );
        };
        let project = &resolved.resolved_id;
        let output = match request.operation {
            WorkspaceTaskOperation::List { query } => db.list_workspace_tasks(project, root, query).map(|page| json!({"project":project,"root_fingerprint":root,"tasks":page.tasks,"total":page.total,"next_offset":page.next_offset})),
            WorkspaceTaskOperation::Read { task_id } => db.read_workspace_task(project, root, &task_id).map(|task| json!({"task":task,"root_fingerprint":root})),
            WorkspaceTaskOperation::Create { title, description, idempotency_key, .. } => db.create_workspace_task(project, root, &title, &description, &idempotency_key).map(|task| json!({"task":task,"root_fingerprint":root})),
            WorkspaceTaskOperation::Update { task_id, expected_revision, title, description, completion_summary, status, .. } => db.update_workspace_task(project, root, WorkspaceTaskUpdate { task_id, expected_revision, title, description, completion_summary, status }).map(|task| json!({"task":task,"root_fingerprint":root})),
            WorkspaceTaskOperation::Complete { task_id, expected_revision, completion_summary, .. } => db.complete_workspace_task(project, root, &task_id, expected_revision, completion_summary).map(|task| json!({"task":task,"root_fingerprint":root})),
        };
        let mut result = match output {
            Ok(output) => ToolResult::ok(output),
            Err(cause) => error(cause.code, cause.message),
        };
        if let Some(permission) = permission {
            add_permission_to_result(&mut result, &permission);
        }
        result
    }
}
