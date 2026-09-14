//! Session-scoped todo records for the consolidated `todo` tool.
//!
//! Todos are first-class Workflow Session state with a small revision-CAS
//! state machine. They are persisted as one bounded JSON document per session
//! under the runtime state dir, alongside (not inside) the Session ledger.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

use super::{TodoToolCall, ToolResult, ToolRuntime};

const TODO_VERSION: u32 = 1;
const TODO_ID_PREFIX: &str = "wc_todo_";
const MAX_TODOS_PER_SESSION: usize = 256;
const MAX_TODO_SUBJECT_CHARS: usize = 4096;
const MAX_TODO_DESCRIPTION_CHARS: usize = 65536;
const MAX_TODO_NOTE_CHARS: usize = 4096;
const MAX_TODO_DEPENDENCIES: usize = 64;
const MAX_ASSIGNEE_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TodoStatus {
    Open,
    InProgress,
    Blocked,
    Resolved,
    Cancelled,
}

impl TodoStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Resolved => "resolved",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TodoRecord {
    pub(crate) todo_id: String,
    pub(crate) session_id: String,
    pub(crate) subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    pub(crate) status: TodoStatus,
    pub(crate) revision: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_at: Option<i64>,
}

impl TodoRecord {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "todo_id": self.todo_id,
            "session_id": self.session_id,
            "subject": self.subject,
            "description": self.description,
            "status": self.status.as_str(),
            "revision": self.revision,
            "depends_on": self.depends_on,
            "assignee": self.assignee,
            "note": self.note,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "resolved_at": self.resolved_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TodoSessionRecord {
    pub(crate) version: u32,
    pub(crate) session_id: String,
    pub(crate) revision: u64,
    pub(crate) todos: Vec<TodoRecord>,
}

impl TodoSessionRecord {
    fn new(session_id: String) -> Self {
        Self {
            version: TODO_VERSION,
            session_id,
            revision: 0,
            todos: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TodoStore {
    state_dir: PathBuf,
}

impl Default for TodoStore {
    fn default() -> Self {
        Self::new(crate::config::runtime_state_dir())
    }
}

impl TodoStore {
    pub(crate) fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    fn record_path(&self, session_id: &str) -> Result<PathBuf, String> {
        validate_session_id(session_id)?;
        Ok(self
            .state_dir
            .join("todos")
            .join(format!("{session_id}.json")))
    }

    pub(crate) fn load(&self, session_id: &str) -> Result<TodoSessionRecord, String> {
        let path = self.record_path(session_id)?;
        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut record: TodoSessionRecord = serde_json::from_str(&content)
                    .map_err(|err| format!("invalid todo record JSON: {err}"))?;
                record.session_id = session_id.to_string();
                Ok(record)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(TodoSessionRecord::new(session_id.to_string()))
            }
            Err(err) => Err(format!("failed to read todo record: {err}")),
        }
    }

    fn save(&self, record: &TodoSessionRecord) -> Result<(), String> {
        let path = self.record_path(&record.session_id)?;
        let parent = path
            .parent()
            .ok_or_else(|| "todo path has no parent".to_string())?;
        fs::create_dir_all(parent).map_err(|err| format!("failed to create todo dir: {err}"))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("todos.json");
        let tmp = path.with_file_name(format!(
            ".{file_name}.tmp-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let data = serde_json::to_vec_pretty(record)
            .map_err(|err| format!("failed to serialize todo record: {err}"))?;
        fs::write(&tmp, data)
            .and_then(|_| fs::rename(&tmp, &path))
            .map_err(|err| {
                let _ = fs::remove_file(&tmp);
                format!("failed to write todo record: {err}")
            })
    }
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.starts_with("wc_sess_") && session_id.len() > 8 {
        Ok(())
    } else {
        Err(format!("invalid session id '{session_id}'"))
    }
}

fn validate_todo_id(todo_id: &str) -> Result<(), String> {
    if todo_id.starts_with(TODO_ID_PREFIX) && todo_id.len() > TODO_ID_PREFIX.len() {
        Ok(())
    } else {
        Err(format!("invalid todo id '{todo_id}'"))
    }
}

fn now_unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn invalid_arguments(message: impl Into<String>) -> ToolResult {
    ToolResult::err_with_output(
        format!("invalid_arguments: {}", message.into()),
        json!({
            "error_kind": "invalid_arguments",
            "failure_kind": "invalid_arguments",
            "state_changed": false,
        }),
    )
}

fn revision_conflict(expected: u64, actual: u64) -> ToolResult {
    ToolResult::err_with_output(
        format!("todo_revision_conflict: expected {expected}, actual {actual}"),
        json!({
            "error_kind": "todo_revision_conflict",
            "expected_revision": expected,
            "actual_revision": actual,
            "state_changed": false,
        }),
    )
}

fn state_error(message: impl Into<String>) -> ToolResult {
    ToolResult::err_with_output(
        format!("invalid_todo_state: {}", message.into()),
        json!({
            "error_kind": "invalid_todo_state",
            "state_changed": false,
        }),
    )
}

fn io_error(context: &str, message: String) -> ToolResult {
    ToolResult::err_with_output(
        format!("todo_store_{context}"),
        json!({
            "error_kind": "todo_store_error",
            "state_changed": false,
            "message": message,
        }),
    )
}

impl ToolRuntime {
    /// Dispatcher for the consolidated `todo` tool surface.
    pub(crate) async fn consolidated_todo_tool(
        &self,
        call: TodoToolCall,
        auth: Option<&crate::auth::AuthContext>,
    ) -> ToolResult {
        let _ = auth; // session-scope authorization is enforced by the kernel.
        let session_id = call.session_id().to_string();
        if !self.sessions.contains_session(&session_id) {
            return super::session_context::unknown_session_result(&session_id);
        }
        let mut record = match self.todo_store.load(&session_id) {
            Ok(record) => record,
            Err(message) => return io_error("load", message),
        };
        let outcome = match call {
            TodoToolCall::Create {
                session_id,
                subject,
                description,
                depends_on,
            } => todo_create(&mut record, &session_id, subject, description, depends_on),
            TodoToolCall::List { status, .. } => todo_list(&record, status),
            TodoToolCall::Get { todo_id, .. } => todo_get(&record, &todo_id),
            TodoToolCall::Claim {
                todo_id,
                assignee,
                expected_revision,
                ..
            } => todo_claim(&mut record, &todo_id, assignee, expected_revision),
            TodoToolCall::Release {
                todo_id,
                expected_revision,
                ..
            } => todo_release(&mut record, &todo_id, expected_revision),
            TodoToolCall::Advance {
                todo_id,
                status,
                note,
                expected_revision,
                ..
            } => todo_advance(&mut record, &todo_id, status, note, expected_revision),
            TodoToolCall::Complete {
                todo_id,
                resolution,
                expected_revision,
                ..
            } => todo_complete(&mut record, &todo_id, resolution, expected_revision),
        };
        let (result, mutated) = match outcome {
            Ok(outcome) => outcome,
            Err(result) => return result,
        };
        if mutated {
            if let Err(message) = self.todo_store.save(&record) {
                return io_error("save", message);
            }
        }
        result
    }
}

fn next_todo_id(record: &TodoSessionRecord) -> String {
    let mut counter = record.todos.len() as u64 + 1;
    loop {
        let candidate = format!("{TODO_ID_PREFIX}{counter:04}");
        if !record.todos.iter().any(|todo| todo.todo_id == candidate) {
            return candidate;
        }
        counter += 1;
    }
}

fn find_todo<'a>(
    record: &'a mut TodoSessionRecord,
    todo_id: &str,
) -> Result<&'a mut TodoRecord, ToolResult> {
    if let Err(message) = validate_todo_id(todo_id) {
        return Err(invalid_arguments(message));
    }
    record
        .todos
        .iter_mut()
        .find(|todo| todo.todo_id == todo_id)
        .ok_or_else(|| {
            ToolResult::err_with_output(
                format!("unknown_todo_id: {todo_id}"),
                json!({
                    "error_kind": "unknown_todo_id",
                    "todo_id": todo_id,
                    "state_changed": false,
                }),
            )
            .with_recovery(super::RecoveryKind::FixInput, None)
        })
}

fn check_revision(todo: &TodoRecord, expected_revision: u64) -> Result<(), ToolResult> {
    if todo.revision != expected_revision {
        return Err(revision_conflict(expected_revision, todo.revision));
    }
    Ok(())
}

fn todo_create(
    record: &mut TodoSessionRecord,
    session_id: &str,
    subject: String,
    description: Option<String>,
    depends_on: Option<Vec<String>>,
) -> Result<(ToolResult, bool), ToolResult> {
    let subject = subject.trim();
    if subject.is_empty() || subject.chars().count() > MAX_TODO_SUBJECT_CHARS {
        return Err(invalid_arguments(format!(
            "subject must be 1..={MAX_TODO_SUBJECT_CHARS} chars"
        )));
    }
    if let Some(description) = description.as_deref() {
        if description.chars().count() > MAX_TODO_DESCRIPTION_CHARS {
            return Err(invalid_arguments(format!(
                "description must be at most {MAX_TODO_DESCRIPTION_CHARS} chars"
            )));
        }
    }
    let depends_on = depends_on.unwrap_or_default();
    if depends_on.len() > MAX_TODO_DEPENDENCIES {
        return Err(invalid_arguments(format!(
            "depends_on accepts at most {MAX_TODO_DEPENDENCIES} entries"
        )));
    }
    for dependency in &depends_on {
        if let Err(message) = validate_todo_id(dependency) {
            return Err(invalid_arguments(message));
        }
        if !record.todos.iter().any(|todo| todo.todo_id == *dependency) {
            return Err(invalid_arguments(format!(
                "unknown depends_on todo '{dependency}'"
            )));
        }
    }
    if record.todos.len() >= MAX_TODOS_PER_SESSION {
        return Err(state_error(format!(
            "todo capacity {MAX_TODOS_PER_SESSION} reached for this session"
        )));
    }
    let now = now_unix_millis();
    let todo = TodoRecord {
        todo_id: next_todo_id(record),
        session_id: session_id.to_string(),
        subject: subject.to_string(),
        description,
        status: TodoStatus::Open,
        revision: 1,
        depends_on,
        assignee: None,
        note: None,
        created_at: now,
        updated_at: now,
        resolved_at: None,
    };
    let output = todo.to_json();
    record.revision += 1;
    record.todos.push(todo);
    Ok((
        ToolResult::ok(json!({
            "success": true,
            "action": "create",
            "todo": output,
            "revision": record.revision,
        })),
        true,
    ))
}

fn todo_list(
    record: &TodoSessionRecord,
    status: Option<String>,
) -> Result<(ToolResult, bool), ToolResult> {
    let todos: Vec<Value> = record
        .todos
        .iter()
        .filter(|todo| match status.as_deref() {
            Some(filter) => todo.status.as_str() == filter,
            None => true,
        })
        .map(TodoRecord::to_json)
        .collect();
    Ok((
        ToolResult::ok(json!({
            "success": true,
            "action": "list",
            "session_id": record.session_id,
            "todos": todos,
            "revision": record.revision,
        })),
        false,
    ))
}

fn todo_get(record: &TodoSessionRecord, todo_id: &str) -> Result<(ToolResult, bool), ToolResult> {
    let mut probe = record.clone();
    let todo = find_todo(&mut probe, todo_id)?;
    let output = todo.to_json();
    Ok((
        ToolResult::ok(json!({
            "success": true,
            "action": "get",
            "todo": output,
        })),
        false,
    ))
}

fn todo_claim(
    record: &mut TodoSessionRecord,
    todo_id: &str,
    assignee: String,
    expected_revision: u64,
) -> Result<(ToolResult, bool), ToolResult> {
    let assignee = assignee.trim();
    if assignee.is_empty() || assignee.chars().count() > MAX_ASSIGNEE_CHARS {
        return Err(invalid_arguments(format!(
            "assignee must be 1..={MAX_ASSIGNEE_CHARS} chars"
        )));
    }
    let todo = find_todo(record, todo_id)?;
    check_revision(todo, expected_revision)?;
    if todo.status != TodoStatus::Open {
        return Err(state_error(format!(
            "claim requires an open todo, current status is {}",
            todo.status.as_str()
        )));
    }
    todo.status = TodoStatus::InProgress;
    todo.assignee = Some(assignee.to_string());
    todo.revision += 1;
    todo.updated_at = now_unix_millis();
    let output = todo.to_json();
    record.revision += 1;
    Ok((
        ToolResult::ok(json!({
            "success": true,
            "action": "claim",
            "todo": output,
            "revision": record.revision,
        })),
        true,
    ))
}

fn todo_release(
    record: &mut TodoSessionRecord,
    todo_id: &str,
    expected_revision: u64,
) -> Result<(ToolResult, bool), ToolResult> {
    let todo = find_todo(record, todo_id)?;
    check_revision(todo, expected_revision)?;
    if todo.status != TodoStatus::InProgress {
        return Err(state_error(format!(
            "release requires an in_progress todo, current status is {}",
            todo.status.as_str()
        )));
    }
    todo.status = TodoStatus::Open;
    todo.assignee = None;
    todo.revision += 1;
    todo.updated_at = now_unix_millis();
    let output = todo.to_json();
    record.revision += 1;
    Ok((
        ToolResult::ok(json!({
            "success": true,
            "action": "release",
            "todo": output,
            "revision": record.revision,
        })),
        true,
    ))
}

fn todo_advance(
    record: &mut TodoSessionRecord,
    todo_id: &str,
    status: String,
    note: Option<String>,
    expected_revision: u64,
) -> Result<(ToolResult, bool), ToolResult> {
    let target = match status.as_str() {
        "blocked" => TodoStatus::Blocked,
        "cancelled" => TodoStatus::Cancelled,
        other => {
            return Err(invalid_arguments(format!(
                "advance status must be blocked or cancelled, got '{other}'"
            )))
        }
    };
    if let Some(note) = note.as_deref() {
        if note.chars().count() > MAX_TODO_NOTE_CHARS {
            return Err(invalid_arguments(format!(
                "note must be at most {MAX_TODO_NOTE_CHARS} chars"
            )));
        }
    }
    let todo = find_todo(record, todo_id)?;
    check_revision(todo, expected_revision)?;
    if matches!(todo.status, TodoStatus::Resolved | TodoStatus::Cancelled) {
        return Err(state_error(format!(
            "cannot advance a {} todo",
            todo.status.as_str()
        )));
    }
    todo.status = target;
    todo.note = note;
    todo.revision += 1;
    todo.updated_at = now_unix_millis();
    let output = todo.to_json();
    record.revision += 1;
    Ok((
        ToolResult::ok(json!({
            "success": true,
            "action": "advance",
            "todo": output,
            "revision": record.revision,
        })),
        true,
    ))
}

fn todo_complete(
    record: &mut TodoSessionRecord,
    todo_id: &str,
    resolution: Option<String>,
    expected_revision: u64,
) -> Result<(ToolResult, bool), ToolResult> {
    if let Some(resolution) = resolution.as_deref() {
        if resolution.chars().count() > MAX_TODO_NOTE_CHARS {
            return Err(invalid_arguments(format!(
                "resolution must be at most {MAX_TODO_NOTE_CHARS} chars"
            )));
        }
    }
    let dependencies: Vec<String> = {
        let todo = find_todo(record, todo_id)?;
        check_revision(todo, expected_revision)?;
        if matches!(todo.status, TodoStatus::Resolved | TodoStatus::Cancelled) {
            return Err(state_error(format!(
                "todo is already {}",
                todo.status.as_str()
            )));
        }
        todo.depends_on.clone()
    };
    for dependency in dependencies {
        let resolved = record
            .todos
            .iter()
            .find(|candidate| candidate.todo_id == dependency)
            .map(|candidate| candidate.status == TodoStatus::Resolved)
            .unwrap_or(false);
        if !resolved {
            return Err(state_error(format!(
                "dependency '{dependency}' must be resolved before completion"
            )));
        }
    }
    let todo = find_todo(record, todo_id)?;
    let now = now_unix_millis();
    todo.status = TodoStatus::Resolved;
    todo.note = resolution.or(todo.note.take());
    todo.revision += 1;
    todo.updated_at = now;
    todo.resolved_at = Some(now);
    let output = todo.to_json();
    record.revision += 1;
    Ok((
        ToolResult::ok(json!({
            "success": true,
            "action": "complete",
            "todo": output,
            "revision": record.revision,
        })),
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, TodoStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TodoStore::new(dir.path().join("state"));
        (dir, store)
    }

    fn sample_record() -> TodoSessionRecord {
        TodoSessionRecord::new("wc_sess_test".to_string())
    }

    #[test]
    fn create_assigns_monotonic_ids_and_persists() {
        let (_dir, store) = temp_store();
        let mut record = sample_record();
        let (result, mutated) =
            todo_create(&mut record, "wc_sess_test", "first".to_string(), None, None)
                .expect("create");
        assert!(mutated && result.success);
        assert_eq!(record.todos[0].todo_id, "wc_todo_0001");
        store.save(&record).expect("save");
        let reloaded = store.load("wc_sess_test").expect("load");
        assert_eq!(reloaded, record);
    }

    #[test]
    fn claim_requires_open_and_revision_match() {
        let mut record = sample_record();
        todo_create(&mut record, "wc_sess_test", "t".to_string(), None, None).unwrap();
        let conflict = todo_claim(&mut record, "wc_todo_0001", "agent-1".to_string(), 9)
            .expect_err("stale revision");
        assert_eq!(conflict.output["error_kind"], "todo_revision_conflict");
        let (result, _) =
            todo_claim(&mut record, "wc_todo_0001", "agent-1".to_string(), 1).expect("claim");
        assert!(result.success);
        assert_eq!(record.todos[0].status, TodoStatus::InProgress);
        todo_claim(&mut record, "wc_todo_0001", "agent-2".to_string(), 2)
            .expect_err("already in_progress");
    }

    #[test]
    fn release_returns_to_open() {
        let mut record = sample_record();
        todo_create(&mut record, "wc_sess_test", "t".to_string(), None, None).unwrap();
        todo_claim(&mut record, "wc_todo_0001", "agent-1".to_string(), 1).unwrap();
        let (_, _) = todo_release(&mut record, "wc_todo_0001", 2).expect("release");
        assert_eq!(record.todos[0].status, TodoStatus::Open);
        assert_eq!(record.todos[0].assignee, None);
    }

    #[test]
    fn complete_requires_resolved_dependencies() {
        let mut record = sample_record();
        todo_create(&mut record, "wc_sess_test", "dep".to_string(), None, None).unwrap();
        todo_create(
            &mut record,
            "wc_sess_test",
            "main".to_string(),
            None,
            Some(vec!["wc_todo_0001".to_string()]),
        )
        .unwrap();
        todo_complete(&mut record, "wc_todo_0002", None, 1).expect_err("dependency not resolved");
        todo_claim(&mut record, "wc_todo_0001", "agent-1".to_string(), 1).unwrap();
        todo_complete(&mut record, "wc_todo_0001", None, 2).expect("complete dependency");
        todo_claim(&mut record, "wc_todo_0002", "agent-1".to_string(), 1).unwrap();
        let (result, _) = todo_complete(&mut record, "wc_todo_0002", None, 2).expect("complete");
        assert!(result.success);
        assert_eq!(record.todos[1].status, TodoStatus::Resolved);
        assert!(record.todos[1].resolved_at.is_some());
    }

    #[test]
    fn advance_targets_blocked_or_cancelled_only() {
        let mut record = sample_record();
        todo_create(&mut record, "wc_sess_test", "t".to_string(), None, None).unwrap();
        todo_advance(
            &mut record,
            "wc_todo_0001",
            "blocked".to_string(),
            Some("waiting on api".to_string()),
            1,
        )
        .expect("advance to blocked");
        assert_eq!(record.todos[0].status, TodoStatus::Blocked);
        todo_advance(&mut record, "wc_todo_0001", "done".to_string(), None, 2)
            .expect_err("invalid target status");
    }

    #[test]
    fn list_filters_by_status() {
        let mut record = sample_record();
        todo_create(&mut record, "wc_sess_test", "a".to_string(), None, None).unwrap();
        todo_create(&mut record, "wc_sess_test", "b".to_string(), None, None).unwrap();
        todo_claim(&mut record, "wc_todo_0002", "agent-1".to_string(), 1).unwrap();
        let (result, _) = todo_list(&record, Some("in_progress".to_string())).expect("list");
        let todos = result.output["todos"].as_array().expect("todos");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0]["todo_id"], "wc_todo_0002");
    }
}
