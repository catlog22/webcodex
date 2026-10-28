//! 工作区待办的数据契约；登记状态不代表执行或验证证据。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTaskStatus {
    #[default]
    Open,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceTask {
    pub task_id: String,
    pub project: String,
    pub title: String,
    pub description: String,
    pub completion_summary: String,
    pub status: WorkspaceTaskStatus,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceTaskSummary {
    pub task_id: String,
    pub project: String,
    pub title: String,
    pub status: WorkspaceTaskStatus,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceTaskPage {
    pub tasks: Vec<WorkspaceTaskSummary>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceTaskDetail {
    pub task: WorkspaceTask,
    pub root_fingerprint: String,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceTaskListQuery {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub status: Option<WorkspaceTaskStatus>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceTaskRequest {
    pub project: String,
    pub operation: WorkspaceTaskOperation,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceTaskOperation {
    List {
        #[serde(default)]
        query: WorkspaceTaskListQuery,
    },
    Read {
        task_id: String,
    },
    Create {
        title: String,
        #[serde(default)]
        description: String,
        idempotency_key: String,
        expected_root_fingerprint: String,
    },
    Update {
        task_id: String,
        expected_revision: u64,
        expected_root_fingerprint: String,
        title: String,
        description: String,
        #[serde(default)]
        completion_summary: String,
        status: WorkspaceTaskStatus,
    },
    Complete {
        task_id: String,
        expected_revision: u64,
        expected_root_fingerprint: String,
        completion_summary: String,
    },
}

#[derive(Debug)]
pub struct WorkspaceTaskUpdate {
    pub task_id: String,
    pub expected_revision: u64,
    pub title: String,
    pub description: String,
    pub completion_summary: String,
    pub status: WorkspaceTaskStatus,
}
