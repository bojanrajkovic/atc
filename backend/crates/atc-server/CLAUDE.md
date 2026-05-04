# CLAUDE.md — atc-server

Last verified: 2026-05-04 (Phase 2c: outbox migration, txn helpers, pg metrics rename)

> Canonical documentation lives in `docs/architecture/backend-server.md`. This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Axum HTTP server wiring `atc-core` (state store) and `atc-github` (webhook parsing) together. Provides HTTP endpoints for webhook ingestion, REST state snapshots, and WebSocket event streaming. The only executable crate in the backend workspace. **Phase 2b:** Adds PostgreSQL shadow writes via `PgStore` — events are written durably to PG in parallel with in-memory mutations, with drift observability via Prometheus counters.

## Modules

| Module | Role |
|--------|------|
| `main` | Server entry point, config loading, AppState creation, router setup, eviction task lifecycle; instantiates PgStore from pool (Phase 2b) |
| `config` | figment-based Config struct, GitHubConfig with webhook_secret, Config::load() |
| `db` | `init_pool(url)` — connects sqlx PgPool and runs embedded migrations; extracted from main so it is testable as library code |
| `routes` | HTTP route handlers: `POST /v1/webhooks/github`, `GET /v1/state`, `GET /v1/ws`, health/ready probes; webhook handler dual-write flow outside seq mutex (Phase 2b) |
| `state` | AppState struct (now includes `pg_store`), SeqEvent type (sidecar contract documented in `docs/architecture/backend-server.md` § SeqEvent Sidecar Contract) |
| `ws` | WebSocket upgrade handler, broadcast subscription, SeqEvent serialization and push |
| `assets` | rust-embed static file serving, SPA fallback, dev proxy to Vite |
| `metrics` | Prometheus layer, build_info gauge, process collector, PG write counter registration (`register_pg_write_counters`) (Phase 2b/2c) |
| `persist` | `PgStore` implementing `atc-core::PersistentStore` trait; predicated UPSERTs for runs and jobs; `pub(crate)` transaction helpers `upsert_run_in_txn`, `upsert_job_in_txn`, `insert_outbox_run_in_txn`, `insert_outbox_job_in_txn` for atomic outbox writes (Phase 2c) |
| `migrations/` | SQL migration files embedded at compile time via `sqlx::migrate!()`. `0001_initial_runs_jobs.sql` creates `runs` and `jobs` tables. `0002_outbox.sql` creates the `outbox` append-only event log table with BIGSERIAL primary key, `kind` discriminator, and JSONB payload. Run automatically on startup when `ATC_DATABASE_URL` is set. |

## TypeScript Generation

`SeqEvent` and `StateSnapshot` derive `#[derive(TS)]` with `#[ts(export)]` to generate TypeScript interfaces for WebSocket and REST payloads.

**Serialization format:**
- `SeqEvent` is serialized as-is (contains `seq` and `event` fields)
- `StateSnapshot` uses `#[serde(rename_all = "camelCase")]` so the field `pool_stats` serializes as `poolStats` in JSON
- WebSocket events use the adjacently-tagged serde format inherited from `atc-core::WebhookEvent` (discriminated union with `type` and `data` fields)

Generated types are written to `frontend/src/lib/types/generated/` via `just types` recipe.

## Contracts

These rules are enforced by implementation and verified by tests:

- **Webhook ingestion:** HMAC-SHA256 verification (when secret configured), parse via atc-github, apply to in-memory store, broadcast SeqEvent, return 200 always in shadow mode (Phase 2b)
- **Seq ordering:** `Mutex<u64>` held across in-memory store mutation + seq assignment in the webhook handler, ensuring WS event seq order matches commit order. Strictly monotonic with no gaps. Resets on server restart.
- **Broadcast semantics:** Bounded channel (capacity 256) means slow subscribers may miss events. LaggingError logs warning but does not disconnect. Broadcast happens under the seq mutex so state_handler never advertises a cursor for events that haven't been broadcast yet.
- **State snapshot:** `Mutex<u64>` held across snapshot + seq read in the state handler, ensuring the cursor matches the snapshot content. StateSnapshot.seq is the next seq to assign; all events with seq < N are reflected in snapshot.
- **Shadow PG writes (Phase 2b):** After releasing the seq mutex, the handler shadow-writes captured envelopes to PgStore outside the mutex (so PG latency doesn't block concurrent webhooks). Both in-memory and PG must apply the same state transition rule (predecessors_of), so divergence is observable (via parity counter) but not a logical error.
- **PG write failures (Phase 2b/2c):** Classified as parity (0 rows affected → WHERE predicate rejected) or transient (sqlx error). Increments `atc_pg_write_failures_total` counter with appropriate `kind` label. Failures do not fail the webhook (shadow mode).
- **WebSocket:** Clients connect and receive SeqEvent stream in real time. Disconnection is clean (no crash, no effect on other clients)
- **Config:** ATC_GITHUB__WEBHOOK_SECRET loads webhook_secret. If None, HMAC verification skipped

## Testing

```bash
cargo test -p atc-server        # 38+ tests across three tiers (count has grown with db_readyz_tests)
cargo clippy -p atc-server -- -D warnings
cargo test -p atc-server --test e2e_tests  # 3 full-stack e2e tests
```

**Docker required:** The full test suite includes `db_readyz_tests` which uses testcontainers to boot ephemeral PostgreSQL instances. `cargo test -p atc-server` (and `just test`) now require Docker or OrbStack to be running.

macOS/OrbStack users: export `DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock` before running tests.

Test organization by tier:

- **Route-level oneshot tests** (config_tests, routes_tests, etc.) — Use tower's `oneshot()` to send requests directly through the router without binding a network port. Isolate endpoint behavior.
- **Full-stack ephemeral tests** (e2e_tests, ws_tests) — Start an ephemeral TcpListener on 127.0.0.1:0, spawn a real server, send HTTP/WebSocket clients through real network I/O. Verify end-to-end flows.

**Concurrency requirement:** Tests using `PrometheusMetricLayer::pair()` must be marked with `#[serial_test::serial]` because the PrometheusBuilder global recorder can only be installed once per binary. The `PROMETHEUS_INIT` OnceLock in `tests/common/mod.rs` ensures this is called exactly once and reused across tests.

## Key References

- Architecture: `docs/architecture/backend-server.md` (full design)
- Design plan: `docs/design-plans/2026-04-11-server-wiring.md`
- Domain model: `backend/crates/atc-core/` (StateStore, events)
- GitHub integration: `backend/crates/atc-github/` (parse_webhook, verify_signature)
