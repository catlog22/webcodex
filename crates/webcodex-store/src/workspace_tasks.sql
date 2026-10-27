CREATE TABLE IF NOT EXISTS workspace_tasks (
    task_id TEXT PRIMARY KEY,
    project TEXT NOT NULL,
    root_fingerprint TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done', 'cancelled')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    completion_summary TEXT NOT NULL DEFAULT '',
    CHECK (status != 'done' OR length(trim(completion_summary)) > 0),
    UNIQUE(project, root_fingerprint, idempotency_key)
);
CREATE INDEX IF NOT EXISTS workspace_tasks_project
    ON workspace_tasks(project, root_fingerprint, created_at, task_id);
