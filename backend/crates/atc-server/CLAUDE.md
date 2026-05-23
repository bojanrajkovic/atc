# CLAUDE.md — atc-server

Last verified: 2026-05-23

> Canonical documentation lives in `docs/architecture/backend-server.md`. This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

The only executable crate in the backend workspace. Wires the six library crates together under an Axum HTTP server: ingests GitHub webhooks (via `atc-github`), dispatches events to the active `PersistentStore` (from `atc-persist`), serves a REST state snapshot and WebSocket event stream to the frontend, and handles graceful shutdown orchestration. Storage mode — in-memory (`atc-store-mem`) or Postgres (`atc-store-pg`) — is selected at startup based on whether `ATC_DATABASE_URL` is set.

## Sharp edges

**Shutdown ordering must cover every live emitter.** The join chain in `shutdown.rs` has a comment block enumerating all emitter categories. When adding a new background task that emits OTel spans or metrics, extend that comment and add its join before `otel::shutdown` fires — a task still running when the OTel providers are shut down silently drops its final spans.

**OTel global state is process-wide; concurrent tests that inspect in-memory exporters must be serialized.** Tests that call `force_flush()` or `get_finished_*()` on the in-memory span/metric exporters must carry `#[serial_test::serial]`. A shared `OnceLock`-guarded harness installs the exporters exactly once per test binary; running such tests concurrently surfaces one test's emissions inside another's assertions.

**`init_otel` runs before the tracing subscriber is initialized.** Any log or trace macro fired inside `init_otel` dispatches to the no-op global subscriber and silently disappears. Use `eprintln!` for pre-init diagnostics.

## Key References

- Architecture: `docs/architecture/backend-server.md`
- ADR-0005: `docs/architecture-decisions/0005-write-path-trait-relocation.md`
- ADR-0006: `docs/architecture-decisions/0006-stores-own-background-task-lifecycle.md`
- ADR-0008: `docs/architecture-decisions/0008-persistence-crate-split.md`
