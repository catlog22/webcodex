use serde_json::{json, Value};

use super::sessions::session_execution_context_schema;

const SESSION_ID_PATTERN: &str = "^wc_sess_[A-Za-z0-9_]+$";
const TODO_ID_PATTERN: &str = "^wc_todo_[A-Za-z0-9_]+$";
const MAX_ATTACHED_PROJECTS: usize = 8;

fn session_id_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "pattern": SESSION_ID_PATTERN,
        "description": description,
    })
}

fn todo_id_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "pattern": TODO_ID_PATTERN,
        "description": description,
    })
}

fn checkpoint_id_schema(description: &str) -> Value {
    // Keep this as a plain string: the flattened ToolCallRequest field must
    // stay one semantic variant shared with other visible checkpoint owners.
    json!({
        "type": "string",
        "description": description,
    })
}

fn revision_schema(description: &str) -> Value {
    json!({
        "type": "integer",
        "minimum": 0,
        "description": description,
    })
}

fn project_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "description": description,
    })
}

fn labels_schema(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "string",
            "maxLength": 64,
            "pattern": "^[A-Za-z0-9._-]+$",
        },
        "maxItems": 20,
        "description": description,
    })
}

fn validation_schema(description: &str) -> Value {
    json!({
        "type": "object",
        "description": description,
        "additionalProperties": false,
        "properties": {
            "status": {"type": "string", "enum": ["unknown", "passed", "failed"]},
            "commands": {
                "type": "array",
                "items": {"type": "string", "maxLength": 200},
                "maxItems": 20,
                "description": "Command summaries supplied by the caller; stdout/stderr and env values are not stored.",
            },
            "summary": {"anyOf": [{"type": "string"}, {"type": "null"}], "maxLength": 500},
        },
        "required": [],
    })
}

/// Consolidated Workflow Session control surface. Per-action required fields
/// are enforced by the runtime after explicit project/session resolution;
/// the schema keeps the flat object invariant shared by every runtime tool.
pub fn session_action_input_schema() -> Value {
    let restore_action: Vec<Value> = {
        let mut actions: Vec<Value> = vec![
            json!("start"),
            json!("get"),
            json!("list"),
            json!("attach_project"),
            json!("detach_project"),
            json!("update_context"),
            json!("compact"),
        ];
        #[cfg(feature = "workspace-checkpoints")]
        actions.push(json!("restore"));
        actions.push(json!("summary"));
        actions.push(json!("close"));
        actions
    };
    let mut schema = json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": restore_action,
                "description": "Session lifecycle action. start creates a Session (single project or ordered multi-workspace projects list); get/list read; attach_project/detach_project manage additional authorized workspaces; update_context replaces the execution context; compact distills session state into one Memory/knowhow entry; restore re-applies an exact checkpoint; summary reads the bounded ledger; close ends the Session."
            },
            "project": project_schema("Complete runtime project id or unambiguous project input, independently resolved and authorized on every call. Optional for start (omission creates a project-less Session; mutually exclusive with projects); required for attach_project, detach_project, and update_context; optional for restore (checkpoint scope) and list (filter)."),
            "projects": {
                "type": "array",
                "items": {"type": "string", "minLength": 1},
                "minItems": 1,
                "maxItems": MAX_ATTACHED_PROJECTS,
                "uniqueItems": true,
                "description": "Ordered multi-workspace project inputs for start only; mutually exclusive with project. The first entry becomes the canonical Session project; every entry is independently resolved and authorized."
            },
            "session_id": session_id_schema("Required wc_sess_* Workflow Session id for get, attach_project, detach_project, update_context, compact, restore, summary, and close."),
            "title": {"type": "string", "description": "Optional session title for start."},
            "session_mode": {"type": "string", "enum": ["collaborate", "solo"], "description": "Optional session mode for start; omission uses the runtime default. Named session_mode to stay clear of the flattened worktree/checkout mode owned by project checkout tools."},
            "execution_context": session_execution_context_schema("Complete replacement execution context for start or update_context. `{}` clears all defaults. Cannot store credentials, SSH configuration, keys, or arbitrary options."),
            "expected_revision": revision_schema("CAS guard required for attach_project and detach_project; taken from the latest session read."),
            "note": {"type": "string", "description": "Optional operator note included verbatim in the compacted summary (compact); the runtime enforces the 2000-char bound."},
            "limit": {"type": "integer", "minimum": 1, "description": "Optional bound for list and summary."},
        },
        "required": ["action"],
        "additionalProperties": false,
    });
    #[cfg(feature = "workspace-checkpoints")]
    {
        let properties = schema["properties"]
            .as_object_mut()
            .expect("session schema properties");
        properties.insert(
            "checkpoint_id".to_string(),
            checkpoint_id_schema("Required exact wc_ckpt_* id for restore; must belong to the Session project or an attached project."),
        );
        properties.insert(
            "confirm".to_string(),
            json!({"type": "boolean", "description": "Required true for restore (runtime-enforced); restore rewrites workspace state."}),
        );
    }
    schema
}

/// Session-scoped todo records with revision CAS. Todos are first-class
/// Workflow Session state, distinct from free-form collaboration messages.
pub fn todo_action_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["create", "list", "get", "claim", "release", "advance", "complete"],
                "description": "Todo state-machine action. create adds a todo; list/get read; claim moves open -> in_progress with an assignee; release moves in_progress -> open; advance moves to blocked or cancelled with a note; complete resolves the todo."
            },
            "session_id": session_id_schema("Required wc_sess_* Workflow Session id for every action."),
            "todo_id": todo_id_schema("Required exact wc_todo_* id for get, claim, release, advance, and complete."),
            "subject": {"type": "string", "minLength": 1, "maxLength": 4096, "description": "Required short todo subject for create."},
            "description": {"type": "string", "maxLength": 65536, "description": "Optional longer description for create."},
            "depends_on": {"type": "array", "items": {"type": "string", "pattern": TODO_ID_PATTERN}, "maxItems": 64, "uniqueItems": true, "description": "Optional todo ids that must complete first (create)."},
            "assignee": {"type": "string", "minLength": 1, "maxLength": 256, "description": "Required claiming member identity for claim."},
            "status": {"type": "string", "enum": ["blocked", "cancelled"], "description": "Required target status for advance."},
            "resolution": {"type": "string", "maxLength": 4096, "description": "Optional completion note for complete."},
            "note": {"type": "string", "description": "Optional reason recorded on the todo for advance; the runtime enforces the 4096-char bound."},
            "expected_revision": revision_schema("Required CAS guard for claim, release, advance, and complete; taken from the latest todo read."),
        },
        "required": ["action"],
        "additionalProperties": false,
    })
}

/// Unified workspace checkpoint control. restore and delete rewrite workspace
/// state and require confirm=true.
pub fn checkpoint_action_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["create", "list", "show", "restore", "delete"],
                "description": "Checkpoint action. create snapshots the project workspace; list/show read; restore rewrites workspace state from an exact checkpoint; delete removes one. restore and delete require confirm=true."
            },
            "project": project_schema("Required complete runtime project id or unambiguous project input, independently resolved and authorized on every call."),
            "checkpoint_id": checkpoint_id_schema("Required exact wc_ckpt_* id for show, restore, and delete."),
            "confirm": {"type": "boolean", "description": "Required true for restore and delete (runtime-enforced)."},
            "title": {"type": "string", "description": "Optional human-readable title for create."},
            "note": {"type": "string", "description": "Optional note for create; not used by restore."},
            "labels": labels_schema("Optional checkpoint labels for create."),
            "validation": validation_schema("Optional caller-supplied validation metadata for create; recorded only, never executed."),
            "limit": {"type": "integer", "minimum": 1, "description": "Optional bound for list."},
        },
        "required": ["action"],
        "additionalProperties": false,
    })
}
