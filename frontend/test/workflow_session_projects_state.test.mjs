import test from "node:test";
import assert from "node:assert/strict";
import {
  workflowSessionProjectEntries,
  workflowSessionProjectsRevision,
  attachableWorkflowSessionProjects,
  summarizeSessionProjectsMutation,
} from "../dist/workflow_session_projects_state.js";

test("workflow session project entries normalize, dedupe, and canonical-first", () => {
  const entries = workflowSessionProjectEntries({
    projects: [
      { project: "agent:other:repo", canonical: false },
      { project: "wc_proj_123", canonical: true },
      { project: "agent:other:repo", canonical: false },
      { project: "", canonical: false },
      { project: 42, canonical: false },
    ],
  });
  assert.deepEqual(entries, [
    { project: "wc_proj_123", canonical: true },
    { project: "agent:other:repo", canonical: false },
  ]);
  assert.deepEqual(workflowSessionProjectEntries(null), []);
  assert.deepEqual(workflowSessionProjectEntries({}), []);
});

test("workflow session projects revision defaults to zero when absent", () => {
  assert.equal(workflowSessionProjectsRevision({ projects_revision: 7 }), 7);
  assert.equal(workflowSessionProjectsRevision({ projects_revision: "7" }), 0);
  assert.equal(workflowSessionProjectsRevision({}), 0);
});

test("attachable projects exclude attached and dedupe, sorted by label", () => {
  const attachable = attachableWorkflowSessionProjects(
    [
      { id: "agent:b:two", client_id: "b" },
      { id: "agent:a:one", client_id: "a" },
      { id: "agent:a:one", client_id: "a" },
      { id: "wc_proj_123", client_id: "" },
      { id: "agent:c:bad" },
    ],
    [
      { project: "wc_proj_123", canonical: true },
      { project: "agent:a:one", canonical: false },
    ]
  );
  assert.deepEqual(attachable, [
    { id: "agent:b:two", clientId: "b", label: "agent:b:two (b)" },
    { id: "agent:c:bad", clientId: "", label: "agent:c:bad" },
  ]);
});

test("mutation summary surfaces ok, revision, and conflict error", () => {
  const ok = summarizeSessionProjectsMutation({
    success: true,
    output: { action: "attach_project", revision: 3 },
  });
  assert.deepEqual(ok, { ok: true, revision: 3, error: "" });

  const conflict = summarizeSessionProjectsMutation({
    success: false,
    error: "session attached-workspace revision conflict: expected 1, actual 2",
    output: { error_kind: "session_projects_revision_conflict", state_changed: false },
  });
  assert.equal(conflict.ok, false);
  assert.equal(conflict.error.indexOf("revision conflict") !== -1, true);

  const generic = summarizeSessionProjectsMutation(null);
  assert.equal(generic.ok, false);
  assert.equal(generic.error, "workspace update failed");
});
