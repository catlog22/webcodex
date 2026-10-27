import { useEffect, useRef, useState } from "react";
import { Alert, Badge, Button, Select, Textarea, TextInput } from "@mantine/core";
import { useProduct, type ProductKey } from "../../i18n/product";
import { useLocale } from "../../i18n/locale";
import type { WorkspaceTask, WorkspaceTaskOperation, WorkspaceTaskPage, WorkspaceTaskStatus } from "../../models/workspace";
import { projectName, useWorkspace, workspaceQuery } from "../workspace/WorkspaceContext";
import { WorkspaceDialog } from "../workspace/WorkspaceDialog";
import "./workspace-tasks.css";

const STATUS_KEYS: Record<WorkspaceTaskStatus, ProductKey> = { open: "taskOpen", in_progress: "taskInProgress", done: "taskDone", cancelled: "taskCancelled" };
type Draft = { project: string; root: string; title: string; description: string; completion_summary: string; status: WorkspaceTaskStatus; task_id?: string; revision?: number; key: string };
type Response<T> = { success: boolean; output: T & { error_kind?: string } };

async function request<T>(project: string, operation: WorkspaceTaskOperation): Promise<T> {
  const response = await workspaceQuery<Response<T>>({ kind: "workspace_tasks", request: { project, operation } });
  if (!response.success) throw new Error(response.output.error_kind || "workspace_task_request_failed");
  return response.output;
}

export function WorkspaceTasksPanel() {
  const workspace = useWorkspace();
  return <TaskWorkspace key={workspace.contextKey} />;
}

function TaskWorkspace() {
  const workspace = useWorkspace();
  const p = useProduct();
  const { formatTime } = useLocale();
  const [project, setProject] = useState("");
  const [page, setPage] = useState<WorkspaceTaskPage | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState<ProductKey | null>(null);
  const [saveError, setSaveError] = useState<ProductKey | null>(null);
  const sequence = useRef(0);
  const alive = useRef(true);
  const createPayload = useRef<Extract<WorkspaceTaskOperation, { action: "create" }> | null>(null);
  const unavailable = workspace.busy || workspace.state.readiness.server !== "ready"
    || (workspace.error && ["authenticationRequired", "permissionDenied"].includes(workspace.errorReason));
  const selectedProject = useRef(project);
  selectedProject.current = project;
  useEffect(() => { alive.current = true; return () => { alive.current = false; sequence.current += 1; }; }, []);
  useEffect(() => {
    const observed = ++sequence.current;
    setPage(null); setError(null);
    if (!project || unavailable) { setLoading(false); return; }
    setLoading(true);
    void request<WorkspaceTaskPage>(project, { action: "list", query: { limit: 50 } }).then(value => {
      if (alive.current && observed === sequence.current) setPage(value);
    }).catch(() => { if (alive.current && observed === sequence.current) setError("tasksLoadError"); })
      .finally(() => { if (alive.current && observed === sequence.current) setLoading(false); });
  }, [project, unavailable, revision]);
  const open = async (id: string) => {
    const observed = ++sequence.current;
    setLoading(true); setError(null);
    try {
      const result = await request<{ task: WorkspaceTask; root_fingerprint: string }>(project, { action: "read", task_id: id });
      if (alive.current && observed === sequence.current) {
        setSaveError(null); createPayload.current = null;
        setDraft({ ...result.task, root: result.root_fingerprint, key: crypto.randomUUID() });
      }
    } catch { if (alive.current && observed === sequence.current) setError("tasksLoadError"); }
    finally { if (alive.current && observed === sequence.current) setLoading(false); }
  };
  const more = async () => {
    if (!page || page.next_offset === null) return;
    const observed = ++sequence.current;
    setLoading(true); setError(null);
    try {
      const next = await request<WorkspaceTaskPage>(project, { action: "list", query: { limit: 50, offset: page.next_offset } });
      if (alive.current && observed === sequence.current) {
        if (next.root_fingerprint !== page.root_fingerprint) { setPage(null); setError("tasksLoadError"); return; }
        const tasks = new Map(page.tasks.map(task => [task.task_id, task]));
        next.tasks.forEach(task => tasks.set(task.task_id, task));
        setPage({ ...next, tasks: [...tasks.values()] });
      }
    } catch { if (alive.current && observed === sequence.current) setError("tasksLoadError"); }
    finally { if (alive.current && observed === sequence.current) setLoading(false); }
  };
  const save = async () => {
    if (!draft || saving || unavailable) return;
    const current = draft;
    setSaving(true); setSaveError(null);
    let operation: WorkspaceTaskOperation;
    if (current.task_id && current.revision !== undefined) {
      operation = { action: "update", task_id: current.task_id, expected_revision: current.revision,
        expected_root_fingerprint: current.root, title: current.title, description: current.description, completion_summary: current.completion_summary, status: current.status };
    } else {
      operation = createPayload.current ?? { action: "create", title: current.title, description: current.description,
        idempotency_key: current.key, expected_root_fingerprint: current.root };
      createPayload.current = operation;
    }
    try {
      await request(current.project, operation);
      if (alive.current && selectedProject.current === current.project) { setDraft(null); createPayload.current = null; setRevision(value => value + 1); }
    } catch (cause) {
      if (alive.current && selectedProject.current === current.project) {
        const code = cause instanceof Error ? cause.message : "";
        setSaveError(code === "workspace_task_revision_conflict" || code === "workspace_task_target_changed" ? "tasksConflict" : "tasksSaveError");
        if (code === "workspace_task_content_invalid") createPayload.current = null;
      }
    } finally { if (alive.current) setSaving(false); }
  };
  const locked = saving || Boolean(createPayload.current);
  return <section className="page-section workspace-page" data-webcodex-page="tasks" aria-labelledby="tasks-title">
    <header className="page-heading-row"><h1 id="tasks-title">{p("workspaceTasks")}</h1>
      <Button variant="default" disabled={!project || unavailable || loading} onClick={() => setRevision(value => value + 1)}>{p("refresh")}</Button></header>
    <p className="field-help">{p("tasksPurpose")}</p>
    <div className="workspace-task-toolbar">
      <Select label={p("tasksChoose")} placeholder={p("tasksChoose")} searchable value={project || null}
        disabled={unavailable || saving} data={workspace.projects.filter(item => item.id).map(item => ({ value: item.id, label: projectName(item) }))}
        onChange={value => { sequence.current += 1; setProject(value || ""); setPage(null); setDraft(null); createPayload.current = null; }} />
      <Button disabled={!page || unavailable || loading} onClick={() => {
        if (!page) return;
        createPayload.current = null; setSaveError(null);
        setDraft({ project: page.project, root: page.root_fingerprint, title: "", description: "", completion_summary: "", status: "open", key: crypto.randomUUID() });
      }}>{p("taskNew")}</Button>
    </div>
    {error && <Alert color="red" role="alert">{p(error)}</Alert>}
    {unavailable && <p role="status">{p("tasksLoadError")}</p>}
    {loading && <p role="status">{p("loading")}</p>}
    {page && !unavailable && <><p className="field-help">{page.total} · {p("workspaceTasks")}</p>
      {page.tasks.length === 0 ? <p>{p("tasksEmpty")}</p> : <ul className="workspace-task-list">{page.tasks.map(task => <li key={task.task_id}>
        <button type="button" disabled={loading} onClick={() => void open(task.task_id)}>
          <span><strong>{task.title}</strong><time>{formatTime(task.updated_at * 1000)}</time></span>
          <Badge variant="light">{p(STATUS_KEYS[task.status])}</Badge>
        </button></li>)}</ul>}
      {page.next_offset !== null && <Button variant="default" disabled={loading} onClick={() => void more()}>{p("tasksMore")}</Button>}
    </>}
    {draft && !unavailable && <WorkspaceDialog title={p(draft.task_id ? "taskEdit" : "taskNew")} busy={saving} onClose={() => { setDraft(null); createPayload.current = null; }}>
      <form className="workspace-task-form" onSubmit={event => { event.preventDefault(); void save(); }}>
        <TextInput label={p("taskTitle")} required maxLength={200} value={draft.title} disabled={locked} onChange={event => setDraft({ ...draft, title: event.currentTarget.value })} />
        <Textarea label={p("taskDescription")} maxLength={8000} autosize minRows={5} maxRows={14} value={draft.description} disabled={locked} onChange={event => setDraft({ ...draft, description: event.currentTarget.value })} />
        {draft.task_id && <Select label={p("taskStatus")} value={draft.status} disabled={saving} allowDeselect={false}
          data={(Object.keys(STATUS_KEYS) as WorkspaceTaskStatus[]).map(value => ({ value, label: p(STATUS_KEYS[value]) }))}
          onChange={value => { if (value && value in STATUS_KEYS) setDraft({ ...draft, status: value as WorkspaceTaskStatus }); }} />}
        {draft.task_id && (draft.status === "done" || draft.completion_summary) && <Textarea label={p("taskCompletionSummary")} required={draft.status === "done"} maxLength={2000} autosize minRows={4} value={draft.completion_summary} disabled={saving} onChange={event => setDraft({ ...draft, completion_summary: event.currentTarget.value })} />}
        {saveError && <Alert color="red" role="alert">{p(saveError)}</Alert>}
        <div className="shell-actions"><Button type="button" variant="default" disabled={saving} onClick={() => { setDraft(null); createPayload.current = null; }}>{p("cancel")}</Button>
          <Button type="submit" disabled={saving || !draft.title.trim() || draft.status === "done" && !draft.completion_summary.trim()} loading={saving}>{p("save")}</Button></div>
      </form>
    </WorkspaceDialog>}
  </section>;
}
