//! Consolidated action tools for the LSP, validation, and Goal clusters.
//!
//! Each schema is a flat object with an `action` enum plus optional
//! per-action fields, following the `session_tools` precedent. Per-action
//! required fields and per-action bounds are enforced by the runtime after
//! explicit resolution; the schema keeps the flat-object invariant shared by
//! every runtime tool.

use serde_json::{json, Value};
use webcodex_core::workflow_session_contract::TOOL_RESULT_EXPECTATION_FIELD;

const GOAL_ID_PATTERN: &str = "^wc_goal_[0-9a-f]{32}$";
const TASK_ID_PATTERN: &str = "^wc_agent_task_[0-9a-f]{32}$";
const SESSION_ID_PATTERN: &str = "^wc_sess_[0-9a-f]{32}$";

fn canonical_id(pattern: &str, description: &str) -> Value {
    json!({"type": "string", "pattern": pattern, "description": description})
}

fn idempotency_key(description: &str) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": 128,
        "description": description
    })
}

fn goal_lifecycle() -> Value {
    json!({
        "type": "string",
        "enum": ["active", "completed", "cancelled"],
        "description": "Closed authoritative Goal lifecycle. Execution/presentation states such as implementing, blocked, or waiting_validation are not Goal lifecycle values."
    })
}

/// Consolidated LSP navigation surface. All actions are pure reads scoped to
/// one project; per-action required fields and clamp bounds are enforced by
/// the runtime (mirrors the legacy lsp_* tools).
pub fn lsp_action_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": [
                    "status",
                    "document_symbols",
                    "document_diagnostics",
                    "hover",
                    "workspace_symbols",
                    "goto_definition",
                    "find_references",
                    "call_hierarchy"
                ],
                "description": "LSP action. status reports server state; document_symbols/document_diagnostics read one file; hover/goto_definition/find_references/call_hierarchy resolve one 1-based position; workspace_symbols searches by query. All actions are pure reads."
            },
            "project": {
                "type": "string",
                "minLength": 1,
                "description": "Required complete runtime project id for every action."
            },
            "path": {
                "type": "string",
                "minLength": 1,
                "description": "Project-relative UTF-8 path to a supported source file. Required for document_symbols, document_diagnostics, hover, goto_definition, find_references, and call_hierarchy."
            },
            "line": {
                "type": "integer",
                "minimum": 1,
                "description": "Required 1-based line number for hover, goto_definition, find_references, and call_hierarchy."
            },
            "column": {
                "type": "integer",
                "minimum": 1,
                "description": "Required 1-based Unicode scalar column for hover, goto_definition, find_references, and call_hierarchy."
            },
            "query": {
                "type": "string",
                "minLength": 1,
                "maxLength": 200,
                "description": "Required workspace symbol query for workspace_symbols (1..200 characters after trimming)."
            },
            "include_declaration": {
                "type": "boolean",
                "description": "Optional for find_references (default true)."
            },
            "direction": {
                "type": "string",
                "enum": ["incoming", "outgoing", "both"],
                "description": "Optional call direction for call_hierarchy (default both)."
            },
            "depth": {
                "type": "integer",
                "minimum": 1,
                "maximum": 2,
                "description": "Optional breadth-first depth for call_hierarchy (default 1, maximum 2)."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "description": "Optional bound: symbols (default 100, max 500), diagnostics (default 100, max 200), workspace_symbols (default 50, max 200), goto_definition (default 20, max 100), find_references (default 50, max 200), call_hierarchy edges (default 50, max 100). The runtime clamps per action."
            },
            "session_id": {
                "type": "string",
                "description": "Optional wc_sess_* business Session id for session-scoped recording."
            },
        },
        "required": ["action", "project"],
        "additionalProperties": false,
    })
}

/// Consolidated validation surface. cargo_fmt/cargo_check/cargo_test/go_test
/// execute validations (long runs hand off to the same-execution Job);
/// summary reads the bounded validation history of one Session. Per-action
/// defaults and clamp bounds are enforced by the runtime, matching
/// the legacy cargo_* / go_test / validation_summary tools.
pub fn validate_action_input_schema() -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["cargo_fmt", "cargo_check", "cargo_test", "go_test", "summary"],
                "description": "Validation action. cargo_fmt/cargo_check/cargo_test/go_test execute validations; summary reads the bounded validation event history for one Session."
            },
            "project": {
                "type": "string",
                "minLength": 1,
                "description": "Required complete runtime project id for every action."
            },
            "session_id": {
                "type": "string",
                "description": "Required wc_sess_* business Session id for summary; optional for the cargo/go actions (session-scoped recording)."
            },
            "cwd": {
                "type": "string",
                "description": "Optional project-relative working directory for the cargo/go actions."
            },
            "check": {
                "type": "boolean",
                "description": "cargo_fmt only: true performs pure read-only `cargo fmt -- --check`; omission/false ensures formatting (mutating `cargo fmt` only when a stable rustfmt diff is proven)."
            },
            "all_targets": {
                "type": "boolean",
                "description": "Optional for cargo_check (default true) and cargo_test."
            },
            "all_features": {
                "type": "boolean",
                "description": "Optional for cargo_check and cargo_test."
            },
            "no_default_features": {
                "type": "boolean",
                "description": "Optional for cargo_check and cargo_test."
            },
            "features": {
                "type": "string",
                "description": "Optional feature list for cargo_check and cargo_test."
            },
            "package": {
                "type": "string",
                "description": "Optional `-p` package for cargo_check and cargo_test."
            },
            "filter": {
                "type": "string",
                "description": "Optional cargo_test test-name filter."
            },
            "lib": {
                "type": "boolean",
                "description": "Optional cargo_test: when true, include Cargo's --lib target selector. Omission and false have the same ordinary target-selection semantics."
            },
            "packages": {
                "type": "array",
                "items": {"type": "string", "minLength": 1},
                "description": "Optional project-relative Go package scopes for go_test."
            },
            "no_run": {
                "type": "boolean",
                "description": "Optional cargo_test compile-only mode."
            },
            "require_tests": {
                "type": "boolean",
                "description": "Optional cargo_test executed-test proof control; omission keeps the normal non-zero proof requirement."
            },
            "min_tests": {
                "type": "integer",
                "minimum": 1,
                "maximum": webcodex_core::runner_protocol::CARGO_TEST_MIN_TESTS_MAX,
                "description": "Optional cargo_test minimum executed-test proof."
            },
            "timeout_secs": {
                "type": "integer",
                "minimum": 1,
                "description": "Total validation runtime budget in seconds (minimum 1). Per-action defaults and upper clamps (120/3600) are enforced by the runtime."
            },
            "sync_wait_secs": {
                "type": "integer",
                "minimum": 1,
                "description": "Optional bounded synchronous grace before same-execution Job handoff; never extends timeout_secs. With cargo_fmt check=false it is accepted as inert compatibility input."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "description": "Optional bound for summary (default 20; clamped to 100 by the runtime)."
            },
        },
        "required": ["action", "project"],
        "additionalProperties": false,
    });
    schema["properties"][TOOL_RESULT_EXPECTATION_FIELD] = json!({
        "type": "string",
        "enum": ["success", "failure", "observe"],
        "description": "Optional pre-execution validation-result expectation for the cargo/go actions. Omit (or use success) for the normal success-required path; failure is for negative tests that must reach a completed known failing validation result; observe accepts either. The real validation ToolResult and process outcome remain unchanged."
    });
    schema
}

/// Consolidated durable Goal surface. create/update/associate actions mutate
/// with idempotency keys (update additionally requires expected_revision CAS);
/// get/present_plan/list are reads. Mirrors the legacy goal tools.
pub fn goal_action_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": [
                    "create",
                    "get",
                    "present_plan",
                    "list",
                    "update",
                    "associate_agent_task",
                    "associate_workflow_session"
                ],
                "description": "Goal action. create registers a durable Goal; get/present_plan read one exact caller-owned Goal (present_plan renders a sparse read-only MCP App card); list pages caller-visible Goals; update mutates title/objective/lifecycle with revision CAS; associate_agent_task/associate_workflow_session record bounded correlations that never grant target-domain authority."
            },
            "goal_id": canonical_id(GOAL_ID_PATTERN, "Required canonical durable Goal id for get, present_plan, update, associate_agent_task, and associate_workflow_session. It is exact identity only and is never a bearer credential or execution selector."),
            "title": {
                "type": "string",
                "minLength": 1,
                "maxLength": 200,
                "description": "Required Goal title for create; optional replacement title for update."
            },
            "objective": {
                "type": "string",
                "minLength": 1,
                "maxLength": 8192,
                "description": "Required authoritative objective for create; optional replacement for update. The Server additionally enforces an 8192-byte UTF-8 bound."
            },
            "idempotency_key": idempotency_key("Required caller-generated key for create, update, associate_agent_task, and associate_workflow_session. Exact retry replays; changed reuse fails closed."),
            "expected_revision": {
                "type": "integer",
                "minimum": 1,
                "description": "Required exact observed Goal revision for update. Stale mutation fails closed and returns the current revision only."
            },
            "lifecycle": goal_lifecycle(),
            "terminal_reason": {
                "type": "string",
                "minLength": 1,
                "maxLength": 4096,
                "description": "Optional bounded terminal reason for update; valid only with an explicit completed or cancelled transition."
            },
            "task_id": canonical_id(TASK_ID_PATTERN, "Required exact durable AgentTask id for associate_agent_task. Association is correlation only and never grants TaskAttempt, Project, Runner, or execution authority."),
            "session_id": canonical_id(SESSION_ID_PATTERN, "Required exact Workflow Session id for associate_workflow_session. The target Session is independently re-authorized before association; the correlation never grants Session or Project authority."),
            "offset": {
                "type": "integer",
                "minimum": 0,
                "description": "Optional page offset for list (default 0)."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "Optional page size for list (default 50, maximum 100)."
            },
        },
        "required": ["action"],
        "additionalProperties": false,
    })
}
