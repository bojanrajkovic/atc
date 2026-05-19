# CLAUDE.md — atc-store-pg

Last verified: 2026-05-18

> Canonical documentation lives in `docs/architecture/backend-server.md` (Persistence § PG mode) and `docs/architecture/metrics.md` (PG-mode emit sites + `PgMetrics`). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture docs.

## Purpose

PostgreSQL-backed [`PersistentStore`](../atc-persist/src/lib.rs) implementation. `PgStore` owns the connection pool, the broadcast sender that fans `CommittedEvent`s to WS subscribers, the drain task's watermark + heartbeat atomics, and the four background `JoinHandle`s (listener, drain, outbox heartbeat, outbox sweep). The PG-mode `PgMetrics` surface, the LISTEN/NOTIFY background tasks, the snapshot-read helpers (unscoped `read_snapshot` + repository-scoped `read_snapshot_for_repos`), the pool init + `DbInitError` wrapper, and the embedded SQL migrations all live here.

## File map

| File | Contents |
|------|----------|
| `src/lib.rs` | Module declarations + re-exports (`PgStore`, `PgStoreStartError`, `DbInitError`, `init_pool`, and behind `cfg(any(test, feature = "test-support"))` the `PgStoreTestHooks`/`PgStoreTestHandles` types). |
| `src/store/mod.rs` | Constants (broadcast capacity, outbox cadences, retention floor, per-task shutdown budgets), `PgStoreStartError`, `SqlRepr` impls, `PgStore` struct, `start` / `start_inner` / `ping`, `hostname_or_unknown`. |
| `src/store/writes.rs` | `impl PersistentStore for PgStore` and the free-fn transaction helpers (`upsert_*_in_txn`, `insert_outbox_*_in_txn`, `notify_outbox_seq_in_txn`). |
| `src/store/retention.rs` | Outbox heartbeat + sweep spawn fns and the per-tick bodies. |
| `src/store/test_hooks.rs` | `cfg(any(test, feature = "test-support"))` — `PgStoreTestHooks`, `PgStoreTestHandles`, `start_with_test_hooks`, the test-only `impl PgStore { … }` sync-tick + accessor methods. |
| `src/listener.rs` | PG LISTEN/NOTIFY listener task plus the drain task with ring-buffer dedup and gap-healing backstop. |
| `src/reads.rs` | `read_all_runs` / `read_all_jobs` (unscoped) and `read_runs_for_repos` / `read_jobs_for_repos` (scoped via `unnest`-joined positional `(org, repo)` arrays) snapshot helpers, used by `PgStore::read_snapshot` and `read_snapshot_for_repos` respectively. |
| `src/db.rs` | `init_pool`, the `MIGRATOR` static, and the `DbInitError { Migrate(Box<MigrateError>), Connect(sqlx::Error) }` enum. |
| `src/metrics.rs` | `PgMetrics` struct + cached OTel instruments + pre-built `[KeyValue; 1]` slices; observable-gauge registration via `Weak<AtomicI64>`. |
| `src/invariants.rs` | `cfg(any(test, feature = "test-support"))` — placeholder for future PG-side invariant assertion helpers. |
| `migrations/` | `0001_initial_runs_jobs.sql`, `0002_outbox.sql`, `0003_runs_placeholder.sql`, `0004_outbox_watermarks.sql`, `0005_runs_org_repo_idx.sql` (B-tree index on `runs(org, repo)` for stable scoped-read plans). |

## Sharp edges

**`sqlx::query!` macro hot reload.** Every `sqlx::query!` / `sqlx::query_scalar!` call in this crate is checked at compile time against the `.sqlx/` cache at `backend/.sqlx/`. The cache is keyed by SQL string hash + bind types, NOT by source path or crate name, so moving a `query!` call between files inside this crate (or from another crate into this one) does not invalidate the cache. If you change the SQL string or its bind types, regenerate the cache via `cargo sqlx prepare` from `backend/`, and commit the regenerated JSON files alongside the SQL change. `cargo sqlx prepare --check` is the CI gate — run it locally before opening a PR.

**Migrations live with the store, anchored via `MIGRATOR`.** The `sqlx::migrate!("./migrations")` macro resolves relative to `CARGO_MANIFEST_DIR`, so the `.sql` files under `migrations/` MUST stay co-located with this crate. Tests in `atc-server` that need to run migrations against a caller-managed pool (e.g. `db_readyz_tests` configures `acquire_timeout` for the unreachable-DB path) use `atc_store_pg::db::MIGRATOR.run(&pool)` rather than calling the macro inline. The migration order is intent-fixed: 0001 (runs/jobs schema) → 0002 (outbox + NOTIFY trigger) → 0003 (runs.placeholder column for FK-only stubs) → 0004 (outbox_watermarks for retention) → 0005 (`runs_org_repo_idx` for scoped reads). New migrations append a higher number; never edit a checked-in migration.

**`DbInitError` is the public error surface.** `init_pool` returns `Result<PgPool, DbInitError>` so callers can pattern-match on `DbInitError::Migrate(_)` / `DbInitError::Connect(_)` without naming `sqlx::Error` directly. This is why `atc-server` keeps `sqlx` out of its `[dependencies]` table — every production-source path through the PG init either calls `init_pool` or names types from this crate, never `sqlx::Error::Migrate(_)` directly. Future contributors adding new fallible PG init paths should extend `DbInitError` rather than leaking `sqlx::Error` through the public API.

**Test hooks are gated behind `test-support`.** `PgStoreTestHooks`, `PgStoreTestHandles`, `PgStore::start_with_test_hooks`, and the test-only `impl PgStore { outbox_heartbeat_once, replica_id, broadcast_watermark, … }` methods all live in `src/store/test_hooks.rs` under `#[cfg(any(test, feature = "test-support"))]`. The `test-support` feature is activated by this crate's self-ref dev-dep (for `#[cfg(test)]` use) and by `atc-server`'s cross-crate dev-dep (for the integration test binary). Production builds never see these symbols.

**Per-task shutdown budgets live here, not in `atc-server::shutdown`.** The four constants `SHUTDOWN_TIMEOUT_DRAIN`, `SHUTDOWN_TIMEOUT_LISTENER`, `SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT`, `SHUTDOWN_TIMEOUT_OUTBOX_SWEEP` are owned by `src/store/mod.rs` and consumed by `PgStore::shutdown()` via `atc_persist::join_with_timeout`. The orchestration in `atc-server::shutdown` no longer references them — `persist.shutdown().await` delegates the per-task join budget into this crate.

**`read_snapshot_for_repos` keeps the live cursor.** Same watermark-before-MVCC-view ordering as the unscoped `read_snapshot`: load `broadcast_watermark` with `Ordering::Acquire`, then open a REPEATABLE READ transaction, then read filtered rows. The scoped query joins against `unnest($1::text[], $2::text[]) AS scope(org, repo)` so the plan stays stable regardless of how many repos the caller requests; the `0005_runs_org_repo_idx` index covers the join predicate. Empty `repos` short-circuits without issuing the query — the live watermark is still surfaced so quiet-repo callers don't reconcile against a stale cursor.

**`PgMetrics::register` is the only public constructor; it MUST be called after `atc-server::otel::init_otel`.** The global meter provider is the precondition. Production `main.rs` upholds the ordering; the integration harness installs an in-memory meter via the `OnceLock` guard in `tests/integration/common/mod.rs` before any test constructs a `PgStore`. The observable gauges take `Weak<AtomicI64>` so callbacks for prior `PgStore` instances become no-ops when their tasks finish dropping strong refs — see the inline doc on `PgMetrics::register_with_meter` for the multi-store accumulation story.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Persistence (PG mode), § Drain pipeline, § NOTIFY emission
- Metrics: `docs/architecture/metrics.md` § PG-mode emit sites, § Outbox retention
- ADR-0006 (stores own background task lifecycle)
- ADR-0007 (clock-bound retention semantics; `OUTBOX_RETENTION_FLOOR` rationale)
- ADR-0008 (persistence crate split)
