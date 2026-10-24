# Vendored: axum-otel-metrics

- Upstream: https://github.com/ttys3/axum-otel-metrics
- Version: 0.14.1 (crates.io), upstream commit `86c950ae0cffb29ca31fb22eb5b1e14d2371124e`
- License: MIT. Upstream ships no LICENSE file, only `license = "MIT"` in the
  manifest, so `LICENSE` holds the standard MIT text.

## Why

Upstream 0.14.1 depends on `opentelemetry` and `opentelemetry-semantic-conventions`
0.32. ATC uses 0.33. With two `opentelemetry` versions in the build, the
middleware reads the 0.32 global meter provider (a no-op), so HTTP metrics were
never recorded.

## Patch

- `Cargo.toml`: `opentelemetry` and `opentelemetry-semantic-conventions`
  `0.32` -> `0.33`. No source changes were needed.
- Excluded from the workspace (`backend/Cargo.toml`) so `cargo fmt`, clippy and
  tests skip upstream code; its in-crate tests need the dropped dev-dependencies.
- Dropped `[dev-dependencies]` and upstream examples/tests to avoid pulling in
  unused crates.

## Removal

Remove this directory and point `atc-server` back at crates.io when
axum-otel-metrics releases an `opentelemetry` 0.33 build.
