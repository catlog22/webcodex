//! Integration tests for operator-controlled tool exposure.

use super::support::*;
use crate::tool_runtime::{ListToolsOptions, ToolCall, ToolExposure, ToolRuntime};
use serde_json::json;

fn exposure_enabled_only(names: &[&str]) -> ToolExposure {
    ToolExposure::from_env_vars(Some(&names.join(",")), None).unwrap()
}

fn exposure_disabled(names: &[&str]) -> ToolExposure {
    ToolExposure::from_env_vars(None, Some(&names.join(","))).unwrap()
}

#[tokio::test]
async fn disabled_tool_fails_closed_at_dispatch() {
    let runtime =
        ToolRuntime::new_for_tests().with_tool_exposure(exposure_disabled(&["read_files"]));
    let result = runtime
        .dispatch(
            ToolCall::from_tool_name(
                "read_files",
                json!({"project": "agent:oe:a", "items": [{"path": "src/lib.rs"}]}),
            )
            .unwrap(),
        )
        .await;
    assert!(!result.success, "{:?}", result.output);
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("read_files") && error.contains("disabled")),
        "{:?}",
        result.error
    );
}

#[tokio::test]
async fn enabled_allow_list_narrows_dispatch_surface() {
    let runtime =
        ToolRuntime::new_for_tests().with_tool_exposure(exposure_enabled_only(&["list_tools"]));
    // Allowed tool keeps working.
    let allowed = runtime
        .dispatch(ToolCall::from_tool_name("list_tools", json!({})).unwrap())
        .await;
    assert!(allowed.success, "{:?}", allowed.output);
    // Anything outside the allow list fails closed, even valid calls.
    let denied = runtime
        .dispatch(
            ToolCall::from_tool_name(
                "read_files",
                json!({"project": "agent:oe:a", "items": [{"path": "a"}]}),
            )
            .unwrap(),
        )
        .await;
    assert!(!denied.success, "{:?}", denied.output);
}

#[tokio::test]
async fn unrestricted_default_changes_nothing() {
    let runtime = ToolRuntime::new_for_tests();
    let result = runtime
        .dispatch(ToolCall::from_tool_name("list_tools", json!({})).unwrap())
        .await;
    assert!(result.success, "{:?}", result.output);
}

#[test]
fn list_tools_payload_hides_disabled_tools() {
    let runtime =
        ToolRuntime::new_for_tests().with_tool_exposure(exposure_disabled(&["cargo_test"]));
    let payload = runtime.list_tools_payload(ListToolsOptions::default());
    let tools = payload["tools"].as_array().expect("tools array");
    assert!(!tools.is_empty());
    assert!(
        !tools.iter().any(|tool| tool["name"] == "cargo_test"),
        "cargo_test must be hidden"
    );
    assert!(tools.iter().any(|tool| tool["name"] == "cargo_check"));
}

#[tokio::test]
async fn tool_manifest_hides_disabled_and_exact_miss_is_existence_hidden() {
    let runtime =
        ToolRuntime::new_for_tests().with_tool_exposure(exposure_disabled(&["cargo_test"]));
    let payload = runtime
        .dispatch(ToolCall::from_tool_name("tool_manifest", json!({})).unwrap())
        .await;
    assert!(payload.success, "{:?}", payload.output);
    let tools = payload.output["tools"].as_array().expect("tools array");
    assert!(!tools.iter().any(|tool| tool["name"] == "cargo_test"));

    let exact = runtime
        .dispatch(
            ToolCall::from_tool_name("tool_manifest", json!({"tool_name": "cargo_test"})).unwrap(),
        )
        .await;
    assert!(!exact.success, "{:?}", exact.output);
    assert_eq!(exact.output["code"], "unknown_tool_manifest_tool");
}

#[test]
fn exposure_validation_rejects_unknown_names_and_conflicts() {
    let err = ToolExposure::from_env_vars(Some("session, nope"), None).unwrap_err();
    assert!(err.contains("nope"), "{err}");
    let err = ToolExposure::from_env_vars(None, Some("nope")).unwrap_err();
    assert!(err.contains("nope"), "{err}");
    let err = ToolExposure::from_env_vars(Some("session"), Some("session")).unwrap_err();
    assert!(err.contains("conflict"), "{err}");
}
