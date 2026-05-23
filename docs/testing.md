# Testing

Last verified: 2026-05-23

Backend-test patterns that surprise. Setup, dev-loop commands, runner choice (nextest vs cargo test), E2E coverage scope, and the no-source-grep-assertion rule all live in [`CONTRIBUTING.md`](../CONTRIBUTING.md) § Running Tests; this doc covers the three foot-guns that aren't obvious from running `just test` once.

## Docker is required and won't silently skip

PG-backed integration tests boot a shared PostgreSQL container via testcontainers with `ReuseDirective::Always` — each test creates its own ephemeral database inside the shared container. Without Docker (or OrbStack on macOS), those tests fail loudly. They do NOT silently skip; CI will mirror the same failure.

The shared container (`atc-test-pg`) survives across `just test` invocations to amortize startup cost. Each test's ephemeral `test_*` database is dropped at process exit, but the container itself accumulates state. `just cleanup-test-pg` reclaims the container and any leftover `test_*` databases.

## Tests touching OTel exporters must be `#[serial_test::serial]`

The OTel global state (tracer provider, meter provider, propagator) is process-wide. `force_flush()` followed by `get_finished_spans()` / `get_finished_metrics()` is non-atomic across concurrent tests — one test's flush would surface another test's emissions inside the assertion window.

Every test that reads from the in-memory OTel exporters MUST carry `#[serial_test::serial]`. The annotation serializes execution against every other `#[serial_test::serial]`-marked test in the same test binary. Without it, the assertion failure is non-deterministic and usually misattributed to "flaky network" or "timing."

## The OTel exporter harness uses `OnceLock` to install exactly once

The harness in `tests/integration/common/mod.rs` installs the in-memory span + metric exporters under an `OnceLock` guard so each test binary registers them exactly once. Two reasons this matters:

1. The OTel SDK panics on re-registering a tracer / meter provider in the same process. A test fixture that installs them eagerly per-test would crash the second test in the binary.
2. nextest spawns one process per test binary; the `OnceLock` is process-scoped, so concurrent binaries each install independently without contention.

If you add a new test that needs to inspect OTel emissions, use the existing harness — do NOT reach for `opentelemetry_sdk::trace::TracerProvider::builder()` directly.
