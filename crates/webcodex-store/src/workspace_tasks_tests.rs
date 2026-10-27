use crate::Database;
use webcodex_core::workspace_task::{
    WorkspaceTaskListQuery, WorkspaceTaskStatus, WorkspaceTaskUpdate,
};

#[test]
fn tasks_persist_and_fence_workspace_identity_retries_and_concurrent_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.sqlite");
    let project = "agent:runner:project";
    let task = {
        let db = Database::open(&path).unwrap();
        let task = db
            .create_workspace_task(project, "root-a", "Investigate", "Details", "request-1")
            .unwrap();
        assert_eq!(
            db.create_workspace_task(project, "root-a", "Investigate", "Details", "request-1")
                .unwrap(),
            task
        );
        assert_eq!(
            db.create_workspace_task(project, "root-a", "Changed", "Details", "request-1")
                .unwrap_err()
                .code,
            "workspace_task_idempotency_conflict"
        );
        task
    };
    let db = Database::open(&path).unwrap();
    assert_eq!(
        db.read_workspace_task(project, "root-a", &task.task_id)
            .unwrap(),
        task
    );
    for (scope, root) in [
        ("agent:other:project", "root-a"),
        (project, "replaced-root"),
    ] {
        assert_eq!(
            db.read_workspace_task(scope, root, &task.task_id)
                .unwrap_err()
                .code,
            "workspace_task_not_found"
        );
        assert!(db
            .list_workspace_tasks(scope, root, WorkspaceTaskListQuery::default())
            .unwrap()
            .tasks
            .is_empty());
    }
    let update = || WorkspaceTaskUpdate {
        task_id: task.task_id.clone(),
        expected_revision: task.revision,
        title: "Updated".into(),
        description: "Notes".into(),
        completion_summary: "Validated and finished".into(),
        status: WorkspaceTaskStatus::Done,
    };
    assert_eq!(
        db.complete_workspace_task(project, "root-a", &task.task_id, task.revision, "".into())
            .unwrap_err()
            .code,
        "workspace_task_summary_required"
    );
    let updated = db
        .update_workspace_task(project, "root-a", update())
        .unwrap();
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.status, WorkspaceTaskStatus::Done);
    assert_eq!(updated.completion_summary, "Validated and finished");
    assert_eq!(
        db.update_workspace_task(project, "root-a", update())
            .unwrap_err()
            .code,
        "workspace_task_revision_conflict"
    );
    assert!(db
        .list_workspace_tasks(
            project,
            "root-a",
            WorkspaceTaskListQuery {
                status: Some(WorkspaceTaskStatus::Open),
                ..Default::default()
            }
        )
        .unwrap()
        .tasks
        .is_empty());
}

#[test]
fn task_lists_are_bounded_and_content_is_validated_before_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join("tasks.sqlite")).unwrap();
    for index in 0..3 {
        db.create_workspace_task(
            "project",
            "root",
            &format!("Task {index}"),
            "body",
            &format!("key-{index}"),
        )
        .unwrap();
    }
    let first = db
        .list_workspace_tasks(
            "project",
            "root",
            WorkspaceTaskListQuery {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(first.tasks.len(), 2);
    let second = db
        .list_workspace_tasks(
            "project",
            "root",
            WorkspaceTaskListQuery {
                limit: Some(2),
                offset: first.next_offset.unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(second.tasks.len(), 1);
    assert!(!first
        .tasks
        .iter()
        .any(|task| task.task_id == second.tasks[0].task_id));
    assert!(second.next_offset.is_none());
    for (title, body) in [("", "body"), ("bad\nname", "body"), ("title", "bad\0body")] {
        assert_eq!(
            db.create_workspace_task("project", "root", title, body, "invalid")
                .unwrap_err()
                .code,
            "workspace_task_content_invalid"
        );
    }
    assert_eq!(
        db.list_workspace_tasks("project", "root", WorkspaceTaskListQuery::default())
            .unwrap()
            .tasks
            .len(),
        3
    );
}
