//! Output envelopes for the consolidated `lsp` / `validate` / `goal` tools.

use serde_json::Value;

use super::common::{
    array_schema, nullable_schema, open_object_schema, schema_type, wrapped_output_schema,
};

pub(super) fn output_schema_for_tool(name: &str) -> Option<Value> {
    match name {
        "lsp" => Some(wrapped_output_schema(vec![
            (
                "success",
                schema_type("boolean", "True when the action completed."),
            ),
            (
                "action",
                schema_type("string", "Echoed lsp action that produced this result."),
            ),
            (
                "status",
                nullable_schema("string", "Server status summary for the status action."),
            ),
            (
                "symbols",
                array_schema(
                    open_object_schema("Bounded symbol node."),
                    "Symbol nodes for document_symbols/workspace_symbols results.",
                ),
            ),
            (
                "diagnostics",
                array_schema(
                    open_object_schema("Normalized diagnostic."),
                    "Normalized diagnostics for the document_diagnostics action.",
                ),
            ),
            (
                "hover",
                nullable_schema("string", "Hover text, when applicable."),
            ),
            (
                "locations",
                array_schema(
                    open_object_schema("Bounded source location."),
                    "Locations for goto_definition/find_references results.",
                ),
            ),
            (
                "calls",
                open_object_schema("Call hierarchy result for the call_hierarchy action."),
            ),
            (
                "server",
                nullable_schema("string", "Resolved LSP server identity, when reported."),
            ),
        ])),
        "validate" => Some(wrapped_output_schema(vec![
            (
                "success",
                schema_type("boolean", "True when the action completed."),
            ),
            (
                "action",
                schema_type(
                    "string",
                    "Echoed validate action that produced this result.",
                ),
            ),
            (
                "job_id",
                nullable_schema(
                    "string",
                    "Same-execution Job id for long validations handed off to observation.",
                ),
            ),
            (
                "summary",
                open_object_schema("Validation outcome summary for the executed validation."),
            ),
            (
                "events",
                array_schema(
                    open_object_schema("Bounded validation history event."),
                    "Validation history events for the summary action.",
                ),
            ),
        ])),
        "goal" => Some(wrapped_output_schema(vec![
            (
                "success",
                schema_type("boolean", "True when the action committed."),
            ),
            (
                "action",
                schema_type("string", "Echoed goal action that produced this result."),
            ),
            (
                "goal_id",
                nullable_schema("string", "Exact wc_goal_* Goal id, when applicable."),
            ),
            (
                "goal",
                open_object_schema("Goal record payload for create/get/update results."),
            ),
            (
                "goals",
                array_schema(
                    open_object_schema("Bounded goal summary."),
                    "Goal summaries for the list action.",
                ),
            ),
            (
                "goal_plan",
                open_object_schema("Sparse read-only Goal Plan card for the present_plan action."),
            ),
            (
                "revision",
                schema_type(
                    "integer",
                    "Current Goal revision; pass as expected_revision on the next mutation.",
                ),
            ),
        ])),
        _ => None,
    }
}
