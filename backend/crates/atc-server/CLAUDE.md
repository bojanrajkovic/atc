# CLAUDE.md — atc-server

Last verified: 2026-07-04

> Canonical documentation lives in `docs/architecture/backend-server.md`. This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

The only executable crate in the backend workspace. Wires the six library crates together under an Axum HTTP server: ingests GitHub webhooks (via `atc-github`), dispatches events to the active `PersistentStore` (from `atc-persist`), serves a REST state snapshot and WebSocket event stream to the frontend, and handles graceful shutdown orchestration. Storage mode — in-memory (`atc-store-mem`) or Postgres (`atc-store-pg`) — is selected at startup based on whether `ATC_DATABASE_URL` is set. Also owns the `auth.github` OAuth login/callback/logout/whoami endpoints (`auth.rs`) and the GitHub API client they use (`github_client.rs`), mounted only when `auth.mode = "github"`.

## Sharp edges

**Shutdown ordering must cover every live emitter.** The join chain in `shutdown.rs` has a comment block enumerating all emitter categories. When adding a new background task that emits OTel spans or metrics, extend that comment and add its join before `otel::shutdown` fires — a task still running when the OTel providers are shut down silently drops its final spans.

**Don't write `axum::response::Redirect::to` in this crate.** It sends 303, not 302 — see `auth.rs`'s `redirect_302` and the architecture doc's "OAuth login and callback" section for why 302 is required here.

**A new gated route group should follow `auth_enabled`'s construction-time-`.merge()` shape**, not an in-handler mode check — see `routes::api_routes` and the architecture doc.

**`sqlx` is a dev-dependency only (Cargo.toml comment, #169) — don't write `sqlx::Error` as a named type in `src/`.** `atc_store_pg::SessionStore` methods return `Result<_, sqlx::Error>`, but code here consumes that error generically (`impl std::fmt::Display`, e.g. `auth.rs`'s `session_store_failed` and `session_from_cookie`'s `-> Result<_, impl std::fmt::Display>`) rather than spelling out the concrete type — which would force promoting `sqlx` to a real dependency and reopen the boundary #169 closed.

**OTel global state is process-wide; concurrent tests that inspect in-memory exporters must be serialized — and so must every test that emits into the same instrumented path.** Tests that call `force_flush()` or `get_finished_*()` on the in-memory span/metric exporters must carry `#[serial_test::serial]`. That alone is not sufficient: `#[serial]` only excludes other `#[serial]`-marked tests, not plain `#[tokio::test]`s — a non-serial test exercising the same span/metric-emitting handler can still run concurrently and land its emission inside a serial reader's snapshot window. The rule in practice: once ANY handler gains span/metric instrumentation, every test that exercises it (not just the tests that read the exporter) needs `#[serial_test::serial]`, matching `webhook_ingestion_tests.rs`'s blanket serial marking of every test that calls `webhook_handler`. `auth_tests/{login_callback,logout,public_repos,session_lifecycle,whoami}.rs` are serial for the same reason, once `callback_handler` gained the `auth.callback` span + `atc_auth_logins_total`/`atc_auth_callback_duration_seconds` (#469).

**`init_otel` runs before the tracing subscriber is initialized.** Any log or trace macro fired inside `init_otel` dispatches to the no-op global subscriber and silently disappears. Use `eprintln!` for pre-init diagnostics.

**`PublicRepoCache` is deliberately in-process and per-replica, not shared via Postgres.** Two replicas disagreeing on the public-repo set for up to one `repo_auth_ttl` window after a flip is an accepted tradeoff (ADR-0014), not a bug — don't "fix" it by adding a shared cache table without revisiting that decision.

## Key References

- Architecture: `docs/architecture/backend-server.md`
- ADR-0005: `docs/architecture-decisions/0005-persistentstore-trait-relocation.md`
- ADR-0006: `docs/architecture-decisions/0006-stores-own-background-task-lifecycle.md`
- ADR-0008: `docs/architecture-decisions/0008-persistence-crate-split.md`
- ADR-0014: `docs/architecture-decisions/0014-native-github-auth-mode.md` (no GitHub tokens stored — the `auth.github` login/callback flow's core invariant)
