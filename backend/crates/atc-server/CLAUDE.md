# CLAUDE.md — atc-server

Last verified: 2026-05-10

> Canonical documentation lives in `docs/architecture/backend-server.md`. This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Axum HTTP server wiring `atc-core` (state machine) and `atc-github` (webhook parsing) together. Provides HTTP endpoints for webhook ingestion, REST state snapshots, and WebSocket event streaming. The only executable crate in the backend workspace. The webhook write path dispatches through `AppState.persist: Arc<dyn PersistentStore>` — either `PgStore` (transactional outbox) or `InMemoryStore`. See ADR 0005.

## Storage modes

Two runtime modes. Only external Postgres is production-supported — see `docs/architecture/backend-server.md` § "Storage modes — operator guidance" for the canonical write-up.

- **External Postgres** (`ATC_DATABASE_URL` set) — required for `replicaCount > 1` (chart guard refuses to render otherwise). The drain task is the sole broadcaster.
- **In-memory** (`ATC_DATABASE_URL` unset) — **dev-only**, single-replica only. Useful for `just dev` against curl/smee.io-fired webhooks; lossy on restart; do not deploy to production. The webhook handler broadcasts directly under the seq mutex.

## Modules

| Module | Role |
|--------|------|
| `main` | Entry point, config loading, AppState creation, router setup, lifecycle (eviction task; in PG mode also listener + drain tasks) |
| `config` | figment-based `Config` with `GitHubConfig`; `database_listener_url` falls back to `database_url` at runtime |
| `db` | `init_pool(url)` — connects sqlx PgPool and runs embedded migrations |
| `routes` | HTTP route handlers: webhook ingestion, REST state snapshot, WebSocket upgrade, health/ready probes |
| `state` | `AppState` struct and broadcast types (`SeqEvent`, `StateSnapshot`) |
| `ws` | WebSocket upgrade handler, broadcast subscription, event push |
| `assets` | rust-embed static file serving with SPA fallback and dev proxy to Vite |
| `otel` | OTel SDK initialization: tracer + meter providers (OTLP/HTTP), `TraceContextPropagator`, base-2 exponential histogram view, `metrics-exporter-otel` recorder install, sampler env-var parsing, provider shutdown helper. Wired at startup; flushed at shutdown. |
| `metrics` | OTel-emitted metric registration helpers (`register_build_info`, `register_pg_write_counters`, `register_listener_metrics`); process metrics collector. The `metrics` crate facade emits through the OTel recorder installed by `otel::init_otel`. See `docs/architecture/metrics.md` for the canonical surface and authoring contract. |
| `persist` | `PersistentStore` trait (ADR 0005) with `PgStore` and `InMemoryStore` impls; transaction helpers (`upsert_*`, `insert_outbox_*`, `notify_outbox_seq_in_txn`) and read helpers (`read_all_runs`, `read_all_jobs`) |
| `listener` | PG LISTEN/NOTIFY background tasks: listener task plus drain task with ring-buffer dedup and gap-healing backstop |
| `shutdown` | Cooperative shutdown orchestration: joins emitters (drain, listener, eviction, process collector, axum graceful-shutdown drain), then flushes the OTel providers via `otel::shutdown` |
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
- **State snapshot (PG mode):** Loads `broadcast_watermark` BEFORE opening a REPEATABLE READ transaction reading runs (`WHERE placeholder=false`) and jobs. The cursor is monotonic in commit order; the frontend tolerates `entity_count >= last_seq` (snapshot may include commits the drain hasn't yet broadcast). Placeholder stub runs are excluded.
- **State snapshot (in-memory mode):** Holds `Mutex<u64>` across snapshot and seq read. `lastSeq=0` is the cold-start sentinel.
- **PG access:** `AppState.pg_pool: Option<sqlx::PgPool>`. Used by the state handler for REPEATABLE READ snapshots and by `/readyz`. `PgStore` holds its own pool internally for the write path. `atc_pg_write_failures_total{kind=parity|transient}` and `atc_pg_notify_emitted_total{kind}` are emitted from `PgStore::apply_*_event`.
- **WebSocket:** Clean disconnection — no crash, no effect on other clients.
- **Config:** `ATC_GITHUB__WEBHOOK_SECRET` loads `webhook_secret`. If `None`, HMAC verification is skipped.
- **OTel pipeline gating:** When `OTEL_EXPORTER_OTLP_ENDPOINT` is unset, `init_otel` returns `None` and no provider, exporter, recorder, or background task is initialized — `metrics::*` macros resolve through the no-op recorder. When set, `init_otel` builds OTLP/HTTP tracer + meter providers, installs `TraceContextPropagator` globally, registers the shared `exponential_histogram_view` (so all histograms emit as base-2 exponential aggregations), and installs the `metrics-exporter-otel` recorder behind the `metrics` facade. All `register_*` description calls run unconditionally; under the no-op recorder they are cheap.
- **Spans:** boundary instrumentation lives in `routes::webhook_handler` (`webhook.handler`, root, manually constructed so `traceparent` extraction can attach the parent context before entry), `atc_github::webhook` (`webhook.verify`, `webhook.parse`), `persist::PgStore` / `InMemoryStore` (`persist.apply.run_event`, `persist.apply.job_event`, `persist.notify.emit`), and `listener` (`listener.task`, `listener.recv`, `drain.task`, `drain.pass`, `drain.broadcast`). Spawned futures (`spawn_listener_task`, `spawn_drain_task`) construct a task-lifetime root at spawn time and attach via `.instrument(span)` because `tokio::spawn` does NOT propagate the calling task's parent span. Span names + attributes are documented in `docs/architecture/metrics.md` § "Span inventory".
- **OTel SDK shutdown:** `OtelHandles` returned by `init_otel` flow into `run_shutdown_orchestration` (`shutdown.rs`) and are consumed by `otel::shutdown` AFTER every emitter handle (drain, listener, eviction, process collector, axum graceful-shutdown drain) has joined. The "no live emitter when shutdown fires" invariant is documented in a comment block in `shutdown.rs` enumerating the emitter join steps; new emitter categories MUST extend that comment so the join chain stays accurate.
- **OTel env-var contract:** `init_otel` reads the spec-standard envs directly — `OTEL_EXPORTER_OTLP_ENDPOINT` (gates init; HTTP/protobuf only), `OTEL_SERVICE_NAME` (resource attribute, defaults to `"atc"`), `OTEL_RESOURCE_ATTRIBUTES` (auto-extracted by the SDK builder), `OTEL_TRACES_SAMPLER` and `OTEL_TRACES_SAMPLER_ARG` (parsed manually because the SDK's autoload is incomplete on the 0.31 line). Accepted samplers: `always_on`, `always_off`, `traceidratio`, `parentbased_always_on` (default), `parentbased_always_off`, `parentbased_traceidratio`. Invalid sampler / out-of-range arg → `eprintln!` to stderr and fall back to default; never aborts startup. (Pre-init failures use `eprintln!` rather than `tracing::*` because `init_otel` runs BEFORE `init_tracing_subscriber` — the subscriber is composed with the tracer returned from this call, so any tracing macro fired here would dispatch to the no-op global subscriber and silently disappear.) `OTEL_EXPORTER_OTLP_PROTOCOL` is NOT honored — gRPC is out of scope.

## Testing

```bash
cargo nextest run -p atc-server # ~337 tests across three tiers (route-level oneshot, full-stack ephemeral, PG-backed)
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
