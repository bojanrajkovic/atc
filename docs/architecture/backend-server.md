# Backend Server — Architecture

Last verified: 2026-05-09

## Purpose

The backend server (`atc-server` crate) is an Axum HTTP server that serves as the single entry point for the ATC application. It provides:

- A REST API surface with liveness (`/healthz`) and readiness (`/readyz`) probes
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

### PG Table Notes

The `runs` PG table carries a `placeholder BOOLEAN NOT NULL DEFAULT false` column. Rows with `placeholder = true` are FK-only stubs created when a job event arrives before its parent run event; they are promoted to real rows when the matching `workflow_run` webhook arrives. The state snapshot (`/v1/state`) always reads `WHERE placeholder = false`.

### RunStateMachine Architecture

The `RunStateMachine` is the single source of truth for all entity state. It is backed by:

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

1. **Forward-only transitions** — A job cannot transition from Completed back to Queued. Violations return `StateMachineError::InvalidTransition`.
2. **Idempotent reapplication** — Applying the same event twice is safe; the second application is a no-op. This allows out-of-order event tolerance (e.g., a job event may arrive before its run event).
3. **Conclusion implies completion** — If a job has a conclusion set, its status must be Completed.
4. **Index consistency** — Every job in the primary map exists in exactly one of `jobs_by_repo` (by repo) and exactly one entry in `jobs_by_run` (by run_id). Applying or evicting a job updates both indexes.
5. **No orphaned jobs** — At the index level, every job is registered in `jobs_by_run` under its `run_id`. The run itself may not yet exist in the `runs` map if no `RunEvent` has arrived (out-of-order tolerance), but the job is always findable by its run_id.

### TTL Eviction and Cleanup

The `evict_expired()` method runs periodically (e.g., every 30 minutes) and removes completed jobs whose completion timestamp exceeds the configured TTL. Active jobs (Queued, InProgress) are never evicted; only Completed jobs are candidates. Eviction removes entries from all maps (`jobs`, `jobs_by_repo`, `jobs_by_run`) and cleans up empty index entries.

### Runner Pool Stats (Frontend-Derived)

Runner pool statistics are derived views over the current job state. They are computed entirely on the frontend by `computePoolStats(jobs: Job[]): RunnerPoolStats[]` (in `frontend/src/lib/stores/runners.svelte.ts`); the backend does not ship them on the wire. The `RunnerPoolStats` type still derives `#[derive(TS)]` so the generated TypeScript type used by the frontend stays under ts-rs control.

**RunnerPoolStats fields (used by the frontend derivation):**
- `labels: Vec<String>` — Runner label set (e.g., ["linux", "x86_64"]) grouped into this pool
- `group_name: String` — Friendly pool name (e.g., "Default", "macOS")
- `running: u32` — Count of currently running jobs in this pool
- `queued: u32` — Count of queued jobs waiting for a runner in this pool
- `is_elastic: bool` — Derived from runner `group_id == Some(0)`. Indicates whether the pool auto-scales (true) or has fixed capacity (false).
- `total: Option<u32>` — Maximum capacity of the pool. Always `None` until operator capacity configuration is implemented. Used to render capacity bars and thresholds in the frontend.

The snapshot returns `QueryResult` only (no inline pool-stats computation). The wire carries no `pool_stats` or `pool_stats_after` fields (see ADR 0004). The lexicographic sort by `labels` is the responsibility of `computePoolStats` on the frontend.

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
**Rationale:** No consumers of `/health` exist yet, so a no-alias rename is safe. Kubernetes conventions (kubelet probes) favor the `-z` suffix. Separate endpoints allow `/healthz` to signal liveness and `/readyz` to signal readiness independently.

**Decision:** Use figment with `ATC_*` environment variable prefix for all server configuration
**Alternatives considered:** clap for CLI flags, hand-rolled `std::env::var` reads, config file only
**Rationale:** figment's layered model (struct defaults → env vars) requires zero boilerplate and the nested `__` convention (`ATC_GITHUB__WEBHOOK_SECRET` → `config.github.webhook_secret`) sets up future GitHub configuration without rework. SocketAddr deserializes from string directly via serde, so no custom parsing is needed. Env-var-only configuration fits the container deployment model where chart values become env vars.

Config fields and their `ATC_*` env var overrides:
- `http_addr` (`ATC_HTTP_ADDR`) — default `0.0.0.0:8080`
- `metrics_addr` (`ATC_METRICS_ADDR`) — default `0.0.0.0:9090`
- `database_url` (`ATC_DATABASE_URL`) — default `None`
- `database_listener_url` (`ATC_DATABASE_LISTENER_URL`) — default `None` (falls back to `ATC_DATABASE_URL` when unset). Use to point the PG listener at a session-mode endpoint when the main pool runs through transaction-mode PgBouncer.
- `log_filter` (`ATC_LOG_FILTER`) — default `"info"` (passed to `EnvFilter`)
- `log_format` (`ATC_LOG_FORMAT`) — default `pretty` in debug builds, `json` in release builds

**Decision:** Branch tracing format on `LogFormat` (debug → pretty, release → JSON)
**Alternatives considered:** Always JSON, always pretty, runtime-only env var
**Rationale:** Developer builds benefit from ANSI-colored pretty output without any configuration. Production/container builds default to structured JSON for log aggregators. Both can be overridden via `ATC_LOG_FORMAT`, satisfying the override ACs without special-casing in code. The `cfg!(debug_assertions)` default mirrors the existing assets.rs pattern for compile-time branching.

## PostgreSQL Integration

ATC uses [sqlx](https://github.com/launchdarkis/sqlx) as its PostgreSQL client. The pool is created on startup when `ATC_DATABASE_URL` is set.

### Storage modes — operator guidance

ATC supports two runtime storage modes:

- **External Postgres** (`ATC_DATABASE_URL` set) — the production-supported mode. Required for any deployment with `replicaCount > 1` (the Helm chart's template-render-time `{{ fail }}` guard refuses to render multi-replica without a Postgres URL). The webhook handler is write-only (transactional UPSERT + outbox INSERT + `pg_notify`); the drain task is the sole broadcaster; `/v1/state` reads from a REPEATABLE READ snapshot.
- **In-memory** (`ATC_DATABASE_URL` unset) — **dev-only**. Single-replica only. State lives in `atc_core::RunStateMachine` behind an `RwLock`; events broadcast directly from the webhook handler under the seq mutex; on process exit, all state is lost. Useful for `just dev` against curl-fired or smee.io-tunneled webhooks; do not run this in production. Multi-replica deployments using this mode would silently fork state per replica with no convergence — there is no leader, no write replication, and no readback synchronization.

If you find yourself wanting to run in-memory mode against more than one replica, the answer is: configure Postgres. The chart guard catches this at `helm template` / `helm install` time; the binary's `ensure_pg_scheme()` catches the misconfigured-URL variant at startup.

### Startup behavior

| Scenario | Behavior |
|---|---|
| `ATC_DATABASE_URL` unset | In-memory mode; `pg_pool = None`; no migration step |
| `ATC_DATABASE_URL` (or `ATC_DATABASE_LISTENER_URL`) set with a non-`postgres://` / `postgresql://` scheme | `ensure_pg_scheme()` in `main.rs` logs `"<VAR> must be a postgres:// or postgresql:// URL; got scheme <X>. ATC only supports external PostgreSQL."` and `process::exit(1)` BEFORE any sqlx call. Mirrors the chart-time guard in `deploy/helm/atc/templates/deployment.yaml`. |
| `ATC_DATABASE_URL` set, connect fails | `tracing::error!` + `process::exit(1)` |
| `ATC_DATABASE_URL` set, connect succeeds, migrations fail | `tracing::error!` + `process::exit(1)` |
| `ATC_DATABASE_URL` set, everything succeeds, DB lost at runtime | Process stays up; `/readyz` returns 503 |
| `ATC_DATABASE_URL` set, drain task active | First-pass startup latency observed via `atc_pg_drain_startup_seconds` (one observation per process lifetime) |

### Schema migrations

Migrations live in `backend/crates/atc-server/migrations/`. They are embedded in the binary at compile time via `sqlx::migrate!("./migrations")` and run automatically on startup. Re-running is idempotent (tracked by `_sqlx_migrations` table).

Current schema:
- `0001_initial_runs_jobs.sql` — Creates `runs` and `jobs` tables with columns, FK, CHECK constraints, and indexes to support the read patterns and TTL eviction.
- `0002_outbox.sql` — Creates `outbox` table with `BIGSERIAL seq` primary key (durable monotonic-not-gapless cursor), `kind TEXT CHECK('run','job')`, `run_id BIGINT`, `job_id BIGINT NULL`, `payload JSONB` (stores `RunEventEnvelope` / `JobEventEnvelope` — NOT `SeqEvent`), `inserted_at TIMESTAMPTZ DEFAULT now()`. Index on `run_id` for the drain forwarder. No FK to `runs` (append-only log; eviction is independent).
- `0003_runs_placeholder.sql` — Adds `placeholder BOOLEAN NOT NULL DEFAULT false` to the `runs` table. The job-stub INSERT in `upsert_job_in_txn` writes `placeholder = true` to satisfy the FK constraint when a job event arrives before its parent run event. Real workflow_run UPSERTs leave `placeholder = false`, and the `ON CONFLICT UPDATE` clause explicitly sets `placeholder = false` to promote stubs to real rows. `/v1/state` reads `WHERE placeholder = false` to exclude stub rows from snapshots.

### sqlx feature flags

`sqlx` is configured with: `postgres, runtime-tokio, tls-rustls-aws-lc-rs, chrono, migrate, macros, json`

- `macros` — Enables `query!`/`query_as!` for compile-time SQL checking. Requires either a live DB or the `.sqlx/` offline cache at compile time.
- `migrate` — Enables `sqlx::migrate!()` for binary-embedded migrations (does NOT require a live DB at compile time).

### Testing

Backend tests that require PostgreSQL use [testcontainers](https://testcontainers.com) to boot ephemeral containers. **`just test` requires Docker or OrbStack to be running.**

macOS/OrbStack users: export `DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock` before running `just test`.

## Boundaries

**Owns:** HTTP routing, request handling, frontend asset serving, dev proxy, server lifecycle (bind, serve, shutdown)
**Does not own:** Domain logic (atc-core), GitHub API integration (atc-github), frontend build process, authentication
**Prohibitions:** Do not put business logic in route handlers — extract to atc-core. Do not call GitHub API directly from handlers — use atc-github. Do not serve assets from filesystem in release mode — always use rust-embed.

## Metrics

The server binds a second TCP listener (default `0.0.0.0:9090`, overridden via `ATC_METRICS_ADDR`) for the Prometheus scrape endpoint. Serving metrics on a separate port keeps the metrics surface out of the application ingress and lets Kubernetes `NetworkPolicy` rules grant scrape access to Prometheus without exposing the full API.

See [`metrics.md`](metrics.md) for the metric authoring contract, axum-prometheus wiring, and per-metric interpretation blocks for every metric exposed at `/metrics`.

## Server Wiring

The server wires together `atc-core` (state store) and `atc-github` (webhook parsing) into a cohesive HTTP API. The design separates **state mutation** (webhook ingestion) from **state delivery** (REST snapshot and WebSocket stream), allowing each path to evolve independently.

### AppState

A shared `AppState` struct is passed to all handlers via Axum's `State` extractor:

```rust
struct AppState {
    state_machine: Arc<RunStateMachine>,
    webhook_tx: broadcast::Sender<SeqEvent>,
    webhook_secret: Option<String>,
    seq: Arc<Mutex<u64>>,
    persist: Arc<dyn PersistentStore>,
    pg_pool: Option<sqlx::PgPool>,
    min_pending_seq: Arc<AtomicI64>,
    last_drain_pass_at: Arc<AtomicI64>,
    broadcast_watermark: Arc<AtomicI64>,
}
```

- **`state_machine`** — Reference to the shared `atc-core` RunStateMachine. In in-memory mode, `InMemoryStore` applies events here. In PG mode, the state machine is not used by the webhook handler. REST state snapshot reads from the state machine in in-memory mode.
- **`webhook_tx`** — Sender side of a bounded `tokio::sync::broadcast` channel (capacity 256). **In PG mode the drain task is the SOLE writer.** In in-memory mode `InMemoryStore::apply_*_event` broadcasts directly under the seq mutex. WebSocket clients subscribe as receivers.
- **`webhook_secret`** — Optional GitHub webhook secret loaded from `ATC_GITHUB__WEBHOOK_SECRET`. If `None`, HMAC verification is skipped. If `Some`, signatures are required and validated.
- **`seq`** — `Arc<tokio::sync::Mutex<u64>>` counter holding the **highest committed sequence number** in **in-memory mode only**. Wrapped in `Arc` so `InMemoryStore` can hold a shared reference. In PG mode this mutex stays at 0 (seq comes from the outbox `BIGSERIAL`). The counter is pre-incremented inside `InMemoryStore::apply_*_event`: `*seq_guard += 1; let seq = *seq_guard;` — so the first event returns `seq = 1`. In in-memory mode, the state handler acquires this mutex across the snapshot + seq read to guarantee cursor/content consistency. In PG mode the state handler uses a REPEATABLE READ transaction instead.
- **`pg_pool`** — `Option<sqlx::PgPool>`. `Some` when `ATC_DATABASE_URL` is configured and a connection pool was successfully created and migrated at startup. `None` in in-memory mode. Used by the state handler for REPEATABLE READ snapshots and by the `/readyz` probe. **Not** used directly by the webhook write path — see `persist` below.
- **`persist`** — `Arc<dyn PersistentStore>` (ADR 0005). The write-path dispatch point for webhook ingestion. `PgStore` when `ATC_DATABASE_URL` is set; `InMemoryStore` otherwise. Route handler calls `state.persist.apply_*_event(env).await` without branching on storage mode.
- **`min_pending_seq`** — `Arc<AtomicI64>` initialized to `i64::MAX`. The listener task calls `fetch_min(seq, Release)` on each NOTIFY, registering the outbox seq that triggered the notification. The drain task swaps this to `i64::MAX` (`AcqRel`) at the start of each pass to capture the lowest pending seq, computing `pass_start_floor = watermark.min(backstop.saturating_sub(1))`. This **gap-healing backstop** ensures that if a NOTIFY for seq=K arrives while the drain is mid-pass, the next pass rescans from `K-1` and does not miss K. Ring-buffer dedup prevents K from being rebroadcast if it was already broadcast in the current or a recent pass.
- **`last_drain_pass_at`** — `Arc<AtomicI64>` storing a Unix-epoch millisecond timestamp. The drain task stores `now_millis()` unconditionally on every iteration (both NOTIFY-driven passes and heartbeat-only ticks). `/readyz` checks this value when `pg_pool` is `Some`: if the age exceeds `READYZ_HEARTBEAT_STALENESS_MS` (30 s), the probe returns 503 `{"status":"drain_stale"}`.
- **`broadcast_watermark`** — `Arc<AtomicI64>` holding the highest outbox `seq` the drain has fetched and broadcast through `webhook_tx`. Advanced after every successful drain pass. Read by `state_handler` as the PG-mode `lastSeq` — it reflects **commit order** (the drain only sees committed rows via `SELECT`), unlike `MAX(outbox.seq)` which reflects allocation order and could advance past data invisible to a concurrent REPEATABLE READ snapshot. Seeded at boot from `COALESCE(MAX(seq), 0)` so `/v1/state` returns a sensible cursor before the first post-startup drain pass completes.

Task handles live in `main.rs` scope; `AppState` does not own them.

### SeqEvent Wire Contract

`SeqEvent` is the broadcast envelope carrying a domain event and the seq it was assigned at commit time:

```rust
pub struct SeqEvent {
    pub seq: u64,
    pub event: WebhookEvent,
}
```

The frontend derives `RunnerPoolStats` from the underlying job state via `computePoolStats(runStore.jobs)` — see `frontend/src/lib/stores/runners.svelte.ts` (ADR 0004). The webhook handler does not take a pool-stats snapshot under the seq mutex.

- **All successful events (Run or Job):** `SeqEvent { seq, event }` is broadcast. `seq` is the value of the AppState `seq` counter after pre-increment (first commit broadcasts `seq=1`).
- **Failed transitions:** No broadcast occurs and no `SeqEvent` is emitted. Clients never receive events that are not reflected in the store.

**Wire format:** Serialized via `#[serde(rename_all = "camelCase")]`; TypeScript type emitted by ts-rs as `{ seq: bigint, event: WebhookEvent }`.

### Webhook Ingestion (`POST /v1/webhooks/github`)

**Responsibility:** Receive GitHub webhook payloads, verify signatures, parse to domain events, and commit them durably to PG (or apply in-memory when PG is not configured).

**Flow:**
1. Extract `X-GitHub-Event` header and raw body from HTTP request.
2. If `webhook_secret` is configured, verify HMAC-SHA256 signature from `X-Hub-Signature-256` header (via `atc_github::verify_signature`). Return 401 if verification fails.
3. Parse payload via `atc_github::parse_webhook(event_type, body)`, yielding one of:
   - `ParseResult::Parsed(Box<WebhookEvent>)` — Continue to store ingestion
   - `ParseResult::Skipped { event_type }` — Return 200 with `{"status": "skipped"}`
   - `ParseResult::Err(ParseError)` — Return 422 with error details
4. Dispatch through `state.persist.apply_*_event(env).await` (ADR 0005 — no mode branching in the handler):
   - **`PgStore` path** (when `ATC_DATABASE_URL` is set):
     - `PgStore::apply_*_event` calls `pool.begin()` internally. On failure → `PersistError::Backend`.
     - Calls `upsert_*_in_txn` in the open transaction. On `0 rows affected` (predicate rejected) → `PersistError::InvalidTransition`; on sqlx error → `PersistError::Backend`.
     - Calls `insert_outbox_*_in_txn`. The `BIGSERIAL` allocates the durable seq. On failure → `PersistError::Backend`.
     - Emits `SELECT pg_notify('atc_outbox', seq::text)` inside the same transaction. PG queues the NOTIFY and delivers on COMMIT; aborted transactions silently drop it.
     - Calls `tx.commit()`. On failure → `PersistError::Backend`.
     - Emits metrics: `atc_pg_write_failures_total{kind="parity"}` on `InvalidTransition`; `atc_pg_write_failures_total{kind="transient"}` on backend errors; `atc_pg_notify_emitted_total{kind}` after a successful commit.
     - **Does NOT broadcast to `webhook_tx`, does NOT apply to the RunStateMachine, and does NOT touch the `seq` mutex.** The drain task is the sole broadcaster in PG mode.
     - Returns `Ok(<u64 seq>)` on success.
   - **`InMemoryStore` path** (when `ATC_DATABASE_URL` is unset):
     - `InMemoryStore::apply_*_event` acquires `seq` mutex **before** any mutation (ordering invariant).
     - Applies the event to `RunStateMachine`. On `StateMachineError::InvalidTransition` → `PersistError::InvalidTransition`. No broadcast emitted for rejected transitions.
     - Pre-increments seq (`*seq_guard += 1; let seq = *seq_guard`) and broadcasts `SeqEvent { seq, event }` under the mutex.
     - Returns `Ok(seq)` on success.
5. Match the result:
   - `Ok(seq)` → 200 `{"status": "accepted", "seq": <seq>}`.
   - `Err(PersistError::InvalidTransition)` → 200 `{"status": "rejected"}`. (Not a 4xx — preserves the existing parity-rejection contract.)
   - `Err(PersistError::Backend(_))` → 503 `{"status": "error"}`.

**Error responses:**
- **400** — Missing `X-GitHub-Event` header
- **401** — Invalid or missing signature when secret is configured; SHA-1 signature when SHA-256 is expected
- **422** — Malformed JSON body or unknown action/conclusion values
- **503** — PG `pool.begin()` failed, mid-txn backend error, or `tx.commit()` failed

**PG mode ordering guarantee:** Seq ordering in PG mode comes from the outbox `BIGSERIAL`, which allocates strictly increasing values inside each committed transaction. The listener fires `min_pending_seq.fetch_min(seq, Release)` on each NOTIFY; the drain processes rows in `ORDER BY seq` and applies ring-buffer dedup. The seq mutex is not involved in PG mode.

**In-memory mode ordering guarantee:** `InMemoryStore` acquires the seq mutex before applying the event, incrementing the counter, and broadcasting, serializing the full pipeline so that seq values are strictly monotonically increasing and their order always matches the in-memory apply order.

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

**Responsibility:** Return the full current state snapshot and the highest committed seq cursor.

**PG mode flow:**
1. Load `broadcast_watermark` (the drain's commit-order cursor) **before** opening the snapshot transaction. Every seq ≤ this value has been fetched by the drain (which only sees committed rows) and broadcast through `webhook_tx`.
2. Open a `pool.begin()` transaction. On failure → 503.
3. Set `TRANSACTION ISOLATION LEVEL REPEATABLE READ`. The MVCC snapshot view is taken at this statement, strictly **after** the watermark load, so every row reflected in `lastSeq` is also visible in the snapshot.
4. Call `persist::read_all_runs(&mut tx)` — reads `WHERE placeholder=false` (FK-stub rows excluded).
5. Call `persist::read_all_jobs(&mut tx)`.
6. Commit the transaction (read-only; a commit failure is non-fatal for the response content).
7. Convert the watermark `i64` to `u64` as `last_seq`; serialize the response as `StateSnapshot`.

We deliberately do not use `MAX(outbox.seq)` as the cursor: BIGSERIAL is allocated pre-commit and can commit out of order. A tx with allocated `seq=10` still in-flight while `seq=11` commits would let `MAX(seq)` return 11 even though `seq=10`'s mutation isn't visible. The drain's `broadcast_watermark` only advances after a successful pass, and the drain only sees committed rows via SELECT, so the cursor is monotonic in commit order.

**In-memory mode flow:**
1. Acquire the `seq` mutex (prevents any webhook from committing during the read).
2. Read the store snapshot under the store's read lock (`QueryResult { runs, jobs }`).
3. Read `seq` as `last_seq`.
4. Release the mutex and serialize.

**StateSnapshot:**
```rust
struct StateSnapshot {
    last_seq: u64,            // Highest committed seq; serializes as "lastSeq"
    runs: Vec<WorkflowRun>,
    jobs: Vec<Job>,
}
```

**Cursor semantics:** `last_seq` is the highest committed seq the drain has broadcast (PG mode) or the in-memory counter's current value (in-memory mode). `last_seq = 0` is the cold-start sentinel. All events with `seq <= last_seq` are guaranteed reflected in the snapshot. In PG mode the snapshot may additionally include commits the drain has not yet broadcast — those are buffered on the WS side and applied idempotently when their `SeqEvent`s arrive. The frontend filters `seq > lastSeq` against the buffer.

Return 200 with the JSON snapshot.

**Snapshot/stream reconciliation:** A client can call `GET /v1/state` to establish baseline state, note the returned `lastSeq`, then connect to `GET /v1/ws` and filter incoming `SeqEvent`s to those with `seq > lastSeq` (strictly greater than — buffered events with `seq <= lastSeq` are already reflected in the snapshot and discarded). This protocol allows robust reconnection and bootstrap.

### Configuration

Server wiring configuration extends the existing figment-based config:

- **`github.webhook_secret`** (`ATC_GITHUB__WEBHOOK_SECRET` env var) — Optional string. If present, all webhook requests must carry a valid HMAC-SHA256 signature. If absent, signatures are not required or verified.

### Lifecycle Wiring

In `main.rs`:
1. Load config via `Config::load()`
2. If `ATC_DATABASE_URL` is set: call `atc_server::db::init_pool(url)` — connects `sqlx::PgPool` and runs embedded migrations; exit(1) on failure
3. Create `RunStateMachine` with `SystemClock` and TTL (default 1 hour)
4. Create broadcast channel: `tokio::sync::broadcast::channel(256)`
5. Call `metrics::register_pg_write_counters()` after the recorder is installed.
6. Construct `persist: Arc<dyn PersistentStore>` — `PgStore::new(pool)` when `pg_pool` is `Some`, otherwise `InMemoryStore::new(state_machine, seq, webhook_tx)` (ADR 0005).
7. Create `AppState` with all components (`state_machine`, `webhook_tx`, `webhook_secret`, `seq: Arc<Mutex<u64>>`, `persist`, `pg_pool`, `min_pending_seq`, `last_drain_pass_at`, `broadcast_watermark`) and pass to Axum via `.with_state()`. The webhook handler dispatches uniformly through `state.persist.apply_*_event(env)`; the route handler does not touch `pg_pool` for the write path.
8. Start background eviction task: `start_eviction_task(state_machine.clone())`
9. **If `pg_pool` is `Some`** (before `axum::serve`):
   1. Derive `listener_url` from `cfg.database_listener_url` if set, else fall back to `cfg.database_url`.
   2. `PgListener::connect(&listener_url)` — fail-fast on Err (exit(1)).
   3. `listener.listen(NOTIFY_CHANNEL)` — fail-fast on Err (exit(1)).
   4. `SELECT COALESCE(MAX(seq), 0) FROM outbox` — query the initial watermark; seed both the drain task's local watermark and `broadcast_watermark` from this value (so `/v1/state` returns a sensible cursor before the first post-startup drain pass). Fail-fast on Err (exit(1)).
   5. Spawn listener task (receives PG NOTIFYs, calls `min_pending_seq.fetch_min(seq, Release)`, fires `Arc<Notify>`).
   6. Spawn drain task (wakes on `Arc<Notify>` or 5 s heartbeat tick; NOTIFY-driven passes fetch `seq > pass_start_floor ORDER BY seq` in pages, apply ring-buffer dedup, broadcast `SeqEvent`s, advance `watermark`; every iteration updates `last_drain_pass_at`).
10. Bind the server to `http_addr` via `axum::serve`
11. On graceful shutdown, execute the two-phase cooperative shutdown sequence — see § [Supervision and Shutdown](#supervision-and-shutdown) below.

The eviction task runs periodically (default every 30 minutes) and removes completed jobs whose completion timestamp exceeds the TTL. This keeps in-memory state bounded and prevents unbounded growth.

## Supervision and Shutdown

ATC uses a two-phase cooperative shutdown model implemented in `backend/crates/atc-server/src/shutdown.rs`. The orchestration function `run_shutdown_orchestration` is called from `main` and awaits the full sequence to completion before the process exits.

### Cancellation surfaces

Five background tasks observe cancellation signals and exit cooperatively:

1. **Eviction task** (`atc-core::state_machine::start_eviction_task`) — `tokio::select!` on `cancel.cancelled()` vs `ticker.tick()`. Exits at its next tick boundary after the token fires.
2. **Listener task** (`atc-server::listener::spawn_listener_task`) — `tokio::select!` on `cancel.cancelled()` vs `pg_listener.recv()`. Exits cooperatively.
3. **Drain task** (`atc-server::listener::spawn_drain_task`) — checks the token only between drain passes, never inside `drain_pass()`. The current pass always runs to completion (or to a Postgres error); cancellation fires at the next inter-pass check. This ensures the current outbox batch is fully broadcast before shutdown.
4. **Process metrics collector** (`atc-server::metrics::spawn_process_collector`) — `tokio::select!` on `cancel.cancelled()` vs `ticker.tick()`. `Collector::collect()` is synchronous; the cancel arm fires between ticks, not mid-collect.
5. **WebSocket handlers** (`atc-server::ws::handle_socket`) — each spawned as a tracked future in `ws_tracker`. Observe a separate `ws_close` token (see Phase 2 below).

### Two cancellation tokens and phase ordering

Two `CancellationToken`s are constructed in `main.rs` before `AppState`:

- **`shutdown`** — phase-1 token. Cancelled by the signal handler (SIGTERM or Ctrl-C). Observed by the two axum `with_graceful_shutdown` futures, the eviction task, the listener task, the drain task, and the process metrics collector.
- **`ws_close`** — phase-2 token. Cancelled by `run_shutdown_orchestration` only after the drain handle has been awaited or its timeout has expired. Observed exclusively by WS handlers.

The two-token design ensures WS handlers see the drain task finish broadcasting before they themselves close. A single shared token would cause the WS handler's cancel arm to fire simultaneously with the drain exiting — truncating the event stream before all buffered events reach clients.

**Phase 1** (fires on either of two triggers):

- **Signal-driven (normal path):** SIGTERM / SIGINT → signal handler → `shutdown.cancel()`.
- **Self-healing on serve failure:** if either spawned axum `serve` task exits unexpectedly before any signal arrives (e.g., an accept-loop failure), `run_shutdown_orchestration` observes that exit via `tokio::select!`, logs an `error!` naming the affected serve, and calls `shutdown.cancel()` itself so the remaining tasks shut down cooperatively rather than getting orphaned by a half-up process.

In both cases, once `shutdown` is cancelled:

```
shutdown.cancel()
  ├── axum serves: graceful_shutdown future resolves → stops accepting new connections
  ├── eviction task: cancel arm fires → exits at next tick
  ├── listener task: cancel arm fires → exits
  ├── metrics task: cancel arm fires → exits at next tick
  └── drain task: cancel arm fires between passes → finishes current pass → exits
```

**Phase 2** (fires after drain is awaited):

```
ws_close.cancel()
  └── each WS handler's biased select arm fires
        ├── delivers any remaining buffered broadcast events (biased order)
        └── sends Message::Close(CloseFrame { code: 1001, reason: "going away" })
              → handler returns → ws_tracker counts down
```

### WS handler — biased select

The WS handler loop uses `tokio::select! { biased; … }` with arms ordered:

1. `rx.recv()` — drain buffered broadcast events.
2. `socket.recv()` — detect client-initiated close or read errors.
3. `ws_close.cancelled()` — send `Close(1001 "going away")` and exit.

Because the select is biased, `rx.recv()` is checked first on every iteration. While the broadcast channel has buffered events that are `Ready`, the handler drains them before falling through to the `ws_close` arm. Only when `rx.recv()` would block (no buffered events and no new broadcast) does the cancel arm fire and send the Close frame. This is the mechanism for "events before close" on the clean, idle-client path.

If a client sends a Close frame during shutdown, `socket.recv()` (arm 2, above the cancel arm in biased order) wins; the handler exits via the client-initiated branch and does not send a server-side Close 1001.

### WS task tracking — `TaskTracker`

`AppState` holds a `pub ws_tracker: TaskTracker`. The WS handler wraps each upgrade future via `state.ws_tracker.track_future(handle_socket(...))` before passing it to `ws.on_upgrade(...)`. This lets `main` call `ws_tracker.wait()` after `ws_close.cancel()` and know when all in-flight WS handlers have finished sending their Close frames.

`TaskTracker::close()` is called before `wait()` — this is the signal that makes `wait()` return once the in-flight count reaches zero. A late WS upgrade that arrives between `close()` and `wait()` returning is still tracked; since `ws_close` is already cancelled by that point, the late handler enters its close arm immediately and exits in milliseconds.

### `webhook_tx_keepalive` invariant

`main.rs` keeps a `broadcast::Sender<SeqEvent>` clone (`webhook_tx_keepalive`) alive through `ws_tracker.wait()`. Without it, the following race occurs:

1. When the axum serve future resolves (phase 1), it drops `AppState` and its embedded `webhook_tx` sender.
2. When the drain task exits (phase 1), it drops its own sender clone.
3. If no other sender clone is alive, the broadcast channel closes.
4. WS handlers' `rx.recv()` returns `RecvError::Closed` and they exit via the closed-channel branch — before `ws_close.cancel()` fires at phase 2.
5. Clients receive a TCP reset instead of `Close(1001)`.

Holding `webhook_tx_keepalive` through step 5 of the orchestration sequence (`ws_tracker.wait()`) prevents channel closure before the WS handlers observe `ws_close`. The keepalive is explicitly dropped via `drop(webhook_tx_keepalive)` in `run_shutdown_orchestration` after `ws_tracker.wait()` returns, to document the invariant.

### Per-task timeout budgets

| Constant | Value | Applies to |
|---|---|---|
| `SHUTDOWN_TIMEOUT_DRAIN` | 5 s | drain handle (worst case: one in-flight 500-row pass with PG round-trips) |
| `SHUTDOWN_TIMEOUT_WS` | 2 s | `ws_tracker.wait()` — time for connected WS clients to receive their Close frames |
| `SHUTDOWN_TIMEOUT_SERVES` | 3 s | spawned axum serve tasks |
| `SHUTDOWN_TIMEOUT_LISTENER` | 1 s | listener handle |
| `SHUTDOWN_TIMEOUT_EVICTION` | 1 s | eviction handle |
| `SHUTDOWN_TIMEOUT_METRICS` | 1 s | process metrics collector handle |

Aggregate worst-case shutdown: ~13 seconds.

**On per-handle timeout:** `AbortHandle::abort()` is called (best-effort and asynchronous — the task may run until its next await point). An `error!` log naming the task is emitted. Orchestration continues.

**On `ws_tracker.wait()` or serves-join timeout:** No per-task abort surface is available. An `error!` log is emitted and orchestration continues; remaining tasks are reaped by runtime drop at process exit.

### Operator shutdown contract

**Signal:** ATC shuts down gracefully on SIGTERM or SIGINT. The signal handler cancels the phase-1 token, which begins the sequence above.

**Aggregate timeout:** Worst-case shutdown completes in approximately 13 seconds (5 s drain + 2 s WS tracker + 3 s serves + 1 s each for listener, eviction, and metrics, if every timeout fires). In practice, tasks exit well within budget on clean shutdown.

**K8s settings:** `terminationGracePeriodSeconds: 30` (the Kubernetes default) is sufficient. The 13-second aggregate worst case leaves 17 seconds of headroom. No adjustment to this value is needed for ATC's shutdown budget.

**Load-balancer de-registration:** Kubernetes removes a pod from `Endpoints` after the pod enters `Terminating`, but the propagation delay to the load balancer means in-flight requests may still arrive for a few seconds after SIGTERM. A `preStop` lifecycle hook (e.g., a `sleep 5`) can absorb this propagation delay before the shutdown signal is delivered. This is tracked as a separate operational improvement (issue #79) and is not part of the pod-internal shutdown sequence described here.

**Webhook durability during shutdown:** Webhooks committed to the outbox before SIGTERM are durable. The drain task finishes its current pass (up to 5 s) and broadcasts committed rows before phase 2 begins. Any outbox rows that were committed but not yet drained when the drain task exits remain in the outbox and are broadcast by the next replica on its startup drain pass. No committed webhook is lost; only latency increases during the shutdown window.

**In-memory mode:** The listener and drain tasks do not exist. The `drain_handle` and `listener_handle` are `None`; phase 1 skips their timeouts. In-memory state is lost on process exit; this is expected and documented as a dev-only mode.

### Health Probes

- `/readyz` — Readiness probe. In in-memory mode (no PG pool), returns 200 immediately. When `ATC_DATABASE_URL` is configured: (1) runs `SELECT 1` against the pool — 503 `{"status":"db_unreachable"}` if the DB is unreachable; (2) checks the drain heartbeat age — if `last_drain_pass_at` is older than `READYZ_HEARTBEAT_STALENESS_MS` (30 s), returns 503 `{"status":"drain_stale"}`. A healthy drain updates its heartbeat every 5 s (`HEARTBEAT_TICK`), so any value older than 30 s indicates the drain task has stalled. Returns 200 `{"status":"ok"}` when both checks pass.
- `/healthz` — Liveness probe. Returns 200 unconditionally — process up = alive regardless of DB state.

### NOTIFY Emission and Drain Pipeline

**Two-task coalescing structure:** The listener pipeline splits into two cooperating tasks:

1. **Listener task** — Holds a dedicated long-lived `PgListener` connection. Receives PG NOTIFY payloads on the `atc_outbox` channel, increments `atc_pg_notify_received_total`, calls `min_pending_seq.fetch_min(seq, Release)` to register the notified seq for gap-healing, and fires an `Arc<tokio::sync::Notify>` to wake the drain task. Does not fetch rows itself.

2. **Drain task** — Waits on `Arc<Notify>` (level-triggered), but also wakes on a 5 s heartbeat tick to refresh `last_drain_pass_at`. On every iteration (notify or tick): stores `now_millis()` to `last_drain_pass_at`. On heartbeat-only wakes, skips the pass body. On NOTIFY-driven wakes:
   - Swaps `min_pending_seq` to `i64::MAX` (AcqRel) to capture the gap-healing backstop. Computes `pass_start_floor = watermark.min(backstop.saturating_sub(1))`.
   - **Pagination loop:** fetches pages of `DRAIN_BATCH_SIZE=500` rows `WHERE seq > page_cursor ORDER BY seq`, advancing `page_cursor` on each page until a partial page is returned.
   - For each row: decodes the JSONB payload as `RunEventEnvelope` or `JobEventEnvelope`. On decode failure, logs an error and skips. On unknown `kind`, increments `atc_pg_drain_unknown_kind_total` and skips.
   - **Ring-buffer dedup:** Before broadcasting a seq, checks `recent_set` (HashSet over the ring buffer of capacity `DEDUP_CAP=2048`). If already seen, increments `atc_pg_drain_duplicate_skipped_total` and skips. If new, inserts into ring and set; evicts the oldest entry if the ring is full.
   - **Broadcasts** `SeqEvent { seq: u64, event: WebhookEvent }` on `webhook_tx` for each row that passes dedup.
   - After the full pagination loop, advances `watermark` to the highest seq seen. Refreshes `last_drain_pass_at` again.

**Gap-healing backstop:** The concurrent-commits race: webhook A commits seq=1 and fires NOTIFY, but before the drain wakes, webhook B commits seq=2. The drain wakes on B's NOTIFY, calls `swap(MAX, AcqRel)`, gets backstop=1 (A registered it first), computes `floor = watermark.min(0) = 0`, and scans from 0 — fetching both seq=1 and seq=2 in order. Without the backstop, a drain that woke on B's NOTIFY and started scanning from `watermark=0` would also catch A; the backstop is a safety net for the case where the drain has already advanced `watermark` past where A's NOTIFY would land.

**DSN session-mode contract:** The listener connection (`ATC_DATABASE_LISTENER_URL` or fallback to `ATC_DATABASE_URL`) must be a session-mode endpoint. Transaction-mode PgBouncer reassigns the underlying connection between transactions, silently dropping LISTEN registrations. When the main pool uses transaction-mode PgBouncer, set `ATC_DATABASE_LISTENER_URL` to a direct Postgres DSN or a session-mode PgBouncer endpoint.

**Reconnect loss window:** If the listener task reconnects after a connection drop, any NOTIFYs delivered during the reconnect window are not received. This is healed automatically: the next NOTIFY after reconnection triggers a drain pass that fetches `seq > watermark`, catching up all rows that were inserted while the listener was disconnected. No data is lost; only latency increases during the reconnect window.

### Modules

- **`state.rs`** — `AppState` struct (fields: `state_machine`, `webhook_tx`, `webhook_secret`, `seq: Arc<Mutex<u64>>`, `persist: Arc<dyn PersistentStore>`, `pg_pool`, `min_pending_seq`, `last_drain_pass_at`, `broadcast_watermark`) and the `SeqEvent { seq, event }` broadcast type (ADR 0004)
- **`routes.rs`** — Route handlers for `/v1/webhooks/github`, `/v1/ws`, `/v1/state`, `/healthz`, `/readyz`; defines `StateSnapshot { last_seq, runs, jobs }`; webhook handler dispatches through `state.persist.apply_*_event(env)` uniformly for both modes (ADR 0005), returns `{"status":"accepted","seq":<u64>}` on success; state handler uses REPEATABLE READ in PG mode; `/readyz` checks drain heartbeat staleness in PG mode
- **`ws.rs`** — WebSocket connection handling and message broadcast logic
- **`persist.rs`** — `pub trait PersistentStore` with `apply_run_event` and `apply_job_event`; `pub struct PgStore` + `impl PersistentStore for PgStore` — owns its own transaction lifecycle, emits metrics; `pub struct InMemoryStore` + `impl PersistentStore for InMemoryStore` — locks seq mutex, applies to RunStateMachine, broadcasts; `pub(crate)` transaction helpers `upsert_run_in_txn`, `upsert_job_in_txn`, `insert_outbox_run_in_txn`, `insert_outbox_job_in_txn`, `notify_outbox_seq_in_txn`; `pub(crate)` read helpers `read_all_runs`, `read_all_jobs` used by the state handler (ADR 0005)

## Files

- `backend/crates/atc-server/src/main.rs` — Server entry point, config loading, tracing branching, router composition, eviction task lifecycle
- `backend/crates/atc-server/src/config.rs` — figment-based Config struct, LogFormat enum, GitHubConfig with webhook_secret, Config::load()
- `backend/crates/atc-server/src/db.rs` — `init_pool(url)`: connects sqlx PgPool and runs embedded migrations; extracted from main so it is reachable by integration tests
- `backend/crates/atc-server/src/routes.rs` — API route definitions (healthz, readyz, webhook, state, ws endpoints)
- `backend/crates/atc-server/src/state.rs` — AppState struct (includes `persist: Arc<dyn PersistentStore>` and `seq: Arc<Mutex<u64>>` per ADR 0005) and `SeqEvent { seq, event }` type; `StateSnapshot` lives in `routes.rs`
- `backend/crates/atc-server/src/ws.rs` — WebSocket handler, broadcast subscription, SeqEvent serialization
- `backend/crates/atc-server/src/assets.rs` — rust-embed struct, embedded file serving, SPA fallback, dev proxy
- `backend/crates/atc-server/src/metrics.rs` — Prometheus layer, build_info gauge, process collector, PG write counter registration (`atc_pg_write_failures_total`, `atc_pg_in_memory_drift_total`)
- `backend/crates/atc-server/src/persist.rs` — `pub trait PersistentStore`; `PgStore` and `InMemoryStore` impls (ADR 0005); `pub(crate)` transaction helpers for UPSERT+outbox+NOTIFY pattern; `pub(crate)` read helpers for state handler
- `backend/crates/atc-server/src/listener.rs` — PG LISTEN/NOTIFY background tasks: `spawn_listener_task` (receives notifications, registers seq in `min_pending_seq`, fires `Arc<Notify>`) and `spawn_drain_task` (wakes on notify or 5 s heartbeat; NOTIFY-driven passes fetch outbox rows by `seq > pass_start_floor ORDER BY seq` in pages, decode payload, apply ring-buffer dedup, broadcast `SeqEvent`s, advance watermark; every iteration updates `last_drain_pass_at`). Constants: `DRAIN_BATCH_SIZE=500`, `HEARTBEAT_TICK=5s`, `DEDUP_CAP=2048`. Spawned only when `pg_pool` is `Some`. The `connect_listener_fails_on_bad_url` unit test wraps `connect_listener` in a 2 s `tokio::time::timeout` to cap test runtime — sqlx's default `connect_timeout` is 30 s, which would otherwise dominate the lib-test wall clock for a negative-path assertion.
- `backend/crates/atc-server/build.rs` — vergen-gix Emitter emitting VERGEN_* compile-time env vars; also emits `cargo:rerun-if-changed=migrations` so sqlx::migrate!() re-embeds files on SQL changes
- `backend/crates/atc-server/migrations/0001_initial_runs_jobs.sql` — Initial schema: `runs` and `jobs` tables with CHECK constraints, FK, and indexes
- `backend/crates/atc-server/migrations/0002_outbox.sql` — Outbox table: `BIGSERIAL seq` PK, `kind` discriminator, `run_id`/`job_id`, `payload JSONB` (domain event envelope), `inserted_at TIMESTAMPTZ`; `outbox_run_idx` on `run_id`
- `backend/.sqlx/` — Offline query cache (committed to repo). Generated by `cargo sqlx prepare --workspace -- --tests` to enable `SQLX_OFFLINE=true` builds without a live DB at compile time. Updated whenever SQL queries change.
- `backend/Cargo.toml` — Workspace definition with shared dependency versions
- `backend/crates/atc-server/Cargo.toml` — Server crate dependencies
