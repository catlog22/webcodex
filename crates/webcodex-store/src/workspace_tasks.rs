use crate::Database;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use webcodex_core::workspace_task::{
    WorkspaceTask, WorkspaceTaskListQuery, WorkspaceTaskPage, WorkspaceTaskStatus,
    WorkspaceTaskSummary, WorkspaceTaskUpdate,
};

const MAX_TASKS: i64 = 1000;

#[derive(Debug)]
pub struct WorkspaceTaskError {
    pub code: &'static str,
    pub message: &'static str,
}

fn error(code: &'static str, message: &'static str) -> WorkspaceTaskError {
    WorkspaceTaskError { code, message }
}

fn storage_error(cause: rusqlite::Error) -> WorkspaceTaskError {
    tracing::warn!(error = %cause, "Workspace task storage failed");
    error(
        "workspace_task_storage_unavailable",
        "Workspace task storage is unavailable",
    )
}

fn validate_scope(project: &str, root: &str) -> Result<(), WorkspaceTaskError> {
    if project.is_empty() || project.len() > 512 || root.is_empty() || root.len() > 256 {
        return Err(error(
            "workspace_task_scope_invalid",
            "An exact Project and root identity are required",
        ));
    }
    Ok(())
}

fn validate_content(title: &str, description: &str) -> Result<(), WorkspaceTaskError> {
    if title.trim().is_empty()
        || title.chars().count() > 200
        || title.chars().any(char::is_control)
        || description.chars().count() > 8000
        || description.len() > 32000
        || description
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(error("workspace_task_content_invalid", "Title must contain 1..200 characters without controls; description is limited to 8000 characters and 32000 bytes"));
    }
    Ok(())
}

fn status_text(status: WorkspaceTaskStatus) -> &'static str {
    match status {
        WorkspaceTaskStatus::Open => "open",
        WorkspaceTaskStatus::InProgress => "in_progress",
        WorkspaceTaskStatus::Done => "done",
        WorkspaceTaskStatus::Cancelled => "cancelled",
    }
}

fn task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceTask> {
    let revision: i64 = row.get(5)?;
    let status: String = row.get(4)?;
    let status = match status.as_str() {
        "open" => WorkspaceTaskStatus::Open,
        "in_progress" => WorkspaceTaskStatus::InProgress,
        "done" => WorkspaceTaskStatus::Done,
        "cancelled" => WorkspaceTaskStatus::Cancelled,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(WorkspaceTask {
        task_id: row.get(0)?,
        project: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        status,
        revision: u64::try_from(revision)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, revision))?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        completion_summary: row.get(8)?,
    })
}

const COLUMNS: &str = "task_id, project, title, description, status, revision, created_at, updated_at, completion_summary";

impl Database {
    pub fn create_workspace_task(
        &self,
        project: &str,
        root: &str,
        title: &str,
        description: &str,
        key: &str,
    ) -> Result<WorkspaceTask, WorkspaceTaskError> {
        validate_scope(project, root)?;
        validate_content(title, description)?;
        if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
            return Err(error(
                "workspace_task_key_invalid",
                "idempotency_key must contain 1..128 bytes without controls",
            ));
        }
        let title = title.trim();
        let hash = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(title, description)).expect("string tuple serializes")
            )
        );
        let mut conn = self.lock_connection(crate::StoreDomain::WorkspaceTask);
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let existing: Option<(String, String)> = tx.query_row(
            "SELECT task_id, request_hash FROM workspace_tasks WHERE project=?1 AND root_fingerprint=?2 AND idempotency_key=?3",
            params![project, root, key], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(storage_error)?;
        if let Some((id, previous)) = existing {
            if previous != hash {
                return Err(error(
                    "workspace_task_idempotency_conflict",
                    "The key was already used with different task content",
                ));
            }
            return tx
                .query_row(
                    &format!("SELECT {COLUMNS} FROM workspace_tasks WHERE task_id=?1"),
                    [&id],
                    task_row,
                )
                .map_err(storage_error);
        }
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM workspace_tasks WHERE project=?1 AND root_fingerprint=?2",
                params![project, root],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if count >= MAX_TASKS {
            return Err(error(
                "workspace_task_capacity_reached",
                "This workspace has reached the 1000-task limit",
            ));
        }
        let id = format!("wc_wtask_{}", uuid::Uuid::new_v4().simple());
        let now = chrono::Utc::now().timestamp();
        tx.execute("INSERT INTO workspace_tasks (task_id,project,root_fingerprint,title,description,status,revision,created_at,updated_at,idempotency_key,request_hash) VALUES (?1,?2,?3,?4,?5,'open',1,?6,?6,?7,?8)", params![id, project, root, title, description, now, key, hash]).map_err(storage_error)?;
        let task = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM workspace_tasks WHERE task_id=?1"),
                [&id],
                task_row,
            )
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(task)
    }

    pub fn read_workspace_task(
        &self,
        project: &str,
        root: &str,
        id: &str,
    ) -> Result<WorkspaceTask, WorkspaceTaskError> {
        validate_scope(project, root)?;
        self.lock_connection(crate::StoreDomain::WorkspaceTask).query_row(
            &format!("SELECT {COLUMNS} FROM workspace_tasks WHERE project=?1 AND root_fingerprint=?2 AND task_id=?3"),
            params![project, root, id], task_row).optional().map_err(storage_error)?
            .ok_or_else(|| error("workspace_task_not_found", "Workspace task was not found"))
    }

    pub fn list_workspace_tasks(
        &self,
        project: &str,
        root: &str,
        query: WorkspaceTaskListQuery,
    ) -> Result<WorkspaceTaskPage, WorkspaceTaskError> {
        validate_scope(project, root)?;
        if query
            .query
            .as_ref()
            .is_some_and(|q| q.chars().count() > 200 || q.contains('\0'))
        {
            return Err(error(
                "workspace_task_query_invalid",
                "Query is limited to 200 characters without NUL",
            ));
        }
        let limit = query.limit.unwrap_or(20).clamp(1, 50);
        let offset = i64::try_from(query.offset)
            .map_err(|_| error("workspace_task_offset_invalid", "offset is too large"))?;
        let conn = self.lock_connection(crate::StoreDomain::WorkspaceTask);
        // 列表不加载说明正文，避免分页把大量任务内容带入模型上下文。
        let mut stmt = conn.prepare("SELECT task_id, project, title, '', status, revision, created_at, updated_at, '' FROM workspace_tasks WHERE project=?1 AND root_fingerprint=?2 AND (?3 IS NULL OR status=?3) AND (?6 IS NULL OR instr(title, ?6)>0) ORDER BY created_at DESC, task_id DESC LIMIT ?4 OFFSET ?5").map_err(storage_error)?;
        let rows = stmt
            .query_map(
                params![
                    project,
                    root,
                    query.status.map(status_text),
                    limit as i64 + 1,
                    offset,
                    query.query
                ],
                task_row,
            )
            .map_err(storage_error)?;
        let mut tasks = rows
            .map(|row| {
                row.map(|task| WorkspaceTaskSummary {
                    task_id: task.task_id,
                    project: task.project,
                    title: task.title,
                    status: task.status,
                    revision: task.revision,
                    created_at: task.created_at,
                    updated_at: task.updated_at,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        let next_offset = (tasks.len() > limit).then(|| (offset as usize).saturating_add(limit));
        tasks.truncate(limit);
        let total = conn.query_row("SELECT COUNT(*) FROM workspace_tasks WHERE project=?1 AND root_fingerprint=?2 AND (?3 IS NULL OR status=?3) AND (?4 IS NULL OR instr(title, ?4)>0)", params![project, root, query.status.map(status_text), query.query], |row| row.get::<_, i64>(0)).map_err(storage_error)?;
        let total = usize::try_from(total).map_err(|_| {
            error(
                "workspace_task_storage_unavailable",
                "Workspace task count cannot be represented",
            )
        })?;
        Ok(WorkspaceTaskPage {
            tasks,
            total,
            next_offset,
        })
    }

    pub fn complete_workspace_task(
        &self,
        project: &str,
        root: &str,
        id: &str,
        expected_revision: u64,
        summary: String,
    ) -> Result<WorkspaceTask, WorkspaceTaskError> {
        let current = self.read_workspace_task(project, root, id)?;
        self.update_workspace_task(
            project,
            root,
            WorkspaceTaskUpdate {
                task_id: current.task_id,
                expected_revision,
                title: current.title,
                description: current.description,
                completion_summary: summary,
                status: WorkspaceTaskStatus::Done,
            },
        )
    }

    pub fn update_workspace_task(
        &self,
        project: &str,
        root: &str,
        update: WorkspaceTaskUpdate,
    ) -> Result<WorkspaceTask, WorkspaceTaskError> {
        validate_scope(project, root)?;
        validate_content(&update.title, &update.description)?;
        let expected_revision = i64::try_from(update.expected_revision)
            .ok()
            .filter(|revision| *revision > 0 && *revision < i64::MAX)
            .ok_or_else(|| {
                error(
                    "workspace_task_revision_invalid",
                    "Revision is outside SQLite's supported update range",
                )
            })?;
        if update.completion_summary.chars().count() > 2000
            || update.completion_summary.len() > 8000
            || update
                .completion_summary
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
            || update.status == WorkspaceTaskStatus::Done
                && update.completion_summary.trim().is_empty()
        {
            return Err(error(
                "workspace_task_summary_required",
                "Completed tasks require a summary of 1..2000 characters and at most 8000 bytes",
            ));
        }
        let mut conn = self.lock_connection(crate::StoreDomain::WorkspaceTask);
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let current = tx.query_row(&format!("SELECT {COLUMNS} FROM workspace_tasks WHERE project=?1 AND root_fingerprint=?2 AND task_id=?3"), params![project, root, update.task_id], task_row).optional().map_err(storage_error)?
            .ok_or_else(|| error("workspace_task_not_found", "Workspace task was not found"))?;
        if current.revision != update.expected_revision {
            return Err(error(
                "workspace_task_revision_conflict",
                "The task changed; read it again before editing",
            ));
        }
        tx.execute("UPDATE workspace_tasks SET title=?1, description=?2, status=?3, revision=revision+1, updated_at=?4, completion_summary=?7 WHERE task_id=?5 AND revision=?6", params![update.title.trim(), update.description, status_text(update.status), chrono::Utc::now().timestamp(), update.task_id, expected_revision, update.completion_summary]).map_err(storage_error)?;
        let task = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM workspace_tasks WHERE task_id=?1"),
                [&update.task_id],
                task_row,
            )
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(task)
    }
}
