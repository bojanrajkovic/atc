# CLAUDE.md — atc-server

Last verified: 2026-05-04 (Phase 2c: drop pg_store from AppState; webhook handler uses pg_pool directly)

> Canonical documentation lives in `docs/architecture/backend-server.md`. This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Axum HTTP server wiring `atc-core` (state store) and `atc-github` (webhook parsing) together. Provides HTTP endpoints for webhook ingestion, REST state snapshots, and WebSocket event streaming. The only executable crate in the backend workspace. **Phase 2c:** PostgreSQL writes are driven directly via `AppState.pg_pool` (`&PgPool`) in the webhook handler; the `PgStore` wrapper is retained in `persist.rs` for use by `tests/persist_pg_tests.rs` but is no longer mounted in `AppState`.

## Modules

| Module | Role |
|--------|------|
| `main` | Server entry point, config loading, AppState creation, router setup, eviction task lifecycle; connects pool and stores it in AppState.pg_pool |
| `config` | figment-based Config struct, GitHubConfig with webhook_secret, Config::load() |
| `db` | `init_pool(url)` — connects sqlx PgPool and runs embedded migrations; extracted from main so it is testable as library code |
| `routes` | HTTP route handlers: `POST /v1/webhooks/github`, `GET /v1/state`, `GET /v1/ws`, health/ready probes; webhook handler writes to in-memory store and broadcasts SeqEvent under seq mutex |
| `state` | AppState struct (fields: `store`, `webhook_tx`, `webhook_secret`, `seq`, `pg_pool`), SeqEvent type (sidecar contract documented in `docs/architecture/backend-server.md` § SeqEvent Sidecar Contract) |
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

- **Webhook ingestion:** HMAC-SHA256 verification (when secret configured), parse via atc-github, apply to in-memory store, broadcast SeqEvent, return 200 always.
- **Seq ordering:** `Mutex<u64>` held across in-memory store mutation + seq assignment in the webhook handler, ensuring WS event seq order matches commit order. Strictly monotonic with no gaps. Resets on server restart.
- **Broadcast semantics:** Bounded channel (capacity 256) means slow subscribers may miss events. LaggingError logs warning but does not disconnect. Broadcast happens under the seq mutex so state_handler never advertises a cursor for events that haven't been broadcast yet.
- **State snapshot:** `Mutex<u64>` held across snapshot + seq read in the state handler, ensuring the cursor matches the snapshot content. StateSnapshot.seq is the next seq to assign; all events with seq < N are reflected in snapshot.
- **PG access:** `AppState.pg_pool` is `Option<sqlx::PgPool>`. `Some(pool)` when `ATC_DATABASE_URL` is configured; `None` in in-memory-only mode. The webhook handler uses `&pool` directly for PG writes (Phase 2c); `PgStore` is no longer mounted in AppState.
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
