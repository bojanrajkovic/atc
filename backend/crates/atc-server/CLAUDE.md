# CLAUDE.md — atc-server

Last verified: 2026-04-12

> Canonical documentation lives in `docs/architecture/backend-server.md`. This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Axum HTTP server wiring `atc-core` (state store) and `atc-github` (webhook parsing) together. Provides HTTP endpoints for webhook ingestion, REST state snapshots, and WebSocket event streaming. The only executable crate in the backend workspace.

## Modules

| Module | Role |
|--------|------|
| `main` | Server entry point, config loading, AppState creation, router setup, eviction task lifecycle |
| `config` | figment-based Config struct, GitHubConfig with webhook_secret, Config::load() |
| `routes` | HTTP route handlers: `POST /v1/webhooks/github`, `GET /v1/state`, `GET /v1/ws`, health/ready probes |
| `state` | AppState struct, SeqEvent type |
| `ws` | WebSocket upgrade handler, broadcast subscription, SeqEvent serialization and push |
| `assets` | rust-embed static file serving, SPA fallback, dev proxy to Vite |
| `metrics` | Prometheus layer, build_info gauge, process collector |

## Contracts

These rules are enforced by implementation and verified by tests:

- **Webhook ingestion:** HMAC-SHA256 verification (when secret configured), parse via atc-github, apply to store, broadcast SeqEvent, return appropriate status codes (200/401/422)
- **Seq ordering:** AtomicU64 incremented on each successfully processed event. Strictly monotonic with no gaps. Resets on server restart.
- **Broadcast semantics:** Bounded channel (capacity 256) means slow subscribers may miss events. LaggingError logs warning but does not disconnect.
- **State snapshot:** StateSnapshot.seq is the next seq to assign; all events with seq < N are reflected in snapshot
- **WebSocket:** Clients connect and receive SeqEvent stream in real time. Disconnection is clean (no crash, no effect on other clients)
- **Config:** ATC_GITHUB__WEBHOOK_SECRET loads webhook_secret. If None, HMAC verification skipped

## Testing

```bash
cargo test -p atc-server        # 36 tests across three tiers
cargo clippy -p atc-server -- -D warnings
cargo test -p atc-server --test e2e_tests  # 3 full-stack e2e tests
```

Test organization by tier:

- **Route-level oneshot tests** (config_tests, routes_tests, etc.) — Use tower's `oneshot()` to send requests directly through the router without binding a network port. Isolate endpoint behavior.
- **Full-stack ephemeral tests** (e2e_tests, ws_tests) — Start an ephemeral TcpListener on 127.0.0.1:0, spawn a real server, send HTTP/WebSocket clients through real network I/O. Verify end-to-end flows.

**Concurrency requirement:** Tests using `PrometheusMetricLayer::pair()` must be marked with `#[serial_test::serial]` because the PrometheusBuilder global recorder can only be installed once per binary. The `PROMETHEUS_INIT` OnceLock in `tests/common/mod.rs` ensures this is called exactly once and reused across tests.

## Key References

- Architecture: `docs/architecture/backend-server.md` (full design)
- Design plan: `docs/design-plans/2026-04-11-server-wiring.md`
- Domain model: `backend/crates/atc-core/` (StateStore, events)
- GitHub integration: `backend/crates/atc-github/` (parse_webhook, verify_signature)
