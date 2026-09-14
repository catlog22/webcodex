// DOM-free workspace projection state for Workflow Session console detail:
// attached project normalization, attachable registered-project filtering, and
// mutation result summarizing. Revision values feed the CAS fence the console
// backend enforces on every attach/detach.
export function workflowSessionProjectEntries(detail) {
    if (!detail || !Array.isArray(detail.projects)) {
        return [];
    }
    const seen = new Set();
    const entries = [];
    for (const raw of detail.projects) {
        const project = raw && typeof raw.project === "string" ? raw.project : "";
        if (!project || seen.has(project)) {
            continue;
        }
        seen.add(project);
        entries.push({ project, canonical: !!(raw && raw.canonical) });
    }
    // Canonical workspace renders first regardless of payload ordering.
    entries.sort((a, b) => Number(b.canonical) - Number(a.canonical));
    return entries;
}
export function workflowSessionProjectsRevision(detail) {
    const revision = detail && detail.projects_revision;
    return typeof revision === "number" && Number.isFinite(revision) && revision >= 0
        ? Math.floor(revision)
        : 0;
}
export function attachableWorkflowSessionProjects(registered, entries) {
    const attached = new Set(entries.map((entry) => entry.project));
    const seen = new Set();
    const attachable = [];
    for (const raw of registered) {
        const id = raw && typeof raw.id === "string" ? raw.id : "";
        if (!id || attached.has(id) || seen.has(id)) {
            continue;
        }
        seen.add(id);
        const clientId = raw && typeof raw.client_id === "string" ? raw.client_id : "";
        attachable.push({ id, clientId, label: clientId ? id + " (" + clientId + ")" : id });
    }
    attachable.sort((a, b) => a.label.localeCompare(b.label));
    return attachable;
}
// The console endpoint renders the ToolResult JSON: top-level `success`, the
// human message in `error`, and the typed payload (including the new revision)
// under `output`.
export function summarizeSessionProjectsMutation(result) {
    const ok = !!(result && result.success);
    const output = (result && result.output) || {};
    const revision = output.revision;
    const error = !ok
        ? String((result && result.error) ||
            output.message ||
            output.error_kind ||
            "workspace update failed")
        : "";
    return {
        ok,
        revision: typeof revision === "number" && Number.isFinite(revision) ? Math.floor(revision) : 0,
        error,
    };
}
