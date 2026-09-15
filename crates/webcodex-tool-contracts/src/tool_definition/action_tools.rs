//! Consolidated `lsp` / `validate` / `goal` action tool definitions.

use super::RunnerCapabilityRequirement::{LspReadOnlyNavigation, Shell};
use super::ToolVisibility::ModelVisible;
use super::{
    adaptive_runtime_direct, captures_validation_output, context_reobservable, def, model_spec,
    require_all_scopes, ToolDefinition, TOOL_CATEGORY_GOAL, TOOL_CATEGORY_LSP,
    TOOL_CATEGORY_VALIDATION,
};
use crate::metadata::{
    ToolPathHint::None as NoPath,
    ToolRisk::{JobRun, Read, WorkflowManage},
    COMMUNICATION_MANAGE, JOB_RUN, PROJECT_READ, TOOL_PROVIDER_CONTROL, TOOL_PROVIDER_RUNNER,
};
use crate::registry::input_schemas::{
    goal_action_input_schema, lsp_action_input_schema, validate_action_input_schema,
};
use webcodex_core::authority::COMMUNICATION_MANAGE_SCOPES;

pub(super) const DEFINITIONS: &[ToolDefinition] = &[
    context_reobservable(model_spec(
        def(
            "lsp",
            super::ToolAuditPolicy::TYPED_CANONICAL,
            ModelVisible,
            TOOL_CATEGORY_LSP,
            Some(LspReadOnlyNavigation),
            TOOL_PROVIDER_RUNNER,
            super::ToolSemanticContract {
                effect: super::ToolEffect::Observe,
                risk: Read,
                approval: super::ToolApprovalPolicy::None,
                idempotency: super::ToolIdempotency::PureRead,
            },
            Some(PROJECT_READ),
            true,
            NoPath,
            false,
            false,
            super::ToolSessionEvidencePolicy::NONE,
        ),
        "Unified read-only LSP navigation. Actions: status, document_symbols, document_diagnostics, hover, goto_definition, find_references, call_hierarchy (one 1-based position), workspace_symbols (query). Consolidates the former lsp_* tools; never starts a server, runs checks, or executes project code.",
        lsp_action_input_schema,
    )),
    adaptive_runtime_direct(
        captures_validation_output(model_spec(
            def(
                "validate",
                super::ToolAuditPolicy::TYPED_CANONICAL
                    .execution(super::ToolAuditExecutionPolicy::TEXT),
                ModelVisible,
                TOOL_CATEGORY_VALIDATION,
                Some(Shell),
                TOOL_PROVIDER_RUNNER,
                super::ToolSemanticContract {
                    effect: super::ToolEffect::Execute,
                    risk: JobRun,
                    approval: super::ToolApprovalPolicy::Standard,
                    idempotency: super::ToolIdempotency::NonIdempotent,
                },
                Some(JOB_RUN),
                true,
                NoPath,
                false,
                false,
                super::ToolSessionEvidencePolicy::NONE,
            ),
            "Unified structured validation. Actions: cargo_fmt (check=true read-only), cargo_check, cargo_test (executed-test proof), go_test, summary (bounded history of one Session). Long runs return a Job; sync_wait_secs bounds the synchronous grace. Consolidates cargo_*/go_test/validation_summary.",
            validate_action_input_schema,
        )
        .with_execution(super::ToolExecutionContract::new(
            super::ToolExecutionForm::StructuredValidation,
            super::ToolExecutionLifetime::Runner,
            super::ToolExecutionStart::SyncFirst,
            super::ToolExecutionContinuation::ObserveJobs,
        ))),
        95,
    ),
    require_all_scopes(
        model_spec(
            def(
                "goal",
                super::ToolAuditPolicy::typed_fields(&[
                    super::ToolAuditResultField::pointer("goal_id", "/goal/summary/goal_id"),
                    super::ToolAuditResultField::pointer("lifecycle", "/goal/summary/lifecycle"),
                    super::ToolAuditResultField::pointer("revision", "/goal/summary/revision"),
                    super::ToolAuditResultField::value("created"),
                    super::ToolAuditResultField::value("replayed"),
                    super::ToolAuditResultField::value("state_changed"),
                    super::ToolAuditResultField::value("error_kind"),
                ]),
                ModelVisible,
                TOOL_CATEGORY_GOAL,
                None,
                TOOL_PROVIDER_CONTROL,
                super::ToolSemanticContract {
                    effect: super::ToolEffect::Mutate,
                    risk: WorkflowManage,
                    approval: super::ToolApprovalPolicy::Standard,
                    idempotency: super::ToolIdempotency::Keyed,
                },
                Some(COMMUNICATION_MANAGE),
                false,
                NoPath,
                false,
                false,
                super::ToolSessionEvidencePolicy::NONE,
            ),
            "Unified durable Goal control. Actions: create (idempotency-keyed), get, present_plan (read-only App card), list, update (revision CAS plus idempotency key), associate_agent_task, associate_workflow_session (bounded correlations, no target-domain authority). Consolidates the former goal tools.",
            goal_action_input_schema,
        ),
        COMMUNICATION_MANAGE_SCOPES,
    ),
];
