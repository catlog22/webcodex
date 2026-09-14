# Consolidated Session Tool Surface: Design Notes

This document records the design of the consolidated `session` / `todo` /
`checkpoint` ModelVisible tools, modeled on the pi-gateway (pi-maestro-flow)
Gateway MCP tool surface and adapted to WebCodex runtime invariants.

## Goals

The Workflow Session cluster historically exposed 20+ flat, narrowly-scoped
tools (`start_session`, `session_summary`, `update_session_context`,
`close_session`, `post_session_message`, `workspace_checkpoint_*`, ...).
Following the pi-gateway (pi-maestro-flow) Gateway MCP design, the
model-facing surface is consolidated into three action-dispatched tools:

- `session` — Workflow Session lifecycle (start/get/list/attach_project/
    detach_project/update_context/compact/restore/summary/close).
- `todo` — session-scoped todo records (create/list/get/claim/release/
    advance/complete).
- `checkpoint` — workspace checkpoints (create/list/show/restore/delete),
    consolidating the former `workspace_checkpoint_*` hidden tools.

## Legacy → consolidated mapping

The legacy tools remain fully functional; the consolidated surface is the
recommended path for new model flows.

| Legacy tool | Consolidated action |
|---|---|
| `start_session` | `session start` (`project` or multi-workspace `projects[]`; note: the hidden start-only guard fields `deny_write_tools`/`deny_shell_tools` stay on the legacy tool only) |
| `get_session_assignment`, `session_summary` | `session summary` |
| `update_session_context` | `session update_context` |
| `close_session` | `session close` |
| `session list`-style console reads | `session list`, `session get` |
| (new capability) | `session attach_project`, `session detach_project`, `session compact`, `session restore` |
| (new capability) | `todo *` (no legacy equivalent) |
| `workspace_checkpoint_create` | `checkpoint create` |
| `workspace_checkpoint_list` | `checkpoint list` |
| `workspace_checkpoint_show` | `checkpoint show` |
| `workspace_checkpoint_restore` | `checkpoint restore` (also `session restore` for session-scoped restore) |
| `workspace_checkpoint_delete` | `checkpoint delete` |
| `lsp_status`, `document_symbols`, `document_diagnostics`, `hover`, `workspace_symbols`, `goto_definition`, `find_references`, `call_hierarchy` | `lsp status`, `lsp document_symbols`, `lsp document_diagnostics`, `lsp hover`, `lsp workspace_symbols`, `lsp goto_definition`, `lsp find_references`, `lsp call_hierarchy` |
| `cargo_fmt`, `cargo_check`, `cargo_test`, `go_test` | `validate cargo_fmt`, `validate cargo_check`, `validate cargo_test`, `validate go_test` |
| `validation_summary` | `validate summary` |
| `create_goal` | `goal create` |
| `get_goal` | `goal get` |
| `present_goal_plan` | `goal present_plan` |
| `list_goals` | `goal list` |
| `update_goal` | `goal update` |
| `associate_goal_agent_task` | `goal associate_agent_task` |
| `associate_goal_workflow_session` | `goal associate_workflow_session` |

- `lsp` — unified read-only LSP navigation (status/document_symbols/
    document_diagnostics/hover/workspace_symbols/goto_definition/
    find_references/call_hierarchy), consolidating the 8 former `lsp_*`-style
    flat tools. All actions are pure `PROJECT_READ`.
- `validate` — unified structured validation (cargo_fmt/cargo_check/
    cargo_test/go_test plus the `summary` history read), consolidating the
    former `cargo_*`/`go_test` tools and `validation_summary`. Executes as
    `JOB_RUN` with the same sync-first/Job-handoff contract.
- `goal` — unified durable Goal control (create/get/present_plan/list/
    update/associate_agent_task/associate_workflow_session), consolidating
    the 6 former goal tools. Global (never project-scoped); mutations are
    idempotency-keyed and `update` additionally requires `expected_revision`
    CAS.

## Non-goals

- Teammate/role primitives (pi-gateway `teammate`) are not ported.
- The legacy per-tool surface (`start_session`, `session_summary`,
  `workspace_checkpoint_*`, the LSP/validation/goal flat tools, ...) is
  preserved unchanged; the consolidated tools are additive and reuse the same
  runtime handlers.

## Schema shape

Every input schema is a flat JSON object with:

- `action` (required string enum) selecting the operation;
- optional per-action fields, all at the top level;
- `additionalProperties: false`.

This follows the `ssh_resource` / `plugin_tool` precedent and the repository
invariant that each tool input schema is a flat object. Action payloads are
mirrored at the runtime layer by `SessionToolCall` / `TodoToolCall` /
`CheckpointToolCall` (internally `#[serde(tag = "action")]` enums) carried as
`ToolCall::Session` / `ToolCall::Todo` / `ToolCall::Checkpoint`.

## Flattened ToolCallRequest invariants

Visible tool schemas contribute top-level property names to the flattened GPT
Actions `ToolCallRequest` envelope. Existing tests hard-pin several invariants
that shaped this design:

- Hidden start-only fields (`deny_write_tools`, `deny_shell_tools`) must never
  appear in a visible schema. The consolidated `session` tool therefore does
  not expose them; the legacy hidden `start_session` tool retains them.
- `mode` is owned by project checkout tools (`checkout`/`worktree`). The
  session start mode is exposed as `session_mode` (`collaborate`/`solo`) and
  serde-renamed in `SessionToolCall::Start`.
- A field shared with an existing visible tool must keep the *same* schema
  semantics (type/const/pattern), otherwise the flattened schema becomes an
  `anyOf` union and breaks the pinned OpenAPI assertions. Concretely:
  `note` is a plain `{"type":"string"}` (length bounds are runtime-enforced),
  `confirm` is a plain `{"type":"boolean"}` (the `confirm: true` requirement is
  enforced by the runtime, not the schema), and `checkpoint_id` is a plain
  string without a pattern.
- `checkpoint` (and `session restore`) are feature-gated behind
  `workspace-checkpoints`, exactly like the `workspace_checkpoint_*` tools:
  the tool definition, `ToolCall` variant, OpenAPI/flattened properties, and
  the `restore` action in the `session` schema only exist with the feature.
  Without the feature, `session restore` fails closed with
  `workspace_checkpoints_unavailable`.

## Multi-workspace Sessions

- `SessionProjectsStore` persists one bounded JSON record per session under
  `state_dir/session_projects/<session_id>.json` (atomic tmp+rename writes).
  Records carry a `revision` used as a CAS fence: `attach_project` and
  `detach_project` require `expected_revision`.
- Authorization model: attached workspaces never inherit the canonical
  project's authority. Each `projects[]` entry, and every attach/detach
  target, is independently resolved via `resolve_project_input_for_auth`.
  The generic session-project-mismatch precheck in dispatch does not apply to
  attach/detach/restore-with-explicit-project: their `project` accessor returns
  `None` so the handler owns scope validation.
- Project-less Sessions remain legal (same semantics as `start_session`);
  `session compact` requires a canonical project because its output lands in
  project-scoped Memory.

## Todo state machine

- Persisted per session under `state_dir/todos/<session_id>.json`.
- States: `open -> in_progress` (claim, requires assignee), `in_progress ->
  open` (release), `-> blocked|cancelled` (advance, with note), `-> resolved`
  (complete, records `resolved_at`).
- Every mutating action takes `expected_revision`; conflicts return
  `todo_revision_conflict` with expected/actual revisions.
- `complete` requires all `depends_on` todos to be `resolved`.
- Ids are monotonic per session (`wc_todo_0001`, ...); capacity is bounded
  (256 per session).

## Compact → knowhow

`session compact` builds a deterministic JSON body:

- session lifecycle and event counts (`SessionSummary.counts`),
- the full execution context,
- open (non-resolved/non-cancelled) todos from the todo store,
- an optional operator note (secret-like input is rejected via
  `secret_like_value`).

It is written through the canonical `memory_set` runtime path with key
`session_compact.<session_id>` and tags `["compaction","session"]`, under the
canonical project's memory scope. Output echoes the structured body plus the
memory key/revision, so callers can verify the durable record.

## Restore

`session restore` requires `confirm: true` and an exact `wc_ckpt_*` id. The
checkpoint may live under the canonical project or any attached project; the
handler probes the checkpoint store across those scopes and then delegates to
the canonical `workspace_checkpoint_restore` implementation (HEAD-match fence,
Runner file-checkpoint protocol, changed-path reporting). All session-scoped
restore paths go through `authorize_session_target` (immutable creation-time
authority + project re-resolution).

## Tool registration

- Definitions: `crates/webcodex-tool-contracts/src/tool_definition/session_tools.rs`
  (`session`, `todo` always; `checkpoint` in `CHECKPOINT_DEFINITIONS` gated).
- Schemas: `registry/input_schemas/session_tools.rs` and
  `registry/output_schemas/session_tools.rs` (envelopes expose `success`,
  `action`, and per-action fields).
- Discovery: `session`/`todo` (and gated `checkpoint`) join the `runtime`
  discovery group in `tool_catalog.rs`.
- OAuth scope authority derives from tool metadata (`SESSION_COLLABORATE` for
  session/todo, `PROJECT_WRITE` for checkpoint), so
  `oauth_scope_policy_for_runtime_tool` needs no manual entry.
- `requires_project` is `false` for `session`/`todo` (action-dependent; the
  runtime enforces per-action project requirements) following the
  `ssh_resource` precedent; `checkpoint` is `true`.

## Testing

- Contract tests: `crates/webcodex-tool-contracts/src/tests/session_tools.rs`
  (schema shape, action enums, CAS fields, feature gating).
- Runtime contract tests: round-trip parsing of action payloads
  (`tool_call_tests.rs`).
- Unit tests: `src/tool_runtime/session_projects.rs` (store + CAS),
  `src/tool_runtime/todo.rs` (state machine + persistence).
- Integration tests: `src/tool_runtime/tests/session_tools.rs` — multi-workspace
  start, attach/detach CAS, todo persistence across runtime instances, compact
  memory write/read-back + secret rejection, restore confirm fence and
  delegation (feature-gated), consolidated checkpoint CRUD round trip.

## Operator tool exposure (`WEBCODEX_ENABLED_TOOLS` / `WEBCODEX_DISABLED_TOOLS`)

- `src/tool_runtime/tool_exposure.rs` parses comma-separated allow/deny lists
  at startup, validating every name against the registered tool catalog.
  Unknown names or an enabled∩disabled overlap abort startup (fail closed).
- `ToolRuntime::with_tool_exposure` (default: unrestricted) filters every
  listing surface — MCP `tools/list` (JSON post-filter, so extension, gateway,
  and app specs are covered), `list_tools`, and `tool_manifest` — and the
  dispatch terminal gate answers disabled tools exactly like unknown tools.
- Production wiring reads the env vars once in `src/lib.rs`.
- Tests: `src/tool_runtime/tests/tool_exposure.rs` (12) and
  `src/mcp_tests/tool_exposure.rs` (3).

## Console multi-workspace

- `WorkflowSessionConsoleDetail` gains `projects` (canonical-first entries) and
  `projects_revision`; the runtime populates them from `SessionProjectsStore`.
- New console endpoints (exact-session authorization, CAS fence):
  - `POST /api/console/workflow-session/projects` — `{session_id, action:
    attach|detach, project, expected_revision}`; delegates to the consolidated
    `session` tool mutation so authorization and revision semantics stay
    identical to the model-facing surface.
  - `POST /api/console/projects` — registered projects (id + client_id) for the
    workspace picker.
- Frontend: `frontend/src/workflow_session_projects_state.ts` (DOM-free
  normalization/attachable filtering/mutation summarizing), workspace chips
  with detach affordance and an attach form in the session detail, and a
  revision-conflict resync path. The module joins the classic console bundle in
  `scripts/build.mjs` (`RUNTIME_INLINE`-style inline list, watched sources, and
  dist emit).
- Tests: `src/host_console_http.rs` (detail projection, attach/detach CAS
  round trip, picker listing) and
  `frontend/test/workflow_session_projects_state.test.mjs`.

## Skill external document whitelist (`skills.resource_roots`)

- Skills can reference documents outside their package. The wire-level path
  normalization (`webcodex_core::runner_skill::normalize_runner_skill_resource_path`)
  is syntactic-only (empty/control/oversized rejected); the Runner confines the
  resolved file to the Skill package **or** an operator-configured
  `skills.resource_roots` entry — anything else fails with
  `skill_resource_not_allowed`.
- Runner enforcement (`configured_skills::read_resource`): absolute paths read
  via a symlink-free component walk; relative paths may traverse with `..`; the
  canonicalized target must sit inside the package or a resource root. The
  sensitive-path blacklist (`is_secret_path`) and per-component symlink
  rejection still apply everywhere, and the 512KB/48KB size limits are
  unchanged. Managed (`skill_store`) skills intentionally stay package-confined.
- `skills.resource_roots` validation mirrors `skills.roots` (absolute, no
  traversal, dedup by path identity, cross-field overlap with `roots` rejected,
  same 16-entry cap).
- Project-source skills keep the legacy file-protocol confinement
  (runner-side package check); their "elsewhere" remains reachable through the
  normal project read tools.
- Tests: runner `configured_skills_tests` (whitelist positive/negative,
  sensitive-path precedence), `skills_config_tests` (validation), core
  `runner_skill` (normalizer), server `tool_runtime` skills suites.
