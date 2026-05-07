# CLAUDE.md — atc-server

Last verified: 2026-05-07 (in-memory mode reframed as dev-only per Phase 5 follow-up; see "Storage modes" subsection. Phase 5 operational metrics + Phase 3c notes from prior versions preserved below)

> Canonical documentation lives in `docs/architecture/backend-server.md`. This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Axum HTTP server wiring `atc-core` (state store) and `atc-github` (webhook parsing) together. Provides HTTP endpoints for webhook ingestion, REST state snapshots, and WebSocket event streaming. The only executable crate in the backend workspace. **Phase 2c:** PostgreSQL writes are driven directly via `AppState.pg_pool` (`&PgPool`) in the webhook handler; the `PgStore` wrapper is retained in `persist.rs` for use by `tests/persist_pg_tests.rs` but is no longer mounted in `AppState`.

## Storage modes

Two runtime modes; **only external Postgres is production-supported** — see `docs/architecture/backend-server.md` § "Storage modes — operator guidance" for the canonical write-up.

- **External Postgres** (`ATC_DATABASE_URL` set) — required for `replicaCount > 1` (chart guard refuses to render otherwise). Drain task is sole broadcaster.
- **In-memory** (`ATC_DATABASE_URL` unset) — **dev-only**, single-replica only. Useful for `just dev` against curl/smee.io-fired webhooks; lossy on restart; do not deploy to production. The webhook handler broadcasts directly under the seq mutex (see `routes.rs:432-436`).

## Modules

| Module | Role |
|--------|------|
| `main` | Server entry point, config loading, AppState creation, router setup, eviction task lifecycle; connects pool and stores it in AppState.pg_pool; when pool is configured, clones it to `pg_pool_for_listener`, initializes `PgListener` (connect + LISTEN + `COALESCE(MAX(seq),0)` watermark), spawns listener and drain tasks before binding `axum::serve` |
| `config` | figment-based Config struct, GitHubConfig with webhook_secret, Config::load(); `database_listener_url: Option<String>` reads `ATC_DATABASE_LISTENER_URL` (falls back to `database_url` at runtime) |
| `db` | `init_pool(url)` — connects sqlx PgPool and runs embedded migrations; extracted from main so it is testable as library code |
| `routes` | HTTP route handlers: `POST /v1/webhooks/github`, `GET /v1/state`, `GET /v1/ws`, health/ready probes; **PG mode** — webhook handler runs transactional PG UPSERT+outbox INSERT, then returns `{"status":"accepted","seq":<i64>}` without touching the seq mutex, in-memory store, or broadcast channel; **in-memory mode** — applies to in-memory store, increments seq mutex, broadcasts SeqEvent; `/v1/state` in PG mode opens a REPEATABLE READ transaction, reads runs/jobs/MAX(outbox.seq) atomically; `/readyz` checks drain heartbeat staleness |
| `state` | AppState struct (fields: `store`, `webhook_tx`, `webhook_secret`, `seq`, `pg_pool`, `min_pending_seq`, `last_drain_pass_at`); `SeqEvent { seq, event }` broadcast type (Phase 3b removed `pool_stats_after`). `seq: Mutex<u64>` is **pre-incremented** in in-memory mode only. `min_pending_seq: Arc<AtomicI64>` initialized to `i64::MAX`; listener calls `fetch_min(seq, Release)` on each NOTIFY; drain calls `swap(MAX, AcqRel)` to capture. `last_drain_pass_at: Arc<AtomicI64>` holds epoch-millis of last drain pass; `/readyz` returns 503 if age > `READYZ_HEARTBEAT_STALENESS_MS=30_000` |
| `ws` | WebSocket upgrade handler, broadcast subscription, SeqEvent serialization and push |
| `assets` | rust-embed static file serving, SPA fallback, dev proxy to Vite |
| `metrics` | Prometheus layer with explicit `install_recorder()` and custom histogram buckets (`atc_pg_drain_startup_seconds` + `_seconds` suffix fallback); `build_info` gauge; process collector; PG write counters (`register_pg_write_counters`); LISTEN/NOTIFY counters (`register_listener_metrics`). Phase 5 added six operational metrics: `atc_pg_outbox_lag_seconds`, `atc_pg_drain_pass_duration_seconds`, `atc_pg_wake_coalesced_total`, `atc_pg_drain_startup_seconds`, `atc_pg_broadcast_watermark`, `atc_pg_min_pending_seq` (see `docs/architecture/backend-server.md` § Operational metrics for the per-metric blocks). |
| `persist` | `PgStore` implementing `atc-core::PersistentStore` trait; predicated UPSERTs for runs and jobs; `pub(crate)` transaction helpers `upsert_run_in_txn`, `upsert_job_in_txn`, `insert_outbox_run_in_txn`, `insert_outbox_job_in_txn` for atomic outbox writes (Phase 2c); `notify_outbox_seq_in_txn` emits `pg_notify` inside the open transaction (Phase 2d) |
| `listener` | PG LISTEN/NOTIFY background tasks: listener task receives NOTIFY payloads and calls `min_pending_seq.fetch_min(seq, Release)`; drain task wakes on `Arc<Notify>` signal or 5s heartbeat tick, fetches outbox rows (`seq > watermark`, paginated in batches of `DRAIN_BATCH_SIZE=500`), decodes each as `RunEventEnvelope`, deduplicates via ring buffer (`DEDUP_CAP=2048`), broadcasts to `webhook_tx`, advances watermark, and updates `last_drain_pass_at`. Gap-healing: drain computes `pass_start_floor = watermark.min(backstop.saturating_sub(1))` to rescan rows that may have been missed. Spawned only when pg_pool is Some. (Phase 3c) |
| `migrations/` | SQL migration files embedded at compile time via `sqlx::migrate!()`. `0001_initial_runs_jobs.sql` creates `runs` and `jobs` tables. `0002_outbox.sql` creates the `outbox` append-only event log table with BIGSERIAL primary key, `kind` discriminator, and JSONB payload. `0003_runs_placeholder.sql` adds `placeholder BOOLEAN NOT NULL DEFAULT false` to `runs`; stub rows (job-before-run FK stubs) use `placeholder=true` and are promoted to `false` by real run UPSERTs. Run automatically on startup when `ATC_DATABASE_URL` is set. |

## TypeScript Generation

`SeqEvent` and `StateSnapshot` derive `#[derive(TS)]` with `#[ts(export)]` to generate TypeScript interfaces for WebSocket and REST payloads.

**Serialization format:**
- `SeqEvent` is serialized as `{ seq, event }` (camelCase via `#[serde(rename_all = "camelCase")]`); Phase 3b removed `pool_stats_after`
- `StateSnapshot { last_seq, runs, jobs }` uses `#[serde(rename_all = "camelCase")]` so `last_seq` serializes as `lastSeq` in JSON; Phase 3b removed `pool_stats`
- WebSocket events use the adjacently-tagged serde format inherited from `atc-core::WebhookEvent` (discriminated union with `type` and `data` fields)

Generated types are written to `frontend/src/lib/types/generated/` via `just types` recipe.

## Contracts

These rules are enforced by implementation and verified by tests:

- **Webhook ingestion (PG mode):** HMAC-SHA256 verification (when secret configured), parse via atc-github, then begin transaction, UPSERT run/job, INSERT outbox row, emit `SELECT pg_notify('atc_outbox', seq::text)`, commit. **No seq mutex acquired, no in-memory store apply, no broadcast.** Returns `{"status":"accepted","seq":<i64>}` on success, 503 for transient PG failures. The drain is the sole broadcaster in PG mode.
- **Webhook ingestion (in-memory mode):** HMAC-SHA256 verification (when secret configured), parse via atc-github, acquire seq mutex, pre-increment (`*seq_guard += 1; let seq = *seq_guard`), apply to StateStore, broadcast SeqEvent. Returns `{"status":"processed"}`.
- **NOTIFY emission:** Inside the webhook handler transaction (after outbox INSERT, before commit), `SELECT pg_notify('atc_outbox', seq::text)` emits the outbox row's seq to all listeners. PG queues NOTIFYs during a txn and delivers on COMMIT; aborted txns silently drop. Metric: `atc_pg_notify_emitted_total{kind}`.
- **Drain pipeline:** Listener calls `min_pending_seq.fetch_min(seq, Release)` on each NOTIFY, signals `Arc<Notify>`. Drain wakes, calls `swap(MAX, AcqRel)` to capture backstop floor, computes `pass_start_floor = watermark.min(backstop.saturating_sub(1))` for gap-healing rescans. Fetches rows `seq > pass_start_floor ORDER BY seq LIMIT DRAIN_BATCH_SIZE`, paginates until partial page, deduplicates via `VecDeque<i64>` + `HashSet<i64>` ring buffer (`DEDUP_CAP=2048`), decodes each as `RunEventEnvelope`, broadcasts to `webhook_tx`, advances watermark. **On successful pass**: updates `last_drain_pass_at`, advances `broadcast_watermark` (the commit-order cursor read by `state_handler` as `lastSeq`). **On failed pass**: re-registers the captured backstop into `min_pending_seq` (so a transient query failure doesn't lose the gap-healing signal) and does NOT refresh the heartbeat (so `/readyz` reflects sustained drain failure after 30s). Heartbeat-only ticks (5s `HEARTBEAT_TICK` arm of `tokio::select!`, no NOTIFY pending) refresh `last_drain_pass_at` to keep `/readyz` fresh during quiet periods. Metrics: `atc_pg_drain_duplicate_skipped_total`, `atc_pg_drain_unknown_kind_total`. Local watermark initializes to `COALESCE(MAX(seq), 0)` at boot (ADR 0002 Decision 5); `broadcast_watermark` seeds from the same value.
- **Seq ordering (in-memory mode):** `Mutex<u64>` acquired BEFORE apply + seq assignment + broadcast. Strictly monotonic, no gaps in-process. Resets to `0` on restart. In PG mode, ordering is provided by outbox `BIGSERIAL` + drain's ascending ORDER BY.
- **Broadcast semantics:** Bounded channel (capacity 256) means slow subscribers may miss events. LaggingError logs warning but does not disconnect. In in-memory mode, broadcast happens under seq mutex. In PG mode, the drain is the sole broadcaster and operates without seq mutex.
- **State snapshot (PG mode):** Loads `broadcast_watermark` (the drain's commit-order cursor) BEFORE opening a REPEATABLE READ transaction that reads runs (`WHERE placeholder=false`) and jobs from a single MVCC snapshot. `MAX(outbox.seq)` is **not** used as `lastSeq` — BIGSERIAL is allocated pre-commit and can commit out of order, which would let the cursor advance past data the snapshot can't see and cause the frontend to permanently drop a buffered event. The drain's `broadcast_watermark` only advances after a successful pass, and the drain only sees committed rows via SELECT, so the cursor is monotonic in commit order. The frontend invariant the snapshot must uphold: `entity_count >= last_seq` (snapshot reflects everything the cursor advertises, plus possibly newer commits the drain hasn't broadcast yet — those are buffered and applied idempotently). Placeholder stub runs are always excluded.
- **State snapshot (in-memory mode):** `Mutex<u64>` held across snapshot + seq read, ensuring cursor matches snapshot content. `lastSeq=0` is the cold-start sentinel.
- **PG access:** `AppState.pg_pool` is `Option<sqlx::PgPool>`. `Some(pool)` when `ATC_DATABASE_URL` is configured; `None` in in-memory-only mode. The webhook handler calls `persist::upsert_*_in_txn` + `persist::insert_outbox_*_in_txn` + `persist::notify_outbox_seq_in_txn` in a single transaction (Phase 2c/2d outbox+notify path). `PgStore` is no longer mounted in AppState; it exists in `persist.rs` for `tests/persist_pg_tests.rs`. `atc_pg_write_failures_total` (labels: `kind=parity` or `kind=transient`) and `atc_pg_in_memory_drift_total` track PG write failures and post-commit in-memory divergence.
- **WebSocket:** Clients connect and receive SeqEvent stream in real time. Disconnection is clean (no crash, no effect on other clients)
- **Config:** ATC_GITHUB__WEBHOOK_SECRET loads webhook_secret. If None, HMAC verification skipped

## Testing

```bash
cargo test -p atc-server        # 110+ tests across three tiers (unit + integration)
cargo clippy -p atc-server -- -D warnings
cargo test -p atc-server --test e2e_tests  # 3 full-stack e2e tests
```

**Docker required:** The PG-backed tier (`db_readyz_tests`, `persist_pg_tests`, `transactional_writes_tests`, `outbox_tests`, `notify_listener_tests`, `phase_3c_*`) uses testcontainers to boot ephemeral PostgreSQL instances. `cargo test -p atc-server` (and `just test`) require Docker or OrbStack to be running.

macOS/OrbStack users: export `DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock` before running tests.

Test organization by tier:

- **Route-level oneshot tests** (`config_tests`, `routes_tests`, `webhook_hmac_tests`, `webhook_ingestion_tests`, `metrics`, `metrics_router_isolation`, `state_tests`) — Use tower's `oneshot()` to send requests directly through the router without binding a network port. Isolate endpoint behavior. (`sidecar_tests` was deleted in Phase 3b alongside the `pool_stats_after` field.)
- **Full-stack ephemeral tests** (`e2e_tests`, `ws_tests`) — Start an ephemeral TcpListener on 127.0.0.1:0, spawn a real server, send HTTP/WebSocket clients through real network I/O. Verify end-to-end flows.
- **PG-backed integration tests** (`db_readyz_tests`, `persist_pg_tests`, `transactional_writes_tests`, `outbox_tests`, `notify_listener_tests`) — Boot ephemeral Postgres via testcontainers, mount the router with a real `pg_pool: Some(pool)`, and exercise the transactional outbox path end-to-end (Phase 2b/2c/2d). `outbox_tests.rs` covers the 11 Phase 2c outbox ACs; `transactional_writes_tests.rs` covers the inverted-from-shadow behavioral contract (PG-failure → 503, transactional atomicity, no drift tolerated); `notify_listener_tests.rs` covers Phase 2d LISTEN/NOTIFY AC1–AC13 and follows the same testcontainers + `#[serial]` pattern.
- **Phase 3c tests** (`phase_3c_state_pg_read`, `phase_3c_drain_forwards`, `phase_3c_gap_healing`, `phase_3c_readyz`, `phase_3c_restart_recovery`, `phase_3c_row_lock_serialization`) — Dedicated PG-backed tests for Phase 3c contracts: REPEATABLE READ state snapshot with `broadcast_watermark` cursor (T1/T1b/T2/T3), drain broadcast end-to-end (T4/T5), gap-healing backstop + ring-buffer dedup with reverse-order concurrent commits (T6/T6b/T7), `/readyz` heartbeat staleness (T8/T9), restart recovery (T10), row-lock serialization (T11). T2 asserts `entity_count >= last_seq` — the snapshot can include commits the drain hasn't yet broadcast (which is safe, frontend applies them idempotently from the buffer). T6 uses two concurrent SQL transactions with reverse commit order to force the gap-healing rescan.

**Concurrency requirement:** Tests using `PrometheusMetricLayer::pair()` must be marked with `#[serial_test::serial]` because the PrometheusBuilder global recorder can only be installed once per binary. The `PROMETHEUS_INIT` OnceLock in `tests/common/mod.rs` ensures this is called exactly once and reused across tests.

## Key References

- Architecture: `docs/architecture/backend-server.md` (full design)
- Design plan: `docs/design-plans/2026-04-11-server-wiring.md`
- Domain model: `backend/crates/atc-core/` (StateStore, events)
- GitHub integration: `backend/crates/atc-github/` (parse_webhook, verify_signature)
