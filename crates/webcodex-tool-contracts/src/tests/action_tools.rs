//! Contract tests for the consolidated `lsp` / `validate` / `goal` tools.

use super::*;

fn action_enum(schema: &Value) -> Vec<String> {
    schema["properties"]["action"]["enum"]
        .as_array()
        .expect("action enum")
        .iter()
        .map(|action| action.as_str().expect("string").to_string())
        .collect()
}

fn spec_input_schema(name: &str) -> Value {
    let specs = registered_tool_specs();
    spec_named(&specs, name).input_schema.clone()
}

#[test]
fn lsp_actions_cover_navigation_surface() {
    assert_eq!(
        action_enum(&spec_input_schema("lsp")),
        [
            "status",
            "document_symbols",
            "document_diagnostics",
            "hover",
            "workspace_symbols",
            "goto_definition",
            "find_references",
            "call_hierarchy",
        ]
    );
}

#[test]
fn validate_actions_cover_validation_surface() {
    assert_eq!(
        action_enum(&spec_input_schema("validate")),
        [
            "cargo_fmt",
            "cargo_check",
            "cargo_test",
            "go_test",
            "summary"
        ]
    );
}

#[test]
fn goal_actions_cover_durable_goal_surface() {
    assert_eq!(
        action_enum(&spec_input_schema("goal")),
        [
            "create",
            "get",
            "present_plan",
            "list",
            "update",
            "associate_agent_task",
            "associate_workflow_session",
        ]
    );
}

#[test]
fn action_tools_share_flat_object_invariants() {
    for name in ["lsp", "validate", "goal"] {
        let schema = spec_input_schema(name);
        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["additionalProperties"], json!(false));
        assert!(schema["properties"]["action"].is_object(), "{name} action");
        for hidden in [
            "deny_write_tools",
            "deny_shell_tools",
            "temporary_project_name",
            "detail",
            "resume_session_id",
            "bind_current",
            "new_session",
        ] {
            assert!(
                schema["properties"].get(hidden).is_none(),
                "{name} must not expose hidden start-only field {hidden}"
            );
        }
    }
}

#[test]
fn goal_actions_expose_revision_cas_and_canonical_ids() {
    let schema = spec_input_schema("goal");
    let properties = schema["properties"].as_object().expect("properties");
    assert!(properties.contains_key("expected_revision"));
    assert!(properties.contains_key("idempotency_key"));
    for canonical in ["goal_id", "task_id", "session_id"] {
        assert_eq!(
            properties[canonical]["type"],
            json!("string"),
            "{canonical} canonical id"
        );
        assert!(properties[canonical]["pattern"].is_string(), "{canonical}");
    }
}

#[test]
fn action_tools_expose_output_envelopes() {
    for name in ["lsp", "validate", "goal"] {
        let output = output_schema_for_tool(name);
        assert_eq!(output["type"], json!("object"));
        assert_eq!(output["properties"]["success"]["type"], json!("boolean"));
        assert!(
            output["properties"]["output"]["properties"]["action"].is_object(),
            "{name} action"
        );
    }
}
