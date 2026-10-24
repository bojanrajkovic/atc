# Vendored: opentelemetry-system-metrics

- Upstream: https://github.com/haixuanTao/opentelemetry-system-metrics
- Version: 0.32.0 (crates.io), upstream commit `41d514fbb41cf3607af594210c71d81242a9990b`
- License: Apache-2.0 (see `LICENSE`)

## Why

Upstream 0.32.0 depends on `opentelemetry` 0.32. The rest of ATC uses 0.33, so
the `Meter` type passed to `init_process_observer` came from a different crate
version and didn't type-check.

## Patch

- `Cargo.toml`: `opentelemetry` `0.32` -> `0.33`. No source changes were needed.
- `src/lib.rs`: removed a redundant `.clone()` on a `u32` (clippy `clone_on_copy`, CI uses `-D warnings`).
- Dropped `[dev-dependencies]`, `[[example]]` entries, `examples/`, and CI files
  to avoid pulling in unused crates.

## Removal

Remove this directory and point `atc-server` back at crates.io when upstream
publishes a release compatible with `opentelemetry` 0.33.
