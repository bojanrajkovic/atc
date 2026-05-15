CREATE TABLE runs (
    id BIGINT PRIMARY KEY,
    org TEXT NOT NULL,
    repo TEXT NOT NULL,
    workflow_name TEXT,
    workflow_path TEXT,
    branch TEXT,
    head_sha TEXT NOT NULL,
    commit_message TEXT,
    event TEXT NOT NULL,
    display_title TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('Queued', 'InProgress', 'Completed')),
    conclusion TEXT CHECK (conclusion IS NULL OR conclusion IN (
        'Success', 'Failure', 'Cancelled', 'TimedOut', 'ActionRequired',
        'Stale', 'Neutral', 'Skipped', 'StartupFailure'
    )),
    html_url TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    run_started_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX runs_status_updated_at_idx ON runs (status, updated_at DESC);

CREATE TABLE jobs (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    run_id BIGINT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('Queued', 'Waiting', 'InProgress', 'Completed')),
    conclusion TEXT CHECK (conclusion IS NULL OR conclusion IN (
        'Success', 'Failure', 'Cancelled', 'TimedOut', 'ActionRequired',
        'Stale', 'Neutral', 'Skipped'
    )),
    runner_id BIGINT,
    runner_name TEXT,
    runner_group_id BIGINT,
    runner_group_name TEXT,
    labels TEXT[] NOT NULL DEFAULT '{}',
    steps JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE INDEX jobs_run_id_idx ON jobs (run_id);
CREATE INDEX jobs_status_completed_at_idx ON jobs (status, completed_at);
