# Testing

Last verified: 2026-05-23

Conventions and patterns the backend test suite depends on. Setup and dev-loop basics live in [`CONTRIBUTING.md`](../CONTRIBUTING.md) § Running Tests; this doc covers the patterns that aren't obvious from running `just test` once.

## Docker is required for the backend integration suite

PG-backed integration tests boot a single shared PostgreSQL container via testcontainers with `ReuseDirective::Always`; each test creates its own ephemeral database. Without Docker (or OrbStack on macOS), those tests fail loudly — they do NOT silently skip. `just test` runs the suite end-to-end and is the canonical pre-push gate.

The shared container (`atc-test-pg`) survives across `just test` invocations. To reclaim the container and the `test_*` databases it accumulates, run `just cleanup-test-pg`.

macOS / OrbStack: testcontainers-rs reads `DOCKER_HOST`. Export it in any shell that runs `just test`:

```bash
export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock
```

## Tests that touch OTel must be serialized

The OTel global state (tracer provider, meter provider, propagator) is process-wide. `force_flush()` + `get_finished_spans()` / `get_finished_metrics()` are non-atomic across concurrent tests: one test's flush would surface another test's emissions.

Every test that reads from the in-memory OTel exporters MUST carry `#[serial_test::serial]`. The annotation serializes execution against every other `#[serial_test::serial]`-marked test in the same test binary.

The harness that installs the in-memory exporters is in `tests/integration/common/mod.rs` and uses `OnceLock` to install them exactly once per test binary. This pattern matters because:

- The OTel SDK rejects re-installing a tracer / meter provider; calling it twice in the same process panics.
- Without the `OnceLock` guard, parallel test binaries (nextest spawns one process per crate) would each try to install, and the second would fail.

## Test runners

Use `just test` for the full suite (what CI and the pre-push hook run) or `cargo nextest run -p <crate> <filter>` for focused dev loops. Do not use bare `cargo test` — it runs sequentially and is meaningfully slower than nextest on this codebase.

Examples:

- `cargo nextest run -p atc-server shutdown::` — every test in the shutdown module
- `cargo nextest run -p atc-core eviction` — every test with "eviction" in the name
- `cargo nextest run -p atc-server --test graceful_shutdown` — a single integration test file

## E2E (Playwright)

`just test` runs Vitest (unit + browser) + cargo nextest, but does NOT include Playwright E2E. Run `just test-e2e` separately when touching:

- `frontend/src/lib/connection.ts` (snapshot shape, reviver, buffer filter)
- Any store that affects what E2E tests drive via the `window.__stores` bridge
- `frontend/e2e/lib/ws-mock.ts` or snapshot mock helpers
- Any wire-contract field that E2E fixtures embed inline

The pre-push hook does not run E2E either — if you skip `just test-e2e` locally, regressions in these layers ship to CI before being caught.

## What NOT to do

**No source-level grep assertions.** Tests that `readFileSync` a source file and regex-match its content to enforce invariants like "this file only reads `storeX.fieldY`" fire on innocuous refactors, miss semantically-identical variants (e.g. `const { x } = obj` vs `obj.x`), and replicate what code review and lint rules already do. Prefer: behavioral test > lint rule > reviewer guidance > source grep. Reserve source-level checks for ESLint / Biome custom AST rules where they belong.
