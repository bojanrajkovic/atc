# Backend Server — Architecture

Last verified: 2026-05-18 (root-doc sync: AppState 6 fields, persistence crate split)

## Purpose

The backend server (`atc-server` crate) is an Axum HTTP server that serves as the single entry point for the ATC application. It provides:

- A REST API surface with liveness (`/healthz`) and readiness (`/readyz`) probes
- Frontend asset serving in release mode via rust-embed
- Development proxy to Vite dev server in debug mode via reqwest
- Configurable address binding and logging format via environment variables

The server binds to `http_addr` (default `0.0.0.0:8080`) configured via `ATC_HTTP_ADDR` environment variable and is the only executable crate in the backend workspace. Six library crates sit beneath it: `atc-core` (pure domain types + state machine), `atc-github` (webhook parsing + HMAC verification + translation), `atc-wire` (serializable wire types: `CommittedEvent`, `StateSnapshot`), `atc-persist` (the `PersistentStore` trait + `LivenessError` + `join_with_timeout`), `atc-store-mem` (in-memory `PersistentStore` impl, dev only), and `atc-store-pg` (Postgres `PersistentStore` impl, production). The persistence split is canonicalized in [ADR-0008](../architecture-decisions/0008-persistence-crate-split.md).

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
- **Job** — A unit of work within a run (e.g., "test-linux", "build-docker"). Identified by `job_id`. Belongs to exactly one run. State: Queued → InProgress → Completed (with optional conclusion: Success, Failure, Skipped, etc.). Queued may also transition directly to Completed — GitHub emits `workflow_job completed` for Queued jobs when a run is cancelled before those jobs start.
- **Step** — An individual action within a job (e.g., "Checkout code", "Run tests"). Identified by `step_id`. Belongs to exactly one job. Carries conclusion and conclusion text. Steps are immutable after completion.

### PG Table Notes

The `runs` PG table carries a `placeholder BOOLEAN NOT NULL DEFAULT false` column. Rows with `placeholder = true` are FK-only stubs created when a job event arrives before its parent run event; they are promoted to real rows when the matching `workflow_run` webhook arrives. The state snapshot (`/v1/state`) always reads `WHERE placeholder = false`.

### InMemoryStore Architecture

`InMemoryStore` (in the `atc-store-mem` crate at `backend/crates/atc-store-mem/src/lib.rs`, with test-support inspection helpers in `backend/crates/atc-store-mem/src/invariants.rs`) is the single source of truth for all entity state in in-memory mode. It is backed by:

- **Primary maps** — `jobs: Map<JobId, Job>` and `runs: Map<RunId, WorkflowRun>` store complete entity snapshots. All mutations are made to these maps.
- **Secondary indexes** — `jobs_by_repo: Map<RepoKey, Set<JobId>>` and `jobs_by_run: Map<RunId, Set<JobId>>` enable fast lookups by context. They are built on first-sight job insertion and cleaned up during eviction.
- **RwLock for concurrency** — All state is protected by a single `RwLock<StateData>`. Read operations (snapshot, query) acquire read locks; mutations (apply_event, evict_expired) acquire write locks.
- **Seq mutex** — A `Mutex<u64>` counter is acquired before state writes so that seq allocation and state mutation are atomic, preventing interleaving.
- **Clock trait** — A pluggable time source (`TestClock` in tests, `SystemClock` in production) allows deterministic testing and clock mocking.
- **TTL eviction** — Completed jobs are retained for a configurable duration (default 1 hour). The `evict_expired()` method removes expired completed jobs from all maps and indexes. Active jobs are never evicted.

**Pure state-transition functions** — `atc-core::state_machine` exports three free functions (`apply_run_event`, `apply_job_event`, `is_evictable`) with no locks, no async, and no side effects. `InMemoryStore` delegates all entity mutation to these functions, handling locking, indexing, seq accounting, and broadcasting itself.

### Domain Events

The store mutates only through domain events, ensuring an audit trail and facilitating event replay:

- **RunEvent** — Models workflow run state: Requested, Queued, InProgress, Completed. Transitions are forward-only (no backward transitions allowed). `RunEventEnvelope` carries `workflow_name` and `workflow_path` as `Option<String>` to handle GitHub webhook payloads that omit the workflow object on some events (`in_progress`, `completed`). When present, these fields are stored; when `None`, the store preserves existing values via `.or()` pattern.
- **JobEvent** — Models job state: Queued, Waiting, InProgress, Completed. Also carries: run_id (which run this job belongs to), conclusion (success/failure/skipped/etc.), and steps (array of completed/active steps). The `Waiting` variant represents jobs waiting for approval (e.g., environment protection rules, required reviewers). `InProgress.runner` is `Option<RunnerInfo>` to handle GitHub's `in_progress` webhook arriving before runner assignment is complete; `None` falls through to the existing runner value via `.or()` pattern. `RunnerInfo` carries `id`, `name`, and `group_name` only — `group_id` was retired (it was captured but never consumed; the only logical branch that read it was a frontend elasticity heuristic that has been replaced by operator-declared `capacity: null`).

Events are created from GitHub webhook payloads by `atc-github` and applied to the store via `apply_event()`. The store validates state machine transitions and rejects invalid ones (e.g., Completed → Queued).

### State Machine Invariants

1. **Forward-only transitions** — A job cannot transition from Completed back to Queued or to any earlier state. Violations return `StateMachineError::InvalidTransition`. Note: `Queued → Completed` is valid — GitHub emits this for jobs cancelled before they start.
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
- `total: RunnerPoolTotal` — Three-state declared capacity for this pool. Adjacent-tagged enum with three variants:
  - `Bounded(u32)` — operator declared an integer ceiling. Drives capacity-bar rendering and saturation-threshold colors.
  - `Unbounded` — operator declared the pool with `capacity: null`. Frontend renders a distinct affordance (icon + accessible label) instead of a saturation bar.
  - `Undeclared` — pool observed in webhook traffic but absent from the operator's `runner_pools` config. Frontend renders the count only, no bar, no affordance.

  Populated frontend-side via the merge in `computePoolStats`, keyed by canonical label-set against `StateSnapshot.runner_pool_capacities`. The wire only carries operator-declared `RunnerPoolCapacity { labels, capacity: Option<u32> }` entries; the three-way variant is composed on the frontend. The backend does not re-derive `RunnerPoolTotal` (stays consistent with ADR 0004: operator config is additive over derived stats; the trait owns event-derived state only).

The snapshot returns `StateSnapshot { last_seq, runs, jobs, runner_pool_capacities }` (no inline pool-stats computation). The wire carries no `pool_stats` or `pool_stats_after` fields (see ADR 0004). The lexicographic sort by `labels` is the responsibility of `computePoolStats` on the frontend, which also performs the capacity merge.

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
- `database_url` (`ATC_DATABASE_URL`) — default `None`
- `database_listener_url` (`ATC_DATABASE_LISTENER_URL`) — default `None` (falls back to `ATC_DATABASE_URL` when unset). Use to point the PG listener at a session-mode endpoint when the main pool runs through transaction-mode PgBouncer.
- `log_filter` (`ATC_LOG_FILTER`) — default `"info"` (passed to `EnvFilter`)
- `log_format` (`ATC_LOG_FORMAT`) — default `pretty` in debug builds, `json` in release builds

OpenTelemetry export is configured via spec-standard `OTEL_*` env vars read by the SDK directly (and, for the sampler, by `init_otel`); they are not modeled in `Config`. See `deployment.md` § "Environment Variables" for the operator-facing list and `metrics.md` § "Metric and span authoring contract" for the authoring rules.

**Decision:** Branch tracing format on `LogFormat` (debug → pretty, release → JSON)
**Alternatives considered:** Always JSON, always pretty, runtime-only env var
**Rationale:** Developer builds benefit from ANSI-colored pretty output without any configuration. Production/container builds default to structured JSON for log aggregators. Both can be overridden via `ATC_LOG_FORMAT`, satisfying the override ACs without special-casing in code. The `cfg!(debug_assertions)` default mirrors the existing assets.rs pattern for compile-time branching.

## PostgreSQL Integration

ATC uses [sqlx](https://github.com/launchdarkis/sqlx) as its PostgreSQL client. The pool is created on startup when `ATC_DATABASE_URL` is set.

### Storage modes — operator guidance

ATC supports two runtime storage modes:

- **External Postgres** (`ATC_DATABASE_URL` set) — the production-supported mode. Required for any deployment with `replicaCount > 1` (the Helm chart's template-render-time `{{ fail }}` guard refuses to render multi-replica without a Postgres URL). The webhook handler is write-only (transactional UPSERT + outbox INSERT + `pg_notify`); the drain task is the sole broadcaster; `/v1/state` reads from a REPEATABLE READ snapshot.
- **In-memory** (`ATC_DATABASE_URL` unset) — **dev-only**. Single-replica only. State lives in `atc_store_mem::InMemoryStore` behind an `RwLock`; events broadcast directly from the webhook handler under the seq mutex; on process exit, all state is lost. Useful for `just dev` against curl-fired or smee.io-tunneled webhooks; do not run this in production. Multi-replica deployments using this mode would silently fork state per replica with no convergence — there is no leader, no write replication, and no readback synchronization.

If you find yourself wanting to run in-memory mode against more than one replica, the answer is: configure Postgres. The chart guard catches this at `helm template` / `helm install` time; the binary's `ensure_pg_scheme()` catches the misconfigured-URL variant at startup.

### Startup behavior

Immediately after tracing is initialized, `main.rs` emits a single `atc-server starting` INFO log line carrying the vergen-embedded build metadata: `version`, `git_describe`, `git_sha`, `rustc_version`, `build_timestamp`, `target_triple`. The same six fields populate the `atc_build_info` gauge (see `docs/architecture/metrics.md`); `version` and `git_describe` both source from `VERGEN_GIT_DESCRIBE` so the operator-facing identifier tracks the git tag the binary was built from rather than `Cargo.toml`'s version (`docs/architecture/metrics.md` § `atc_build_info` carries the rationale). `service.version` on the OTel resource (set in `otel::build_resource`) is also sourced from `VERGEN_GIT_DESCRIBE` for the same reason — spans and OTel metrics carry the same identifier the metric label and OCI image label do. The log line is the operator's fallback diagnostic when the metrics endpoint isn't reachable — early startup crashes, OTel pipeline disabled, container logs as the only surface.

| Scenario | Behavior |
|---|---|
| `ATC_DATABASE_URL` unset | In-memory mode; `pg_pool = None`; no migration step |
| `ATC_DATABASE_URL` (or `ATC_DATABASE_LISTENER_URL`) set with a non-`postgres://` / `postgresql://` scheme | `ensure_pg_scheme()` in `main.rs` logs `"<VAR> must be a postgres:// or postgresql:// URL; got scheme <X>. ATC only supports external PostgreSQL."` and `process::exit(1)` BEFORE any sqlx call. Mirrors the chart-time guard in `deploy/helm/atc/templates/deployment.yaml`. |
| `ATC_DATABASE_URL` set, connect fails | `tracing::error!` + `process::exit(1)` |
| `ATC_DATABASE_URL` set, connect succeeds, migrations fail | `tracing::error!` + `process::exit(1)` |
| `ATC_DATABASE_URL` set, everything succeeds, DB lost at runtime | Process stays up; `/readyz` returns 503 |
| `ATC_DATABASE_URL` set, drain task active | First-pass startup latency observed via `atc_pg_drain_startup_seconds` (one observation per process lifetime) |

### Schema migrations

Migrations live in `backend/crates/atc-store-pg/migrations/` (co-located with the PG store that consumes them — #169 ADR-0008). They are embedded in the binary at compile time via `sqlx::migrate!("./migrations")` inside `atc-store-pg::db`, exposed publicly as `atc_store_pg::db::MIGRATOR`, and run automatically on startup by `atc_store_pg::db::init_pool`. Re-running is idempotent (tracked by `_sqlx_migrations` table).

Current schema:
- `0001_initial_runs_jobs.sql` — Creates `runs` and `jobs` tables with columns, FK, CHECK constraints, and indexes to support the read patterns and TTL eviction.
- `0002_outbox.sql` — Creates `outbox` table with `BIGSERIAL seq` primary key (durable monotonic-not-gapless cursor), `kind TEXT CHECK('run','job')`, `run_id BIGINT`, `job_id BIGINT NULL`, `payload JSONB` (stores `RunEventEnvelope` / `JobEventEnvelope` — NOT `CommittedEvent`), `inserted_at TIMESTAMPTZ DEFAULT now()`. Index on `run_id` for the drain forwarder. No FK to `runs` (append-only log; eviction is independent).
- `0003_runs_placeholder.sql` — Adds `placeholder BOOLEAN NOT NULL DEFAULT false` to the `runs` table. The job-stub INSERT in `upsert_job_in_txn` writes `placeholder = true` to satisfy the FK constraint when a job event arrives before its parent run event. Real workflow_run UPSERTs leave `placeholder = false`, and the `ON CONFLICT UPDATE` clause explicitly sets `placeholder = false` to promote stubs to real rows. `/v1/state` reads `WHERE placeholder = false` to exclude stub rows from snapshots.
- `0004_outbox_watermarks.sql` — Creates `outbox_watermarks(replica_id TEXT PK, broadcast_watermark BIGINT NOT NULL, updated_at TIMESTAMPTZ NOT NULL)`. Each PgStore replica heartbeats its current `broadcast_watermark` here every 30 s; the retention sweep reads `MIN(broadcast_watermark)` across non-stale replicas as the multi-replica deletion floor. `updated_at` has no `DEFAULT now()` on purpose — every retention-path timestamp is bound Rust-side from `Clock::now()` so `TestClock`-driven tests can advance time deterministically (see [ADR 0007](../architecture-decisions/0007-outbox-retention-policy.md)).

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

## Observability

ATC emits metrics and spans through one OpenTelemetry pipeline. When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, `init_otel` (`backend/crates/atc-server/src/otel.rs`) builds an OTLP/HTTP tracer provider and a meter provider, sets both as the OTel globals, registers `TraceContextPropagator` globally, and layers `axum-otel-metrics::HttpMetricsLayer` onto `routes::api_routes()` for HTTP request duration. With the env var unset, the SDK is never initialized — no provider, no exporter, no background-task overhead — and the OTel global meter provider stays at the SDK's no-op default, against which every `PgMetrics` instrument and the process collector resolve as no-ops. An invalid endpoint value (typo, missing scheme, unparseable URI) is treated as unset and disables OTel with an `eprintln!` warning rather than letting the SDK silently fall back to its default `http://localhost:4318` target.

The metric and span authoring contract — naming, attributes, propagation, the `tokio::spawn` Instrument-trait gotcha, the histogram aggregation choice, the per-metric interpretation blocks, and the per-span inventory — lives in [`metrics.md`](metrics.md). This section describes the wiring; the contract lives there.

### Tracing

- **Boundary instrumentation.** Two root request spans cover the public HTTP surface: `webhook.handler` (in `routes::webhook_handler`) is constructed manually so `traceparent` extraction can attach the parent context before the span is entered; `state.snapshot` (in `routes::state_handler`) is constructed manually to record snapshot-size fields before the response is returned. `webhook.handler` descendants: `webhook.verify` (atc-github), `webhook.parse` (atc-github), `persist.apply.run_event` / `persist.apply.job_event` → `persist.outbox.insert.run` / `persist.outbox.insert.job` → `persist.upsert.run` / `persist.upsert.job` → `persist.notify.emit` (atc-store-pg). `state.snapshot` descendant: `persist.read.snapshot` (`PgStore::read_snapshot` / `InMemoryStore::read_snapshot` via `#[tracing::instrument]`). The drain pipeline emits a per-pass `drain.pass` root with N×`drain.broadcast` children; the listener emits a per-NOTIFY `listener.recv` root. In-memory mode emits a per-tick `eviction.sweep` root from `InMemoryStore::evict_expired`. In PG mode the retention task pair emits per-tick `outbox.heartbeat.tick` and `outbox.sweep.tick` roots. The `/v1/ws` upgrade emits one `ws.connection` root per client lifetime (independently rooted — sessions, not RPCs). `/readyz` dispatches through a `persist.liveness` child span that splits the DB ping outcome from the drain-heartbeat staleness check via a late-bound `liveness.outcome` field. The config watcher emits a `config.reload` root per reload attempt, faceted by stage outcome. All long-lived background tasks use per-tick roots — no task-lifetime parent — so every iteration exports on completion. See [`metrics.md`](metrics.md) § "Span inventory" for attributes and § "Task-lifetime root spans are an anti-pattern" for the rationale.
- **Cross-trace causal links.** The outbox row carries a nullable `traceparent` column (migration `0006_outbox_traceparent.sql`); the write path captures the current span's W3C traceparent at INSERT, and the drain task parses it on the way out and attaches it as an OTel span LINK on `drain.broadcast`. Drain stays a per-tick root by design — the LINK lets operators follow "webhook arrived → frame delivered to client" across the disconnected async-tx boundary without breaking the per-tick root invariant. The capture/parse helpers live in `atc-store-pg::traceparent`; under no-op OTel the column is NULL and the link is a no-op.
- **Per-query spans (PG mode).** The connection pool returned by `atc-store-pg::init_pool` is `crate::TracedPool` — `sqlx::PgPool` wrapped in `sqlx_tracing::Pool`. Every `sqlx::query!` / `sqlx::query_scalar!` / `sqlx::query_as!` call against `&self.pool` (or `&mut tx.executor()` inside a transaction) emits a child span (`sqlx.execute`, `sqlx.fetch_one`, etc.) with `db.system.name="postgresql"`, `db.query.text` (template SQL — bind values are inaccessible through sqlx's public `Execute` trait, so they cannot leak), `net.peer.name`, `net.peer.port`. Errors are auto-annotated with `error.type` / `error.message` / `error.stacktrace`. See [`metrics.md`](metrics.md) § "sqlx-tracing per-query spans".
- **W3C trace context.** `TraceContextPropagator` is installed globally in `init_otel`. Incoming `traceparent` headers extract a parent context that the webhook handler attaches to the request span before the first poll. Absent or malformed headers produce a fresh root.
- **Sampler.** Default `ParentBased(root=AlwaysOn)`. `init_otel` reads `OTEL_TRACES_SAMPLER` and `OTEL_TRACES_SAMPLER_ARG` directly (the SDK's autoload does not yet pick these up reliably as of opentelemetry 0.31). Ratio samplers (`traceidratio` / `parentbased_traceidratio`) honor the OTel spec default of `1.0` for an empty `OTEL_TRACES_SAMPLER_ARG`; an explicitly-invalid arg (out of range, unparseable) falls back to the default sampler with an `eprintln!` to stderr. The tracing subscriber is composed AFTER `init_otel` returns — see contract note in `backend/crates/atc-server/CLAUDE.md`.
- **Tokio spawn discipline.** Long-lived spawned futures (`spawn_listener_task`, `spawn_drain_task`, `InMemoryStore::spawn_eviction`) do NOT take a task-lifetime root span. Per-tick handler functions (`listener.recv`, `drain.pass`, `eviction.sweep`) are decorated with `#[tracing::instrument(...)]` directly, so each invocation emits its own root that exports on return. `drain.broadcast` stays nested under `drain.pass` because it is constructed via `info_span!("drain.broadcast").in_scope(...)` inside `drain_pass`. See [`metrics.md`](metrics.md) § "Task-lifetime root spans are an anti-pattern" for why a `.instrument(span)` wrapper at the spawn site would never export.

### Shutdown ordering

OTel SDK tear-down runs after every emitter has joined. `run_shutdown_orchestration` (in `backend/crates/atc-server/src/shutdown.rs`) joins all `PersistentStore`-owned background tasks (drain + listener + outbox heartbeat + outbox sweep in PG mode; eviction in in-memory mode), the process collector, and the axum graceful-shutdown drain BEFORE calling `tracer_provider.shutdown()` and `meter_provider.shutdown()`. The "no live emitter when shutdown fires" invariant is documented in a comment block at the OTel shutdown step in `shutdown.rs`; new emitter categories MUST extend that comment so the next contributor knows where to plug their join.

## Server Wiring

The server wires together `atc-core` (state store) and `atc-github` (webhook parsing) into a cohesive HTTP API. The design separates **state mutation** (webhook ingestion) from **state delivery** (REST snapshot and WebSocket stream), allowing each path to evolve independently.

### AppState

A shared `AppState` struct is passed to all handlers via Axum's `State` extractor:

```rust
struct AppState {
    persist: Arc<dyn PersistentStore>,
    webhook_secret: Option<String>,
    runner_pool_capacities: RwLock<Vec<RunnerPoolCapacity>>,
    config_events_tx: broadcast::Sender<ConfigEvent>,
    shutdown: CancellationToken,
    ws_tracker: TaskTracker,
}
```

- **`persist`** — `Arc<dyn PersistentStore>` (ADR 0005). The write-path dispatch point for webhook ingestion and the read-path for state snapshots. `PgStore` when `ATC_DATABASE_URL` is set; `InMemoryStore` otherwise. Route handlers call `state.persist.apply_*_event(env).await` and `state.persist.read_snapshot().await` without branching on storage mode. The store's broadcast sender (bounded, capacity 256) is the WS handler's subscription seam via `state.persist.subscribe()`. **In PG mode the drain task is the SOLE writer**; in-memory mode broadcasts directly under the seq mutex.
- **`webhook_secret`** — Optional GitHub webhook secret loaded from `ATC_GITHUB__WEBHOOK_SECRET`. If `None`, HMAC verification is skipped. If `Some`, signatures are required and validated.
- **`runner_pool_capacities`** — `tokio::sync::RwLock<Vec<RunnerPoolCapacity>>` wrapping the operator-declared capacity list. Built at startup from `Config::runner_pools`; replaced atomically by the `config_watcher` task on YAML reload. `routes::state_handler` takes a short `.read().await` and clones the slice onto each `/v1/state` snapshot. Tokio's `RwLock` is write-preferring, so a sustained read load from `/v1/state` cannot starve the watcher's writes.
- **`config_events_tx`** — Bounded `broadcast::Sender<ConfigEvent>` (capacity 256). The `config_watcher` task is the sole writer; the WS handler subscribes alongside `persist.subscribe()` and wraps each variant in the wire `WireFrame` shape. Lagged on this channel closes the WS connection symmetrically with the committed channel.
- **`shutdown`** — Shared `CancellationToken` for cooperative shutdown signalling to background tasks and WS handlers.
- **`ws_tracker`** — `TaskTracker` counting live WebSocket handlers. `ws_tracker.wait()` is awaited during shutdown to ensure every WS client receives a `Close(1001)` frame before the process exits.

PG-mode operational fields (`pg_pool`, `min_pending_seq`, `last_drain_pass_at`, `broadcast_watermark`) are owned by `main.rs` and/or the drain/listener tasks directly; they are not part of `AppState`. The seq counter and PG pool are encapsulated within `PgStore` and `InMemoryStore` respectively.

Task handles live in `main.rs` scope; `AppState` does not own them.

### CommittedEvent Wire Contract

`CommittedEvent` is the broadcast envelope carrying a domain event and the seq it was assigned at commit time:

```rust
pub struct CommittedEvent {
    pub seq: u64,
    pub event: WebhookEvent,
}
```

The frontend derives `RunnerPoolStats` from the underlying job state via `computePoolStats(runStore.jobs)` — see `frontend/src/lib/stores/runners.svelte.ts` (ADR 0004). The webhook handler does not take a pool-stats snapshot under the seq mutex.

- **All successful events (Run or Job):** `CommittedEvent { seq, event }` is broadcast. `seq` is the value of the AppState `seq` counter after pre-increment (first commit broadcasts `seq=1`).
- **Failed transitions:** No broadcast occurs and no `CommittedEvent` is emitted. Clients never receive events that are not reflected in the store.

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
     - Emits metrics: `atc_pg_write_failures_total{kind="parity"}` on `InvalidTransition`; `atc_pg_write_failures_total{kind="transient"}` on backend errors; `atc_pg_notify_emitted_total{kind}` after a successful commit. Every `atc_pg_*` emit goes through a cached handle on `PgMetrics` (see [Cached handle convention](metrics.md#cached-handle-convention)).
     - **Does NOT broadcast to `webhook_tx`, does NOT apply in-memory state, and does NOT touch the seq mutex.** The drain task is the sole broadcaster in PG mode.
     - Returns `Ok(<u64 seq>)` on success.
   - **`InMemoryStore` path** (when `ATC_DATABASE_URL` is unset):
     - `InMemoryStore::apply_*_event` acquires `seq` mutex **before** any mutation (ordering invariant).
     - Calls `atc_core::state_machine::apply_*_event` (pure free function) to compute the updated entity. On `StateMachineError::InvalidTransition` → `PersistError::InvalidTransition`. No broadcast emitted for rejected transitions.
     - Pre-increments seq (`*seq_guard += 1; let seq = *seq_guard`) and broadcasts `CommittedEvent { seq, event }` under the mutex.
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

**Responsibility:** Accept WebSocket upgrades and push `CommittedEvent`s to connected clients in real time.

**Flow:**
1. Accept WebSocket upgrade request via Axum's `WebSocketUpgrade` extractor
2. Create a new broadcast receiver: `webhook_tx.subscribe()`
3. Spawn a task that:
   - Awaits messages from the receiver in a loop
   - Serializes each `CommittedEvent` to JSON
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
2. Read the store snapshot under the store's read lock (`StateSnapshot { last_seq, runs, jobs }`).
3. Read `seq` as `last_seq`.
4. Release the mutex and serialize.

**StateSnapshot:**
```rust
struct StateSnapshot {
    last_seq: u64,            // Highest committed seq; serializes as "lastSeq"
    runs: Vec<WorkflowRun>,
    jobs: Vec<Job>,
    runner_pool_capacities: Vec<RunnerPoolCapacity>,
                              // Operator-declared pool ceilings.
                              // serde(default) → empty vec on missing field
                              // (rolling-deploy tolerance for older replicas).
    display_ttl_seconds: u32, // Operator-configured display TTL, stamped by
                              // state_handler. serde(default) → 0 (no filter)
                              // on missing field; the frontend uses it to age
                              // out completed rows reactively. See ADR 0009.
}
```

**Cursor semantics:** `last_seq` is the highest committed seq the drain has broadcast (PG mode) or the in-memory counter's current value (in-memory mode). `last_seq = 0` is the cold-start sentinel. All events with `seq <= last_seq` are guaranteed reflected in the snapshot. In PG mode the snapshot may additionally include commits the drain has not yet broadcast — those are buffered on the WS side and applied idempotently when their `CommittedEvent`s arrive. The frontend filters `seq > lastSeq` against the buffer.

**Capacity composition:** `runner_pool_capacities` is operator config, not observed state. `PersistentStore` (Postgres + InMemory) leaves the field as `Vec::new()` when constructing a snapshot. `routes::state_handler` takes a short `read().await` on `AppState::runner_pool_capacities` and clones the slice onto the response after the persist call returns. The `PersistentStore` trait stays single-purpose — it owns event-derived state only. Capacity changes propagate without a process restart: the `config_watcher` task re-reads the YAML on filesystem change and replaces the RwLock contents atomically (see [Hot-reload](#hot-reload-config_watcher) below).

#### Snapshot cutoff and display TTL

`PersistentStore::read_snapshot(cutoff: Option<DateTime<Utc>>)` filters out completed runs and jobs whose terminal timestamp is older than the supplied cutoff. The cutoff is computed in `routes::state_handler` from `AppState.clock.now() - AppState.display_ttl` (the configured `ATC_DISPLAY_TTL`) — the store is config-agnostic; the route layer is the only place where event-derived state and config meet on the read path.

The predicate is identical on both backends and permissive on `completed_at IS NULL`:

```sql
WHERE (cutoff IS NULL OR status != 'Completed' OR completed_at IS NULL OR completed_at >= cutoff)
```

(`atc-store-mem` evaluates an equivalent Rust expression pre-collect.) PG-mode reads use the composite `(status, completed_at)` index on `runs` added by migration `0007_runs_completed_at.sql` (and the existing `jobs_status_completed_at_idx` for jobs). `cutoff = None` disables filtering — used by test callers that need the unfiltered snapshot. The frontend mirrors the predicate against `uiStore.nowMs` so completed rows age out reactively without an event arriving. See [ADR 0009](../architecture-decisions/0009-display-vs-data-retention.md) for the design rationale and the deliberate in-memory mode narrowing (the in-memory store keeps its hardcoded 1 h completed-eviction TTL; with `ATC_DISPLAY_TTL > 1h` eviction wins).

### Hot-reload (`config_watcher`)

`config_watcher::spawn_config_watcher` (in `backend/crates/atc-server/src/config_watcher.rs`) watches the parent directory of `$ATC_CONFIG_FILE` with `notify-debouncer-full` (500 ms debounce, non-recursive). Each debounced filesystem event triggers a reload through `config::reload_runner_pools` — a narrow-schema parse that only observes the `runner_pools` block, so scalar fields are deliberately ignored. The reload outcome is one of:

- **Applied** — new capacities differ from current AppState. The watcher takes `runner_pool_capacities.write().await`, compares inside the guard (TOCTOU-safe), replaces the inner Vec, releases the guard, then broadcasts `ConfigEvent::Update(new_caps)` on `config_events_tx`. The WS handler wraps it as `WireFrame::ConfigUpdate`.
- **No-op** — content matches current AppState. Counter increments under `reason="noop"`; no broadcast.
- **Failure** — read / parse / validate error. Old capacities stay in place. The watcher logs structured error, increments `atc_config_reload_total{result="failure",reason=<category>}`, and broadcasts `ConfigEvent::ReloadError { reason }`.

Each reload attempt also runs a diagnostic scalar-drift check: the watcher parses the file as a full `Config` (errors suppressed), diffs against a `ScalarSnapshot` captured at startup, and emits a `tracing::warn!` per changed scalar field. This catches the "I edited `http_addr` in YAML — why didn't it take effect" foot-gun without re-architecting hot-reload for scalar fields (out of scope per the design plan).

**Kubernetes ConfigMap atomic-swap.** kubelet projects the ConfigMap via a `..data` symlink that gets atomically renamed on update. The watcher's parent-dir watch sees the rename. The Helm chart mounts the ConfigMap as a directory (`mountPath: /etc/atc`, no `subPath`) — `subPath` mounts block kubelet propagation, so hot-reload literally cannot work behind `subPath`.

**Bare-metal dev / missing parent dir.** If `config_path.parent()` doesn't exist, `spawn_config_watcher` returns `None` with a warn log and the process boots cleanly without hot-reload (the watcher handle in `run_shutdown_orchestration` is `Option<JoinHandle<()>>`).

**Missing-file divergence.** Startup tolerates a missing config file (figment's `Yaml::file` is auto-optional). On reload, a deleted file is treated as `ReloadError::Read` — an operator who deletes the file mid-deploy almost certainly didn't intend to clear all pool capacities.

### WireFrame (WS framing)

The WS endpoint frames every outbound event in an outer `kind` discriminator, defined as `pub enum WireFrame` in `backend/crates/atc-server/src/ws.rs`:

```rust
#[derive(Serialize, ts_rs::TS)]
#[serde(tag = "kind")]
pub enum WireFrame {
    Committed(CommittedEvent),
    #[serde(rename_all = "camelCase")]
    ConfigUpdate { runner_pool_capacities: Vec<RunnerPoolCapacity> },
    ConfigReloadError { reason: String },
    ServerHello { version: String },
    GoingAway { reason: String },
}
```

`#[serde(tag = "kind")]` is internally-tagged: the variant name lands at `kind`, and `CommittedEvent`'s fields (`seq`, `event`) flatten into the same object. The `ConfigUpdate` variant uses `rename_all = "camelCase"` so `runner_pool_capacities` serializes as `runnerPoolCapacities`, matching the existing snapshot convention.

`WireFrame` is local to the `ws.rs` boundary — `CommittedEvent` (in `atc-wire`) and `WebhookEvent` (in `atc-github`) are not modified. Stores remain pure event sources; only the WS handler knows about the outer framing. ts-rs exports `WireFrame.ts` to `frontend/src/lib/types/generated/`.

**Frontend handling is not described here.** Each `WireFrame` variant's rustdoc deliberately stays at the wire-contract layer — payload shape, sequencing, and the backend's own post-emit behavior — and points at `docs/architecture/frontend-app.md` for any UI surfacing (e.g., `ConfigReloadError` → admin alert banner; `ServerHello` mismatch → refresh banner). This split keeps backend-side rustdoc independent of frontend UX iteration; bumping the version, copy, or dismissal model of a banner should never invalidate a wire-contract claim.

**Connection lifecycle (issue #47).** `ServerHello { version }` is sent synchronously as the first text frame on every fresh WS connection, carrying `env!("VERGEN_GIT_DESCRIBE")` so the frontend can detect a backend redeploy across a reconnect (a session's first ServerHello becomes its reference; later mismatches arm a refresh banner). Broadcast receivers are subscribed before the WebSocket upgrade completes, so any committed or config events that fire between subscription and the ServerHello send accumulate in the bounded channel and drain through the `select!` loop AFTER ServerHello ships — one task owns the socket, so the "ServerHello is the first text frame" invariant holds without additional synchronization. On graceful shutdown, `GoingAway { reason }` is sent immediately before the existing `Close(1001 "going away")` frame; both are best-effort because the client may already be gone. The Close-1001 transport signal remains the authoritative shutdown indication — `GoingAway` is informational application-level metadata that lets the frontend's connection indicator render a tailored "server restarting" state during the gap between the close and the next reconnect.

**Lagged on either channel closes the WS.** The WS handler subscribes to both the committed channel (`persist.subscribe()`) and the config channel (`config_events_tx.subscribe()`). Either receiver returning `RecvError::Lagged` closes the socket — the client reconnects, fetches `/v1/state` to re-establish both the seq cursor and the current capacity list. Symmetric handling avoids the silent-drop trap where one channel's overflow goes unnoticed.

Return 200 with the JSON snapshot.

**Snapshot/stream reconciliation:** A client can call `GET /v1/state` to establish baseline state, note the returned `lastSeq`, then connect to `GET /v1/ws` and filter incoming `CommittedEvent`s to those with `seq > lastSeq` (strictly greater than — buffered events with `seq <= lastSeq` are already reflected in the snapshot and discarded). This protocol allows robust reconnection and bootstrap.

### Configuration

Server wiring configuration uses figment with three layers, lowest precedence to highest: `defaults → Yaml::file($ATC_CONFIG_FILE | "/etc/atc/config.yaml") → Env::prefixed("ATC_").split("__")`. Missing file is benign (figment's `Yaml::file` is auto-optional). Env carries scalar overrides only — structured config (currently `runner_pools`) is file-only by design.

Notable fields:

- **`github.webhook_secret`** (`ATC_GITHUB__WEBHOOK_SECRET` env var) — Optional string. If present, all webhook requests must carry a valid HMAC-SHA256 signature. If absent, signatures are not required or verified.
- **`runner_pools`** (file-only, no env override) — Operator-declared `Vec<RunnerPoolCapacity { labels: LabelSet, capacity: Option<u32> }>`. figment deserializes YAML directly into `atc_core::RunnerPoolCapacity`; `LabelSet` (BTreeSet) canonicalizes labels (sort + dedup) during deserialization. A post-extract `validate_capacities` scan rejects empty label sets, `capacity == 0`, and duplicate canonicalized label sets. The validated list flows into `AppState::runner_pool_capacities` and is composed onto every `/v1/state` snapshot. See `docs/architecture/deployment.md` § "File-based configuration" for the operator-facing surface.

### Lifecycle Wiring

In `main.rs`:
1. Load config via `Config::load()`.
2. Initialize the OTel pipeline by calling `otel::init_otel(&cfg)`. Returns `Some(handles)` when `OTEL_EXPORTER_OTLP_ENDPOINT` is set (tracer/meter providers built and set as the OTel globals, propagator installed); returns `None` otherwise. The handles flow into `run_shutdown_orchestration` so providers flush during graceful shutdown.
3. Construct the single `CancellationToken` shared by every supervised surface.
4. Call `metrics::register_build_info()` to emit the `atc_build_info` startup gauge against the global meter. The `PgMetrics` instruments are constructed transitively by `PgStore::start` so PG-mode instruments only register when a `PgStore` is built. When OTel is disabled the global meter is a no-op meter and every emit is a cheap no-op.
5. Construct `persist: Arc<dyn PersistentStore>` via the mode-appropriate `start` constructor:
    - **PG mode** (`ATC_DATABASE_URL` set): `db::init_pool(url)` → `listener::connect_listener(listener_url)` → `PgStore::start(pool, listener_conn, shutdown.clone())`. `PgStore::start` runs `SELECT COALESCE(MAX(seq), 0) FROM outbox` to seed the watermark (last fallible step), then spawns the listener and drain tasks internally and stores their `JoinHandle`s on the returned `Arc<PgStore>` (ADR 0006).
    - **In-memory mode**: `InMemoryStore::start(clock, completed_ttl, eviction_period, shutdown.clone())`. The store constructs its own `broadcast::channel(256)`, spawns the eviction task internally, and stores the `JoinHandle` on the returned `Arc<InMemoryStore>` (ADR 0006).
6. Create `AppState { persist, clock, display_ttl, webhook_secret, runner_pool_capacities, config_events_tx, shutdown, ws_tracker, ws_metrics }` and pass to Axum via `.with_state()`. The `runner_pool_capacities` field is built once at startup by walking `Config::runner_pools` (already validated and canonicalized by `Config::load`) and replaced atomically by `config_watcher` on filesystem changes (see § Hot-reload below). `config_events_tx` is the bounded broadcast sender the watcher uses to deliver `ConfigEvent::{Update, ReloadError}` to WS handlers. The `clock` field is the same `Arc<dyn Clock>` handed to the active store, so a `TestClock` in integration tests drives both the snapshot cutoff in `state_handler` and the store's internal time-dependent operations. The `display_ttl` field carries the operator-configured visibility window — see § Snapshot cutoff and display TTL below. The webhook handler dispatches uniformly through `state.persist.apply_*_event(env)`; the route handler does not branch on storage mode. WS handlers obtain their broadcast receiver via `state.persist.subscribe()` and additionally subscribe to `state.config_events_tx`.
7. Bind the server to `http_addr` via `axum::serve`.
8. On graceful shutdown, execute the cooperative shutdown sequence — see § [Supervision and Shutdown](#supervision-and-shutdown) below.

The eviction task runs periodically (default interval: `Duration::from_mins(1)`) and removes completed jobs whose TTL (default: `Duration::from_hours(1)`) has elapsed since `completed_at`. This keeps in-memory state bounded and prevents unbounded growth.

## Supervision and Shutdown

ATC uses a single-token cooperative shutdown model implemented in `backend/crates/atc-server/src/shutdown.rs`. The orchestration function `run_shutdown_orchestration` is called from `main` and awaits the full sequence to completion before the process exits.

### Cancellation surfaces

Five supervised surfaces observe a single shared `CancellationToken` (`shutdown`) and exit cooperatively when it fires:

1. **Eviction task** (spawned internally by `InMemoryStore::start`) — `tokio::select!` on `cancel.cancelled()` vs `ticker.tick()`. Exits at its next tick boundary after the token fires. Joined by `InMemoryStore::shutdown()`.
2. **Listener task** (spawned internally by `PgStore::start`) — `tokio::select!` on `cancel.cancelled()` vs `pg_listener.recv()`. Exits cooperatively. Joined by `PgStore::shutdown()`.
3. **Drain task** (spawned internally by `PgStore::start`) — checks the token only between drain passes, never inside `drain_pass()`. The current pass always runs to completion (or to a Postgres error); cancellation fires at the next inter-pass check. After the loop exits, the task runs one bounded `SELECT COUNT(*) FROM outbox WHERE seq > watermark` (1 s timeout) and records the result into `atc_pg_drain_shutdown_remaining_rows` so operators can verify the cooperative-shutdown assumption that the unscanned tail rarely exceeds one drain pass; on query failure or timeout the observation is skipped (logged) rather than recorded as zero. Joined by `PgStore::shutdown()`.
4. **Process metrics collector** (`atc-server::metrics::spawn_process_collector`) — `tokio::select!` on `cancel.cancelled()` vs `ticker.tick()`. `Collector::collect()` is synchronous; the cancel arm fires between ticks, not mid-collect.
5. **WebSocket handlers** (`atc-server::ws::handle_socket`) — each spawned as a tracked future in `ws_tracker`. The select loop watches `shutdown.cancelled()` (top of the biased order); on cancel the handler emits `WireFrame::GoingAway` followed by `Close(1001 "going away")` and returns. See § WireFrame (WS framing) for the lifecycle invariants.

The two `axum::serve(...).with_graceful_shutdown(...)` futures observe the same token through `cancelled_owned()` clones.

### Trigger paths

`shutdown.cancel()` fires from one of two paths:

- **Signal-driven (normal path):** SIGTERM / SIGINT → signal handler → `shutdown.cancel()`.
- **Self-healing on serve failure:** if either spawned `axum::serve` task exits unexpectedly before any signal arrives (e.g., an accept-loop failure), `run_shutdown_orchestration` observes that exit via `tokio::select!`, logs an `error!` naming the affected serve, and calls `shutdown.cancel()` itself so the remaining tasks shut down cooperatively rather than getting orphaned by a half-up process.

In either case, once `shutdown` is cancelled:

```
shutdown.cancel()
  ├── axum serves:    graceful_shutdown future resolves → stops accepting new connections
  ├── eviction task:  cancel arm fires → exits at next tick
  ├── listener task:  cancel arm fires → exits
  ├── metrics task:   cancel arm fires → exits at next tick
  ├── drain task:     cancel arm fires between passes → finishes current pass → exits
  └── WS handlers:    cancel arm fires → send Close(1001) → return → ws_tracker counts down
```

The orchestration then awaits `ws_tracker.wait()` (so connected clients receive their Close frames before runtime drop), joins the spawned serve tasks, and joins the store-owned background tasks via `persist.shutdown()` — one call that internally joins the listener + drain (PG mode) or the eviction task (in-memory mode) within their per-task budgets. The process metrics collector is joined last, then the OTel pipeline flushes.

### Why a single token suffices

A previous design used two tokens (`shutdown` for tasks, a separate `ws_close` for WS handlers) plus a `webhook_tx_keepalive` clone, so WS clients on the dying replica would receive every event from the drain task's final pass before their Close frame. That ordering is not load-bearing — clients reconnect to a healthy replica and fetch `/v1/state`, whose REPEATABLE READ snapshot reflects every committed row in PG (including ones the dying replica's drain didn't broadcast). The frontend uses `snapshot.lastSeq` as its cursor and resumes WS event delivery from there; see `frontend/src/lib/connection.ts`.

The new replica does not re-broadcast the dying replica's unprocessed outbox rows: its drain seeds `initial_watermark` from `MAX(seq) FROM outbox` at startup and only broadcasts seqs strictly greater than that. Catch-up is purely the snapshot endpoint's job.

### WS handler — biased select

The WS handler loop uses `tokio::select! { biased; … }` with arms ordered:

1. `shutdown.cancelled()` — send `Close(1001 "going away")` and exit.
2. `rx.recv()` — forward broadcast events as JSON text frames.
3. `socket.recv()` — detect client-initiated close or read errors.

Cancel is first so the cancel signal is preferred over any concurrently-ready arm, keeping shutdown predictable for tests and operators. A client-initiated Close still wins via arm 3 if it arrives independently; the server then exits via the client-initiated branch (no server-side Close 1001).

`main` keeps an `Arc<AppState>` clone alive for the lifetime of the function (`with_state(app_state.clone())` rather than moving the Arc into the router), so the `Arc<dyn PersistentStore>` embedded in `AppState` — and therefore the store's internal broadcast sender — stays alive through the full orchestration. This means the `RecvError::Closed` arm of `rx.recv()` is only reached in genuinely abnormal scenarios — not as part of the normal shutdown path — and the handler simply returns from there without trying to send a Close frame on a torn-down channel.

### WS task tracking — `TaskTracker`

`AppState` holds a `pub ws_tracker: TaskTracker`. The WS handler wraps each upgrade future via `state.ws_tracker.track_future(handle_socket(...))` before passing it to `ws.on_upgrade(...)`. This lets `run_shutdown_orchestration` call `ws_tracker.wait()` after the cancel signal and know when all in-flight WS handlers have finished sending their Close frames.

`TaskTracker::close()` is called before `wait()` — this is the signal that makes `wait()` return once the in-flight count reaches zero. A late WS upgrade that arrives between `close()` and `wait()` returning is still tracked; since `shutdown` is already cancelled by that point, the late handler enters its cancel arm immediately and exits in milliseconds.

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

**Signal:** ATC shuts down gracefully on SIGTERM or SIGINT. The signal handler cancels the `shutdown` token, which begins the sequence above.

**Aggregate timeout:** Worst-case shutdown completes in approximately 13 seconds (5 s drain + 2 s WS tracker + 3 s serves + 1 s each for listener, eviction, and metrics, if every timeout fires). In practice, tasks exit well within budget on clean shutdown.

**K8s settings:** `terminationGracePeriodSeconds: 30` (the Kubernetes default) is sufficient. The 13-second aggregate worst case leaves 17 seconds of headroom. No adjustment to this value is needed for ATC's shutdown budget.

**Load-balancer de-registration:** Kubernetes removes a pod from `EndpointSlice` after the pod enters `Terminating` (`ready=false, serving=true, terminating=true` per `ProxyTerminatingEndpoints`, GA in K8s 1.28), but the propagation delay to kube-proxy and any cloud-LB controller means new requests may still arrive for a few seconds after the kubelet would otherwise send SIGTERM. The chart absorbs this propagation delay with a `preStop` `sleep` action (`shutdown.preStopSleepSeconds`, default 5 s; opt out with `0`) and pairs it with `terminationGracePeriodSeconds` (default 30 s) sized for the ~13 s aggregate shutdown budget plus the preStop hold. The `/readyz` shutdown short-circuit (returning 503 `{"status":"shutting_down"}` once `state.shutdown` is cancelled) gives cloud-LB controllers and service meshes that watch readiness probes directly an immediate signal — independent of the EndpointSlice flip. See `docs/architecture/deployment.md` § Graceful shutdown for the operator-side surface.

**Webhook durability during shutdown:** Webhooks committed to the outbox before or during the shutdown window are durable in PG. The dying replica's drain task may exit before broadcasting all committed-but-undrained rows; this is intentional. Clients reconnecting to a healthy replica fetch `/v1/state`, whose REPEATABLE READ snapshot reflects every committed row (including ones the dying replica didn't broadcast) and exposes the corresponding `lastSeq` cursor. Subsequent WS events flow normally from there. No committed webhook is lost.

**In-memory mode:** The listener and drain tasks do not exist. The `drain_handle` and `listener_handle` are `None`; their join steps are skipped. In-memory state is lost on process exit; this is expected and documented as a dev-only mode.

### Health Probes

- `/readyz` — Readiness probe. Short-circuits to 503 `{"status":"shutting_down"}` whenever `state.shutdown.is_cancelled()` is true — checked first so a draining pod stops doing PG work on every probe pass. Otherwise: in in-memory mode (no PG pool), returns 200 immediately. When `ATC_DATABASE_URL` is configured: (1) runs `SELECT 1` against the pool — 503 `{"status":"db_unreachable"}` if the DB is unreachable; (2) checks the drain heartbeat age — if `last_drain_pass_at` is older than `READYZ_HEARTBEAT_STALENESS_MS` (30 s), returns 503 `{"status":"drain_stale"}`. A healthy drain updates its heartbeat every 5 s (`HEARTBEAT_TICK`), so any value older than 30 s indicates the drain task has stalled. Returns 200 `{"status":"ok"}` when all checks pass.
- `/healthz` — Liveness probe. Returns 200 unconditionally — process up = alive regardless of DB state.

### NOTIFY Emission and Drain Pipeline

**Two-task coalescing structure:** The listener pipeline splits into two cooperating tasks:

1. **Listener task** — Holds a dedicated long-lived `PgListener` connection. Receives PG NOTIFY payloads on the `atc_outbox` channel, increments `atc_pg_notify_received_total`, calls `min_pending_seq.fetch_min(seq, Release)` to register the notified seq for gap-healing, and fires an `Arc<tokio::sync::Notify>` to wake the drain task. Does not fetch rows itself.

2. **Drain task** — Waits on `Arc<Notify>` (level-triggered), but also wakes on a 5 s heartbeat tick to refresh `last_drain_pass_at`. On every iteration (notify or tick): stores `clock.now().timestamp_millis()` (sourced from `PgStore.clock: Arc<dyn Clock>`) to `last_drain_pass_at`. On heartbeat-only wakes, skips the pass body. On NOTIFY-driven wakes:
   - Swaps `min_pending_seq` to `i64::MAX` (AcqRel) to capture the gap-healing backstop. Computes `pass_start_floor = watermark.min(backstop.saturating_sub(1))`.
   - **Pagination loop:** fetches pages of `DRAIN_BATCH_SIZE=500` rows `WHERE seq > page_cursor ORDER BY seq`, advancing `page_cursor` on each page until a partial page is returned.
   - For each row: decodes the JSONB payload as `RunEventEnvelope` or `JobEventEnvelope`. On decode failure, logs an error and skips. On unknown `kind`, increments `atc_pg_drain_unknown_kind_total` and skips.
   - **Ring-buffer dedup:** Before broadcasting a seq, checks `recent_set` (HashSet over the ring buffer of capacity `DEDUP_CAP=2048`). If already seen, increments `atc_pg_drain_duplicate_skipped_total` and skips. If new, inserts into ring and set; evicts the oldest entry if the ring is full.
   - **Broadcasts** `CommittedEvent { seq: u64, event: WebhookEvent }` on `webhook_tx` for each row that passes dedup. The outbox-lag histogram observation (`atc_pg_outbox_lag_seconds`) reads `clock.now() - row.inserted_at` through the same `PgStore.clock`, so under `TestClock` it is deterministic — see [`metrics.md`](metrics.md) § "Operational metrics".
   - After the full pagination loop, advances `watermark` to the highest seq seen. Refreshes `last_drain_pass_at` again.

**Wall-clock seam.** Every wall-clock read on the PG hot path — heartbeat refresh, liveness staleness comparison, outbox-lag broadcast observation — flows through `PgStore.clock: Arc<dyn Clock>` (production: `SystemClock`; tests: `TestClock` with `advance()`). Monotonic latency measurements (drain-pass duration, drain startup, eviction-sweep elapsed) intentionally bypass this seam and use `std::time::Instant` directly — wall-clock would be semantically wrong (it can jump backward under NTP) and tests do not assert on those histogram values. See the trait doc-comment in `backend/crates/atc-core/src/clock.rs` for the canonical rationale. A `disallowed-methods` clippy lint in `backend/clippy.toml` blocks new direct `chrono::Utc::now` / `std::time::SystemTime::now` callers across the workspace.

**Gap-healing backstop:** The concurrent-commits race: webhook A commits seq=1 and fires NOTIFY, but before the drain wakes, webhook B commits seq=2. The drain wakes on B's NOTIFY, calls `swap(MAX, AcqRel)`, gets backstop=1 (A registered it first), computes `floor = watermark.min(0) = 0`, and scans from 0 — fetching both seq=1 and seq=2 in order. Without the backstop, a drain that woke on B's NOTIFY and started scanning from `watermark=0` would also catch A; the backstop is a safety net for the case where the drain has already advanced `watermark` past where A's NOTIFY would land.

**DSN session-mode contract:** The listener connection (`ATC_DATABASE_LISTENER_URL` or fallback to `ATC_DATABASE_URL`) must be a session-mode endpoint. Transaction-mode PgBouncer reassigns the underlying connection between transactions, silently dropping LISTEN registrations. When the main pool uses transaction-mode PgBouncer, set `ATC_DATABASE_LISTENER_URL` to a direct Postgres DSN or a session-mode PgBouncer endpoint.

**Reconnect loss window:** If the listener task reconnects after a connection drop, any NOTIFYs delivered during the reconnect window are not received. This is healed automatically: the next NOTIFY after reconnection triggers a drain pass that fetches `seq > watermark`, catching up all rows that were inserted while the listener was disconnected. No data is lost; only latency increases during the reconnect window.

### Modules

- **`state.rs`** — `AppState` struct (fields: `persist: Arc<dyn PersistentStore>`, `webhook_secret`, `runner_pool_capacities: Vec<RunnerPoolCapacity>`, `shutdown: CancellationToken`, `ws_tracker: TaskTracker`). The broadcast envelope `CommittedEvent { seq, event }` and the REST baseline `StateSnapshot { last_seq, runs, jobs, runner_pool_capacities }` live in the `atc-wire` crate (ADR 0008); WS handlers obtain their receiver via `state.persist.subscribe()` (ADR 0006).
- **`routes.rs`** — Route handlers for `/v1/webhooks/github`, `/v1/ws`, `/v1/state`, `/healthz`, `/readyz`; webhook handler dispatches through `state.persist.apply_*_event(env)` uniformly for both modes (ADR 0005), returns `{"status":"accepted","seq":<u64>}` on success; state handler reads `read_snapshot` from `PersistentStore`, then composes `runner_pool_capacities` from `AppState` onto the response (capacity is config, not store-owned state); uses REPEATABLE READ in PG mode; `/readyz` checks drain heartbeat staleness in PG mode
- **`ws.rs`** — WebSocket connection handling and message broadcast logic. Subscribes via `state.persist.subscribe()` (ADR 0006).
- **Persistence layering.** The `PersistentStore` trait, `LivenessError`, the `PersistError` re-export, and the shared `join_with_timeout` helper live in [`atc-persist`](../../backend/crates/atc-persist/CLAUDE.md). `InMemoryStore` lives in [`atc-store-mem`](../../backend/crates/atc-store-mem/CLAUDE.md). `PgStore` and its supporting modules (writes, retention, listener+drain, snapshot reads, metrics, db init, embedded migrations) live in [`atc-store-pg`](../../backend/crates/atc-store-pg/CLAUDE.md). `atc-server` no longer carries any `src/persist/` module — the constructor seam is `Arc<dyn PersistentStore>` and the file-by-file inventory below points into the per-store crates. Layering anchors: ADR-0005 (trait relocation), ADR-0006 (stores own background-task lifecycle), ADR-0008 (persistence crate split).

## Files

- `backend/crates/atc-server/src/main.rs` — Server entry point, config loading, tracing branching, router composition. Imports `atc_store_pg::{db, listener, DbInitError, PgStore}` and `atc_server::persist::InMemoryStore`; constructs `Arc<dyn PersistentStore>` via `PgStore::start` / `InMemoryStore::start` (ADR 0006); the per-mode background-task lifecycle is owned by the store, not main. The migration-vs-connect discriminator at the `init_pool` call site matches on `DbInitError::Migrate(_)`.
- `backend/crates/atc-server/src/config.rs` — figment-based Config struct, LogFormat enum, GitHubConfig with webhook_secret, Config::load()
- `backend/crates/atc-server/src/routes.rs` — API route definitions (healthz, readyz, webhook, state, ws endpoints)
- `backend/crates/atc-server/src/state.rs` — `AppState` struct (six fields: `persist`, `webhook_secret`, `runner_pool_capacities` (RwLock-wrapped), `config_events_tx`, `shutdown`, `ws_tracker`). `CommittedEvent { seq, event }` and `StateSnapshot { last_seq, runs, jobs, runner_pool_capacities }` live in `backend/crates/atc-wire/src/lib.rs` (ADR 0008). The `ConfigEvent` enum that flows through `config_events_tx` lives in `backend/crates/atc-server/src/config_watcher.rs`.
- `backend/crates/atc-server/src/ws.rs` — WebSocket handler. Subscribes via `state.persist.subscribe()` (ADR 0006); per-connection event forwarding and CommittedEvent serialization unchanged.
- `backend/crates/atc-server/src/assets.rs` — rust-embed struct, embedded file serving, SPA fallback, dev proxy
- `backend/crates/atc-server/src/otel.rs` — `init_otel`: builds the OTLP/HTTP tracer + meter providers (when `OTEL_EXPORTER_OTLP_ENDPOINT` is set), installs `TraceContextPropagator` globally, registers the shared `exponential_histogram_view` so all histograms emit as base-2 exponential aggregations, and sets both providers as the OTel globals. `shutdown(handles)`: flushes both providers; called from `run_shutdown_orchestration` after every emitter has joined.
- `backend/crates/atc-server/src/metrics.rs` — Server-side OTel registration helpers: `register_build_info` (one-shot startup gauge against the global meter) and `spawn_process_collector` (wraps `opentelemetry-system-metrics::init_process_observer` in a `ProcessCollectorHandle` joined during shutdown). The PG-mode `PgMetrics` surface lives in `atc-store-pg::metrics`. All emit sites resolve through the OTel global meter installed by `otel::init_otel`. The metric inventory and authoring contract live in [`metrics.md`](metrics.md).
- `backend/crates/atc-server/build.rs` — vergen-gix Emitter emitting VERGEN_* compile-time env vars
- `backend/crates/atc-store-mem/src/lib.rs` — `InMemoryStore`: HashMap + indexes + seq mutex + clock + ttl + broadcast_tx + eviction `JoinHandle`; `start()` / `new_for_test()` constructors; the eviction sweep is a private `InMemoryStore::spawn_eviction` associated function called inside `start()`. Test-support inspection helpers in `atc-store-mem/src/invariants.rs`.
- `backend/crates/atc-store-pg/src/lib.rs` — Re-exports for the PG store crate (`PgStore`, `PgStoreStartError`, `DbInitError`, `init_pool`; `PgStoreTestHooks`/`PgStoreTestHandles` behind `cfg(any(test, feature = "test-support"))`).
- `backend/crates/atc-store-pg/src/store/` — `mod.rs` (constants, `PgStoreStartError`, `SqlRepr` impls, `PgStore` struct, `start` / `start_inner` / `ping` and `hostname_or_unknown`), `writes.rs` (`impl PersistentStore for PgStore` plus free-fn transaction helpers `upsert_*_in_txn`, `insert_outbox_*_in_txn`, `notify_outbox_seq_in_txn`), `retention.rs` (outbox heartbeat + sweep spawn fns and tick bodies), `test_hooks.rs` (`cfg`-gated test seams). Per-task shutdown budgets `SHUTDOWN_TIMEOUT_DRAIN`, `SHUTDOWN_TIMEOUT_LISTENER`, `SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT`, `SHUTDOWN_TIMEOUT_OUTBOX_SWEEP` live in `store/mod.rs` and are consumed by `PgStore::shutdown()`.
- `backend/crates/atc-store-pg/src/listener.rs` — PG LISTEN/NOTIFY background tasks: `spawn_listener_task` (receives notifications, registers seq in `min_pending_seq`, fires `Arc<Notify>`) and `spawn_drain_task` (wakes on notify or 5 s heartbeat; NOTIFY-driven passes fetch outbox rows by `seq > pass_start_floor ORDER BY seq` in pages, decode payload, apply ring-buffer dedup, broadcast `CommittedEvent`s, advance watermark; every iteration updates `last_drain_pass_at`). Constants: `DRAIN_BATCH_SIZE=500`, `HEARTBEAT_TICK=5s`, `DEDUP_CAP=2048`. The sole production caller is `PgStore::start_inner`. The `connect_listener_fails_on_bad_url` unit test wraps `connect_listener` in a 2 s `tokio::time::timeout` to cap test runtime — sqlx's default `connect_timeout` is 30 s, which would otherwise dominate the lib-test wall clock for a negative-path assertion.
- `backend/crates/atc-store-pg/src/reads.rs` — `read_all_runs` / `read_all_jobs` REPEATABLE READ helpers used by `PgStore::read_snapshot`.
- `backend/crates/atc-store-pg/src/db.rs` — `init_pool(url)`: connects sqlx PgPool and runs embedded migrations; returns `Result<PgPool, DbInitError>` so atc-server's main can pattern-match on `DbInitError::Migrate(_)` without naming `sqlx::Error` directly. The `MIGRATOR` static (`sqlx::migrate!("./migrations")`) is exposed publicly for test fixtures that need to run migrations against a caller-managed pool.
- `backend/crates/atc-store-pg/src/metrics.rs` — `PgMetrics` struct + `PgMetrics::register(...)` constructor: cached OTel instruments + pre-built `[KeyValue; N]` attribute slices for every `atc_pg_*` emit site, owned by `PgStore`. The observable gauges take `Weak<AtomicI64>` so callbacks for prior `PgStore` instances become no-ops once their tasks drop strong refs.
- `backend/crates/atc-store-pg/migrations/0001_initial_runs_jobs.sql` — Initial schema: `runs` and `jobs` tables with CHECK constraints, FK, and indexes
- `backend/crates/atc-store-pg/migrations/0002_outbox.sql` — Outbox table: `BIGSERIAL seq` PK, `kind` discriminator, `run_id`/`job_id`, `payload JSONB` (domain event envelope), `inserted_at TIMESTAMPTZ`; `outbox_run_idx` on `run_id`
- `backend/.sqlx/` — Offline query cache (committed to repo). Generated by `cargo sqlx prepare --workspace -- --tests` to enable `SQLX_OFFLINE=true` builds without a live DB at compile time. Updated whenever SQL queries change.
- `backend/Cargo.toml` — Workspace definition with shared dependency versions
- `backend/crates/atc-server/Cargo.toml` — Server crate dependencies
