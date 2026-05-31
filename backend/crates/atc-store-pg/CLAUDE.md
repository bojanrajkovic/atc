# CLAUDE.md — atc-store-pg

Last verified: 2026-05-30

> Canonical documentation lives in `docs/architecture/backend-server.md` (Persistence § PG mode, § Drain pipeline, § NOTIFY emission) and `docs/architecture/metrics.md` (PG-mode emit sites). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture docs.

## Purpose

PostgreSQL-backed `PersistentStore` implementation. Owns pool initialization, embedded migrations, and the connection between incoming webhook events and the WebSocket fan-out. All production state lives in Postgres; in-memory state inside the store is ephemeral coordination (watermarks, heartbeat atomics, broadcast sender).

## Conceptual groupings

**Write path — transactional outbox + NOTIFY emit.** Each `apply_*_event` call writes the domain row and an outbox row inside one transaction, then emits a `pg_notify` at commit. The notification payload carries only a sequence number (well under Postgres's per-payload limit); the drain reconstructs the full event from the outbox row. This is the core write loop; everything else exists to support it.

**Read path + liveness.** Snapshot reads query all runs and jobs directly (bypassing the outbox). The liveness check issues a trivial round-trip to Postgres and maps failures to `LivenessError::DbUnreachable` for the `/readyz` handler. Pool initialization and migration application also belong to this concern — they are the preconditions for any read or write.

**Lifecycle + retention — listener, drain, heartbeat, sweep.** Four background tasks start with the store and shut down with it. The listener holds a dedicated LISTEN connection (session mode required — see Sharp edges). The drain consumes NOTIFY events from the listener, deduplicates via a ring buffer, and re-queries the outbox to fan events to WS subscribers; it includes a gap-healing backstop for missed notifications. Heartbeat and sweep own outbox retention: heartbeat keeps the watermark alive, sweep evicts rows older than the retention floor. Per-task shutdown budgets are constants inside this crate, not in `atc-server`.

## Sharp edges

**`sqlx::query!` requires the offline cache or a live `DATABASE_URL` at compile time.** The cache lives in `backend/.sqlx/`. If you change a query's SQL string or bind types, regenerate the cache with `cargo sqlx prepare` run from `backend/`, and commit the regenerated JSON files alongside the SQL change. `cargo sqlx prepare --check` is the CI gate.

**Migrations must stay co-located with this crate.** The `sqlx::migrate!` macro resolves relative to `CARGO_MANIFEST_DIR`. Moving or splitting the `migrations/` directory breaks the embedded migrator. New migrations append a higher number; never edit a checked-in migration file.

**Re-run detection lives in the run UPSERT predicate, not the FSM.** GitHub re-runs reuse the same `run_id` with a higher `run_attempt`. The forward-only `WHERE runs.status = ANY(...)` guard would otherwise reject a fresh `Queued`/`InProgress` event arriving on top of a `Completed` row. The predicate is therefore `WHERE (runs.status = ANY($N::text[]) AND EXCLUDED.run_attempt = runs.run_attempt) OR EXCLUDED.run_attempt > runs.run_attempt`. The same-attempt clause on the status branch is load-bearing: without it a *delayed lower-attempt* event (e.g. attempt-1 `completed` arriving after attempt 2 is live) would match — `InProgress` is a valid predecessor of `Completed` — and regress `run_attempt` while closing the live attempt with the stale conclusion. `conclusion`, `completed_at`, and `run_started_at` use CASE expressions that take the incoming value (instead of `COALESCE`-preserving the old one) when `EXCLUDED.run_attempt > runs.run_attempt` — i.e. terminal state is reset on a new attempt. `run_attempt` is always written from `EXCLUDED` (the predicate guarantees it never regresses). `atc-store-mem` implements the same semantics — fresh-start on a higher attempt, hard reject on a lower one; keep the two paths behaviorally aligned. The FK-stub `runs` insert in `upsert_job_in_txn` seeds `run_attempt` to 1.

**Jobs are filtered to drop prior attempts on read, not deleted on re-run.** GitHub assigns fresh job IDs per attempt under the reused `run_id`, so a re-run's job rows accumulate alongside the prior attempt's. `jobs.run_attempt` (migration `0009`) records each job's attempt; `read_all_jobs` joins `runs` and filters `WHERE j.run_attempt >= r.run_attempt`. The `>=` (not `=`) is deliberate: GitHub emits no `workflow_run.requested` for a queued re-run, so the first signal can be a `workflow_job.queued` at attempt 2 while the run row is still attempt 1 — those queued jobs must stay visible, so only *lower* (stale) attempts are dropped, never higher/incoming ones. In steady state no job outlives its run's attempt, so there is no mixing. Filtering (not deleting) is also reorder-safe in general. `atc-store-mem` applies the same parent-attempt filter in `read_snapshot_inner`.

**The LISTEN connection requires session-mode pooling.** `LISTEN` state is session-scoped in Postgres. A transaction-mode or statement-mode pooler (e.g. PgBouncer in transaction mode) drops the subscription on each connection hand-back. The listener task must use a dedicated connection acquired outside the shared pool, not a pooled connection.

**Test hooks are gated behind the `test-support` feature.** The test-only types and methods are compiled only under `#[cfg(any(test, feature = "test-support"))]`. The feature is activated by the crate's self-ref dev-dep (for unit tests) and by `atc-server`'s cross-crate dev-dep (for integration tests). Production builds never see these symbols; do not activate the feature in a production dependency.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Persistence (PG mode), § Drain pipeline, § NOTIFY emission
- Metrics: `docs/architecture/metrics.md` § PG-mode emit sites, § Outbox retention
- ADR-0006 (stores own background task lifecycle)
- ADR-0007 (clock-bound retention semantics; retention floor rationale)
- ADR-0008: `docs/architecture-decisions/0008-persistence-crate-split.md`
