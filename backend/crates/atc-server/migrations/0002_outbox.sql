CREATE TABLE outbox (
    seq         BIGSERIAL PRIMARY KEY,
    kind        TEXT      NOT NULL CHECK (kind IN ('run', 'job')),
    run_id      BIGINT    NOT NULL,
    job_id      BIGINT    NULL,
    payload     JSONB     NOT NULL,
    inserted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX outbox_run_idx ON outbox (run_id);
