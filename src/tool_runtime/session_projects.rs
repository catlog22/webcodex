//! Session-scoped attached-workspace registry for the consolidated `session` tool.
//!
//! The canonical Session project stays in the Workflow Session ledger; this
//! store holds only *additional* authorized workspaces attached at start or
//! via `attach_project`. Every attached project is still independently
//! resolved and authorized on every tool call — the registry is association
//! context, never authority.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

const SESSION_PROJECTS_VERSION: u32 = 1;
const MAX_ATTACHED_PROJECTS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionProjectsError {
    InvalidSessionId(String),
    InvalidProject(String),
    Conflict { expected: u64, actual: u64 },
    NotFound(String),
    Capacity,
    Io(String),
}

impl SessionProjectsError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidSessionId(_) => "invalid_session_id",
            Self::InvalidProject(_) => "invalid_project",
            Self::Conflict { .. } => "session_projects_revision_conflict",
            Self::NotFound(_) => "attached_project_not_found",
            Self::Capacity => "attached_project_capacity",
            Self::Io(_) => "session_projects_io",
        }
    }

    pub(crate) fn into_tool_result(self) -> super::ToolResult {
        super::ToolResult::err_with_output(
            self.code(),
            json!({
                "error_kind": self.code(),
                "state_changed": false,
                "message": self.message(),
            }),
        )
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::InvalidSessionId(value) => format!("invalid session id '{value}'"),
            Self::InvalidProject(value) => format!("invalid attached project '{value}'"),
            Self::Conflict { expected, actual } => format!(
                "session attached-workspace revision conflict: expected {expected}, actual {actual}"
            ),
            Self::NotFound(project) => format!("attached project '{project}' not found"),
            Self::Capacity => format!(
                "attached workspace capacity {MAX_ATTACHED_PROJECTS} reached for this session"
            ),
            Self::Io(message) => message.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AttachedProject {
    pub(crate) project: String,
    pub(crate) attached_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionProjectsRecord {
    pub(crate) version: u32,
    pub(crate) session_id: String,
    pub(crate) revision: u64,
    pub(crate) attached: Vec<AttachedProject>,
}

impl SessionProjectsRecord {
    fn new(session_id: String) -> Self {
        Self {
            version: SESSION_PROJECTS_VERSION,
            session_id,
            revision: 0,
            attached: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SessionProjectsStore {
    state_dir: PathBuf,
}

impl Default for SessionProjectsStore {
    fn default() -> Self {
        Self::new(crate::config::runtime_state_dir())
    }
}

impl SessionProjectsStore {
    pub(crate) fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    fn record_path(&self, session_id: &str) -> Result<PathBuf, SessionProjectsError> {
        validate_session_id(session_id)?;
        Ok(self
            .state_dir
            .join("session_projects")
            .join(format!("{session_id}.json")))
    }

    pub(crate) fn load(
        &self,
        session_id: &str,
    ) -> Result<SessionProjectsRecord, SessionProjectsError> {
        let path = self.record_path(session_id)?;
        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut record: SessionProjectsRecord =
                    serde_json::from_str(&content).map_err(|err| {
                        SessionProjectsError::Io(format!("invalid record JSON: {err}"))
                    })?;
                record.session_id = session_id.to_string();
                Ok(record)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(SessionProjectsRecord::new(session_id.to_string()))
            }
            Err(err) => Err(SessionProjectsError::Io(format!(
                "failed to read record: {err}"
            ))),
        }
    }

    fn save(&self, record: &SessionProjectsRecord) -> Result<(), SessionProjectsError> {
        let path = self.record_path(&record.session_id)?;
        let parent = path
            .parent()
            .ok_or_else(|| SessionProjectsError::Io("record path has no parent".to_string()))?;
        fs::create_dir_all(parent).map_err(|err| {
            SessionProjectsError::Io(format!("failed to create record dir: {err}"))
        })?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("record.json");
        let tmp = path.with_file_name(format!(
            ".{file_name}.tmp-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let data = serde_json::to_vec_pretty(record).map_err(|err| {
            SessionProjectsError::Io(format!("failed to serialize record: {err}"))
        })?;
        fs::write(&tmp, data)
            .and_then(|_| fs::rename(&tmp, &path))
            .map_err(|err| {
                let _ = fs::remove_file(&tmp);
                SessionProjectsError::Io(format!("failed to write record: {err}"))
            })
    }

    /// Bulk replacement used by `session start` on a fresh session; not a CAS path.
    pub(crate) fn initialize(
        &self,
        session_id: &str,
        projects: Vec<String>,
    ) -> Result<SessionProjectsRecord, SessionProjectsError> {
        validate_session_id(session_id)?;
        let mut seen = std::collections::BTreeSet::new();
        let attached = projects
            .into_iter()
            .map(|project| {
                let project = project.trim().to_string();
                if project.is_empty() {
                    return Err(SessionProjectsError::InvalidProject(project));
                }
                if !seen.insert(project.clone()) {
                    return Err(SessionProjectsError::InvalidProject(format!(
                        "duplicate attached project '{project}'"
                    )));
                }
                Ok(AttachedProject {
                    project,
                    attached_at: now_unix_millis(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if attached.len() > MAX_ATTACHED_PROJECTS {
            return Err(SessionProjectsError::Capacity);
        }
        let record = SessionProjectsRecord {
            version: SESSION_PROJECTS_VERSION,
            session_id: session_id.to_string(),
            revision: 1,
            attached,
        };
        self.save(&record)?;
        Ok(record)
    }

    pub(crate) fn attach(
        &self,
        session_id: &str,
        project: String,
        expected_revision: u64,
    ) -> Result<SessionProjectsRecord, SessionProjectsError> {
        let mut record = self.load(session_id)?;
        check_revision(&record, expected_revision)?;
        let project = project.trim().to_string();
        if project.is_empty() {
            return Err(SessionProjectsError::InvalidProject(project));
        }
        if record.attached.iter().any(|entry| entry.project == project) {
            return Err(SessionProjectsError::InvalidProject(format!(
                "project '{project}' is already attached"
            )));
        }
        if record.attached.len() >= MAX_ATTACHED_PROJECTS {
            return Err(SessionProjectsError::Capacity);
        }
        record.attached.push(AttachedProject {
            project,
            attached_at: now_unix_millis(),
        });
        record.revision += 1;
        self.save(&record)?;
        Ok(record)
    }

    pub(crate) fn detach(
        &self,
        session_id: &str,
        project: &str,
        expected_revision: u64,
    ) -> Result<SessionProjectsRecord, SessionProjectsError> {
        let mut record = self.load(session_id)?;
        check_revision(&record, expected_revision)?;
        let index = record
            .attached
            .iter()
            .position(|entry| entry.project == project)
            .ok_or_else(|| SessionProjectsError::NotFound(project.to_string()))?;
        record.attached.remove(index);
        record.revision += 1;
        self.save(&record)?;
        Ok(record)
    }
}

fn check_revision(
    record: &SessionProjectsRecord,
    expected_revision: u64,
) -> Result<(), SessionProjectsError> {
    if record.revision != expected_revision {
        return Err(SessionProjectsError::Conflict {
            expected: expected_revision,
            actual: record.revision,
        });
    }
    Ok(())
}

fn validate_session_id(session_id: &str) -> Result<(), SessionProjectsError> {
    if session_id.starts_with("wc_sess_") && !session_id[8..].is_empty() {
        Ok(())
    } else {
        Err(SessionProjectsError::InvalidSessionId(
            session_id.to_string(),
        ))
    }
}

fn now_unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, SessionProjectsStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SessionProjectsStore::new(dir.path().join("state"));
        (dir, store)
    }

    #[test]
    fn load_missing_record_returns_empty_revision_zero() {
        let (_dir, store) = temp_store();
        let record = store.load("wc_sess_a").expect("load");
        assert_eq!(record.revision, 0);
        assert!(record.attached.is_empty());
    }

    #[test]
    fn initialize_and_reload_roundtrip() {
        let (_dir, store) = temp_store();
        let record = store
            .initialize("wc_sess_a", vec!["agent:oe:b".to_string()])
            .expect("initialize");
        assert_eq!(record.revision, 1);
        let reloaded = store.load("wc_sess_a").expect("reload");
        assert_eq!(reloaded, record);
    }

    #[test]
    fn attach_advances_revision_and_persists() {
        let (_dir, store) = temp_store();
        store.initialize("wc_sess_a", vec![]).expect("initialize");
        let record = store
            .attach("wc_sess_a", "agent:oe:b".to_string(), 1)
            .expect("attach");
        assert_eq!(record.revision, 2);
        assert_eq!(record.attached.len(), 1);
        let reloaded = store.load("wc_sess_a").expect("reload");
        assert_eq!(reloaded.attached[0].project, "agent:oe:b");
    }

    #[test]
    fn stale_revision_conflicts() {
        let (_dir, store) = temp_store();
        store.initialize("wc_sess_a", vec![]).expect("initialize");
        let err = store
            .attach("wc_sess_a", "agent:oe:b".to_string(), 7)
            .expect_err("stale revision must conflict");
        assert!(matches!(err, SessionProjectsError::Conflict { .. }));
    }

    #[test]
    fn duplicate_attach_rejected() {
        let (_dir, store) = temp_store();
        store
            .initialize("wc_sess_a", vec!["agent:oe:b".to_string()])
            .expect("initialize");
        let err = store
            .attach("wc_sess_a", "agent:oe:b".to_string(), 1)
            .expect_err("duplicate must fail");
        assert!(matches!(err, SessionProjectsError::InvalidProject(_)));
    }

    #[test]
    fn detach_unknown_project_not_found() {
        let (_dir, store) = temp_store();
        store.initialize("wc_sess_a", vec![]).expect("initialize");
        let err = store
            .detach("wc_sess_a", "agent:oe:missing", 1)
            .expect_err("unknown project must fail");
        assert!(matches!(err, SessionProjectsError::NotFound(_)));
    }

    #[test]
    fn detach_removes_and_advances_revision() {
        let (_dir, store) = temp_store();
        store
            .initialize(
                "wc_sess_a",
                vec!["agent:oe:b".to_string(), "agent:oe:c".to_string()],
            )
            .expect("initialize");
        let record = store.detach("wc_sess_a", "agent:oe:b", 1).expect("detach");
        assert_eq!(record.revision, 2);
        assert_eq!(record.attached.len(), 1);
        assert_eq!(record.attached[0].project, "agent:oe:c");
    }
}
