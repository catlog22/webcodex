//! Integration tests for the consolidated `lsp` / `validate` / `goal` tools.

use super::support::*;
use crate::tool_runtime::{GoalToolCall, LspToolCall, ToolCall, ToolRuntime, ValidateToolCall};
use serde_json::json;

fn goal_runtime() -> (tempfile::TempDir, ToolRuntime) {
    let temp = tempfile::tempdir().unwrap();
    let db = std::sync::Arc::new(crate::db::Database::open(&temp.path().join("goals.db")).unwrap());
    let runtime = ToolRuntime::new_for_tests().with_communication_database(db);
    (temp, runtime)
}

#[tokio::test]
async fn lsp_status_unknown_project_fails_closed() {
    let runtime = ToolRuntime::new_for_tests();
    let result = runtime
        .dispatch(ToolCall::Lsp(LspToolCall::Status {
            project: "agent:oe:missing".to_string(),
            session_id: None,
        }))
        .await;
    assert!(!result.success, "{:?}", result.output);
    assert!(result.error.is_some_and(|error| !error.is_empty()));
}

#[tokio::test]
async fn lsp_workspace_symbols_dispatch_reaches_project_resolution() {
    let runtime = ToolRuntime::new_for_tests();
    let result = runtime
        .dispatch(ToolCall::Lsp(LspToolCall::WorkspaceSymbols {
            project: "agent:oe:missing".to_string(),
            query: "ToolRuntime".to_string(),
            limit: None,
            session_id: None,
        }))
        .await;
    assert!(!result.success, "{:?}", result.output);
    assert!(result.error.is_some_and(|error| !error.is_empty()));
}

#[tokio::test]
async fn validate_cargo_check_unknown_project_fails_closed() {
    let runtime = ToolRuntime::new_for_tests();
    let result = runtime
        .dispatch(ToolCall::Validate(ValidateToolCall::CargoCheck {
            project: "agent:oe:missing".to_string(),
            session_id: None,
            cwd: None,
            all_targets: None,
            all_features: None,
            no_default_features: None,
            features: None,
            package: None,
            timeout_secs: None,
            sync_wait_secs: None,
        }))
        .await;
    assert!(!result.success, "{:?}", result.output);
    assert!(result.error.is_some_and(|error| !error.is_empty()));
}

#[tokio::test]
async fn validate_summary_unknown_session_fails_closed() {
    let runtime = ToolRuntime::new_for_tests();
    let result = runtime
        .dispatch(ToolCall::Validate(ValidateToolCall::Summary {
            project: "agent:oe:any".to_string(),
            session_id: format!("wc_sess_{}", "9".repeat(32)),
            limit: None,
        }))
        .await;
    assert!(!result.success, "{:?}", result.output);
    assert!(result.error.is_some_and(|error| !error.is_empty()));
}

#[test]
fn validate_rejects_sync_wait_secs_without_executing_action_at_parse() {
    // summary never executes validation, so sync_wait_secs is meaningless there.
    let err = ToolCall::from_tool_name(
        "validate",
        json!({
            "action": "summary",
            "project": "agent:oe:a",
            "session_id": format!("wc_sess_{}", "1".repeat(32)),
            "sync_wait_secs": 5
        }),
    )
    .unwrap_err();
    assert!(err.contains("sync_wait_secs"), "{err}");
}

#[test]
fn validate_rejects_result_expectation_on_summary_at_parse() {
    let err = ToolCall::from_tool_name(
        "validate",
        json!({
            "action": "summary",
            "project": "agent:oe:a",
            "session_id": format!("wc_sess_{}", "1".repeat(32)),
            "result_expectation": "failure"
        }),
    )
    .unwrap_err();
    assert!(
        err.contains("result expectation") || err.contains("result_expectation"),
        "{err}"
    );
}

#[test]
fn validate_cargo_fmt_sync_wait_still_requires_check_true() {
    let err = ToolCall::from_tool_name(
        "validate",
        json!({"action": "cargo_fmt", "project": "agent:oe:a", "sync_wait_secs": 5}),
    )
    .unwrap_err();
    assert!(err.contains("check=true"), "{err}");
    let parsed = ToolCall::from_tool_name(
        "validate",
        json!({
            "action": "cargo_fmt",
            "project": "agent:oe:a",
            "check": true,
            "sync_wait_secs": 5,
            "result_expectation": "observe"
        }),
    )
    .unwrap();
    assert!(matches!(
        parsed,
        ToolCall::Validate(ValidateToolCall::CargoFmt { .. })
    ));
}

#[tokio::test]
async fn goal_create_get_update_present_plan_and_list_roundtrip() {
    let (_temp, runtime) = goal_runtime();
    let bob = auth_context(Some("bob-consolidated"), false);

    let created = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::Create {
                title: "Consolidated goal control".to_string(),
                objective: "One action tool drives the durable Goal lifecycle.".to_string(),
                idempotency_key: "bob-consolidated-create".to_string(),
            }),
            Some(&bob),
        )
        .await;
    assert!(created.success, "{:?}", created.output);
    let goal_id = created.output["goal"]["summary"]["goal_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(created.output["goal"]["summary"]["revision"], 1);

    // Exact retry replays instead of creating a second Goal.
    let replay = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::Create {
                title: "Consolidated goal control".to_string(),
                objective: "One action tool drives the durable Goal lifecycle.".to_string(),
                idempotency_key: "bob-consolidated-create".to_string(),
            }),
            Some(&bob),
        )
        .await;
    assert!(replay.success, "{:?}", replay.output);
    assert_eq!(replay.output["replayed"], true);
    assert_eq!(replay.output["goal"]["summary"]["goal_id"], goal_id);

    let got = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::Get {
                goal_id: goal_id.clone(),
            }),
            Some(&bob),
        )
        .await;
    assert!(got.success, "{:?}", got.output);
    assert_eq!(
        got.output["goal"]["summary"]["title"],
        "Consolidated goal control"
    );

    let stale = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::Update {
                goal_id: goal_id.clone(),
                expected_revision: 7,
                title: None,
                objective: None,
                lifecycle: None,
                terminal_reason: None,
                idempotency_key: "bob-consolidated-stale".to_string(),
            }),
            Some(&bob),
        )
        .await;
    assert!(!stale.success, "{:?}", stale.output);

    let updated = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::Update {
                goal_id: goal_id.clone(),
                expected_revision: 1,
                title: Some("Consolidated goal control v2".to_string()),
                objective: None,
                lifecycle: None,
                terminal_reason: None,
                idempotency_key: "bob-consolidated-update".to_string(),
            }),
            Some(&bob),
        )
        .await;
    assert!(updated.success, "{:?}", updated.output);
    assert_eq!(updated.output["goal"]["summary"]["revision"], 2);
    assert_eq!(
        updated.output["goal"]["summary"]["title"],
        "Consolidated goal control v2"
    );

    let plan = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::PresentPlan {
                goal_id: goal_id.clone(),
            }),
            Some(&bob),
        )
        .await;
    assert!(plan.success, "{:?}", plan.output);
    assert_eq!(plan.output["goal_plan"]["goal_id"], goal_id);
    assert_eq!(plan.output["goal_plan"]["revision"], 2);

    let listed = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::List {
                lifecycle: None,
                offset: None,
                limit: None,
            }),
            Some(&bob),
        )
        .await;
    assert!(listed.success, "{:?}", listed.output);
    let ids: Vec<&str> = listed.output["goals"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|goal| goal["goal_id"].as_str())
        .collect();
    assert!(ids.contains(&goal_id.as_str()), "{ids:?}");
}

#[tokio::test]
async fn goal_get_is_existence_hidden_across_principals() {
    let (_temp, runtime) = goal_runtime();
    let bob = auth_context(Some("bob-hidden"), false);
    let alice = auth_context(Some("alice-hidden"), false);
    let created = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::Create {
                title: "Private durable intent".to_string(),
                objective: "Only the owner principal may observe this Goal.".to_string(),
                idempotency_key: "bob-hidden-create".to_string(),
            }),
            Some(&bob),
        )
        .await;
    let goal_id = created.output["goal"]["summary"]["goal_id"]
        .as_str()
        .unwrap()
        .to_string();
    let foreign = runtime
        .dispatch_with_auth(ToolCall::Goal(GoalToolCall::Get { goal_id }), Some(&alice))
        .await;
    assert!(!foreign.success, "{:?}", foreign.output);
}

#[tokio::test]
async fn goal_associate_unknown_agent_task_fails_closed() {
    let (_temp, runtime) = goal_runtime();
    let bob = auth_context(Some("bob-associate"), false);
    let created = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::Create {
                title: "Correlate work".to_string(),
                objective: "Association is correlation only.".to_string(),
                idempotency_key: "bob-associate-create".to_string(),
            }),
            Some(&bob),
        )
        .await;
    let goal_id = created.output["goal"]["summary"]["goal_id"]
        .as_str()
        .unwrap()
        .to_string();
    let associated = runtime
        .dispatch_with_auth(
            ToolCall::Goal(GoalToolCall::AssociateAgentTask {
                goal_id,
                task_id: format!("wc_agent_task_{}", "f".repeat(32)),
                idempotency_key: "bob-associate-task".to_string(),
            }),
            Some(&bob),
        )
        .await;
    assert!(!associated.success, "{:?}", associated.output);
}
