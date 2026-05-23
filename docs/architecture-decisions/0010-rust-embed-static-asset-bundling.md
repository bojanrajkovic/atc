# ADR 0010 — Embed static assets in the binary via rust-embed

Date: 2026-05-23
Status: Accepted

Last verified: 2026-05-23

## Context

ATC is designed as a single-artifact deployment: one binary, no sidecar processes,
no runtime filesystem layout to coordinate. The architecture research doc from the
ideation phase captures this intent directly — "a single Rust binary that serves
the SPA static files, receives webhooks, handles OAuth, and pushes state over
websocket/SSE." Realizing that goal requires a mechanism to carry the compiled
Svelte/Vite SPA bundle into the binary at compile time.

The frontend build produces a `dist/` directory of HTML, JS, CSS, and static
assets. At runtime, the server must serve these files and handle client-side
routing: requests for paths that don't correspond to actual files should return
`index.html` so the SPA router can take over.

A secondary requirement is frictionless local development: contributors should be
able to run frontend and backend independently, with the backend falling through
to the Vite dev server rather than serving a stale production build.

## Decision

`atc-server` uses the `rust-embed` crate to bundle the frontend `dist/` directory
into the binary at compile time. At runtime, the asset handler:

1. **Exact-path lookup first.** Tries to serve the requested path from the
   embedded asset set. If the file exists, returns it with an inferred
   `Content-Type`.
2. **SPA fallback.** For any request that doesn't match an embedded file, returns
   `index.html` with a 200. API routes that must return 404 are registered
   explicitly in the router and match before the fallback ever runs.

Build-mode branching is handled via Rust's `cfg(debug_assertions)`:

- **Release builds** (what ships): the embedded-asset handler is active.
- **Debug builds** (local dev): the asset handler proxies HTTP requests to the
  Vite dev server at `localhost:5173`, forwarding status codes and
  `Content-Type` headers. If the dev server is unreachable, the handler returns
  a `502 Bad Gateway` rather than silently falling through.

The multi-stage `Dockerfile` compiles the frontend in a Node stage, copies the
resulting `dist/` into the Rust builder stage before `cargo build --release`, and
copies only the final `atc-server` binary into the distroless runtime image. No
`dist/` directory exists in the runtime container — the assets are inside the
binary.

The `rust-embed` derive macro is applied to a marker struct pointing at the
`dist/` path; `rust-embed` handles recursive directory walking, byte-level
embedding, and (in release mode) ETag generation from content hashes.

## Rejected alternatives

### Separate static-asset CDN

Serve the SPA from a CDN; the backend serves only the API. This is the
conventional split for large-scale web services and is technically sound.

The deciding trade-off for ATC: CDN distribution introduces a second deployment
artifact with its own URL, versioning, and invalidation lifecycle. The SPA and
the backend must stay in sync — the SPA depends on the backend's wire types, and
a version mismatch between what the CDN is serving and what the binary expects
is a real failure mode that requires coordination across two deployment targets.
ATC's self-hosted audience values a single binary they can drop behind a reverse
proxy; adding a CDN layer (or an S3 bucket + CloudFront configuration) contradicts
that deployment story. Rejected on deployment surface grounds.

### Runtime filesystem — read `dist/` at startup

The binary reads the frontend bundle from a path on the local filesystem (e.g.
`/usr/share/atc/dist/`) when it starts. Viable and common for traditional
package-based deployments.

The deciding trade-off: the container image must carry both the binary and the
`dist/` directory in separate filesystem locations, the operator must ensure they
stay in sync on upgrades, and the distroless runtime image strategy (no shell,
no package manager, minimal attack surface) becomes harder to maintain when
the image needs a directory tree alongside the executable. Rejected because it
breaks the single-artifact property and complicates the container story.

### `include_str!` / `include_bytes!` per file

Rust's built-in `include_bytes!` macro embeds a single file at a compile-time
path. For a tiny asset set this is viable. `rust-embed` wraps the same
underlying mechanism with directory traversal (so every file under `dist/` is
picked up without enumerating them manually), `Content-Type` inference, and
ETag generation.

The deciding trade-off: hand-managing an `include_bytes!` call for each output
file from the Vite build is fragile — file names include content hashes that
change on every build. `rust-embed` makes the directory the unit of embedding
rather than individual files. Rejected as impractical for a hash-named Vite
output tree.

## Consequences

- **Single deployable artifact.** The runtime image is one binary; `docker pull`
  and restart is the entire upgrade procedure for operators.
- **Frontend must be built before `cargo build --release`.** The `Dockerfile`
  encodes this ordering. Local `just build` also requires a prior frontend build.
  A missing `dist/` directory at compile time produces a build error, not a
  runtime 404.
- **Binary size grows with the frontend bundle.** Acceptable at the scale of a
  single SPA; would be revisited if asset volume grew significantly.
- **Dev workflow is unaffected.** The `cfg(debug_assertions)` proxy branch means
  `cargo run` (debug profile) during development routes to Vite without requiring
  a built `dist/`.
- **Cache headers and ETags are handled by `rust-embed` in release mode.** The
  assets module does not need to implement its own cache-control strategy for
  file assets.

## References

- `backend/crates/atc-server/src/assets.rs` — runtime asset handler implementation
- `backend/crates/atc-server/Cargo.toml` — `rust-embed` dependency
- `Dockerfile` — multi-stage build ordering (frontend → planner → deps → builder → distroless runtime)
- `docs/ideation/architecture-research.md` — original "single binary serves everything" framing
- [ATC project doc in Outline](https://outline.gaur-kardashev.ts.net/doc/atc-actions-traffic-control-l2q9oLkftG) — ideation-phase design notes
