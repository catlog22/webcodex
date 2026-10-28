use super::*;

#[tokio::test]
async fn cli_and_app_task_endpoint_shares_mcp_reads_and_completion_summaries() {
    let temp = tempfile::tempdir().unwrap();
    let db = Arc::new(crate::Database::open(&temp.path().join("tasks-console.db")).unwrap());
    let runtime = Arc::new(
        ToolRuntime::new(
            Arc::new(crate::RunnerRegistry::default()),
            Arc::new(RuntimeInfo::default()),
        )
        .with_communication_database(db.clone())
        .with_project_reference_database(db),
    );
    let auth = scoped_oauth(&[SCOPE_PROJECT_READ, crate::auth::SCOPE_PROJECT_WRITE]);
    register_project(&runtime, "tasks", "demo", "/srv/tasks", Some(&auth)).await;
    let root = format!("wc_projroot_{}", "a".repeat(64));
    let mut entry = project("demo", "/srv/tasks");
    entry.root_fingerprint = Some(root.clone());
    crate::test_support::apply_project_inventory_snapshot(
        &runtime.runner_registry,
        "tasks",
        "inst-tasks",
        vec![entry],
    )
    .await;
    let service = Service::new(
        Router::new()
            .hoop(affix_state::inject(runtime.clone()))
            .hoop(affix_state::inject(auth.clone()))
            .push(Router::with_path("tasks").post(workspace::workspace_tasks)),
    );
    let project = "agent:tasks:demo";
    let create = json!({"project":project,"operation":{"action":"create","title":"CLI task","description":"Work instructions","idempotency_key":"cli-request-1","expected_root_fingerprint":root}});
    let mut response = TestClient::post("http://localhost/tasks")
        .json(&create)
        .send(&service)
        .await;
    let created: Value = response.take_json().await.unwrap();
    assert_eq!(created["success"], true, "{created}");
    let id = created["output"]["task"]["task_id"].as_str().unwrap();
    let found = runtime
        .dispatch_with_auth(
            ToolCall::from_tool_name(
                "search_webcodex_resources",
                json!({"kind":"workspace_task","project":project}),
            )
            .unwrap(),
            Some(&auth),
        )
        .await;
    assert!(found.success);
    let uri = found.output["items"][0]["uri"].as_str().unwrap();
    let read = runtime.read_webcodex_resource(uri, Some(&auth)).await;
    assert!(read.success, "{:?} {:?}", read.error, read.output);
    assert_eq!(read.output["data"]["task"]["title"], "CLI task");
    let complete = runtime.dispatch_with_auth(ToolCall::from_tool_name("complete_workspace_task", json!({"project":project,"task_id":id,"expected_revision":1,"expected_root_fingerprint":root,"completion_summary":"Implemented and checked remotely"})).unwrap(), Some(&auth)).await;
    assert!(complete.success, "{:?}", complete.error);
    let mut response = TestClient::post("http://localhost/tasks")
        .json(&json!({"project":project,"operation":{"action":"read","task_id":id}}))
        .send(&service)
        .await;
    let shown: Value = response.take_json().await.unwrap();
    assert_eq!(shown["output"]["task"]["status"], "done");
    assert_eq!(
        shown["output"]["task"]["completion_summary"],
        "Implemented and checked remotely"
    );
    let mut response = TestClient::post("http://localhost/tasks")
        .json(&create)
        .send(&service)
        .await;
    let replay: Value = response.take_json().await.unwrap();
    assert_eq!(replay["output"]["task"]["task_id"], id);
}
