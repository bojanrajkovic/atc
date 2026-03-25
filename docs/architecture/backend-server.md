# Backend Server — Architecture

Last verified: 2026-03-24

## Purpose

The backend server (`atc-server` crate) is an Axum HTTP server that serves as the single entry point for the ATC application. It provides:

- A REST API surface (currently just `/health`, expanded in future phases)
- Frontend asset serving in release mode via rust-embed
- Development proxy to Vite dev server in debug mode via reqwest

The server runs on port 8080 and is the only executable crate in the backend workspace. The other two crates (`atc-core` for domain logic, `atc-github` for GitHub API integration) are placeholder libraries that the server will depend on as features are added.

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

**Decision:** API routes use `/health` and future `/api/*` prefix convention
**Alternatives considered:** No prefix, `/v1/` versioning
**Rationale:** The `/api/` prefix creates a clear boundary between API requests and frontend asset requests. This boundary is used by the dev proxy to decide what to forward to Vite. No versioning yet — will be added when the API stabilizes.

## Boundaries

**Owns:** HTTP routing, request handling, frontend asset serving, dev proxy, server lifecycle (bind, serve, shutdown)
**Does not own:** Domain logic (atc-core), GitHub API integration (atc-github), frontend build process, authentication (future phase)
**Prohibitions:** Do not put business logic in route handlers — extract to atc-core. Do not call GitHub API directly from handlers — use atc-github. Do not serve assets from filesystem in release mode — always use rust-embed.

## Files

- `backend/crates/atc-server/src/main.rs` — Server entry point, tracing setup, router composition
- `backend/crates/atc-server/src/routes.rs` — API route definitions (health endpoint)
- `backend/crates/atc-server/src/assets.rs` — rust-embed struct, embedded file serving, SPA fallback, dev proxy
- `backend/Cargo.toml` — Workspace definition with shared dependency versions
- `backend/crates/atc-server/Cargo.toml` — Server crate dependencies
