# Testing

Last verified: 2026-05-23

Test runners, conventions, environment setup, and the backend-test foot-guns that aren't obvious from running `just test` once. [`CONTRIBUTING.md`](../CONTRIBUTING.md) names the entry-point command + points back here for everything else.

## Running tests

`just test` runs the full suite (what CI and the pre-push hook run): Vitest unit + browser projects, cargo nextest for the backend integration suite, plus the doc-staleness gate. The pre-push hook does NOT include Playwright E2E — see § E2E coverage below.

### Test runners

`just test` is the full suite. For focused dev loops use `cargo nextest run -p <crate> <filter>`. Do not use bare `cargo test` — it runs sequentially and is meaningfully slower than nextest on this codebase.

Examples:

- `cargo nextest run -p atc-server shutdown::` — every test in the shutdown module
- `cargo nextest run -p atc-core eviction` — every test with "eviction" in the name
- `cargo nextest run -p atc-server --test graceful_shutdown` — a single integration test file

### Docker (or OrbStack) is required and won't silently skip

PG-backed integration tests boot a shared PostgreSQL container via testcontainers with `ReuseDirective::Always` — each test creates its own ephemeral database inside the shared container. Without Docker (or OrbStack on macOS), those tests fail loudly. They do NOT silently skip; CI will mirror the same failure.

**macOS / OrbStack:** testcontainers-rs reads `DOCKER_HOST`. Export this in any shell that runs `just test`:

```bash
export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock
```

The shared container (`atc-test-pg`) survives across `just test` invocations to amortize startup cost. Each test's ephemeral `test_*` database is dropped at process exit, but the container itself accumulates state. `just cleanup-test-pg` reclaims the container and any leftover `test_*` databases.

## Foot-guns

### Tests touching OTel exporters must be `#[serial_test::serial]`

The OTel global state (tracer provider, meter provider, propagator) is process-wide. `force_flush()` followed by `get_finished_spans()` / `get_finished_metrics()` is non-atomic across concurrent tests — one test's flush would surface another test's emissions inside the assertion window.

Every test that reads from the in-memory OTel exporters MUST carry `#[serial_test::serial]`. The annotation serializes execution against every other `#[serial_test::serial]`-marked test in the same test binary. Without it, the assertion failure is non-deterministic and usually misattributed to "flaky network" or "timing."

### The OTel exporter harness uses `OnceLock` to install exactly once

The harness in `tests/integration/common/mod.rs` installs the in-memory span + metric exporters under an `OnceLock` guard so each test binary registers them exactly once. Two reasons this matters:

1. The OTel SDK panics on re-registering a tracer / meter provider in the same process. A test fixture that installs them eagerly per-test would crash the second test in the binary.
2. nextest spawns one process per test binary; the `OnceLock` is process-scoped, so concurrent binaries each install independently without contention.

If you add a new test that needs to inspect OTel emissions, use the existing harness — do NOT reach for `opentelemetry_sdk::trace::TracerProvider::builder()` directly.

### No source-level grep assertions

Do not write tests that `readFileSync` a source file and regex-match its content to enforce invariants like "this file only reads `storeX.fieldY`." They fire on innocuous refactors, miss semantically-identical variants (e.g. `const { x } = obj` vs `obj.x`), and replicate what code review and lint rules already do. Prefer: behavioral test > lint rule > reviewer guidance > source grep. Reserve source-level checks for ESLint / Biome custom AST rules where they belong.

## E2E coverage

`just test` runs Vitest (unit + browser) + cargo nextest, but does NOT include Playwright E2E. Run `just test-e2e` separately (`pnpm exec playwright test` under the hood) when touching:

- `frontend/src/lib/connection.ts` (snapshot shape, reviver, buffer filter)
- Any store that affects what E2E tests drive via the `window.__stores` bridge
- `frontend/e2e/lib/ws-mock.ts` or snapshot mock helpers
- Any wire-contract field that E2E fixtures embed inline

The pre-push hook does not run E2E either — if you skip `just test-e2e` locally, regressions in these layers ship to CI before being caught.
