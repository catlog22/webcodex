//! Integration tests for the consolidated `session` / `todo` / `checkpoint`
//! action tool surfaces.

use super::super::*;
use super::support::*;
use serde_json::{json, Value};

fn session_start_call(project: Option<String>, projects: Option<Vec<String>>) -> ToolCall {
    ToolCall::Session(SessionToolCall::Start {
        project,
        projects,
        title: Some("consolidated-surface".to_string()),
        mode: None,
        execution_context: None,
    })
}

fn session_get_call(session_id: &str) -> ToolCall {
    ToolCall::Session(SessionToolCall::Get {
        session_id: session_id.to_string(),
    })
}

fn todo_create_call(session_id: &str, subject: &str) -> ToolCall {
    ToolCall::Todo(TodoToolCall::Create {
        session_id: session_id.to_string(),
        subject: subject.to_string(),
        description: None,
        depends_on: None,
    })
}

fn isolate_session_state(runtime: &mut ToolRuntime, state: &std::path::Path) {
    runtime.todo_store = super::super::todo::TodoStore::new(state.join("todos"));
    runtime.session_projects =
        super::super::session_projects::SessionProjectsStore::new(state.join("session_projects"));
}

#[tokio::test]
async fn session_start_with_projects_registers_attached_workspaces() {
    let mut runtime = test_runtime();
    let state = tempfile::tempdir().unwrap();
    isolate_session_state(&mut runtime, state.path());
    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    let project_a =
        register_runner_project_at_path(&runtime, "sess-multi-a", "agent-proj", tmp_a.path()).await;
    let project_b =
        register_runner_project_at_path(&runtime, "sess-multi-b", "agent-proj", tmp_b.path()).await;

    let started = runtime
        .dispatch(session_start_call(
            None,
            Some(vec![project_a.clone(), project_b.clone()]),
        ))
        .await;
    assert!(started.success, "{:?}", started.error);
    assert_eq!(started.output["action"], json!("start"));
    let projects = started.output["projects"]
        .as_array()
        .expect("projects array");
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0]["project"], json!(project_a));
    assert_eq!(projects[0]["canonical"], json!(true));
    assert_eq!(projects[1]["project"], json!(project_b));
    assert_eq!(projects[1]["canonical"], json!(false));
    let session_id = started.output["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    let got = runtime.dispatch(session_get_call(&session_id)).await;
    assert!(got.success);
    assert_eq!(got.output["projects"], started.output["projects"]);
}

#[tokio::test]
async fn session_attach_detach_projects_enforce_revision_cas() {
    let mut runtime = test_runtime();
    let state = tempfile::tempdir().unwrap();
    isolate_session_state(&mut runtime, state.path());
    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    let project_a =
        register_runner_project_at_path(&runtime, "sess-cas-a", "agent-proj", tmp_a.path()).await;
    let project_b =
        register_runner_project_at_path(&runtime, "sess-cas-b", "agent-proj", tmp_b.path()).await;

    let started = runtime
        .dispatch(session_start_call(Some(project_a.clone()), None))
        .await;
    assert!(started.success, "{:?}", started.error);
    let session_id = started.output["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    // start initializes the projects record at revision 1.
    let attached = runtime
        .dispatch(ToolCall::Session(SessionToolCall::AttachProject {
            session_id: session_id.clone(),
            project: project_b.clone(),
            expected_revision: 1,
        }))
        .await;
    assert!(attached.success, "{:?}", attached.error);
    assert_eq!(attached.output["revision"], json!(2));

    let stale = runtime
        .dispatch(ToolCall::Session(SessionToolCall::DetachProject {
            session_id: session_id.clone(),
            project: project_b.clone(),
            expected_revision: 1,
        }))
        .await;
    assert!(!stale.success);
    assert_eq!(
        stale.output["error_kind"],
        json!("session_projects_revision_conflict")
    );

    let detached = runtime
        .dispatch(ToolCall::Session(SessionToolCall::DetachProject {
            session_id: session_id.clone(),
            project: project_b,
            expected_revision: 2,
        }))
        .await;
    assert!(detached.success, "{:?}", detached.error);
    let attached_list = detached.output["projects"]
        .as_array()
        .expect("projects array");
    assert_eq!(attached_list.len(), 1);
    assert_eq!(attached_list[0]["project"], json!(project_a));
}

#[tokio::test]
async fn todo_lifecycle_persists_across_runtime_instances() {
    let state = tempfile::tempdir().unwrap();
    let ledger = state.path().join("sessions.json");
    let mut runtime = test_runtime().with_session_ledger(ledger.clone());
    isolate_session_state(&mut runtime, state.path());
    let started = runtime.dispatch(session_start_call(None, None)).await;
    assert!(started.success, "{:?}", started.error);
    let session_id = started.output["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    let created = runtime
        .dispatch(todo_create_call(&session_id, "write the doc"))
        .await;
    assert!(created.success, "{:?}", created.error);
    assert_eq!(created.output["todo"]["status"], json!("open"));
    let todo_id = created.output["todo"]["todo_id"]
        .as_str()
        .expect("todo_id")
        .to_string();

    let claimed = runtime
        .dispatch(ToolCall::Todo(TodoToolCall::Claim {
            session_id: session_id.clone(),
            todo_id: todo_id.clone(),
            assignee: "agent-1".to_string(),
            expected_revision: 1,
        }))
        .await;
    assert!(claimed.success, "{:?}", claimed.error);
    assert_eq!(claimed.output["todo"]["status"], json!("in_progress"));

    // A fresh runtime over the same ledger and state dir sees the same todo record.
    let mut reloaded = test_runtime().with_session_ledger(ledger);
    isolate_session_state(&mut reloaded, state.path());
    let listed = reloaded
        .dispatch(ToolCall::Todo(TodoToolCall::List {
            session_id,
            status: None,
        }))
        .await;
    assert!(listed.success);
    let todos = listed.output["todos"].as_array().expect("todos");
    assert_eq!(todos.len(), 1);
    assert_eq!(todos[0]["todo_id"], json!(todo_id));
    assert_eq!(todos[0]["status"], json!("in_progress"));
}

#[tokio::test]
async fn session_compact_writes_memory_knowhow_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let db = std::sync::Arc::new(crate::Database::open(&tmp.path().join("webcodex.db")).unwrap());
    let mut runtime = test_runtime().with_memory_database(db.clone());
    isolate_session_state(&mut runtime, state.path());
    let project =
        register_runner_project_at_path(&runtime, "sess-compact", "agent-proj", tmp.path()).await;

    let started = runtime
        .dispatch(session_start_call(Some(project.clone()), None))
        .await;
    assert!(started.success, "{:?}", started.error);
    let session_id = started.output["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    let created = runtime
        .dispatch(todo_create_call(&session_id, "finish the migration"))
        .await;
    assert!(created.success, "{:?}", created.error);

    let compacted = runtime
        .dispatch(ToolCall::Session(SessionToolCall::Compact {
            session_id: session_id.clone(),
            note: Some("mid-session handoff".to_string()),
        }))
        .await;
    assert!(compacted.success, "{:?}", compacted.error);
    assert_eq!(compacted.output["action"], json!("compact"));
    let memory_key = compacted.output["memory_key"]
        .as_str()
        .expect("memory_key")
        .to_string();
    assert_eq!(memory_key, format!("session_compact.{session_id}"));

    let resolved = runtime
        .resolve_project_input_for_auth(&project, None)
        .await
        .unwrap();
    let scope_id = super::super::memory::memory_scope_id(&resolved);
    let stored = db
        .get_project_memory(&scope_id, &memory_key)
        .unwrap()
        .expect("compacted memory entry");
    assert!(stored.tags.iter().any(|tag| tag == "compaction"));
    let body: Value = serde_json::from_str(&stored.body).expect("compact body json");
    assert_eq!(body["session_id"], json!(session_id));
    assert_eq!(body["note"], json!("mid-session handoff"));
    let open_todos = body["open_todos"].as_array().expect("open_todos");
    assert_eq!(open_todos.len(), 1);
    assert_eq!(open_todos[0]["subject"], json!("finish the migration"));
}

#[tokio::test]
async fn session_compact_rejects_secret_like_note() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let db = std::sync::Arc::new(crate::Database::open(&tmp.path().join("webcodex.db")).unwrap());
    let mut runtime = test_runtime().with_memory_database(db);
    isolate_session_state(&mut runtime, state.path());
    let project =
        register_runner_project_at_path(&runtime, "sess-compact-secret", "agent-proj", tmp.path())
            .await;
    let started = runtime
        .dispatch(session_start_call(Some(project), None))
        .await;
    assert!(started.success, "{:?}", started.error);
    let session_id = started.output["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    let compacted = runtime
        .dispatch(ToolCall::Session(SessionToolCall::Compact {
            session_id,
            note: Some("token=ghp_16C7e42F292c6912E7710c838347Ae178B4a".to_string()),
        }))
        .await;
    assert!(!compacted.success);
    assert_eq!(
        compacted.output["error_kind"],
        json!("secret_like_input_rejected")
    );
}

#[cfg(not(feature = "workspace-checkpoints"))]
#[tokio::test]
async fn session_restore_without_feature_fails_closed() {
    let mut runtime = test_runtime();
    let state = tempfile::tempdir().unwrap();
    isolate_session_state(&mut runtime, state.path());
    let started = runtime.dispatch(session_start_call(None, None)).await;
    assert!(started.success, "{:?}", started.error);
    let session_id = started.output["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    let restored = runtime
        .dispatch(ToolCall::Session(SessionToolCall::Restore {
            session_id,
            project: None,
            checkpoint_id: "wc_ckpt_x".to_string(),
            confirm: true,
        }))
        .await;
    assert!(!restored.success);
    assert_eq!(
        restored.output["error_kind"],
        json!("workspace_checkpoints_unavailable")
    );
}

#[cfg(feature = "workspace-checkpoints")]
#[tokio::test]
async fn session_restore_delegates_to_checkpoint_restore_with_confirm() {
    let mut runtime = test_runtime();
    let state = tempfile::tempdir().unwrap();
    isolate_session_state(&mut runtime, state.path());
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    init_git_repo(root);
    commit_file(root, "a.txt", "base\n", "base commit");
    std::fs::write(root.join("a.txt"), "checkpoint\n").unwrap();
    let project =
        register_runner_project_at_path(&runtime, "sess-restore", "agent-proj", root).await;

    let bootstrap = auth_context(None, true);
    let started = runtime
        .dispatch_with_auth(
            session_start_call(Some(project.clone()), None),
            Some(&bootstrap),
        )
        .await;
    assert!(started.success, "{:?}", started.error);
    let session_id = started.output["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    let created = dispatch_checkpoint_with_local_agent(
        &runtime,
        "sess-restore",
        ToolCall::Checkpoint(CheckpointToolCall::Create {
            project: project.clone(),
            title: Some("pre-change".to_string()),
            note: None,
            labels: None,
            validation: None,
        }),
    )
    .await;
    assert!(created.success, "{:?}", created.error);
    let checkpoint_id = created.output["checkpoint_id"]
        .as_str()
        .expect("checkpoint_id")
        .to_string();
    std::fs::write(root.join("a.txt"), "polluted\n").unwrap();

    // confirm=false fails before any Runner request is enqueued, so a plain
    // dispatch (same authority as the session owner) is enough.
    let unconfirmed = runtime
        .dispatch_with_auth(
            ToolCall::Session(SessionToolCall::Restore {
                session_id: session_id.clone(),
                project: None,
                checkpoint_id: checkpoint_id.clone(),
                confirm: false,
            }),
            Some(&bootstrap),
        )
        .await;
    assert!(!unconfirmed.success);
    assert_eq!(unconfirmed.output["error_kind"], json!("confirm_required"));
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "polluted\n"
    );

    let restored = dispatch_checkpoint_with_local_agent(
        &runtime,
        "sess-restore",
        ToolCall::Session(SessionToolCall::Restore {
            session_id,
            project: None,
            checkpoint_id,
            confirm: true,
        }),
    )
    .await;
    assert!(restored.success, "{:?}", restored.error);
    assert_eq!(restored.output["action"], json!("restore"));
    assert_eq!(restored.output["restored"], json!(true));
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "checkpoint\n"
    );
}

#[cfg(feature = "workspace-checkpoints")]
#[tokio::test]
async fn checkpoint_consolidated_create_list_show_delete_roundtrip() {
    let runtime = test_runtime();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    init_git_repo(root);
    commit_file(root, "a.txt", "base\n", "base commit");
    let project =
        register_runner_project_at_path(&runtime, "ckpt-consolidated", "agent-proj", root).await;

    let created = dispatch_checkpoint_with_local_agent(
        &runtime,
        "ckpt-consolidated",
        ToolCall::Checkpoint(CheckpointToolCall::Create {
            project: project.clone(),
            title: Some("consolidated".to_string()),
            note: None,
            labels: None,
            validation: None,
        }),
    )
    .await;
    assert!(created.success, "{:?}", created.error);
    let checkpoint_id = created.output["checkpoint_id"]
        .as_str()
        .expect("checkpoint_id")
        .to_string();

    let listed = runtime
        .dispatch(ToolCall::Checkpoint(CheckpointToolCall::List {
            project: project.clone(),
            limit: None,
        }))
        .await;
    assert!(listed.success);
    assert_eq!(
        listed.output["checkpoints"][0]["checkpoint_id"],
        json!(checkpoint_id)
    );

    let shown = runtime
        .dispatch(ToolCall::Checkpoint(CheckpointToolCall::Show {
            project: project.clone(),
            checkpoint_id: checkpoint_id.clone(),
        }))
        .await;
    assert!(shown.success);
    assert_eq!(shown.output["checkpoint_id"], json!(checkpoint_id));

    let unconfirmed = runtime
        .dispatch(ToolCall::Checkpoint(CheckpointToolCall::Delete {
            project: project.clone(),
            checkpoint_id: checkpoint_id.clone(),
            confirm: false,
        }))
        .await;
    assert!(!unconfirmed.success);
    assert_eq!(unconfirmed.output["error_kind"], json!("confirm_required"));

    let deleted = runtime
        .dispatch(ToolCall::Checkpoint(CheckpointToolCall::Delete {
            project: project.clone(),
            checkpoint_id: checkpoint_id.clone(),
            confirm: true,
        }))
        .await;
    assert!(deleted.success, "{:?}", deleted.error);
    assert_eq!(deleted.output["deleted"], json!(true));
}
