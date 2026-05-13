# CLAUDE.md — atc-server

Last verified: 2026-05-14

> Canonical documentation lives in `docs/architecture/backend-server.md`. This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Axum HTTP server wiring `atc-core` (pure transition functions) and `atc-github` (webhook parsing) together. Provides HTTP endpoints for webhook ingestion, REST state snapshots, and WebSocket event streaming. The only executable crate in the backend workspace. **All state persistence concerns** — the HashMap state + indexes for in-memory mode, the outbox/drain for PG mode, eviction, snapshot reads, and liveness checks — live in `atc-server::persist`. Both the webhook write path AND the `/v1/state` read path AND `/readyz` dispatch through `AppState.persist: Arc<dyn PersistentStore>`. See ADR 0005 (write-path trait relocation) and issue #69 (read-path unify + atc-core purification).

## Storage modes

Two runtime modes. Only external Postgres is production-supported — see `docs/architecture/backend-server.md` § "Storage modes — operator guidance" for the canonical write-up.

- **External Postgres** (`ATC_DATABASE_URL` set) — required for `replicaCount > 1` (chart guard refuses to render otherwise). The drain task is the sole broadcaster.
- **In-memory** (`ATC_DATABASE_URL` unset) — **dev-only**, single-replica only. Useful for `just dev` against curl/smee.io-fired webhooks; lossy on restart; do not deploy to production. `InMemoryStore` owns the seq mutex internally and broadcasts via `webhook_tx` inside `apply_*_event`. Eviction task is spawned only in this mode.

## Modules

| Module | Role |
|--------|------|
| `main` | Entry point, config loading, AppState creation (4 fields: persist, webhook_secret, shutdown, ws_tracker), router setup, lifecycle. Constructs the active `Arc<dyn PersistentStore>` via `PgStore::start` (PG mode) or `InMemoryStore::start` (in-memory mode); the per-store background tasks are spawned and owned by the store itself (ADR 0006). |
| `config` | figment-based `Config` with `GitHubConfig`; `database_listener_url` falls back to `database_url` at runtime |
| `db` | `init_pool(url)` — connects sqlx PgPool and runs embedded migrations |
| `routes` | HTTP route handlers: webhook ingestion, REST state snapshot, WebSocket upgrade, health/ready probes |
| `state` | `AppState` struct and broadcast types (`SeqEvent`, `StateSnapshot`). WS subscribe seam moved to `persist.subscribe()` per ADR 0006. |
| `ws` | WebSocket upgrade handler. Subscribes via `state.persist.subscribe()` (ADR 0006); per-connection forwarding loop unchanged. |
| `assets` | rust-embed static file serving with SPA fallback and dev proxy to Vite |
| `otel` | OTel SDK initialization: tracer + meter providers (OTLP/HTTP), `TraceContextPropagator`, base-2 exponential histogram view, `metrics-exporter-otel` recorder install, sampler env-var parsing, provider shutdown helper. Wired at startup; flushed at shutdown. |
| `metrics` | OTel-emitted metric registration: `register_build_info` (one-shot startup gauge) and `PgMetrics::register` (cached `Counter`/`Gauge`/`Histogram` handles for every `atc_pg_*` emit site, owned by `PgStore`); plus the process metrics collector. The `metrics` crate facade emits through the OTel recorder installed by `otel::init_otel`. See `docs/architecture/metrics.md` for the canonical surface, the authoring contract, and the cached-handle convention. |
| `persist` | Module with `mod.rs` (`PersistentStore` trait — `apply_run_event`, `apply_job_event`, `read_snapshot`, `liveness_check`, `subscribe`, `shutdown`; `LivenessError` enum), `pg.rs` (`PgStore` owns pool + clock + broadcast_tx + broadcast_watermark + last_drain_pass_at + listener/drain `JoinHandle`s; `start()` and `start_with_test_hooks()` both accept `Arc<dyn Clock>` so the drain heartbeat and outbox-lag observation route through one wall-clock seam testable via `TestClock`; emits PG write/notify metrics), `in_memory.rs` (`InMemoryStore` owns HashMap + indexes + seq mutex + clock + ttl + broadcast_tx + eviction `JoinHandle`; `start()` / `new_for_test()` constructors; the eviction task is a private `InMemoryStore::spawn_eviction` associated function called from `start()`, not a sibling module), `reads.rs` (`read_all_runs` / `read_all_jobs` REPEATABLE READ helpers). ADR 0005 + ADR 0006 + issue #69. |
| `listener` | PG LISTEN/NOTIFY background tasks: listener task plus drain task with ring-buffer dedup and gap-healing backstop. Spawned exclusively from `PgStore::start_inner`. |
| `shutdown` | Cooperative shutdown orchestration: joins WS handlers, axum serves, then `persist.shutdown()` (which joins listener+drain in PG mode and eviction in in-memory mode), then the process metrics collector, then flushes the OTel providers via `otel::shutdown`. `join_with_timeout` treats `JoinError::is_cancelled()` as a clean exit (logged at `warn`). |
| `migrations/` | SQL migration files embedded via `sqlx::migrate!()` and run on startup when `ATC_DATABASE_URL` is set |

## TypeScript Generation

`SeqEvent` and `StateSnapshot` derive `#[derive(TS)]` with `#[ts(export)]`. Generated types are written to `frontend/src/lib/types/generated/` via `just types`. See `docs/architecture/backend-server.md` § SeqEvent Wire Contract and § REST State Snapshot for the wire shape.

## Contracts

Enforced by implementation and verified by tests. Full detail in `docs/architecture/backend-server.md`.

- **Webhook ingestion (both modes):** HMAC-SHA256 verification (when secret configured), parse via atc-github, then dispatch through `state.persist.apply_*_event(env).await`. Returns `{"status":"accepted","seq":<u64>}` on success, `{"status":"rejected"}` (200) on `PersistError::InvalidTransition`, 503 on `PersistError::Backend`. (ADR 0005)
- **NOTIFY emission:** Inside the webhook handler transaction (after outbox INSERT, before commit), `SELECT pg_notify('atc_outbox', seq::text)` emits the outbox row's seq to all listeners. PG queues NOTIFYs during a txn and delivers on COMMIT; aborted txns silently drop. Metric: `atc_pg_notify_emitted_total{kind}`.
- **Drain pipeline:** Listener registers NOTIFY-arriving seqs into `min_pending_seq` for gap-healing. Drain wakes on signal or 5s heartbeat tick, swaps the backstop, computes `pass_start_floor = watermark.min(backstop.saturating_sub(1))`, fetches outbox rows ordered by seq (paginated, batch size 500), deduplicates via 2048-entry ring buffer, broadcasts to subscribers, advances `broadcast_watermark`. On failed pass, re-registers the captured backstop and does not refresh the heartbeat (so `/readyz` reflects sustained drain failure after 30s). Sole broadcaster in PG mode.
- **Seq ordering:** In-memory mode acquires `Mutex<u64>` BEFORE apply + seq assignment + broadcast (strictly monotonic, resets to 0 on restart). PG mode draws ordering from outbox `BIGSERIAL` plus drain's ascending ORDER BY.
- **Broadcast semantics:** Bounded channel (capacity 256). When a subscriber falls behind and the channel buffer overflows, `RecvError::Lagged` is returned. The WS handler closes the connection on `Lagged` (logs a warning, then breaks the loop) — the client reconnects and fetches `/v1/state` to re-establish its seq cursor. This prevents a permanently gapped event stream on the client side.
- **State snapshot (PG mode):** `PgStore::read_snapshot` loads its owned `broadcast_watermark` Arc via `Ordering::Acquire` BEFORE opening a REPEATABLE READ transaction reading runs (`WHERE placeholder=false`) and jobs. The cursor is monotonic in commit order; the frontend tolerates `entity_count >= last_seq` (snapshot may include commits the drain hasn't yet broadcast). Placeholder stub runs are excluded.
- **State snapshot (in-memory mode):** `InMemoryStore::read_snapshot` holds its internal seq `Mutex<u64>` across the snapshot read and seq read. `lastSeq=0` is the cold-start sentinel.
- **Liveness check (`/readyz`):** Dispatches through `state.persist.liveness_check().await`. `PgStore` returns `Err(LivenessError::DbUnreachable(e))` on `SELECT 1` failure, `Err(LivenessError::DrainStale { age_ms })` when drain heartbeat is > 30 s old, otherwise `Ok(())`. Both the heartbeat refresh (in the drain loop) and the staleness comparison (here in `liveness_check`) read wall-clock through `PgStore.clock: Arc<dyn Clock>`, so tests can drive the staleness path deterministically by advancing a `TestClock` — see `tests/integration/pg_clock_seam_tests.rs` and `readyz.rs`. `InMemoryStore` always returns `Ok(())`.
- **PG access:** No longer on `AppState`. `PgStore` owns its own pool, broadcast sender, `broadcast_watermark`, `last_drain_pass_at`, and the spawned listener+drain `JoinHandle`s (ADR 0006). `atc_pg_write_failures_total{kind=parity|transient}` and `atc_pg_notify_emitted_total{kind}` are emitted from `PgStore::apply_*_event`.
- **WebSocket:** Clean disconnection — no crash, no effect on other clients.
- **Config:** `ATC_GITHUB__WEBHOOK_SECRET` loads `webhook_secret`. If `None`, HMAC verification is skipped.
- **OTel pipeline gating:** When `OTEL_EXPORTER_OTLP_ENDPOINT` is unset, `init_otel` returns `None` and no provider, exporter, recorder, or background task is initialized — `metrics::*` macros resolve through the no-op recorder. When set, `init_otel` builds OTLP/HTTP tracer + meter providers, installs `TraceContextPropagator` globally, registers the shared `exponential_histogram_view` (so all histograms emit as base-2 exponential aggregations), and installs the `metrics-exporter-otel` recorder behind the `metrics` facade. All `register_*` description calls run unconditionally; under the no-op recorder they are cheap.
- **Spans:** boundary instrumentation lives in `routes::webhook_handler` (`webhook.handler`, root, manually constructed so `traceparent` extraction can attach the parent context before entry), `atc_github::webhook` (`webhook.verify`, `webhook.parse`), `persist::pg::PgStore` / `persist::in_memory::InMemoryStore` (`persist.apply.run_event`, `persist.apply.job_event`, `persist.notify.emit`, `eviction.sweep`), and `listener` (`listener.task`, `listener.recv`, `drain.task`, `drain.pass`, `drain.broadcast`). PG-side spawned futures (`spawn_listener_task`, `spawn_drain_task`) construct a task-lifetime root at spawn time and attach via `.instrument(span)` because `tokio::spawn` does NOT propagate the calling task's parent span — see the follow-up critique on long-lived roots. `InMemoryStore::spawn_eviction` deliberately omits a task-lifetime root: each `evict_expired` call's `#[instrument(name = "eviction.sweep")]` becomes its own root span so per-tick traces export on every sweep instead of accumulating under a parent that only ends at process shutdown. Span names + attributes are documented in `docs/architecture/metrics.md` § "Span inventory".
- **OTel SDK shutdown:** `OtelHandles` returned by `init_otel` flow into `run_shutdown_orchestration` (`shutdown.rs`) and are consumed by `otel::shutdown` AFTER every emitter has joined. The persistent store's background tasks (drain + listener in PG mode; eviction in in-memory mode) are joined together via `persist.shutdown()`; the process collector and axum graceful-shutdown drain are joined separately. The "no live emitter when shutdown fires" invariant is documented in a comment block in `shutdown.rs` enumerating the emitter categories; new emitter categories MUST extend that comment so the join chain stays accurate.
- **OTel env-var contract:** `init_otel` reads the spec-standard envs directly — `OTEL_EXPORTER_OTLP_ENDPOINT` (gates init; HTTP/protobuf only), `OTEL_SERVICE_NAME` (resource attribute, defaults to `"atc"`), `OTEL_RESOURCE_ATTRIBUTES` (auto-extracted by the SDK builder), `OTEL_TRACES_SAMPLER` and `OTEL_TRACES_SAMPLER_ARG` (parsed manually because the SDK's autoload is incomplete on the 0.31 line). Accepted samplers: `always_on`, `always_off`, `traceidratio`, `parentbased_always_on` (default), `parentbased_always_off`, `parentbased_traceidratio`. For ratio samplers (`traceidratio` / `parentbased_traceidratio`), a missing or empty `OTEL_TRACES_SAMPLER_ARG` defaults to `1.0` per OTel spec — selecting a ratio sampler with no arg samples everything, NOT silently falls back to the default sampler. An explicitly-present-but-invalid arg (out of range, unparseable) does fall back to the default with an `eprintln!` warning. Unknown sampler names also fall back; never aborts startup. (Pre-init failures use `eprintln!` rather than `tracing::*` because `init_otel` runs BEFORE `init_tracing_subscriber` — the subscriber is composed with the tracer returned from this call, so any tracing macro fired here would dispatch to the no-op global subscriber and silently disappear.) `OTEL_EXPORTER_OTLP_PROTOCOL` is NOT honored — gRPC is out of scope.

## Testing

```bash
cargo nextest run -p atc-server # tests across four tiers (route-level oneshot, full-stack ephemeral, in-memory store, PG-backed)
cargo clippy -p atc-server -- -D warnings
```

**Docker required.** PG-backed integration tests boot a single shared PostgreSQL container (`atc-test-pg`) via testcontainers with `ReuseDirective::Always`; each test creates its own ephemeral database. Run `just cleanup-test-pg` to reclaim the container and accumulated `test_*` databases.

macOS/OrbStack users: export `DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock` before running tests.

Tests that read the in-memory metric or span exporter MUST be marked `#[serial_test::serial]`. The OTel global state — tracer provider, meter provider, propagator — is process-wide just like the prior Prometheus recorder was, and `force_flush()` + `get_finished_*()` is non-atomic across concurrent tests (one test's flush would surface another's emissions). A `OnceLock`-guarded harness in `tests/integration/common/mod.rs` installs an `InMemorySpanExporter` + `InMemoryMetricExporter` exactly once per test binary.

Test tier organization is in `docs/architecture/backend-server.md` § Testing.

## Key References

- Architecture: `docs/architecture/backend-server.md`
- Design plan: `docs/design-plans/2026-04-11-server-wiring.md`
- Domain model: `backend/crates/atc-core/`
- GitHub integration: `backend/crates/atc-github/`
