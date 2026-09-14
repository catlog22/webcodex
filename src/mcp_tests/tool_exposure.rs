//! Operator tool exposure applies to the MCP tools/list surface.

use super::*;

async fn handle_tools_list(
    runtime: &ToolRuntime,
    auth: Option<&crate::auth::AuthContext>,
) -> Value {
    let request = rpc("tools/list", Some(json!(9201)), mcp_2026_params(json!({})));
    let protocol_era = super::super::inferred_protocol_era(&request);
    let window = crate::client_window::stateless_mcp_window(&request.params);
    let outcome = super::super::handle_mcp_request_with_lifecycle(
        runtime,
        None,
        request,
        auth,
        protocol_era,
        super::super::HostFileImportTrust::Untrusted,
        window.identity.as_ref(),
        None,
        None,
        crate::model_surface::effective_mcp_compact_schemas(
            runtime.runtime_exposure(),
            crate::config::mcp_compact_schemas_override(),
        ),
        false,
        None,
    )
    .await;
    let McpOutcome::Ok(value) = outcome else {
        panic!("expected tools/list Ok outcome, got {outcome:?}")
    };
    value["result"].clone()
}

fn listed_names(result: &Value) -> Vec<String> {
    result["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or("").to_string())
        .collect()
}

#[tokio::test]
async fn mcp_tools_list_hides_operator_disabled_tools() {
    let runtime = test_runtime_with_surface(ModelSurface::FullOperatorRuntime).with_tool_exposure(
        crate::tool_runtime::ToolExposure::from_env_vars(None, Some("cargo_test")).unwrap(),
    );
    let names = listed_names(&handle_tools_list(&runtime, None).await);
    assert!(!names.is_empty());
    assert!(!names.iter().any(|name| name == "cargo_test"), "{names:?}");
    assert!(names.iter().any(|name| name == "cargo_check"));
}

#[tokio::test]
async fn mcp_tools_list_honors_enabled_allow_list() {
    let runtime = test_runtime_with_surface(ModelSurface::FullOperatorRuntime).with_tool_exposure(
        crate::tool_runtime::ToolExposure::from_env_vars(Some("session, todo, list_tools"), None)
            .unwrap(),
    );
    let mut names = listed_names(&handle_tools_list(&runtime, None).await);
    names.sort();
    assert_eq!(
        names,
        vec![
            "list_tools".to_string(),
            "session".to_string(),
            "todo".to_string()
        ],
        "allow-listed surface must be exactly the named tools"
    );
}

#[tokio::test]
async fn unrestricted_exposure_keeps_full_surface() {
    let runtime = test_runtime_with_surface(ModelSurface::FullOperatorRuntime);
    let names = listed_names(&handle_tools_list(&runtime, None).await);
    assert!(names.iter().any(|name| name == "cargo_test"));
}
