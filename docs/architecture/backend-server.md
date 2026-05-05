# Backend Server — Architecture

Last verified: 2026-05-04 (updated 2026-05-04 for Phase 2d LISTEN/NOTIFY: listener task, drain task, ATC_DATABASE_LISTENER_URL, pg_notify inside webhook transaction, five new metrics)

## Purpose

The backend server (`atc-server` crate) is an Axum HTTP server that serves as the single entry point for the ATC application. It provides:

- A REST API surface with liveness (`/healthz`) and readiness (`/readyz`) probes, expanded in future phases
- Frontend asset serving in release mode via rust-embed
- Development proxy to Vite dev server in debug mode via reqwest
- Configurable address binding and logging format via environment variables

The server binds to `http_addr` (default `0.0.0.0:8080`) configured via `ATC_HTTP_ADDR` environment variable and is the only executable crate in the backend workspace. The other two crates (`atc-core` for domain logic, `atc-github` for GitHub API integration) are libraries that the server depends on as features are added.

## GitHub API Integration

The `atc-github` crate provides webhook payload parsing and HMAC-SHA256 signature verification, acting as the boundary between raw HTTP events and the domain model. It has two public entry points:

- **`verify_signature(secret, body, signature)`** — Validates that a webhook signature matches the expected HMAC-SHA256 digest. Returns `VerifyError::InvalidSignature` if verification fails.
- **`parse_webhook(event_type, body)`** — Deserializes JSON payload and translates it to domain events. Accepts the `X-GitHub-Event` header value and raw body bytes. Returns a three-way `ParseResult`:
  - `Parsed(WebhookEvent)` — Successfully translated to either `WebhookEvent::Run(RunEventEnvelope)` or `WebhookEvent::Job(JobEventEnvelope)`
  - `Skipped { event_type }` — Unrecognized event type (e.g., `push`, `pull_request`) — not an error, simply not ATC's concern
  - `Err(ParseError)` — Deserialization or translation failed

#### Empty `runner_group_name` Normalization

GitHub webhook payloads may carry `runner_group_name: ""` (an empty string). The translation layer normalizes this to `None` in the resulting `RunnerInfo.group_name` before constructing domain events. This ensures downstream consumers (store, pool derivation, frontend TopBar) never observe `group_name: Some("")` — an empty string and `None` are semantically equivalent and are treated uniformly as "no group."

### Error Type Contracts

`ParseError` carries structured context for rich observability:

- **`InvalidJson(serde_json::Error)`** — Raw JSON deserialization failure. Includes the underlying serde error for debugging.
- **`UnknownAction { event_type, action }`** — The webhook arrived with an unrecognized action value (e.g., `"reopened"` in a `workflow_run` event). Fields identify which event type and what action was unexpected.
- **`MissingConclusion { event_type, action }`** — A `completed` action arrived without a `conclusion` field. Indicates either a GitHub API change or a malformed payload.
- **`UnknownConclusion { event_type, value }`** — The `conclusion` or step conclusion field contained an unrecognized value (e.g., `"timed_out_upgrade"`). Fields identify the event type (or step context) and the unexpected value for tracing.
- **`UnknownStatus { context, value }`** — A step's `status` field contained an unrecognized value. The `context` field identifies the step (e.g., `"step 'Setup Node'"`).

All error types derive `Debug` for detailed logging and implement `thiserror::Error` for ergonomic error handling and display formatting.

### Internal Module Layout

- **`webhook/mod.rs`** — Public API definitions (verify_signature, parse_webhook, ParseError, ParseResult, WebhookEvent enums)
- **`webhook/verify.rs`** — HMAC-SHA256 signature verification implementation
- **`webhook/types.rs`** — GitHub webhook payload serde structs (WorkflowRunWebhook, WorkflowJobWebhook, and nested types). These are `pub(crate)` only; the public API accepts raw bytes.
- **`webhook/translate.rs`** — Translation functions (translate_run, translate_job) that map GitHub's stringly-typed fields to atc-core domain enums. Private helpers parse conclusion/status strings with structured error context. Takes ownership of webhook payloads to avoid cloning strings.

The internal boundary is strict: only the webhook module exports public types. `lib.rs` re-exports the public API surface, and consumers never touch `types.rs` directly.

## Domain Model

The `atc-core` crate implements the canonical domain model for ATC, consisting of a three-level entity hierarchy and event-driven state management.

### Entity Hierarchy

- **WorkflowRun** — A GitHub Actions workflow invocation (one per push/PR/manual trigger). Identified by `run_id`. State: Queued → InProgress → Completed (Queued may skip directly to Completed for skipped/cancelled-before-start workflows).
- **Job** — A unit of work within a run (e.g., "test-linux", "build-docker"). Identified by `job_id`. Belongs to exactly one run. State: Queued → InProgress → Completed (with optional conclusion: Success, Failure, Skipped, etc.).
- **Step** — An individual action within a job (e.g., "Checkout code", "Run tests"). Identified by `step_id`. Belongs to exactly one job. Carries conclusion and conclusion text. Steps are immutable after completion.

### StateStore Architecture

The `StateStore` is the single source of truth for all entity state. It is backed by:

- **Primary maps** — `jobs: Map<JobId, Job>` and `runs: Map<RunId, WorkflowRun>` store complete entity snapshots. All mutations are made to these maps.
- **Secondary indexes** — `jobs_by_repo: Map<RepoIdentifier, Set<JobId>>` and `jobs_by_run: Map<RunId, Set<JobId>>` enable fast lookups by context. They are derived from the primary maps and rebuilt on every mutation.
- **RwLock for concurrency** — All state is protected by a single `Arc<RwLock<State>>`. Read operations (queries) acquire read locks; mutations (apply_event, evict_expired) acquire write locks.
- **Clock trait** — A pluggable time source (TestClock in tests, SystemClock in production) allows deterministic testing and clock mocking.
- **TTL eviction** — Completed jobs are retained for a configurable duration (default 1 hour). The `evict_expired()` method removes expired completed jobs from all maps and indexes. Active jobs are never evicted.

### Domain Events

The store mutates only through domain events, ensuring an audit trail and facilitating event replay:

- **RunEvent** — Models workflow run state: Requested, Queued, InProgress, Completed. Transitions are forward-only (no backward transitions allowed). `RunEventEnvelope` carries `workflow_name` and `workflow_path` as `Option<String>` to handle GitHub webhook payloads that omit the workflow object on some events (`in_progress`, `completed`). When present, these fields are stored; when `None`, the store preserves existing values via `.or()` pattern.
- **JobEvent** — Models job state: Queued, Waiting, InProgress, Completed. Also carries: run_id (which run this job belongs to), conclusion (success/failure/skipped/etc.), and steps (array of completed/active steps). The `Waiting` variant represents jobs waiting for approval (e.g., environment protection rules, required reviewers). `InProgress.runner` is `Option<RunnerInfo>` to handle GitHub's `in_progress` webhook arriving before runner assignment is complete; `None` falls through to the existing runner value via `.or()` pattern.

Events are created from GitHub webhook payloads by `atc-github` and applied to the store via `apply_event()`. The store validates state machine transitions and rejects invalid ones (e.g., Completed → Queued).

### State Machine Invariants

1. **Forward-only transitions** — A job cannot transition from Completed back to Queued. Violations return `StoreError::InvalidTransition`.
2. **Idempotent reapplication** — Applying the same event twice is safe; the second application is a no-op. This allows out-of-order event tolerance (e.g., a job event may arrive before its run event).
3. **Conclusion implies completion** — If a job has a conclusion set, its status must be Completed.
4. **Index consistency** — Every job in the primary map exists in exactly one of `jobs_by_repo` (by repo) and exactly one entry in `jobs_by_run` (by run_id). Applying or evicting a job updates both indexes.
5. **No orphaned jobs** — At the index level, every job is registered in `jobs_by_run` under its `run_id`. The run itself may not yet exist in the `runs` map if no `RunEvent` has arrived (out-of-order tolerance per AC3.5), but the job is always findable by its run_id.

### TTL Eviction and Cleanup

The `evict_expired()` method runs periodically (e.g., every 30 minutes) and removes completed jobs whose completion timestamp exceeds the configured TTL. Active jobs (Queued, InProgress) are never evicted; only Completed jobs are candidates. Eviction removes entries from all maps (`jobs`, `jobs_by_repo`, `jobs_by_run`) and cleans up empty index entries.

### Runner Pool Stats (Derived Views)

Runner pool statistics are derived views over the store's current state, computed on-demand by query methods. They reflect the number of active jobs, grouped by runner labels (e.g., "linux", "windows", "arm64"), along with pool metadata. These stats are served via REST API endpoints and do not require separate storage.

**RunnerPoolStats fields:**
- `labels: Vec<String>` — Runner label set (e.g., ["linux", "x86_64"]) grouped into this pool
- `group_name: String` — Friendly pool name (e.g., "Default", "macOS")
- `running: u32` — Count of currently running jobs in this pool
- `queued: u32` — Count of queued jobs waiting for a runner in this pool
- `is_elastic: bool` — Derived from runner `group_id == Some(0)`. Indicates whether the pool auto-scales (true) or has fixed capacity (false).
- `total: Option<u32>` — Maximum capacity of the pool. Always `None` until operator capacity configuration is implemented in a later phase. Used to render capacity bars and thresholds in the frontend.

#### Sort Order Contract

Both `StateStore::snapshot()` and `StateStore::pool_stats()` return `Vec<RunnerPoolStats>` sorted by `labels` lexicographically. This is the canonical wire order: sorting is centralized at these two producer sites so that every consumer (broadcast sidecar, REST endpoint, tests) receives the same deterministic order without re-sorting. Clients can perform exact `Vec<RunnerPoolStats>` equality comparisons without additional sorting logic.

## Key Decisions

**Decision:** Use `cfg!(debug_assertions)` to switch between embedded assets and dev proxy
**Alternatives considered:** Environment variable, feature flag, runtime configuration
**Rationale:** Compile-time switching is zero-cost and requires no configuration. Debug builds always proxy to Vite (developers always want HMR). Release builds always embed (deployment is a single binary). No ambiguity or misconfiguration possible.

**Decision:** Use rust-embed to bundle frontend assets into the binary
**Alternatives considered:** Serve from filesystem at runtime, use tower-http ServeDir
**Rationale:** Single-binary deployment is a project goal. rust-embed compiles `frontend/dist/` into the binary at build time. The tradeoff is longer release build times, but deployment simplicity outweighs this for a dashboard application.

**Decision:** Use reqwest for dev proxy instead of hyper directly
**Alternatives considered:** hyper client, tower Layer-based proxy
**Rationale:** reqwest provides a higher-level API that simplifies the proxy implementation. The dev proxy is not performance-critical (only used during development), so the slight overhead of reqwest over raw hyper is acceptable.

**Decision:** `/healthz` and `/readyz` stay at root; no backward-compat `/health` alias
**Alternatives considered:** Deprecation shim for `/health`, versioned health endpoints under `/v1/`
**Rationale:** No consumers of `/health` exist yet, so a no-alias rename is safe. Kubernetes conventions (kubelet probes) favor the `-z` suffix. Separate endpoints allow `/healthz` to signal liveness and `/readyz` to signal readiness independently in future phases.

**Decision:** Use figment with `ATC_*` environment variable prefix for all server configuration
**Alternatives considered:** clap for CLI flags, hand-rolled `std::env::var` reads, config file only
**Rationale:** figment's layered model (struct defaults → env vars) requires zero boilerplate and the nested `__` convention (`ATC_GITHUB__WEBHOOK_SECRET` → `config.github.webhook_secret`) sets up future GitHub configuration without rework. SocketAddr deserializes from string directly via serde, so no custom parsing is needed. Env-var-only configuration fits the container deployment model where chart values become env vars.

Config fields and their `ATC_*` env var overrides:
- `http_addr` (`ATC_HTTP_ADDR`) — default `0.0.0.0:8080`
- `metrics_addr` (`ATC_METRICS_ADDR`) — default `0.0.0.0:9090` (used from Phase 2)
- `database_url` (`ATC_DATABASE_URL`) — default `None`
- `database_listener_url` (`ATC_DATABASE_LISTENER_URL`) — default `None` (falls back to `ATC_DATABASE_URL` when unset). Use to point the PG listener at a session-mode endpoint when the main pool runs through transaction-mode PgBouncer.
- `log_filter` (`ATC_LOG_FILTER`) — default `"info"` (passed to `EnvFilter`)
- `log_format` (`ATC_LOG_FORMAT`) — default `pretty` in debug builds, `json` in release builds

**Decision:** Branch tracing format on `LogFormat` (debug → pretty, release → JSON)
**Alternatives considered:** Always JSON, always pretty, runtime-only env var
**Rationale:** Developer builds benefit from ANSI-colored pretty output without any configuration. Production/container builds default to structured JSON for log aggregators. Both can be overridden via `ATC_LOG_FORMAT`, satisfying the override ACs without special-casing in code. The `cfg!(debug_assertions)` default mirrors the existing assets.rs pattern for compile-time branching.

## PostgreSQL Integration (Phase 2a)

ATC uses [sqlx](https://github.com/launchdarkis/sqlx) as its PostgreSQL client. The pool is created on startup when `ATC_DATABASE_URL` is set.

### Startup behavior

| Scenario | Behavior |
|---|---|
| `ATC_DATABASE_URL` unset | In-memory mode; `pg_pool = None`; no migration step |
| `ATC_DATABASE_URL` set, connect fails | `tracing::error!` + `process::exit(1)` |
| `ATC_DATABASE_URL` set, connect succeeds, migrations fail | `tracing::error!` + `process::exit(1)` |
| `ATC_DATABASE_URL` set, everything succeeds, DB lost at runtime | Process stays up; `/readyz` returns 503 |

### Schema migrations

Migrations live in `backend/crates/atc-server/migrations/`. They are embedded in the binary at compile time via `sqlx::migrate!("./migrations")` and run automatically on startup. Re-running is idempotent (tracked by `_sqlx_migrations` table).

Current schema (Phase 2c):
- `0001_initial_runs_jobs.sql` — Creates `runs` and `jobs` tables with columns, FK, CHECK constraints, and indexes to support Phase 3c read patterns and TTL eviction.
- `0002_outbox.sql` — Creates `outbox` table with `BIGSERIAL seq` primary key (durable monotonic-not-gapless cursor), `kind TEXT CHECK('run','job')`, `run_id BIGINT`, `job_id BIGINT NULL`, `payload JSONB` (stores `RunEventEnvelope` / `JobEventEnvelope` — NOT `SeqEvent`), `inserted_at TIMESTAMPTZ DEFAULT now()`. Index on `run_id` for Phase 3c forwarder drain. No FK to `runs` (append-only log; eviction is independent).

### sqlx feature flags

`sqlx` is configured with: `postgres, runtime-tokio, tls-rustls-aws-lc-rs, chrono, migrate, macros, json`

- `macros` — Enables `query!`/`query_as!` for compile-time SQL checking (used from Phase 2b). Requires either a live DB or the `.sqlx/` offline cache at compile time.
- `migrate` — Enables `sqlx::migrate!()` for binary-embedded migrations (does NOT require a live DB at compile time).

### Testing

Backend tests that require PostgreSQL use [testcontainers](https://testcontainers.com) to boot ephemeral containers. **`just test` requires Docker or OrbStack to be running.**

macOS/OrbStack users: export `DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock` before running `just test`.

## Boundaries

**Owns:** HTTP routing, request handling, frontend asset serving, dev proxy, server lifecycle (bind, serve, shutdown)
**Does not own:** Domain logic (atc-core), GitHub API integration (atc-github), frontend build process, authentication (future phase)
**Prohibitions:** Do not put business logic in route handlers — extract to atc-core. Do not call GitHub API directly from handlers — use atc-github. Do not serve assets from filesystem in release mode — always use rust-embed.

## Metrics

The server binds a second TCP listener (default `0.0.0.0:9090`, overridden via
`ATC_METRICS_ADDR`) exclusively for the Prometheus scrape endpoint. Serving
metrics on a separate port keeps the metrics surface out of the application
ingress and allows Kubernetes `NetworkPolicy` rules to grant scrape access to
Prometheus without exposing the full API.

### axum-prometheus placement

`PrometheusMetricLayer` wraps the main API router (not the metrics router).
Every request to `http_addr` is counted in `axum_http_requests_total` and timed
in `axum_http_requests_duration_seconds`. The metrics router itself is never
wrapped — scrape requests do not appear in request metrics.

`axum-prometheus` installs the global `metrics` recorder via
`PrometheusMetricLayer::pair()`. Do not install `PrometheusBuilder` separately;
doing so will panic with a duplicate-recorder error.

### atc_build_info labels

`register_build_info()` (called once at startup) sets a gauge always equal to
`1.0` with these labels:

| Label | Source | Example |
|---|---|---|
| `version` | `CARGO_PKG_VERSION` | `0.2.0` |
| `git_sha` | `VERGEN_GIT_SHA` (via `build.rs`) | `a1b2c3d...` |
| `rustc_version` | `VERGEN_RUSTC_SEMVER` (via `build.rs`) | `1.94.0` |
| `build_timestamp` | `VERGEN_BUILD_TIMESTAMP` (via `build.rs`) | `2026-04-08T...` |
| `target_triple` | `VERGEN_CARGO_TARGET_TRIPLE` (via `build.rs`) | `x86_64-unknown-linux-gnu` |

`build.rs` uses the `vergen-gix` crate (pure-Rust gix backend; no libgit2
dependency) and emits all five vars as `cargo:rustc-env=` instructions.

### Process collector

`spawn_process_collector()` starts a detached tokio task that calls
`metrics_process::Collector::default().collect()` every 10 seconds. It uses the
same global recorder installed by axum-prometheus. Emitted families include
`process_cpu_seconds_total`, `process_resident_memory_bytes`,
`process_virtual_memory_bytes`, `process_open_fds`, `process_max_fds`,
`process_start_time_seconds`, and `process_threads`.

### PG write counters (Phase 2b/2c)

`atc_pg_write_failures_total` is a counter incremented when a PG write fails.
It carries a `kind` label:

| Label | When | Severity |
|-------|------|----------|
| `kind="parity"` | PG UPSERT matches 0 rows (WHERE predicate rejected): transition invalid under PG predicate | Page-worthy: indicates state machine drift |
| `kind="transient"` | sqlx error on `pool.begin()`, mid-txn, or `tx.commit()` | Alert on sustained rate |

`atc_pg_in_memory_drift_total` counts events where PG committed successfully but
the in-memory apply subsequently diverged — complementary observability to the
failure counter.

Transient PG failures (503) are visible to GitHub and will be retried by GitHub's
delivery mechanism. Parity rejections (200 `{"status":"rejected"}`) are not retried.
Post-commit in-memory drift is logged as a warning; the event is durably recorded
in PG (UPSERT + outbox row) and will be recoverable from the outbox.
These counters are registered at startup via `register_pg_write_counters()` and
appear in `/metrics` output only if PG writes have been attempted.

### LISTEN/NOTIFY metrics (Phase 2d)

Five counters added in Phase 2d for the listener/drain pipeline:

| Counter | Description |
|---------|-------------|
| `atc_pg_notify_emitted_total{kind}` | Incremented each time `pg_notify('atc_outbox', seq::text)` is called inside a webhook transaction. `kind` matches the event discriminator (`run` or `job`). |
| `atc_pg_notify_received_total` | Incremented each time the listener task receives a NOTIFY from PG. |
| `atc_pg_listener_recv_errors_total` | Incremented each time the listener task encounters a receive error (e.g., connection drop during reconnect window). |
| `atc_pg_drain_passes_total` | Incremented each time the drain task wakes up and executes a fetch pass (whether or not any rows are returned). |
| `atc_pg_drain_rows_total` | Incremented by the number of outbox rows fetched in each drain pass. |

### Listener always binds

The metrics listener binds unconditionally at startup regardless of the chart's
`metrics.enabled` value. This is intentional: the chart flag controls whether
Prometheus discovers the endpoint (via ServiceMonitor or pod annotations); the
port is always open so that `kubectl port-forward` and ad-hoc `curl` work
without chart-level changes.

## Server Wiring

The server wires together `atc-core` (state store) and `atc-github` (webhook parsing) into a cohesive HTTP API. The design separates **state mutation** (webhook ingestion) from **state delivery** (REST snapshot and WebSocket stream), allowing each path to evolve independently.

### AppState

A shared `AppState` struct is passed to all handlers via Axum's `State` extractor:

```rust
struct AppState {
    store: Arc<StateStore>,
    webhook_tx: broadcast::Sender<SeqEvent>,
    webhook_secret: Option<String>,
    seq: Mutex<u64>,
    pg_pool: Option<sqlx::PgPool>,
}
```

- **`store`** — Reference to the shared `atc-core` StateStore. All webhook events are applied here, and REST/WebSocket endpoints read from here.
- **`webhook_tx`** — Sender side of a bounded `tokio::sync::broadcast` channel (capacity 256). Every successfully processed webhook is broadcast as a `SeqEvent`. WebSocket clients subscribe as receivers.
- **`webhook_secret`** — Optional GitHub webhook secret loaded from `ATC_GITHUB__WEBHOOK_SECRET`. If `None`, HMAC verification is skipped. If `Some`, signatures are required and validated.
- **`seq`** — `tokio::sync::Mutex<u64>` counter incremented on each successfully ingested event. Protected by a mutex (not an atomic) so that the webhook handler holds the lock across `pool.begin()` + PG commit + in-memory store mutation + seq assignment + broadcast, and the state handler holds it across snapshot + seq read. This ensures WS event seq order matches durable commit order, and REST snapshots are consistent with their cursor. Resets on server restart.
- **`pg_pool`** — `Option<sqlx::PgPool>`. `Some` when `ATC_DATABASE_URL` is configured and a connection pool was successfully created and migrated at startup. `None` in in-memory mode. Used by the webhook handler for transactional writes and by the `/readyz` probe. `PgStore` is no longer mounted in AppState (removed in Phase 2c); the handler calls `persist::upsert_*_in_txn` and `persist::insert_outbox_*_in_txn` directly via `&mut tx`.

The listener task and drain task receive a clone of the existing `CancellationToken`; `AppState` gains no new fields — task handles live in `main.rs` scope.

### SeqEvent Sidecar Contract

`SeqEvent` is the broadcast envelope carrying domain events and derived state:

```rust
pub struct SeqEvent {
    pub seq: u64,
    pub event: WebhookEvent,
    pub pool_stats_after: Option<Vec<RunnerPoolStats>>,
}
```

The `pool_stats_after` field carries a snapshot of the runner pool state taken under the seq mutex immediately after a successful event application:

- **Job events:** `pool_stats_after` is `Some(vec)`, containing the pool stats at that moment. The vector is sorted by `labels` lexicographically per the sort-order contract.
- **Run events:** `pool_stats_after` is `None`. Run-level state does not derive into pool stats.
- **Failed transitions:** No broadcast occurs and no `SeqEvent` is emitted. Clients never receive events that are not reflected in the store (per AC1.5).

**Wire format:** The field serializes as `poolStatsAfter` (camelCase) in JSON via `#[serde(rename_all = "camelCase")]`. TypeScript types are emitted as `poolStatsAfter: Array<RunnerPoolStats> | null` via ts-rs.

**Interaction with REST:** The `pool_stats_after` sidecar complements (does not replace) `StateSnapshot.poolStats` on `GET /v1/state`. The REST snapshot is the only consistent, atomic view a client needs to bootstrap; the WebSocket sidecar is an incremental, real-time feed for display updates.

### Webhook Ingestion (`POST /v1/webhooks/github`)

**Responsibility:** Receive GitHub webhook payloads, verify signatures, parse to domain events, apply to store, and publish to broadcast channel.

**Flow:**
1. Extract `X-GitHub-Event` header and raw body from HTTP request.
2. If `webhook_secret` is configured, verify HMAC-SHA256 signature from `X-Hub-Signature-256` header (via `atc_github::verify_signature`). Return 401 if verification fails.
3. Parse payload via `atc_github::parse_webhook(event_type, body)`, yielding one of:
   - `ParseResult::Parsed(Box<WebhookEvent>)` — Continue to store ingestion
   - `ParseResult::Skipped { event_type }` — Return 200 with `{"status": "skipped"}`
   - `ParseResult::Err(ParseError)` — Return 422 with error details
4. Unbox the event and acquire the `seq` mutex **before any PG I/O**. This is the critical ordering invariant: two concurrent webhooks that race to commit cannot broadcast in a different order than they committed.
5. Branch on `pg_pool`:
   - **`Some(pool)` — Transactional PG path:**
     - Call `pool.begin()` to open a transaction. On failure → 503, drop mutex.
     - Call `persist::upsert_run_in_txn` or `persist::upsert_job_in_txn` inside the transaction. On `PersistError::InvalidTransition` → 200 `{"status":"rejected"}` (auto-rollback, increment `atc_pg_write_failures_total{kind="parity"}`). On `PersistError::Backend` → 503 (increment `kind="transient"`).
     - Call `persist::insert_outbox_run_in_txn` or `persist::insert_outbox_job_in_txn` in the same transaction. On failure, same error path as above.
     - Emit `SELECT pg_notify('atc_outbox', seq::text)` inside the same transaction before commit. PG queues the NOTIFY and delivers it to all listeners on COMMIT; an aborted transaction silently drops the notification. Metric: `atc_pg_notify_emitted_total{kind}`.
     - `tx.commit()`. On failure → 503 `{"status":"database commit failed"}`, increment `kind="transient"`.
     - **After commit:** apply to in-memory store under the same mutex. If in-memory apply fails after PG committed, increment `atc_pg_in_memory_drift_total`, log warning, skip broadcast. Return 200.
   - **`None` — In-memory-only path:** Apply the parsed event directly to the in-memory store via `store.apply_run_event()` or `store.apply_job_event()`. If the transition is invalid, log warning and skip broadcast (no SeqEvent emitted for rejected transitions).
6. On successful apply (either path): increment `seq` by 1, broadcast a `SeqEvent { seq, event, pool_stats_after }` to the webhook channel (still under the mutex).
7. Release the `seq` mutex. Return 200 with `{"status": "processed"}`.

**Error responses:**
- **400** — Missing `X-GitHub-Event` header
- **401** — Invalid or missing signature when secret is configured; SHA-1 signature when SHA-256 is expected
- **422** — Malformed JSON body or unknown action/conclusion values
- **503** — PG `pool.begin()` failed, mid-txn backend error, or `tx.commit()` failed

**Ordering guarantee:** The `seq` mutex is acquired before `pool.begin()` and released after broadcast. This serializes the full pipeline (PG begin → PG commit → in-memory apply → seq increment → broadcast) so that: (a) `seq` values are strictly monotonically increasing with no gaps, (b) their order always matches durable PG commit order, and (c) all events up to a given seq have been broadcast before the mutex is released. `state_handler` can never observe a seq cursor that advertises events WS clients haven't received yet.

### WebSocket Event Stream (`GET /v1/ws`)

**Responsibility:** Accept WebSocket upgrades and push `SeqEvent`s to connected clients in real time.

**Flow:**
1. Accept WebSocket upgrade request via Axum's `WebSocketUpgrade` extractor
2. Create a new broadcast receiver: `webhook_tx.subscribe()`
3. Spawn a task that:
   - Awaits messages from the receiver in a loop
   - Serializes each `SeqEvent` to JSON
   - Sends the JSON as a text frame to the WebSocket client
   - On `RecvError::Lagged` (buffer overflow), logs warning but does not disconnect — the client can recover by fetching `/v1/state` to resync
4. If the client disconnects, the task exits cleanly (no crash; other clients unaffected)

**Lag handling:** If a client is slow and the broadcast channel buffer overflows, `recv()` returns `Err(RecvError::Lagged)`. The handler logs this as a warning and continues, allowing the client to reconnect and fetch the current state via REST. This prevents one slow client from blocking or crashing the server.

### REST State Snapshot (`GET /v1/state`)

**Responsibility:** Return the full current state snapshot and the next `seq` to assign.

**Flow:**
1. Acquire the `seq` mutex (prevents any webhook from committing during the read)
2. Read the store snapshot under the store's read lock
3. Read `seq` from the mutex guard
4. Release the mutex, then serialize the response as a `StateSnapshot`:
   ```rust
   struct StateSnapshot {
       seq: u64,           // Next seq to assign; acts as a cursor
       runs: Vec<WorkflowRun>,
       jobs: Vec<Job>,
       pool_stats: Vec<PoolStat>,
   }
   ```
5. The `seq` value reflects the state: all events with `seq < N` are reflected in the snapshot; the next event will receive `seq = N`. This guarantee holds because the mutex excludes concurrent webhook writes during the read.
6. Return 200 with the JSON snapshot.

**Snapshot/stream reconciliation:** A client can call `GET /v1/state` to establish baseline state, note the returned `seq`, then connect to `GET /v1/ws` and filter incoming `SeqEvent`s to those with `seq >= cursor`. This protocol allows robust reconnection and bootstrap.

### Configuration

Server wiring configuration extends the existing figment-based config:

- **`github.webhook_secret`** (`ATC_GITHUB__WEBHOOK_SECRET` env var) — Optional string. If present, all webhook requests must carry a valid HMAC-SHA256 signature. If absent, signatures are not required or verified.

### Lifecycle Wiring

In `main.rs`:
1. Load config via `Config::load()`
2. If `ATC_DATABASE_URL` is set: call `atc_server::db::init_pool(url)` — connects `sqlx::PgPool` and runs embedded migrations; exit(1) on failure
3. Create `StateStore` with `SystemClock` and TTL (default 1 hour)
4. Create broadcast channel: `tokio::sync::broadcast::channel(256)`
5. Call `metrics::register_pg_write_counters()` after the recorder is installed (Phase 2b/2c)
6. Create `AppState` with all components (`store`, `webhook_tx`, `webhook_secret`, `seq`, `pg_pool`) and pass to Axum via `.with_state()`. Note: `pg_store` was removed in Phase 2c; the webhook handler uses `pg_pool` directly via `persist::*_in_txn` helpers.
7. Start background eviction task: `start_eviction_task(store.clone())`
8. **If `pg_pool` is `Some`** (Phase 2d listener init sequence, before `axum::serve`):
   1. Derive `listener_url` from `cfg.database_listener_url` if set, else fall back to `cfg.database_url`.
   2. `PgListener::connect(&listener_url)` — fail-fast on Err (exit(1)).
   3. `listener.listen(NOTIFY_CHANNEL)` — fail-fast on Err (exit(1)).
   4. `SELECT COALESCE(MAX(seq), 0) FROM outbox` — initialize watermark; fail-fast on Err (exit(1)).
   5. Spawn listener task (receives PG NOTIFYs, fires `Arc<Notify>`).
   6. Spawn drain task (wakes on `Arc<Notify>`, fetches `seq > watermark ORDER BY seq`, logs, advances watermark).
9. Bind the server to `http_addr` via `axum::serve`
10. On graceful shutdown, abort the eviction task, listener task, and drain task

The eviction task runs periodically (default every 30 minutes) and removes completed jobs whose completion timestamp exceeds the TTL. This keeps in-memory state bounded and prevents unbounded growth.

### Health Probes

- `/readyz` — Readiness probe. Returns 200 `{"status":"ok"}` in in-memory mode (no DB configured). When `ATC_DATABASE_URL` is configured, runs `SELECT 1` against the pool; returns 200 on success or 503 `{"status":"db_unreachable"}` if the DB is unreachable.
- `/healthz` — Liveness probe. Unchanged — process up = healthy regardless of DB state.

### NOTIFY Emission and Listener Fetch-Stub (Phase 2d)

**Two-task coalescing structure:** The listener pipeline splits into two cooperating tasks:

1. **Listener task** — Holds a dedicated long-lived `PgListener` connection. Receives PG NOTIFY payloads on the `atc_outbox` channel, increments `atc_pg_notify_received_total`, and fires an `Arc<tokio::sync::Notify>` to wake the drain task. Does not fetch rows itself.
2. **Drain task** — Waits on the `Arc<Notify>` (level-triggered). On each wake, fetches all outbox rows with `seq > watermark ORDER BY seq` from the pool, logs each row (Phase 2d stub — not yet forwarding to WS clients), increments `atc_pg_drain_rows_total` and `atc_pg_drain_passes_total`, and advances the local `watermark`. If a NOTIFY arrives while a drain is in progress, the coalescing flag ensures one additional pass runs after the current one completes rather than starting a concurrent fetch.

**DSN session-mode contract:** The listener connection (`ATC_DATABASE_LISTENER_URL` or fallback to `ATC_DATABASE_URL`) must be a session-mode endpoint. Transaction-mode PgBouncer reassigns the underlying connection between transactions, silently dropping LISTEN registrations. When the main pool uses transaction-mode PgBouncer, set `ATC_DATABASE_LISTENER_URL` to a direct Postgres DSN or a session-mode PgBouncer endpoint.

**Reconnect loss window:** If the listener task reconnects after a connection drop, any NOTIFYs delivered during the reconnect window are not received. This is healed automatically: the next NOTIFY after reconnection triggers a drain pass that fetches `seq > watermark`, catching up all rows that were inserted while the listener was disconnected. No data is lost; only latency increases during the reconnect window.

**Phase 3c pivot:** The drain task currently logs rows as a stub. In Phase 3c, the only code change inside the drain loop is adding a call to `forward_to_ws_clients(rows)` — the task structure, watermark semantics, and coalescing logic are identical.

### Modules

- **`state.rs`** — `AppState` struct (fields: `store`, `webhook_tx`, `webhook_secret`, `seq`, `pg_pool`), `SeqEvent`, `StateSnapshot` types
- **`routes.rs`** — Route handlers for `/v1/webhooks/github`, `/v1/ws`, `/v1/state`; webhook handler transactional outbox flow (Phase 2c)
- **`ws.rs`** — WebSocket connection handling and message broadcast logic
- **`persist.rs`** — `PgStore` implementing `PersistentStore` trait (used in integration tests); `pub(crate)` transaction helpers `upsert_run_in_txn`, `upsert_job_in_txn`, `insert_outbox_run_in_txn`, `insert_outbox_job_in_txn` called by the webhook handler (Phase 2c)

## Files

- `backend/crates/atc-server/src/main.rs` — Server entry point, config loading, tracing branching, router composition, eviction task lifecycle
- `backend/crates/atc-server/src/config.rs` — figment-based Config struct, LogFormat enum, GitHubConfig with webhook_secret, Config::load()
- `backend/crates/atc-server/src/db.rs` — `init_pool(url)`: connects sqlx PgPool and runs embedded migrations; extracted from main so it is reachable by integration tests
- `backend/crates/atc-server/src/routes.rs` — API route definitions (healthz, readyz, webhook, state, ws endpoints)
- `backend/crates/atc-server/src/state.rs` — AppState struct, SeqEvent, StateSnapshot types
- `backend/crates/atc-server/src/ws.rs` — WebSocket handler, broadcast subscription, SeqEvent serialization
- `backend/crates/atc-server/src/assets.rs` — rust-embed struct, embedded file serving, SPA fallback, dev proxy
- `backend/crates/atc-server/src/metrics.rs` — Prometheus layer, build_info gauge, process collector, PG write counter registration (`atc_pg_write_failures_total`, `atc_pg_in_memory_drift_total`) (Phase 2b/2c)
- `backend/crates/atc-server/src/persist.rs` — `PgStore` struct implementing `PersistentStore` (used in `tests/persist_pg_tests.rs`); `pub(crate)` transaction helpers for UPSERT+outbox pattern used by webhook handler (Phase 2c)
- `backend/crates/atc-server/src/listener.rs` — PG LISTEN/NOTIFY background tasks: `start_listener_task` (receives notifications, fires `Arc<Notify>`) and `start_drain_task` (wakes on notify, fetches outbox rows by `seq > watermark ORDER BY seq`, logs, advances watermark). Spawned only when `pg_pool` is `Some`. (Phase 2d)
- `backend/crates/atc-server/build.rs` — vergen-gix Emitter emitting VERGEN_* compile-time env vars; also emits `cargo:rerun-if-changed=migrations` so sqlx::migrate!() re-embeds files on SQL changes
- `backend/crates/atc-server/migrations/0001_initial_runs_jobs.sql` — Initial schema: `runs` and `jobs` tables with CHECK constraints, FK, and indexes
- `backend/crates/atc-server/migrations/0002_outbox.sql` — Outbox table: `BIGSERIAL seq` PK, `kind` discriminator, `run_id`/`job_id`, `payload JSONB` (domain event envelope), `inserted_at TIMESTAMPTZ`; `outbox_run_idx` on `run_id`
- `backend/.sqlx/` — Offline query cache (committed to repo). Generated by `cargo sqlx prepare --workspace -- --tests` during Phase 2b to enable `SQLX_OFFLINE=true` builds without a live DB at compile time. Updated whenever SQL queries change.
- `backend/Cargo.toml` — Workspace definition with shared dependency versions
- `backend/crates/atc-server/Cargo.toml` — Server crate dependencies
