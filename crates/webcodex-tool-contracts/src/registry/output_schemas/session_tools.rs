use serde_json::Value;

use super::common::{
    array_schema, nullable_schema, open_object_schema, schema_type, wrapped_output_schema,
};

fn revision_field() -> (&'static str, Value) {
    (
        "revision",
        schema_type(
            "integer",
            "Current record revision; pass as expected_revision on the next mutation.",
        ),
    )
}

pub(super) fn output_schema_for_tool(name: &str) -> Option<Value> {
    match name {
        "session" => Some(wrapped_output_schema(vec![
            ("success", schema_type("boolean", "True when the action committed.")),
            (
                "action",
                schema_type("string", "Echoed session action that produced this result."),
            ),
            (
                "session_id",
                nullable_schema("string", "Exact wc_sess_* session id, when applicable."),
            ),
            (
                "project",
                nullable_schema("string", "Canonical resolved project, when applicable."),
            ),
            (
                "projects",
                array_schema(
                    schema_type("string", "Resolved attached project id."),
                    "Ordered attached workspace project ids, including the canonical project first.",
                ),
            ),
            (
                "session",
                open_object_schema("Session record payload for get/list/start results."),
            ),
            (
                "sessions",
                array_schema(
                    open_object_schema("Bounded session record."),
                    "Bounded session list.",
                ),
            ),
            (
                "summary",
                open_object_schema("Bounded structured session ledger summary."),
            ),
            (
                "compact",
                open_object_schema(
                    "Compaction result: distilled summary plus the Memory key reference where the output was deposited.",
                ),
            ),
            (
                "checkpoint_id",
                nullable_schema("string", "Exact wc_ckpt_* checkpoint id, when applicable."),
            ),
            ("revision", revision_field().1),
        ])),
        "todo" => Some(wrapped_output_schema(vec![
            ("success", schema_type("boolean", "True when the action committed.")),
            (
                "action",
                schema_type("string", "Echoed todo action that produced this result."),
            ),
            (
                "todo",
                open_object_schema("Exact todo record after the action."),
            ),
            (
                "todos",
                array_schema(open_object_schema("Bounded todo record."), "Bounded todo list."),
            ),
            ("revision", revision_field().1),
        ])),
        "checkpoint" => Some(wrapped_output_schema(vec![
            ("success", schema_type("boolean", "True when the action committed.")),
            (
                "action",
                schema_type("string", "Echoed checkpoint action that produced this result."),
            ),
            (
                "checkpoint",
                open_object_schema("Exact checkpoint record, when applicable."),
            ),
            (
                "checkpoints",
                array_schema(
                    open_object_schema("Bounded checkpoint record."),
                    "Bounded checkpoint list.",
                ),
            ),
            (
                "checkpoint_id",
                nullable_schema("string", "Exact wc_ckpt_* checkpoint id, when applicable."),
            ),
        ])),
        _ => None,
    }
}
