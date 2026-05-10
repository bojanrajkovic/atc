# Phase 2a — PostgreSQL Foundation

**Slug:** `pg-rippling-newt`
**Date:** 2026-05-03
**Phase:** 2a of state-externalization rollout
**ADR refs:** [0002](../../Projects/atc/docs/architecture-decisions/0002-state-externalization-postgres-outbox.md), [0003](../../Projects/atc/docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md), [0004](../../Projects/atc/docs/architecture-decisions/0004-frontend-derived-pool-stats.md)
**Rollout doc:** [`docs/architecture/state-externalization-research/rollout-and-implementation.md`](../../Projects/atc/docs/architecture/state-externalization-research/rollout-and-implementation.md) § Phase 2a

> Canonical home for this design: `docs/design-plans/2026-05-03-pg-rippling-newt.md` (to be written from this plan after approval).

## Context

Phase 1 of the state-externalization rollout is complete — ADRs 0002, 0003, 0004 are accepted. They pinned the architecture (PostgreSQL with current-state tables + transactional outbox + LISTEN/NOTIFY + symmetric replicas; `last_seq` cursor semantics; pool stats moved to frontend). They explicitly deferred several Phase 2 implementation choices: PG client crate, migration tool, schema details, and connection-config shape.

Phase 2a bootstraps the database integration without changing any user-visible behavior. After Phase 2a:

- The binary connects to PostgreSQL when `ATC_DATABASE_URL` is set, and runs migrations on startup.
- The `/readyz` probe reports DB unreachability as `503`.
- An ephemeral PostgreSQL spins up under `just test` (testcontainers) and the migration / readiness flow is verified end-to-end.
- The in-memory `StateStore` is unchanged; nothing reads from or writes to PG yet.

Phase 2b will start writing to PG alongside the in-memory store; Phase 2c adds the outbox; Phase 2d wires LISTEN/NOTIFY. None of those land here.

The rollout doc's acceptance criterion for Phase 2a is reproduced verbatim below:

> `just test` runs an integration test that boots an ephemeral Postgres, runs migrations, and confirms the readiness probe passes.

## Definition of Done

A PR titled `feat(server): add postgres connection pool, schema migration, and readyz probe` (or similarly scoped to the user-facing change — **no phase mechanics in PR titles**) lands on `main` with:

1. `sqlx` chosen as the PG client crate, with feature flags justified.
2. `sqlx-cli` chosen as the migration tool, pinned in `.mise.toml`.
3. `runs` and `jobs` tables defined in a single forward migration; FK + indexes meet Phase 3c read needs.
4. `AppState` carries an `Option<sqlx::PgPool>`; pool is created and migrations run on startup when `ATC_DATABASE_URL` is set.
5. `/readyz` returns 503 when configured-but-unreachable, 200 otherwise (including in-memory mode).
6. Testcontainers integration test boots ephemeral PG, runs migrations, asserts `/readyz` = 200.
7. `just test` runs the testcontainers test alongside existing tests.
8. `docs/architecture/backend-server.md` updated to reflect the new dependency, pool wiring, and probe semantics.
9. In-memory mode (`ATC_DATABASE_URL` unset) continues to pass all existing tests with no behavior change.

## Brainstorm — Open Decisions

### Decision 1: PG client crate

| Option | Pool? | LISTEN support | SQL ergonomics | Compile-time checks | Notes |
|--------|-------|----------------|----------------|---------------------|-------|
| **`sqlx` + `sqlx::PgPool`** | Built-in | `sqlx::postgres::PgListener` (dedicated long-lived client conn) | `query!` / `query_as!` macros, raw SQL | Yes (offline cache at `.sqlx/`, see `SQLX_OFFLINE`) | Async-first; rustls TLS via `tls-rustls-aws-lc-rs` (no OpenSSL system dep) |
| `tokio-postgres` + `deadpool-postgres` | Separate crate | `tokio_postgres::AsyncMessage` on dedicated client | Hand-written prepared statements | None | Lowest-level; full control; more wiring |
| `sea-orm` | Built-in | Through underlying sqlx (sea-orm wraps it) | ORM (Entity/Model + raw escape hatch) | Through sqlx | Adds ORM abstraction we do not need; would still pull sqlx |

**Recommendation: `sqlx`.**

Why:

- ADR 0002 §3 requires a *session-mode-compatible dedicated connection* for `LISTEN`. `sqlx::postgres::PgListener` is the right client API for that requirement: it owns a single long-lived connection, dedicates it to LISTEN, and gives auto-reconnect semantics — separate from the pool. **The session-compatibility requirement applies to the database URL/pooler the listener connects to, and is not solved by `PgListener` itself.** Pointing `PgListener` at a transaction-mode PgBouncer would still drop registrations on connection reassignment. That's an operator-surface concern, scoped to Phase 2d via `ATC_DATABASE_LISTENER_URL` so the listener's URL can target a session-compatible path while the rest of the pool can use transaction-pooled URLs.
- Built-in `PgPool` removes the deadpool-vs-bb8 sub-decision. `sqlx::PgPool` defaults to `max_connections = 10`; that suffices for single-replica Phase 2a and we do not need to tune it now.
- `query!` / `query_as!` give compile-time SQL checking against a real schema. The offline cache lives at `.sqlx/` (sqlx 0.7+; the older `sqlx-data.json` form is gone) and is committed for CI builds where no DB is available — see `SQLX_OFFLINE=true`. The `query!` macros are not needed in Phase 2a (no SQL is written here yet) but the feature flag is enabled so 2b can use them without another Cargo.toml churn.
- `sqlx::migrate!("./migrations")` reads the migration files at **compile time**, embeds them in the binary, and runs them against the live DB at startup. It does not require a live DB at compile time. (`query!`/`query_as!` are the macros that need a DB or the `.sqlx/` offline cache.)
- ORM (sea-orm) buys nothing for ATC's tiny schema (two tables in 2a, three in 2c) and would re-encode predecessor predicates that ADR 0002 §2 wants expressed as raw `UPDATE ... WHERE status IN (...)`.
- `tokio-postgres + deadpool-postgres` is the lower-level alternative and is fully workable, but every feature we'd add (compile-time checks, listener helpers, migrations) we'd build on top. Not worth it for the size of this codebase.

**Feature flags (final):** `["postgres", "runtime-tokio", "tls-rustls-aws-lc-rs", "chrono", "migrate", "macros", "json"]`.

- `postgres` — driver.
- `runtime-tokio` — tokio runtime. The combined `runtime-tokio-rustls` feature is soft-deprecated in favor of split runtime + TLS features in sqlx 0.8.
- `tls-rustls-aws-lc-rs` — rustls TLS with the aws-lc-rs crypto backend (sqlx 0.8 default rustls choice; matches the rustls-without-OpenSSL stance and avoids `tls-native-tls`'s system-dep cost).
- `chrono` — `DateTime<Utc>` ↔ `TIMESTAMPTZ` mapping (ATC stores `created_at`/`updated_at`/`completed_at` as `chrono::DateTime<Utc>`).
- `migrate` — enables the `sqlx::migrate!()` macro for binary-embedded migrations.
- `macros` — enables `query!`/`query_as!` (will be used in Phase 2b; harmless to enable now).
- `json` — enables `serde_json::Value` ↔ `JSONB` mapping. Required for the `jobs.steps` JSONB column from Phase 2b onward; enabling here so the schema decisions match the runtime feature set.

**Build script trigger:** `backend/crates/atc-server/build.rs` (currently vergen-only) gains `println!("cargo:rerun-if-changed=migrations");` so cargo recompiles when migration files change. Without this, edits to `.sql` files don't invalidate the `migrate!()` macro's embedded copy.

### Decision 2: Migration tool

| Option | Where migrations run | Schema-history table | Format | Pairing |
|--------|----------------------|----------------------|--------|---------|
| **`sqlx-cli` + `sqlx::migrate!()`** | Dev: `sqlx migrate run`. Prod: in-binary at startup. | `_sqlx_migrations` | One `.sql` file per version (no up/down pair) | Native to `sqlx` |
| `refinery` | Same dual mode | `refinery_schema_history` | `.sql` or Rust functions | Independent of client; would coexist alongside sqlx |
| `sea-orm-migration` | Same dual mode | `seaql_migrations` | Rust migration files | Couples to sea-orm |

**Recommendation: `sqlx-cli` + `sqlx::migrate!()`.**

Why:

- Pairs natively with `sqlx`. One ecosystem, one schema-history table, one mental model.
- `sqlx::migrate!("./migrations")` embeds the `.sql` files into the binary at compile time, runs them on startup, and records versions in `_sqlx_migrations`. Idempotent re-runs are no-ops. This matches the rollout doc's startup migration model and ADR 0003's single-binary shipping shape.
- Single `.sql` files per version (no down-migration pair). Production downmigrations on PostgreSQL are rarely safely run; ATC's policy will be "fix forward". Down-migrations could be added later if needed; leaving them out now avoids the temptation to write half-correct ones.
- `sqlx-cli` is dev-only — pinned in `.mise.toml` so contributors get the same version. Operators do not install it; the binary handles all production migration runs.

**`.mise.toml` addition:** add a `sqlx-cli` entry. The repo convention (`.mise.toml:1-10`) exact-pins every tool (`rust = "1.94.1"`, `node = "25.9.0"`, etc.), so `sqlx-cli` must be exact-pinned to the latest stable version current at implementation time. The plan does not prescribe a specific version because tool versions move; the implementor pins what's current when the PR is opened.

## Schema

Two tables, one migration: `backend/crates/atc-server/migrations/0001_initial_runs_jobs.sql`.

### `runs` (mirrors `atc_core::WorkflowRun` — see `backend/crates/atc-core/src/run.rs:54-87`)

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | `BIGINT` | PRIMARY KEY | `RunId(i64)` |
| `org` | `TEXT` | NOT NULL | |
| `repo` | `TEXT` | NOT NULL | |
| `workflow_name` | `TEXT` | NULL allowed | `Option<String>` — `None` until an event supplies it; preserved via `.or()` across subsequent events |
| `workflow_path` | `TEXT` | NULL allowed | `Option<String>` — same `.or()` semantics |
| `branch` | `TEXT` | NULL allowed | `Option<String>` — webhook may not include it |
| `head_sha` | `TEXT` | NOT NULL | |
| `commit_message` | `TEXT` | NULL allowed | `Option<String>` |
| `event` | `TEXT` | NOT NULL | GitHub event name |
| `display_title` | `TEXT` | NOT NULL | |
| `status` | `TEXT` | NOT NULL, `CHECK (status IN ('Queued','InProgress','Completed'))` | Matches `RunStatus` PascalCase serde |
| `conclusion` | `TEXT` | NULL allowed, `CHECK (conclusion IS NULL OR conclusion IN ('Success','Failure','Cancelled','TimedOut','ActionRequired','Stale','Neutral','Skipped','StartupFailure'))` | Mirrors `Option<RunConclusion>` enum (`atc-core/src/run.rs:26-45`); same TEXT+CHECK pattern as status |
| `html_url` | `TEXT` | NOT NULL | |
| `created_at` | `TIMESTAMPTZ` | NOT NULL | |
| `run_started_at` | `TIMESTAMPTZ` | NULL allowed | |
| `updated_at` | `TIMESTAMPTZ` | NOT NULL | |

**Phase 2b note:** Webhook events that arrive without `workflow_name`/`workflow_path`/`branch`/`commit_message` must not clobber an already-populated value. The atomic `UPDATE` SQL in 2b will use `COALESCE(EXCLUDED.workflow_name, workflow_name)` (or equivalent) to preserve existing values, mirroring today's in-memory `.or()` semantics. This is a 2b deliverable, but the schema must allow nullability for it to work.

Indexes:
- `CREATE INDEX runs_status_updated_at_idx ON runs (status, updated_at DESC)` — supports Phase 3c "list active runs" snapshot reads and TTL eviction probes.

### `jobs` (mirrors `atc_core::Job`)

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | `BIGINT` | PRIMARY KEY | `JobId(i64)` |
| `name` | `TEXT` | NOT NULL | |
| `run_id` | `BIGINT` | NOT NULL, REFERENCES `runs(id)` ON DELETE CASCADE | CASCADE is a safety net only; normal eviction is job-first (see Schema decisions below) |
| `status` | `TEXT` | NOT NULL, `CHECK (status IN ('Queued','Waiting','InProgress','Completed'))` | Matches `JobStatus` PascalCase serde |
| `conclusion` | `TEXT` | NULL allowed, `CHECK (conclusion IS NULL OR conclusion IN ('Success','Failure','Cancelled','TimedOut','ActionRequired','Stale','Neutral','Skipped'))` | Mirrors `Option<JobConclusion>` enum (`atc-core/src/job.rs:28-45`) |
| `runner_id` | `BIGINT` | NULL allowed | Flattened from `Option<RunnerInfo>` |
| `runner_name` | `TEXT` | NULL allowed | |
| `runner_group_id` | `BIGINT` | NULL allowed | |
| `runner_group_name` | `TEXT` | NULL allowed | |
| `labels` | `TEXT[]` | NOT NULL DEFAULT '{}' | Native PG array; matches `Vec<String>` |
| `steps` | `JSONB` | NOT NULL DEFAULT '[]' | Snapshot-replace semantics; no per-step queries needed |
| `created_at` | `TIMESTAMPTZ` | NOT NULL | |
| `started_at` | `TIMESTAMPTZ` | NULL allowed | |
| `completed_at` | `TIMESTAMPTZ` | NULL allowed | |

Indexes:
- `CREATE INDEX jobs_run_id_idx ON jobs (run_id)` — supports the "load all jobs for a run" snapshot path.
- `CREATE INDEX jobs_status_completed_at_idx ON jobs (status, completed_at)` — supports Phase 3c TTL eviction (`status='Completed' AND completed_at < cutoff`).

### Schema decisions

- **`BIGINT` ids**: `RunId(i64)` and `JobId(i64)` are signed 64-bit; PG `BIGINT` matches exactly. GitHub gives positive ids in practice but the Rust type is signed and mapping signed↔signed avoids `as` casts.
- **`TEXT + CHECK` over `ENUM`**: PG enums are painful to evolve (`ALTER TYPE ... ADD VALUE` cannot be in a transaction in older PG; renames are non-trivial). `TEXT + CHECK` is trivially extended in a future migration by relaxing the CHECK. Names match Rust PascalCase serde so the round-trip is `RunStatus::serialize → "InProgress"` ↔ TEXT.
- **`TEXT[]` for labels**: native PG array works with sqlx's `Vec<String>` ↔ `TEXT[]` mapping out of the box. No JSON-in-text. Sorted-set semantics live in the application (`LabelSet` in `atc-core`).
- **`JSONB` for `steps`**: ATC writes the full step list per job event (snapshot-replace). We never query *into* steps in production paths. JSONB stays flexible if the step shape evolves and is one column instead of a `steps` table with FKs and ordering.
- **Flattened `runner_*` columns** (vs. `JSONB runner` or a `runners` table): four columns in one row, easy to filter, no foreign-key bookkeeping. Cardinality of distinct runners is low; we are not normalizing.
- **`ON DELETE CASCADE`** on `jobs.run_id`: this is a *safety net*, not the primary deletion path. Today's TTL eviction is **job-first** (`backend/crates/atc-core/src/store.rs:487-559`): expire completed jobs by `completed_at + ttl < now`, then evict runs that have no remaining jobs. Phase 3c will mirror that as two SQL DELETEs (jobs first, then orphan runs); the FK is therefore not load-bearing for the normal eviction path. CASCADE only fires if a future code path (admin endpoint, manual cleanup, schema-evolution migration) deletes a run that still has jobs — preventing orphaned `jobs.run_id` references in that case.
- **Index for orphan-run cleanup:** the `jobs(run_id)` index also accelerates the Phase 3c "find runs with no jobs" predicate (`DELETE FROM runs r WHERE NOT EXISTS (SELECT 1 FROM jobs j WHERE j.run_id = r.id)` or equivalent `LEFT JOIN ... WHERE j.id IS NULL`).
- **No outbox / events table here**: explicitly deferred to Phase 2c. This migration is `0001_initial_runs_jobs.sql`; the next migration in 2c will add `outbox`.

## Wiring

### `Cargo.toml` (atc-server)

Add via `cargo add` from inside `backend/crates/atc-server/`. Versions resolve to whatever cargo picks at the time — do **not** pin minor versions in this plan.

```bash
# Runtime dependency
cargo add sqlx --no-default-features \
  --features postgres,runtime-tokio,tls-rustls-aws-lc-rs,chrono,migrate,macros,json

# Dev dependencies (testcontainers)
cargo add --dev testcontainers
cargo add --dev testcontainers-modules --features postgres
```

If `sqlx` rejects the split runtime/TLS feature combo on whatever 0.8.x is current at implementation time (e.g. a feature was renamed), fall back to whatever rustls + tokio combo the current sqlx version exposes — the goal is rustls-with-aws-lc-rs (or rustls-with-ring as a sub-fallback), not a specific feature name.

### `Config` (no change)

`backend/crates/atc-server/src/config.rs` already has `database_url: Option<String>` (loaded from `ATC_DATABASE_URL` via figment). Phase 2a uses it as-is; the listener URL (`ATC_DATABASE_LISTENER_URL`) is **deferred to Phase 2d**.

### `AppState`

`backend/crates/atc-server/src/state.rs` adds:
```rust
pub struct AppState {
    pub store: Arc<StateStore>,
    pub webhook_tx: broadcast::Sender<SeqEvent>,
    pub webhook_secret: Option<String>,
    pub seq: Mutex<u64>,
    pub pg_pool: Option<sqlx::PgPool>,  // <-- new
}
```

`pg_pool` is `Some` iff `ATC_DATABASE_URL` was set at startup. In-memory mode keeps it `None`. Phase 2a does not read or write through the pool — it only exists to be probed by `/readyz`.

### Startup (`main.rs`)

After config load, before binding:

1. If `config.database_url.is_some()`:
   - Create `sqlx::PgPool` (default `max_connections = 10`).
   - Run `sqlx::migrate!("./migrations").run(&pool).await`.
2. Construct `AppState { pg_pool, .. }`.
3. Bind / serve.

**Three startup failure modes**, distinguished:

| Mode | Behavior |
|---|---|
| `ATC_DATABASE_URL` unset | In-memory mode; `pg_pool = None`; no migration step; startup proceeds. |
| `ATC_DATABASE_URL` set, **connect fails** at startup | `tracing::error!` and `process::exit(1)`. (Operator configured DB; we do not silently fall back.) |
| `ATC_DATABASE_URL` set, connects, **migration fails** | `tracing::error!` and `process::exit(1)`. |
| `ATC_DATABASE_URL` set, all good at boot, **DB lost at runtime** | Process stays up; `/readyz` returns 503; K8s liveness probe handles restart if configured. |

This matches the existing pattern in `main.rs` (`process::exit(1)` on `bind` failure) and avoids the foot-gun where a missing-PG operator deployment silently runs in-memory.

### `/readyz`

`backend/crates/atc-server/src/routes.rs`:

```rust
async fn readyz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        match sqlx::query("SELECT 1").execute(pool).await {
            Ok(_) => (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response(),
            Err(e) => {
                tracing::warn!(error = %e, "readyz: db check failed");
                (StatusCode::SERVICE_UNAVAILABLE, Json(HealthResponse { status: "db_unreachable" })).into_response()
            }
        }
    } else {
        (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response()
    }
}
```

`/healthz` (liveness) is unchanged — process up = healthy, regardless of DB.

## Test Harness

### Crate setup

`backend/crates/atc-server/tests/db_readyz_tests.rs` — new test file.

Uses `testcontainers` + `testcontainers-modules::postgres::Postgres` to boot an ephemeral PG container per test (default), connects, lets the binary run migrations, asserts behavior via tower `oneshot()` against the router (no real network bind needed for these tests).

### Tests in this file

| Test | Setup | Assertion |
|---|---|---|
| `readyz_returns_ok_with_healthy_db` | Boot PG container → set `ATC_DATABASE_URL` → start `AppState` → run migrations | `GET /readyz` returns 200 |
| `readyz_returns_503_when_db_unreachable` | Boot PG, build state, **stop container**, then probe | `GET /readyz` returns 503 |
| `migrations_create_runs_and_jobs_tables` | Boot PG, run migrations | `\d runs` / `\d jobs` introspection finds the tables (or a `SELECT count(*)` on each succeeds) |

Existing `routes_tests::readyz_returns_ok` (in-memory, `pg_pool = None`) continues to pass — it asserts the in-memory branch.

### `just test` contract change

**`just test` now requires Docker** (testcontainers spawns a real PG container). This is the documented baseline for `just test` going forward. Document in:
- `CONTRIBUTING.md` — "Prerequisites" / "Running tests"
- `docs/architecture/backend-server.md` — testing section
- `justfile` — comment on the `test` recipe

If Docker is unavailable, the testcontainers tests fail to start (clear error, not silent skip). This is intentional: the project's policy ("don't skip runtime verification") forbids gating tests on environment-detected skips.

Container reuse strategy (e.g. one container per test module via `OnceLock`) is **not** a Phase 2a deliverable. Default behavior — one container per test — is fast enough for three tests; Phase 2b will revisit if test count grows.

## Affected Files

**New:**
- `backend/crates/atc-server/migrations/0001_initial_runs_jobs.sql`
- `backend/crates/atc-server/tests/db_readyz_tests.rs`
- `docs/design-plans/2026-05-03-pg-rippling-newt.md` (canonical copy of this plan)

**Modified:**
- `backend/crates/atc-server/Cargo.toml` — add `sqlx`, `testcontainers`, `testcontainers-modules` via `cargo add`
- `backend/crates/atc-server/build.rs` — add `println!("cargo:rerun-if-changed=migrations");` so cargo invalidates the `sqlx::migrate!()` macro output when migration files change
- `backend/crates/atc-server/src/state.rs` — add `pg_pool: Option<PgPool>` to `AppState`
- `backend/crates/atc-server/src/main.rs` — connect pool, run migrations, exit(1) on failure (connect or migration)
- `backend/crates/atc-server/src/routes.rs` — `/readyz` accepts `State<Arc<AppState>>`, probes pool when present (returns 200/503)
- `backend/crates/atc-server/tests/routes_tests.rs` — update `build_full_app` (and any other `AppState { ... }` literal) to set `pg_pool: None`
- `backend/crates/atc-server/tests/e2e_tests.rs` — same `pg_pool: None` mechanical update wherever `AppState` is constructed
- (any other test in `backend/crates/atc-server/tests/` that constructs `AppState` directly — sweep with `rg 'AppState \{' backend/crates/atc-server/tests/`)
- `backend/crates/atc-server/CLAUDE.md` — note new dependency, list `migrations/` in Modules section
- `docs/architecture/backend-server.md` — pool wiring, `/readyz` semantics, testing prerequisites (doc-mapping gate requires this)
- `.mise.toml` — exact-pin `sqlx-cli` (matches existing tool-pin convention)
- `CONTRIBUTING.md` — Docker prerequisite for `just test`, including OrbStack `DOCKER_HOST` note for macOS users
- `justfile` — **required** comment on the `test` recipe noting the Docker prerequisite (not optional)

**Untouched:**
- `backend/crates/atc-core/**` — domain types unchanged in 2a.
- `backend/crates/atc-github/**` — webhook parsing unchanged.
- `frontend/**` — no contract changes in 2a.
- `deploy/helm/atc/**` — chart already plumbs `ATC_DATABASE_URL`; no chart changes for 2a.

## Acceptance Criteria

Per project convention, paired success/failure cases keyed by the design-plan slug.

### `pg-rippling-newt.AC1` — sqlx + sqlx-cli adopted

- **AC1.1 Success:** `cargo build -p atc-server` compiles with `sqlx` listed in `Cargo.toml` with the documented feature flags. `sqlx-cli` is pinned in `.mise.toml`.
- **AC1.1 Failure:** A build that uses `tokio-postgres` directly, or pulls `sea-orm`, is rejected.

### `pg-rippling-newt.AC2` — schema migration

- **AC2.1 Success:** A fresh PG database, after running `sqlx::migrate!()`, contains `runs` and `jobs` tables with the columns, FK, and indexes specified above. Re-running migrations is a no-op.
- **AC2.1 Failure:** Adding a new table would require operator action outside the binary, or running migrations twice errors.

### `pg-rippling-newt.AC3` — startup wiring

- **AC3.1 Success:** With `ATC_DATABASE_URL` set, the binary connects to PG and runs migrations before binding. With it unset, the binary boots in in-memory mode with no PG attempts.
- **AC3.1 Failure:** With `ATC_DATABASE_URL` set to an unreachable host, the binary silently boots in-memory mode (it must exit(1) instead).
- **AC3.2 Success:** With `ATC_DATABASE_URL` set to a reachable PG instance whose schema cannot run the embedded migrations (e.g., conflicting existing tables, insufficient privileges), the binary exits with code 1 and the migration error is logged via `tracing::error!`.
- **AC3.2 Failure:** Migration errors are logged but the process continues to bind and serve traffic; or `/readyz` returns 200 in a state where migrations did not complete.

**Startup timeout:** Phase 2a does not introduce a startup retry budget — connect failures fail fast. If two pods boot simultaneously and DB is briefly unavailable, K8s' restart-with-backoff handles the retry. A startup retry budget can be added in Phase 5 if operator-data shows it's needed.

### `pg-rippling-newt.AC4` — readiness probe

- **AC4.1 Success:** `GET /readyz` returns 200 in in-memory mode (no DB configured). With DB configured and reachable, it returns 200. With DB configured but unreachable, it returns 503.
- **AC4.1 Failure:** `/readyz` returns 200 when the DB is configured but unreachable; or returns 503 in in-memory mode.

### `pg-rippling-newt.AC5` — testcontainers harness

- **AC5.1 Success:** `just test` boots an ephemeral PG via testcontainers, runs migrations, and asserts `/readyz` returns 200. The DB-loss test asserts `/readyz` returns 503 after the container is stopped.
- **AC5.1 Failure:** Tests rely on a developer-provisioned local PG, or are gated by an environment-detected skip when Docker is unavailable (must fail loudly instead).

### `pg-rippling-newt.AC6` — no behavior regression

Adding `pg_pool: Option<PgPool>` to `AppState` is a struct-literal break for every test helper that constructs `AppState` directly (`tests/routes_tests.rs:25-30` `build_full_app`, `tests/e2e_tests.rs` setup, and any other callsites). These helpers must be updated to include `pg_pool: None`. After those mechanical updates, all existing tests must pass without semantic change.

- **AC6.1 Success:** All existing `cargo test -p atc-server` tests pass after test helpers are updated to construct `AppState` with `pg_pool: None`. The semantic behavior (status codes, response bodies, broadcast/seq behavior, prometheus metrics) is unchanged — `routes_tests::readyz_returns_ok` continues to pass against the new handler taking the in-memory branch.
- **AC6.1 Failure:** A test fails for a reason other than the mechanical struct-literal update; or the new field forces a behavioral change in any existing test path; or any helper is missed and fails to compile.

### `pg-rippling-newt.AC7` — docs

- **AC7.1 Success:** `docs/architecture/backend-server.md` reflects the new pool wiring, `/readyz` semantics, and Docker prerequisite for tests. The `backend/crates/atc-server/CLAUDE.md` lists the migrations directory. The doc-staleness pre-push hook passes.
- **AC7.1 Failure:** Source files in `backend/crates/atc-server/src/*` change without the architecture doc update — the pre-push hook blocks the push.

## Verification

End-to-end manual run (post-implementation):

```bash
# OrbStack on macOS: testcontainers-rs needs DOCKER_HOST pointed at the
# OrbStack socket, since OrbStack does not always populate /var/run/docker.sock
# in a way the bollard client picks up. Export this in any shell that runs
# `just test` or testcontainers-backed commands.
export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock

# 1. Tools
just setup                  # ensures sqlx-cli is on PATH
docker info                 # ensures testcontainers can spin a PG

# 2. Test suite
just test                   # boots ephemeral PG, runs migrations, probes /readyz

# 3. Local dev with Postgres (OrbStack/Docker, either works the same)
docker run -d --rm --name atc-pg -e POSTGRES_PASSWORD=atc -p 5432:5432 postgres:16
ATC_DATABASE_URL=postgres://postgres:atc@localhost:5432/postgres just dev
curl -s localhost:8080/readyz | jq        # {"status":"ok"}
docker stop atc-pg
curl -i localhost:8080/readyz             # 503 db_unreachable
docker start atc-pg                       # (or docker run again)
curl -s localhost:8080/readyz | jq        # {"status":"ok"}

# 4. In-memory mode regression
unset ATC_DATABASE_URL && just dev
curl -s localhost:8080/readyz | jq        # {"status":"ok"}

# 5. Failure mode: configured-but-unreachable at startup
ATC_DATABASE_URL=postgres://nope:nope@localhost:1/nope just dev
# → process exits with code 1, error logged
```

`DOCKER_HOST` should also be documented in `CONTRIBUTING.md`'s testing prerequisites for macOS users on OrbStack.

Doc-mapping gate is exercised by attempting `git push` after `src/*` changes without updating `docs/architecture/backend-server.md` — push is blocked.

## Out of Scope

Each item is explicitly handed to a later sub-phase or a different decision:

- **Writing to PG from the webhook handler** — Phase 2b.
- **Outbox table and transactional outbox writes** — Phase 2c.
- **`LISTEN/NOTIFY` emission and listener task** — Phase 2d.
- **`ATC_DATABASE_LISTENER_URL` config** — Phase 2d.
- **Reading from PG (snapshot, WS forwarder)** — Phase 3c.
- **Cursor rename to `last_seq`** — Phase 3a.
- **Pool stats moved to frontend** — Phase 3b.
- **Helm chart `replicaCount > 1` gating, SQLite mode removal** — Phase 4.
- **Pool-size tuning** — defer until Phase 2b reveals a load profile.
- **Container reuse across tests** (e.g. `OnceLock<PgContainer>`) — defer until test count or runtime warrants it.
- **Splitting an `atc-storage` crate out of `atc-server`** — premature; one binary owns DB wiring through Phase 2.
- **Persisting raw GitHub webhook JSON for audit** — ADR 0002 Out of scope; reconsider in Phase 5.
- **Reconciling the rollout doc with `.gitignore`** — `docs/architecture/state-externalization-research/rollout-and-implementation.md:7-9` instructs sub-phases to write detailed task-by-task implementation plans into `docs/implementation-plans/`, but `.gitignore:53-54` ignores that directory and the canonical artifact policy is `docs/design-plans/` only. The rollout doc is stale on this point. Updating it is **not a 2a deliverable**; flag separately for a doc-cleanup PR.

## Project Deliverables (post-approval)

After plan approval and execution:

1. Create branch `feat/pg-rippling-newt` (from `main`, in a worktree per project convention).
2. Copy this plan to `docs/design-plans/2026-05-03-pg-rippling-newt.md` — the canonical, checked-in home. This file under `~/.claude/plans/` is the working draft. (`docs/implementation-plans/` is gitignored / legacy and is **not** used.)
3. PR per project convention:
   - **Squash merge** — title scoped to the user-facing change, **no phase mechanics**. Suggested: `feat(server): add postgres connection pool, schema migration, and readyz probe`.
   - PR body = squash commit body (what will be / was implemented; no test plan in body).
   - Test plan posted as **first comment** on the PR.
