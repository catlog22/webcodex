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
fn session_actions_cover_lifecycle() {
    let mut expected = vec![
        "start",
        "get",
        "list",
        "attach_project",
        "detach_project",
        "update_context",
        "compact",
    ];
    #[cfg(feature = "workspace-checkpoints")]
    expected.push("restore");
    expected.push("summary");
    expected.push("close");
    assert_eq!(action_enum(&spec_input_schema("session")), expected);
}

#[test]
fn session_start_accepts_bounded_multi_workspace_list() {
    let projects = &spec_input_schema("session")["properties"]["projects"];
    assert_eq!(projects["maxItems"], json!(8));
    assert_eq!(projects["minItems"], json!(1));
}

#[test]
fn todo_actions_cover_state_machine() {
    assert_eq!(
        action_enum(&spec_input_schema("todo")),
        ["create", "list", "get", "claim", "release", "advance", "complete",]
    );
}

#[test]
fn todo_and_session_mutations_expose_revision_cas_field() {
    for name in ["todo", "session"] {
        let schema = spec_input_schema(name);
        let properties = schema["properties"].as_object().expect("properties");
        assert!(
            properties.contains_key("expected_revision"),
            "{name} must expose expected_revision"
        );
    }
}

#[cfg(feature = "workspace-checkpoints")]
#[test]
fn checkpoint_actions_cover_crud() {
    assert_eq!(
        action_enum(&spec_input_schema("checkpoint")),
        ["create", "list", "show", "restore", "delete"]
    );
}

#[cfg(feature = "workspace-checkpoints")]
#[test]
fn checkpoint_restore_and_delete_require_confirm_true() {
    let confirm = &spec_input_schema("checkpoint")["properties"]["confirm"];
    assert_eq!(confirm["type"], json!("boolean"));
    assert!(confirm["description"]
        .as_str()
        .expect("confirm description")
        .contains("true"));
}

#[cfg(feature = "workspace-checkpoints")]
#[test]
fn action_outputs_registered() {
    for name in ["session", "todo", "checkpoint"] {
        let schema = output_schema_for_tool(name);
        assert_eq!(schema["type"], json!("object"));
        assert!(schema["properties"].get("success").is_some());
    }
}
