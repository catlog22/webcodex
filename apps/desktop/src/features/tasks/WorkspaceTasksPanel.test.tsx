import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DesktopMantineProvider } from "../../components/DesktopMantineProvider";
import { LocaleProvider } from "../../i18n/locale";
import { selectOption } from "../../test/select-option";
import { WorkspaceTasksPanel } from "./WorkspaceTasksPanel";

const fixture = vi.hoisted(() => ({ query: vi.fn(), workspace: {
  contextKey: "server-one", busy: false, error: false, errorReason: "loadError",
  state: { readiness: { server: "ready" } }, projects: [{ id: "agent:r:p", name: "Demo", path: "/demo" }],
} }));
vi.mock("../workspace/WorkspaceContext", async importOriginal => ({
  ...await importOriginal<typeof import("../workspace/WorkspaceContext")>(),
  useWorkspace: () => fixture.workspace, workspaceQuery: fixture.query,
}));
const task = { task_id: "wc_wtask_test", project: "agent:r:p", title: "Existing task", description: "Saved instructions", completion_summary: "", status: "open", revision: 3, created_at: 1, updated_at: 1 };
const page = { project: "agent:r:p", root_fingerprint: "root-one", tasks: [task], total: 1, next_offset: null };
function view() { return <DesktopMantineProvider><LocaleProvider><WorkspaceTasksPanel /></LocaleProvider></DesktopMantineProvider>; }
beforeEach(() => {
  vi.resetAllMocks(); localStorage.setItem("webcodex.desktop.locale", "en-US"); fixture.workspace.contextKey = "server-one";
  fixture.query.mockImplementation(async ({ request }) => ({ success: true, output: request.operation.action === "list" ? page : { task, root_fingerprint: "root-one" } }));
});
async function choose() { selectOption(screen.getByRole("combobox", { name: "Choose a workspace" }), "agent:r:p"); await screen.findByRole("button", { name: /Existing task/ }); }

describe("workspace task management", () => {
  it("requires explicit workspace selection, then creates a root-fenced task", async () => {
    render(view()); expect(fixture.query).not.toHaveBeenCalled();
    await choose(); fireEvent.click(screen.getByRole("button", { name: "New task" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Title" }), { target: { value: "New work" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Description" }), { target: { value: "Instructions" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(fixture.query).toHaveBeenCalledWith(expect.objectContaining({ kind: "workspace_tasks", request: {
      project: "agent:r:p", operation: { action: "create", title: "New work", description: "Instructions", expected_root_fingerprint: "root-one", idempotency_key: expect.any(String) },
    } })));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("preserves edits when another writer changes the revision", async () => {
    fixture.query.mockImplementation(async ({ request }) => request.operation.action === "update"
      ? { success: false, output: { error_kind: "workspace_task_revision_conflict" } }
      : { success: true, output: request.operation.action === "list" ? page : { task, root_fingerprint: "root-one" } });
    render(view()); await choose(); fireEvent.click(screen.getByRole("button", { name: /Existing task/ }));
    const title = await screen.findByRole("textbox", { name: "Title" });
    fireEvent.change(title, { target: { value: "My edits" } }); fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText(/task or workspace changed/); expect(title).toHaveValue("My edits");
    expect(fixture.query).toHaveBeenCalledWith(expect.objectContaining({ request: { project: "agent:r:p", operation: expect.objectContaining({ action: "update", expected_revision: 3, expected_root_fingerprint: "root-one" }) } }));
  });

  it("requires and writes a completion summary when marking a task done", async () => {
    render(view()); await choose(); fireEvent.click(screen.getByRole("button", { name: /Existing task/ }));
    await screen.findByRole("textbox", { name: "Title" });
    selectOption(screen.getByRole("combobox", { name: "Status" }), "done");
    const summary = await screen.findByRole("textbox", { name: "Completion summary" });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(summary, { target: { value: "Implemented and verified" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(fixture.query).toHaveBeenCalledWith(expect.objectContaining({ request: { project: "agent:r:p", operation: expect.objectContaining({ action: "update", status: "done", completion_summary: "Implemented and verified", expected_revision: 3 }) } })));
  });

  it("retries an uncertain create with the same key and payload", async () => {
    let creates = 0;
    fixture.query.mockImplementation(async ({ request }) => {
      if (request.operation.action === "create" && ++creates === 1) throw new Error("connection interrupted");
      return { success: true, output: page };
    });
    render(view()); await choose(); fireEvent.click(screen.getByRole("button", { name: "New task" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Title" }), { target: { value: "Exactly once" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" })); await screen.findByText(/Save failed/);
    expect(screen.getByRole("textbox", { name: "Title" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    const calls = fixture.query.mock.calls.filter(([arg]) => arg.request.operation.action === "create");
    expect(calls).toHaveLength(2); expect(calls[0][0]).toEqual(calls[1][0]);
  });
});
