-- Per-replica broadcast watermark heartbeat surface for outbox retention.
--
-- Every PgStore replica heartbeats its `broadcast_watermark` here every 30s.
-- The outbox sweep task reads `MIN(broadcast_watermark)` across non-stale
-- replicas (`updated_at > now() - 90s`) as the floor under which outbox rows
-- can be safely deleted, in addition to the time-based retention cutoff.
--
-- `updated_at` has no DEFAULT now() on purpose: every retention-path
-- timestamp must be bound Rust-side from `Clock::now()` so `TestClock`-driven
-- integration tests can advance time deterministically. SQL `now()` is
-- transaction-start wall-clock and indifferent to the test clock.
--
-- See ADR 0007 (outbox retention policy) for the design rationale.
CREATE TABLE outbox_watermarks (
    replica_id          TEXT        PRIMARY KEY,
    broadcast_watermark BIGINT      NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL
);
