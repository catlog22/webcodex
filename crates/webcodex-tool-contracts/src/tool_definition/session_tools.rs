use super::ToolVisibility::ModelVisible;
use super::{def, model_spec, ToolDefinition, TOOL_CATEGORY_CHECKPOINT, TOOL_CATEGORY_SESSION};
use crate::metadata::{
    ToolPathHint::None as NoPath, ToolRisk, PROJECT_WRITE, SESSION_COLLABORATE,
    TOOL_PROVIDER_CONTROL,
};
use crate::registry::input_schemas::{
    checkpoint_action_input_schema, session_action_input_schema, todo_action_input_schema,
};

pub(super) const DEFINITIONS: &[ToolDefinition] = &[
    model_spec(
        def(
            "session",
            super::ToolAuditPolicy::TYPED_CANONICAL,
            ModelVisible,
            TOOL_CATEGORY_SESSION,
            None,
            TOOL_PROVIDER_CONTROL,
            super::ToolSemanticContract {
                effect: super::ToolEffect::Mutate,
                risk: ToolRisk::WorkflowManage,
                approval: super::ToolApprovalPolicy::None,
                idempotency: super::ToolIdempotency::NonIdempotent,
            },
            Some(SESSION_COLLABORATE),
            false,
            NoPath,
            false,
            false,
            super::ToolSessionEvidencePolicy::NONE,
        ),
        "Unified Workflow Session lifecycle. Actions: start (single or multi-workspace projects list), get, list, attach_project, detach_project, update_context, compact (distill to one Memory entry), restore (exact checkpoint), summary, close. Attached workspaces never inherit canonical project authority.",
        session_action_input_schema,
    ),
    model_spec(
        def(
            "todo",
            super::ToolAuditPolicy::TYPED_CANONICAL,
            ModelVisible,
            TOOL_CATEGORY_SESSION,
            None,
            TOOL_PROVIDER_CONTROL,
            super::ToolSemanticContract {
                effect: super::ToolEffect::Mutate,
                risk: ToolRisk::WorkflowManage,
                approval: super::ToolApprovalPolicy::None,
                idempotency: super::ToolIdempotency::NonIdempotent,
            },
            Some(SESSION_COLLABORATE),
            false,
            NoPath,
            false,
            false,
            super::ToolSessionEvidencePolicy::NONE,
        ),
        "Session-scoped todo records with revision CAS. Actions: create, list, get, claim (-> in_progress with assignee), release (-> open), advance (to blocked/cancelled with note), complete (resolve). Todos are first-class Workflow Session state, distinct from free-form collaboration messages.",
        todo_action_input_schema,
    ),
];

/// Consolidated workspace checkpoint control. Feature-gated like the
/// workspace_checkpoint_* tools it delegates to.
#[cfg(feature = "workspace-checkpoints")]
pub(super) const CHECKPOINT_DEFINITIONS: &[ToolDefinition] = &[
    model_spec(
        def(
            "checkpoint",
            super::ToolAuditPolicy::TYPED_CANONICAL,
            ModelVisible,
            TOOL_CATEGORY_CHECKPOINT,
            None,
            TOOL_PROVIDER_CONTROL,
            super::ToolSemanticContract {
                effect: super::ToolEffect::Mutate,
                risk: ToolRisk::CheckpointManage,
                approval: super::ToolApprovalPolicy::None,
                idempotency: super::ToolIdempotency::NonIdempotent,
            },
            Some(PROJECT_WRITE),
            true,
            NoPath,
            true,
            false,
            super::ToolSessionEvidencePolicy::NONE,
        ),
        "Unified workspace checkpoint control. Actions: create, list, show, restore (requires confirm=true; rewrites workspace state), delete (requires confirm=true). Consolidates the former workspace_checkpoint_* tools; restore and delete remain explicit-confirm destructive operations.",
        checkpoint_action_input_schema,
    ),
];
