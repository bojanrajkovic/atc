# Vendored: sqlx-tracing

- Upstream: https://github.com/jdrouet/sqlx-tracing
- Version: 0.2.1 (crates.io), upstream commit `95aba22fb` (`v0.2.1`)
- License: MIT, declared in upstream `Cargo.toml`. Upstream ships no license
  file, so `LICENSE` is the standard MIT text.

## Why

Upstream 0.2.1 depends on `sqlx` 0.8. ATC uses 0.9, so the `Database` types from
the two versions differ and `TracedPool` fails to satisfy `sqlx::Executor`.

## Patch

Taken from the closed upstream PR jdrouet/sqlx-tracing#21 (commit `62978c0`),
reviewed and trimmed:

- `Cargo.toml`: `sqlx` `0.8` -> `=0.9.0` with the `macros` feature (needed for
  `Executor::describe`), `rust-version = "1.94"`. Dropped dev-dependencies and
  the Docker-based integration tests, plus the `mysql` and `sqlite` features
  (ATC only uses Postgres; this keeps `--all-features` builds and the lockfile
  free of their drivers).
- `src/query.rs` (new): captures SQL, statement, arguments and `persistent`
  before the consuming `Execute::sql` call, so spans can borrow the SQL text.
- `src/{pool,connection,transaction}.rs`: new `Executor` signatures
  (`SqlStr`, owned `Statement`). Connection and transaction impls take a
  separate lifetime so successive queries don't borrow for the whole
  transaction.
- `src/span.rs`: `db.query.text` goes through `AsRef<str>`.

Span names and attributes are unchanged from 0.2.1.

## Removal

Remove this directory and point `atc-store-pg` back at crates.io when upstream
publishes a release compatible with `sqlx` 0.9.
