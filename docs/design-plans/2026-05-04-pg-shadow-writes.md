# Phase 2b — Shadow Current-State Writes to PostgreSQL

**Date:** 2026-05-04
**Phase:** 2b of state-externalization rollout
**ADR refs:** [0002](../../Projects/atc/docs/architecture-decisions/0002-state-externalization-postgres-outbox.md) (esp. Decision 2), [0003](../../Projects/atc/docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) (esp. Decision 2 — monotonic-not-gapless)
**Rollout doc:** `docs/architecture/state-externalization-research/rollout-and-implementation.md` § Phase 2b
**Predecessor PR:** [#48](https://github.com/bojanrajkovic/atc/pull/48) (Phase 2a — `pg-rippling-newt`)

> Working draft of this plan lives at `~/.claude/plans/plan-phase-2b-shadow-cozy-bengio.md` (plan-mode constraint).
> On approval, copy to `docs/design-plans/2026-05-04-pg-shadow-writes.md` (preferred canonical filename) on the feature branch — never to `main`. The implementation lands on the same branch (per `.ed3d/design-plan-guidance.md` rule 8).

## Context

Phase 2a (PR #48) gave the server a `sqlx::PgPool` (`AppState.pg_pool: Option<PgPool>`), an embedded migration that creates the `runs` and `jobs` tables (`backend/crates/atc-server/migrations/0001_initial_runs_jobs.sql`), and a testcontainers integration harness (`backend/crates/atc-server/tests/db_readyz_tests.rs`). The pool is connected on startup when `ATC_DATABASE_URL` is set. Nothing reads from or writes to the pool yet — `/readyz` probes it; that's it.

Phase 2b is the **first write phase**. After 2b, every webhook that produces a domain event causes both:
1. an in-memory `StateStore` mutation (today's authoritative path), AND
2. a parallel `runs`/`jobs` row mutation in PostgreSQL (the "shadow" write — observed but not yet read).

The point of shadow mode is to validate that the durable mutation matches the in-memory mutation **before** Phase 2c attaches an outbox to it and Phase 3c cuts read paths over. Drift between the two stores must be observable, not silent. The webhook-handling contract (HMAC, parse, broadcast under seq mutex, return 200 even on `InvalidTransition` — see `routes.rs:171-249`) is preserved; PG is layered alongside it.

ADR 0002 §2 fixes the durable-mutation mechanism: atomic `UPDATE ... WHERE status IN (predecessors)` with predecessor sets parameterized from the existing Rust state machine. ADR 0002 §2 explicitly forbids reimplementing transition rules in PL/pgSQL or check constraints — the Rust state machine remains the single source of truth, SQL just consumes predecessor sets at call time. ADR 0003 §2 relaxes the ordering contract to monotonic-not-gapless, which matters in 2c (BIGSERIAL gaps from aborted transactions); it does not directly affect 2b but the connection model 2b chooses must compose with 2d's session-mode listener requirement.

The webhook handler today (`routes.rs:114-249`) holds `state.seq.lock().await` across in-memory mutation, seq assignment, and broadcast. The PG write must compose with that critical section without altering the broadcast/seq contract.

The in-memory `apply_*_event` functions (store.rs:152-303) already implement the *exact* shape Phase 2b's SQL must replicate:
- Phase 1: validate transition (today via `existing.status.transition_to(target)?`; in SQL via `WHERE status IN (predecessors)`);
- Phase 2: remove-then-insert with `.or()` field merge (today via `Option::or`; in SQL via `COALESCE(EXCLUDED.x, runs.x)`).

Five fields use sticky `.or()` semantics on `runs` (`workflow_name`, `workflow_path`, `run_started_at`, plus `conclusion` once set, plus the `existing.runner` carry-forward on `jobs` — see store.rs:170-203 and 263-287). Skipping `COALESCE` on those silently regresses webhooks that arrive without `workflow_name` populated, which the Phase 2a precedent flagged at line 119.

## Definition of Done

A PR titled with a user-facing scope (e.g., `feat(server): persist workflow run and job state to PostgreSQL`) — **no phase mechanics in the title** — lands on `main` via squash merge with:

1. `RunStatus::predecessors_of(target)` and `JobStatus::predecessors_of(target)` exist in `atc-core`, return `&'static [Self]` (target included for idempotent same-status), and have a property test asserting they are *consistent* with `transition_to` (every transition `transition_to` accepts implies the source ∈ `predecessors_of(target)`).
2. A `PersistentStore` trait lives in `atc-core` with two methods (`apply_run_event`, `apply_job_event`), each returning `Result<(), PersistError>`. The existing in-memory `StateStore` implements it as a thin delegating wrapper.
3. A new `atc-server::persist::PgStore` struct holds a `PgPool` and implements `PersistentStore` against PostgreSQL. Each method performs a single `INSERT ... ON CONFLICT (id) DO UPDATE ... WHERE status = ANY($preds::text[])` keyed by id, mapping `0 rows affected` to `PersistError::InvalidTransition` and any sqlx error to `PersistError::Backend`.
4. Every SQL statement uses `sqlx::query!` / `sqlx::query_as!` (compile-time-checked). The repo commits a `.sqlx/` offline cache; `CONTRIBUTING.md` documents the `cargo sqlx prepare` ritual after schema or query changes.
5. `AppState` gains `pg_store: Option<Arc<dyn PersistentStore + Send + Sync>>` alongside the existing `pg_pool` (which stays for `/readyz`). The webhook handler calls `state.store.apply_*_event` (in-memory, source of truth for reads) inside the seq mutex; on success, drops the mutex; then calls `pg_store.apply_*_event` outside the mutex. PG I/O does not block concurrent webhooks or `GET /v1/state`.
6. Two Prometheus counters distinguish drift: `atc_shadow_pg_write_failures_total{kind="transient"}` (sqlx errors) and `{kind="parity"}` (PG rejected when in-memory accepted — page-worthy in production).
7. An integration test boots ephemeral PG via testcontainers, fires a sequence of run+job webhooks against the full router, and asserts at every step that `SELECT * FROM runs` / `SELECT * FROM jobs` matches the in-memory `state.store.snapshot()` projection column-by-column.
8. Tests cover: invalid transitions rejected via `0 rows affected` (PG row unchanged); idempotent same-status replay; first-sight creation via `INSERT ... ON CONFLICT DO UPDATE`; parity counter increments under poisoned PG state; transient counter increments under DB outage.
9. The five docs in the **Documents to Update** table (below) are updated together: `docs/architecture/backend-server.md`, `backend/crates/atc-server/CLAUDE.md`, `backend/crates/atc-core/CLAUDE.md`, `docs/architecture/state-externalization-research/rollout-and-implementation.md`, and `CONTRIBUTING.md`. The doc-staleness pre-push hook passes.
10. In-memory mode (`ATC_DATABASE_URL` unset, `pg_store = None`) continues to behave as before — no PG attempts, all existing tests green.

## Codex Review Resolutions

This plan was reviewed by Codex (`xhigh`) before approval. The reviewer flagged three blockers and several important concerns. The plan body below is rewritten to absorb every fix; the table here is a quick map from finding to resolution so future readers don't need to dig through git history to understand why the plan looks the way it does.

| Finding | Class | Resolution |
|---|---|---|
| Job-before-run shadow writes break the FK (`jobs.run_id REFERENCES runs(id)`); in-memory `StateStore` admits unknown jobs | Blocker | `PgStore::apply_job_event` precedes the job UPSERT with a stub-run UPSERT: `INSERT INTO runs (id, org, repo, status='Queued', created_at, updated_at) ... ON CONFLICT (id) DO NOTHING`. Stub status `Queued` admits any later real run event via the predecessor predicate. Stub fields populate via `COALESCE` when the real run event arrives. Broadcast semantics unchanged — still a JobEvent, no synthetic RunEvent. |
| Holding the seq mutex across PG I/O blocks the live path | Blocker | PG write moves *outside* the seq mutex. Order: in-memory apply + seq bump + broadcast (under mutex) → drop mutex → PG write inline (still blocks the HTTP response, but no longer blocks concurrent webhooks or `GET /v1/state`). Phase 2c naturally collapses this when the in-memory path retires. |
| Trait API does not actually compose with `&mut Transaction` for Phase 2c | Blocker | Plan no longer claims "no API change in 2c." Phase 2c bypasses the trait — the route handler uses `sqlx::Transaction` directly to compose the current-state UPSERT and the outbox INSERT in one transaction. The trait survives unchanged as the Phase 2b shadow-write API and the Phase 4 symmetric-backend abstraction. |
| `.sqlx/` location and commands target wrong directory | Important | Corrected throughout: `backend/.sqlx/` (Cargo workspace root is `backend/`, not the repo root). All `cargo sqlx prepare --workspace` invocations are run from `backend/`. |
| Job UPSERT was overwriting `name`, `run_id`, `created_at` | Important | Job UPSERT now preserves these via `COALESCE` (or omits them from the SET clause) — matching `..existing` in `apply_job_event` (store.rs:263-287). |
| `AC6` "no PG attempt" not observable with `pg_store: None` | Important | AC6 weakened to behavioral invariance — assert response is 200, broadcast fires, in-memory snapshot reflects the event. No "PG was not called" assertion. The fact that `pg_store: None` short-circuits at the type level is the proof, not a runtime claim. |
| CI claim about `SQLX_OFFLINE=true` doesn't match `.github/workflows/ci.yml` | Important | Claim dropped. Committed `backend/.sqlx/` cache + no `DATABASE_URL` in CI env is sufficient (sqlx 0.8 auto-uses the offline cache when no DB is reachable). CI workflow file unchanged. |
| Doc set inconsistent across DoD #9 / AC10 / Documents table | Important | All three now reference the same five-doc set. |
| Parity truth table had unreachable `(Err(_), Err(InvalidTransition))` branch | Minor | Removed. PG is short-circuited entirely on in-memory error. |
| `serialize_pascal` was hand-wavy | Minor | Replaced with the concrete strategy: a `pub(crate) fn status_str(s: RunStatus) -> &'static str` (and analogous for `JobStatus`) in `atc-server/src/persist.rs`, mapping each variant by name to match the SQL CHECK constraint values exactly. |
| Affected-files sweep was incomplete | Minor | Added: `tests/common/mod.rs`, `tests/metrics.rs`, `tests/state_tests.rs`, `tests/sidecar_tests.rs`. |
| Squash title leaked rollout mechanics ("alongside in-memory store") | Minor | New title: `feat(server): persist workflow run and job state to PostgreSQL`. |
| "Blanket impl" terminology was wrong (it's a concrete impl, not blanket) | Minor | Corrected throughout. |
| **Post-approval corrections (schema cross-check against migration 0001):** | | |
| Job UPSERT includes `updated_at` column that doesn't exist in the jobs table | Bug | Dropped `updated_at` from INSERT column list, VALUES, and SET clause. Predecessor parameter shifts from `$16::text[]` to `$15::text[]`. |
| Stub-run INSERT violates NOT NULL on `head_sha`, `event`, `display_title`, `html_url` | Bug | Added these four columns to stub INSERT with `''` placeholders; they populate via `COALESCE` when the real run event arrives. |
| `COALESCE(EXCLUDED.name, jobs.name)` in job UPSERT would always pick EXCLUDED since `name` is non-optional | Bug | Changed to `name = jobs.name` — identity field, never overwritten, matching `..existing` semantics. |

## Brainstorm — Open Decisions (resolved)

The four decisions named in the prompt, with options, tradeoffs, and the resolved choice (annotated **[chosen]**).

### Decision 1 — Write placement: `PersistentStore` trait on `atc-core` **[chosen]**

The trait approach — the option I originally rejected as premature abstraction — won on a different argument than the one I'd evaluated. The user's framing: the trait is a *slot* for future Phase 4 work where in-memory mode and PG mode become genuinely interchangeable storage backends behind one wire-up. Phase 2b adopts the slot now so it doesn't need to be retrofitted later, and so the persist module's tests can drive a `dyn PersistentStore` rather than a concrete `PgStore` (cheaper to refactor when the trait grows in 2c/3c).

**Trait location:** `backend/crates/atc-core/src/persist.rs` (new module). atc-core stays sqlx-free because the trait method signatures only reference existing atc-core types (`RunEventEnvelope`, `JobEventEnvelope`, the new `PersistError`).

**Trait shape (Phase 2b — write-only):**

```rust
// atc-core/src/persist.rs
#[derive(Debug)]
pub enum PersistError {
    /// PG `0 rows affected` on the predicated UPDATE,
    /// or in-memory `transition_to` rejection.
    InvalidTransition,
    /// Any backend-specific error (sqlx::Error for PgStore;
    /// unused for in-memory StateStore).
    Backend(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl From<StoreError> for PersistError {
    fn from(_e: StoreError) -> Self { PersistError::InvalidTransition }
}

#[async_trait::async_trait]
pub trait PersistentStore: Send + Sync {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<(), PersistError>;
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<(), PersistError>;
}

#[async_trait::async_trait]
impl PersistentStore for StateStore {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<(), PersistError> {
        StateStore::apply_run_event(self, env).await.map_err(Into::into)
    }
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<(), PersistError> {
        StateStore::apply_job_event(self, env).await.map_err(Into::into)
    }
}
```

**Why `PersistError::InvalidTransition` is a unit variant (no `from`/`to` payload):** PG's `WHERE status = ANY($preds)` with `0 rows affected` doesn't tell us what the existing status was. We only learn "the row's status wasn't in the predecessor set." Carrying a `from: ?` would require a pre-read (defeats single-statement atomicity) or a placeholder (lies). The in-memory variant *does* know `from`, but for the trait surface it's normalized to "invalid." The route handler distinguishes parity vs. transient via `(in_mem_result, pg_result)` joint match, not by inspecting either error's internals.

**`PgStore` lives in atc-server:**

```rust
// atc-server/src/persist.rs
pub struct PgStore { pool: PgPool }

impl PgStore {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query!("SELECT 1 AS ok").fetch_one(&self.pool).await.map(|_| ())
    }
}

#[async_trait::async_trait]
impl PersistentStore for PgStore {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<(), PersistError> {
        let target = derive_target(&env.action);
        let preds = RunStatus::predecessors_of(target);
        let preds_strs: Vec<&'static str> = preds.iter().copied().map(status_str).collect();
        let result = sqlx::query!(
            r#"INSERT INTO runs (
                  id, org, repo, workflow_name, workflow_path, branch, head_sha,
                  commit_message, event, display_title, status, conclusion,
                  html_url, created_at, run_started_at, updated_at
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                         $13, $14, $15, $16)
               ON CONFLICT (id) DO UPDATE SET
                  workflow_name  = COALESCE(EXCLUDED.workflow_name, runs.workflow_name),
                  workflow_path  = COALESCE(EXCLUDED.workflow_path, runs.workflow_path),
                  branch         = EXCLUDED.branch,
                  head_sha       = EXCLUDED.head_sha,
                  commit_message = EXCLUDED.commit_message,
                  display_title  = EXCLUDED.display_title,
                  status         = EXCLUDED.status,
                  conclusion     = COALESCE(EXCLUDED.conclusion, runs.conclusion),
                  html_url       = EXCLUDED.html_url,
                  run_started_at = COALESCE(EXCLUDED.run_started_at, runs.run_started_at),
                  updated_at     = EXCLUDED.updated_at
               WHERE runs.status = ANY($17::text[])"#,
            env.run_id.0, env.org, env.repo, env.workflow_name, env.workflow_path,
            env.branch, env.head_sha, env.commit_message, env.trigger_event,
            env.display_title, target_status_str, conclusion_str, env.html_url,
            env.created_at, env.run_started_at, env.updated_at,
            &preds_strs as &[&str],
        ).execute(&self.pool).await
         .map_err(|e| PersistError::Backend(Box::new(e)))?;

        if result.rows_affected() == 0 {
            return Err(PersistError::InvalidTransition);
        }
        Ok(())
    }
    // analogous apply_job_event ...
}
```

**`AppState` shape:** `pg_pool: Option<PgPool>` (existing — drives `/readyz`) stays. `pg_store: Option<Arc<dyn PersistentStore + Send + Sync>>` is added. Both are populated from a single `db::init_pool` call at startup; the `Arc<dyn PersistentStore>` wraps `PgStore::new(pool.clone())`. Sharing two `Arc`-wrapped handles to one underlying pool is fine — the slight redundancy disappears in Phase 4 when `/readyz` can call a `health_check` method on the trait or the trait covers reads.

**Composition with seq mutex:** the PG write happens *outside* `seq_guard.lock()`. The mutex serializes only the in-memory mutation, seq assignment, and broadcast — all cheap, in-process operations. PG I/O has no business in that critical section because shadow-mode reads are still served from in-memory; the durable write only needs to be eventually consistent.

```
seq_guard = state.seq.lock().await;
in_mem_result = state.store.apply_*_event(env.clone()).await;
if in_mem_result.is_ok() {
    seq = *seq_guard; *seq_guard += 1;
    broadcast(...);
}  // else: in-memory rejected; skip broadcast (existing behavior)
drop(seq_guard);

// Outside the mutex — concurrent webhooks and `GET /v1/state` are not blocked by PG I/O:
if in_mem_result.is_ok() && let Some(pg_store) = &state.pg_store {
    let pg_result = pg_store.apply_*_event(env).await;
    classify_metric(in_mem_result, pg_result);  // parity vs transient vs success
}
```

**Why this is correct for shadow mode and forward-compatible:**
- The HTTP response still blocks on the PG write (intentional: preserves backpressure; if PG is dead, the webhook handler latency surfaces as a 200-with-slow-response rather than a silent fire-and-forget failure).
- Concurrent webhook handlers and `GET /v1/state` are unaffected by PG latency. A stalled PG connection no longer cascades into a serialization point on the live path.
- Codex correctly flagged the original "PG inside the mutex" composition as making shadow mode an availability dependency of the live path. Moving it outside resolves that: a shadow-write failure raises a metric and logs; it does not slow down concurrent traffic or `/v1/state` reads.
- Phase 2c naturally collapses this structure: the in-memory path retires, the seq mutex retires (replaced by the BIGSERIAL outbox cursor), and the PG write becomes the single authoritative path inside a `sqlx::Transaction` (see Forward-Compat Summary).

### Decision 2 — Shadow-mode error policy: log + counter + return 200 **[chosen]**

The user accepted this with the right caveat: Phase 2c flips it. Restating the contract for clarity, with the cutover plan made prominent.

**Phase 2b policy (this PR):** PG-write outcome does not affect HTTP status code. Three classes of PG outcome:

| `(in_mem_result, pg_result)` | Metric | HTTP | Broadcast | Notes |
|---|---|---|---|---|
| `(Ok, Ok)` | (success — implicit) | 200 | yes | Happy path |
| `(Ok, Err(InvalidTransition))` | `kind="parity"` | 200 | yes | **Page-worthy in production** — predecessors disagree |
| `(Ok, Err(Backend(_)))` | `kind="transient"` | 200 | yes | sqlx error; alert on rate |
| `(Err(_), —)` | (no metric) | 200 | no | In-memory rejected; **PG call is skipped entirely** (saves a round-trip; preserves today's contract). PG result column is `—` because the call doesn't happen. |

**Phase 2c reversal (forward-compat note):** When the outbox lands and the current-state UPSERT + outbox INSERT must commit atomically, the trait method's return becomes the load-bearing signal:
- transaction rollback on any error
- route handler returns 5xx on PG failure
- GitHub retries with exponential backoff
- `kind="parity"` and `kind="transient"` metric labels survive — same instrument, new failure semantics

The trait API doesn't change between 2b and 2c — only the route handler's error handling does. That's the value of putting the policy in `routes.rs` rather than inside the trait method: the trait is policy-agnostic.

### Decision 3 — Compile-time SQL via `sqlx::query!` + committed `.sqlx/` cache **[chosen]**

The user pushed back on my original "go dynamic" recommendation, and the pushback is correct.

**Argument that flipped it:** the `macros` feature flag is already on (Phase 2a Cargo.toml), CI runs against testcontainers (so a stale cache surfaces in the same `just test` run that catches a SQL typo), and the `cargo sqlx prepare` ritual is comparable to existing rituals in this repo (e.g., `just types` after a `#[ts(export)]` change). The value of compile-time SQL checking compounds across Phase 2c (outbox INSERT), 3c (snapshot SELECT), and any future query — committing dynamic now would require backfilling cache for every query when we decide to flip later. Going static now sets the pattern for the rest of the rollout.

**Concrete implementation:**

1. **All SQL in `persist.rs` uses `sqlx::query!` / `sqlx::query_as!`.** Both UPSERTs and the future outbox INSERT (Phase 2c) get compile-time type checking.
2. **Commit `backend/.sqlx/` directory** with the offline cache generated by `cargo sqlx prepare --workspace -- --tests`. The Cargo workspace root is `backend/` (see `backend/Cargo.toml`), so `--workspace` writes the cache there — *not* at the repo root. The cache contains per-query JSON metadata keyed by query hash and the `--tests` flag includes `#[cfg(test)]` queries.
3. **CI doesn't need `SQLX_OFFLINE=true` or any workflow change.** sqlx 0.8 transparently uses the committed `backend/.sqlx/` cache when no `DATABASE_URL` is set in the build env — and CI doesn't set one. The repo's existing `.github/workflows/ci.yml` is left unmodified.
4. **Developer workflow** (documented in `CONTRIBUTING.md`): after editing any SQL or migration:
   - Boot a local PG (e.g., the testcontainers one or a docker run) and apply migrations.
   - From `backend/`, run `DATABASE_URL=postgres://... cargo sqlx prepare --workspace -- --tests`.
   - Commit the `backend/.sqlx/` changes alongside the SQL change in the same commit.
   - The integration test suite catches stale-cache cases at runtime — no separate pre-push hook needed.
5. **`build.rs` already has `cargo:rerun-if-changed=migrations`.** No additional rerun-if-changed for `backend/.sqlx/` needed — the macros handle their own dependency tracking.

**`text[]` binding for predecessors:** the macro accepts `&[&str]` for a `text[]` parameter when the SQL contains an explicit cast (`$N::text[]`). Concretely:
- `RunStatus::predecessors_of(target)` returns `&'static [RunStatus]`
- A `pub(crate) fn status_str(s: RunStatus) -> &'static str` helper in `atc-server/src/persist.rs` (and analogous `JobStatus` helper) maps each variant by name to the SQL CHECK constraint values (`"Queued"`, `"InProgress"`, `"Completed"`, `"Waiting"`)
- The handler builds `let preds_strs: Vec<&'static str> = preds.iter().copied().map(status_str).collect();` and binds `&preds_strs as &[&str]`

This avoids depending on serde's serialization shape (which could be changed independently for JSON wire compat without anyone realizing it broke the SQL bind). The CHECK constraint strings are an explicit DB contract; mapping by name keeps it close to the call site.

**Macros vs builder where it doesn't fit:** if a future query needs truly dynamic SQL composition (e.g., variable column lists), `sqlx::query()` (builder) coexists with `sqlx::query!` (macros) in the same module — feature flag already covers both. Phase 2b has zero such cases.

### Decision 4 — Predecessor-set derivation: `predecessors_of` in `atc-core` **[chosen]**

Confirmed. Implementation:

```rust
// backend/crates/atc-core/src/run.rs
impl RunStatus {
    /// Returns the set of statuses that can validly transition to `target`,
    /// inclusive of `target` itself (so same-status replay is admitted).
    /// MUST stay consistent with `transition_to` — verified by property test.
    #[must_use]
    pub fn predecessors_of(target: Self) -> &'static [Self] {
        match target {
            Self::Queued => &[Self::Queued],
            Self::InProgress => &[Self::Queued, Self::InProgress],
            Self::Completed => &[Self::Queued, Self::InProgress, Self::Completed],
        }
    }
}

// backend/crates/atc-core/src/job.rs
impl JobStatus {
    #[must_use]
    pub fn predecessors_of(target: Self) -> &'static [Self] {
        match target {
            Self::Queued => &[Self::Queued],
            Self::Waiting => &[Self::Queued, Self::Waiting],
            Self::InProgress => &[Self::Queued, Self::Waiting, Self::InProgress],
            // NB: Queued -> Completed is invalid for jobs (asymmetry vs. runs)
            Self::Completed => &[Self::InProgress, Self::Completed],
        }
    }
}
```

**Property test** (in atc-core, alongside the existing transition tests):

```rust
proptest! {
    #[test]
    fn run_predecessors_consistent_with_transition_to(
        from in any::<RunStatus>(), to in any::<RunStatus>()
    ) {
        let valid = from.transition_to(to).is_ok();
        let listed = RunStatus::predecessors_of(to).contains(&from);
        prop_assert_eq!(valid, listed);
    }
    // analogous test for JobStatus
}
```

Standard `Arbitrary` impls for the two enums go in a `#[cfg(any(test, feature = "test-support"))]` module; atc-core already exposes `test-support` for `TestClock`, so the addition is one line.

For SQL bind, atc-server converts `&'static [RunStatus]` to `Vec<&'static str>` via the `status_str` helpers introduced in Decision 3 (matching the SQL CHECK constraint exactly: `'Queued'`, `'InProgress'`, `'Completed'`, etc.). `status_str` maps by enum variant name, not by serde, so a hypothetical future `#[serde(rename = ...)]` on the enum cannot silently break the SQL bind.

**Composition with `sqlx::Transaction` in Phase 2c:** `predecessors_of` itself is pure (returns `&'static [RunStatus]`) and composes fine with both `&PgPool` and `&mut Transaction<'_, Postgres>`. The trait method that wraps it, however, takes `&self` and runs against the pool — it does *not* expose an executor parameter, so the trait API is not transactional. Phase 2c therefore bypasses the trait for the transactional path: the route handler opens a `sqlx::Transaction` directly and executes the UPSERT SQL alongside the outbox INSERT in one transaction. The trait survives unchanged as the Phase 2b shadow-write API and the Phase 4 symmetric-backend abstraction. This is honestly recorded in the Forward-Compat Summary.

## Architecture

### Module map

| Crate | File | 2b change |
|---|---|---|
| atc-core | `src/persist.rs` | **NEW.** `PersistError` enum, `PersistentStore` async trait, concrete `impl PersistentStore for StateStore` (delegates to existing `StateStore::apply_*_event`). atc-core stays sqlx-free. |
| atc-core | `src/run.rs` | Add `predecessors_of` impl on `RunStatus` (+ unit + proptest). |
| atc-core | `src/job.rs` | Add `predecessors_of` impl on `JobStatus` (+ unit + proptest). |
| atc-core | `src/lib.rs` | Re-export `PersistentStore`, `PersistError`. |
| atc-core | `Cargo.toml` | Add `async-trait` to dependencies; verify `proptest` is in dev-dependencies. Add a `test-support`-gated `Arbitrary` impl for `RunStatus`/`JobStatus` (or hand-rolled in the test module). |
| atc-server | `src/persist.rs` | **NEW.** `PgStore` struct, `PgStore::new(pool)`, `PgStore::ping()`, `impl PersistentStore for PgStore` with `query!`-macro UPSERTs for runs and jobs. Includes `status_str` helpers for `text[]` predecessor bind. Job UPSERT pre-inserts a stub run row to satisfy the FK on job-before-run delivery. |
| atc-server | `src/state.rs` | Add `pg_store: Option<Arc<dyn PersistentStore + Send + Sync>>` to `AppState`. Keep `pg_pool` (drives `/readyz`). |
| atc-server | `src/routes.rs` | `webhook_handler`: in-memory apply + seq bump + broadcast under the seq mutex; drop the mutex; PG write outside the mutex (only if `pg_store.is_some()` and in-memory apply succeeded); classify joint `(in_mem, pg)` result into metrics. |
| atc-server | `src/main.rs` | When `db::init_pool` succeeds, also build `Arc::new(PgStore::new(pool.clone())) as Arc<dyn PersistentStore + Send + Sync>` and pass it into `AppState`. |
| atc-server | `src/metrics.rs` | Register `atc_shadow_pg_write_failures_total{kind}` counter (use the existing `metrics::counter!` axum-prometheus path). |
| atc-server | `Cargo.toml` | Add `async-trait` to dependencies. sqlx + testcontainers already present. |
| `backend/` | `.sqlx/` | **NEW directory at the Cargo workspace root** (`backend/.sqlx/`, NOT repo-root). Generated by `cargo sqlx prepare --workspace -- --tests` run from `backend/`. Committed. |

### SQL writes

**Run UPSERT (one statement):** keyed by `id` with `ON CONFLICT (id) DO UPDATE ... WHERE runs.status = ANY($N::text[])`. Sketch shown in full in Decision 1. Sticky `COALESCE` merges on `workflow_name`, `workflow_path`, `run_started_at`, `conclusion`. Full overwrite on `branch`, `head_sha`, `commit_message`, `display_title`, `event`, `html_url`, `created_at`, `updated_at`, `status` (status overwrite is gated by the predecessor predicate; the others are payload fields the latest event is authoritative for).

**Job write (two statements):** the job-before-run case requires a stub-run insert before the job UPSERT, because `jobs.run_id REFERENCES runs(id)` rejects a job whose run doesn't exist yet. The in-memory store admits this case (first-sight creation in store.rs:216); the schema must too.

```sql
-- Statement 1: ensure a runs row exists for this run_id, but never clobber a real one.
-- head_sha, event, display_title, html_url are NOT NULL in the schema; stub uses ''
-- placeholders that COALESCE on the real run event arriving later.
INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, status, created_at, updated_at)
VALUES ($1, $2, $3, '', '', '', '', 'Queued', $4, $4)
ON CONFLICT (id) DO NOTHING;

-- Statement 2: the actual job UPSERT.
-- Note: jobs table has NO updated_at column (not in migration 0001_initial_runs_jobs.sql).
-- name is a non-optional String in JobEventEnvelope so COALESCE would always pick EXCLUDED;
-- use jobs.name (identity field, never overwritten) to match ..existing semantics in store.rs.
INSERT INTO jobs (id, run_id, name, status, conclusion, labels, steps,
                  runner_id, runner_name, runner_group_id, runner_group_name,
                  started_at, completed_at, created_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
ON CONFLICT (id) DO UPDATE SET
   name              = jobs.name,                           -- identity field, never overwritten
   run_id            = jobs.run_id,                         -- never re-parented
   status            = EXCLUDED.status,
   conclusion        = COALESCE(EXCLUDED.conclusion, jobs.conclusion),
   labels            = EXCLUDED.labels,                     -- snapshot replace
   steps             = EXCLUDED.steps,                      -- snapshot replace
   runner_id         = COALESCE(EXCLUDED.runner_id,         jobs.runner_id),
   runner_name       = COALESCE(EXCLUDED.runner_name,       jobs.runner_name),
   runner_group_id   = COALESCE(EXCLUDED.runner_group_id,   jobs.runner_group_id),
   runner_group_name = COALESCE(EXCLUDED.runner_group_name, jobs.runner_group_name),
   started_at        = COALESCE(EXCLUDED.started_at,        jobs.started_at),
   completed_at      = COALESCE(EXCLUDED.completed_at,      jobs.completed_at),
   created_at        = jobs.created_at                      -- never overwritten
WHERE jobs.status = ANY($15::text[]);
```

Field-merge rationale (matches `apply_job_event` in store.rs:263-287):
- `..existing` semantics in the in-memory store preserve `id`, `run_id`, `name`, `created_at` across updates — translated to SQL via `jobs.name` (no overwrite) for `name` (`JobEventEnvelope.name` is a non-optional `String`; COALESCE would always pick EXCLUDED and clobber the original) and direct `jobs.x` for `run_id` and `created_at` (immutable identity fields)
- `runner_*`, `started_at`, `completed_at`, `conclusion` use `COALESCE` (sticky once observed)
- `labels` and `steps` are fully replaced (snapshot semantics, store.rs:268-269)
- `status` is overwritten under the predecessor predicate

**Stub-row safety properties:**
- Idempotent: `ON CONFLICT DO NOTHING` makes concurrent stub inserts harmless.
- Stub status `'Queued'` is the lowest in the run state machine, so `predecessors_of(target).contains(Queued)` is true for every target — any later real run UPSERT predicate accepts the stub.
- Stub fields populate via `COALESCE` on the real run's UPSERT — no information is lost.
- Stub does NOT broadcast: only the original JobEvent is broadcast, matching in-memory semantics. The stub is a PG-internal plumbing detail.
- An "orphan stub" (job-before-run where the real run never arrives) is the same parity outcome as the in-memory store's orphan job — both stores have a job whose run is unknown. The stub is a placeholder, not a synthetic event.

**Why two statements not one transaction:** Phase 2b doesn't open a `sqlx::Transaction` — it issues two separate statements. The stub insert is idempotent (`ON CONFLICT DO NOTHING`), so a partial failure between the two statements is acceptable: the orphan stub is harmless, and a retry just re-tries the job UPSERT against a now-existing stub. Phase 2c will wrap both inside a single transaction (along with the outbox INSERT) for proper atomicity.

### Concurrency & broadcast contract preserved

| Property | Today | After 2b |
|---|---|---|
| Seq mutex held across in-memory mutation + seq bump + broadcast | Yes (routes.rs:184-227) | Yes — unchanged. Mutex critical section contains only in-process operations. |
| PG write inside the seq mutex critical section | n/a | **No** — PG write is outside the mutex, after the mutex is released. Concurrent webhooks and `GET /v1/state` are not blocked by PG I/O. |
| WS broadcast order matches commit order | Yes | Yes |
| `GET /v1/state` cursor matches snapshot content | Yes | Yes — snapshot still served from in-memory store |
| `InvalidTransition` returns 200 (no GitHub retry) | Yes | Yes — PG transient/parity also return 200 in shadow mode (flips in 2c) |
| Broadcast on in-memory rejection | No | No |
| HTTP response blocks on PG I/O | n/a | Yes (per-request, not global). The handler awaits the PG write before returning 200. Provides backpressure; surfaces PG outage as slow responses on the affected webhook (with the metric) rather than as silent fire-and-forget loss. |
| Polymorphic write dispatch | n/a | Behind `Arc<dyn PersistentStore>` — slot ready for Phase 4 |

## Schema

**No new migrations.** Phase 2a's `0001_initial_runs_jobs.sql` is sufficient. Phase 2c will add `0002_outbox.sql`.

## Test Plan

### Unit tests (atc-core)

In `backend/crates/atc-core/src/run.rs` and `job.rs` test modules:
- Direct cases for `predecessors_of` (one assertion per (target, expected-list) pair).
- Proptest: `from.transition_to(to).is_ok() ⟺ predecessors_of(to).contains(&from)` for both enums.

In `backend/crates/atc-core/src/persist.rs` test module (new):
- `PersistentStore` impl on `StateStore` round-trips: any `Result<(), StoreError>` from the wrapped store maps to the equivalent `Result<(), PersistError>` shape (Ok→Ok, Err(InvalidTransition)→Err(InvalidTransition)).

### Integration tests (atc-server)

**New: `backend/crates/atc-server/tests/persist_pg_tests.rs`** — exercises `PgStore::apply_*_event` directly against a testcontainers pool. Reuses the `Postgres::default().with_tag("17-alpine").start()` pattern from `db_readyz_tests.rs`.

| AC | Test |
|---|---|
| AC2 | `pg_run_first_sight_creates_row` — unknown id, `RunEvent::Requested` envelope → 1 row in `runs` matching envelope columns |
| AC2 | `pg_run_valid_transition_updates_row` — Queued→InProgress on existing row → status updated, sticky fields preserved |
| AC2 | `pg_run_invalid_transition_returns_invalid_transition` — Completed→InProgress envelope → `Err(InvalidTransition)`, PG row unchanged (verified via post-error SELECT) |
| AC2 | `pg_run_idempotent_same_status_replay` — Queued→Queued envelope twice → `Ok(())` both times, sticky fields merged |
| AC3 | `pg_job_first_sight_creates_row_with_existing_run` — run row pre-exists; job UPSERT creates job, no orphan stub appears |
| AC3 | `pg_job_valid_transition_updates_row`, `pg_job_invalid_transition_returns_invalid_transition`, `pg_job_idempotent_same_status_replay` |
| AC3 | `pg_job_queued_to_completed_rejected` — explicit assertion of the runs-vs-jobs asymmetry (`predecessors_of(JobStatus::Completed)` excludes `Queued`) |
| AC3 | `pg_job_before_run_creates_stub_run` — fire job UPSERT against unknown run_id; assert stub row in `runs` with `status='Queued'`, minimal columns from JobEventEnvelope; assert job row has correct FK |
| AC3 | `pg_real_run_event_reconciles_stub` — after `pg_job_before_run_creates_stub_run`, fire the matching real run event; assert stub status updates per predecessor predicate, sticky fields populate via `COALESCE`, no row identity change |
| AC3 | `pg_two_jobs_same_unknown_run_share_stub` — fire two job UPSERTs for the same unknown run_id concurrently or sequentially; assert exactly one stub run row (idempotency of `ON CONFLICT DO NOTHING`) |
| AC4 | `pg_run_coalesce_preserves_workflow_name` — first event populates `workflow_name`, second omits it; row still has it |
| AC4 | `pg_job_coalesce_preserves_runner` — analogous for `runner_*` flatten |
| AC4 | `pg_job_coalesce_preserves_name_run_id_created_at` — first event populates `name`, `run_id`, `created_at`; second job event with same `id` but different/null name does not clobber any of them |

**New: `backend/crates/atc-server/tests/shadow_writes_tests.rs`** — full webhook-handler dual-write integration. Mounts the full router with `pg_store: Some(Arc::new(PgStore::new(pool.clone())))` and asserts in-memory + PG agreement after each webhook.

| AC | Test |
|---|---|
| AC5 | `dual_write_run_lifecycle` — fire `workflow_run` queued→in_progress→completed; after each, assert `state.store.snapshot()` matches `SELECT * FROM runs` column-by-column |
| AC5 | `dual_write_job_lifecycle` — analogous through Queued/Waiting/InProgress/Completed (run pre-exists; no stub row involved) |
| AC5 | `dual_write_job_before_run_lifecycle` — fire `workflow_job` first, then matching `workflow_run`; assert stub run is created, then reconciled by the real event; in-memory and PG agree at every step |
| AC5 | `dual_write_invalid_transition_skips_pg` — fire a webhook the in-memory store rejects; assert PG row unchanged (PG was never called); response is 200 |
| AC5 | `dual_write_idempotent_replay` — same webhook fired twice; both stores stable; broadcast sent twice (today's behavior) |
| AC5 | `dual_write_pg_outside_seq_mutex` — concurrency probe: fire two webhooks for *different* runs in parallel against a deliberately slow PG (e.g., `pg_sleep(1)` injected via test fixture). Assert both broadcasts fire promptly without waiting on each other's PG round-trip — observable as overlapping handler latencies under 1.2s rather than serialized 2s+ latencies. Confirms the mutex no longer holds across PG I/O. |
| AC6 | `in_memory_mode_behavioral_invariance` — `AppState { pg_store: None, pg_pool: None, .. }`; fire a sequence of webhooks; assert: response 200, broadcast received via subscribed WS channel, in-memory snapshot matches expected projection. The absence of PG calls is proven by `pg_store: None` short-circuiting at the type level — no need for runtime "no PG attempt" assertion. |
| AC7 | `parity_metric_increments_when_pg_rejects` — manually `UPDATE runs SET status='Completed' WHERE id=...`, then send a webhook the in-memory store accepts (Queued→InProgress); PG rejects via `0 rows affected`; assert `kind="parity"` increments by 1 |
| AC7 | `transient_metric_increments_on_db_outage` — start dual-write, then `container.stop()`, send a webhook, assert in-memory write succeeds, assert `kind="transient"` increments |

Per `feedback_test_organization_by_ac.md`: ~500-line cap per Rust test file. Two files keeps each well under that. Per `feedback_no_source_grep_tests.md`: all assertions are behavioral (DB row content, metric values, response bodies), never grep-against-source.

### Trait-impl smoke check

A `#[cfg(test)]` doc-test or compile-time assertion in `atc-core/src/persist.rs` confirms `StateStore: PersistentStore`:

```rust
#[allow(dead_code)]
fn _assert_state_store_impls_trait() {
    fn _f<T: PersistentStore>() {}
    _f::<StateStore>();
}
```

Same idiom in `atc-server/src/persist.rs` for `PgStore: PersistentStore`. Trivial; just exercises the type system.

## Acceptance Criteria

Plain-numbered ACs (no slug prefix per user preference). Each AC has a paired success/failure case.

**AC1 — `predecessors_of` introduced and consistent with `transition_to`**
- *Success:* Both impls return `&'static [Self]` (target included for idempotent replay); proptest passes.
- *Failure:* `predecessors_of(JobStatus::Completed)` includes `Queued` (would silently re-introduce a transition `transition_to` rejects).

**AC2 — Run-event durable write via `PgStore`**
- *Success:* `PgStore::apply_run_event` UPSERTs with `WHERE runs.status = ANY($preds::text[])`. First-sight inserts; valid transitions update with `COALESCE` field merge; invalid transitions return `PersistError::InvalidTransition` and leave PG unchanged.
- *Failure:* Any reachable Phase 2b webhook event causes a PG row whose state diverges from `apply_run_event` after replay (column-by-column equality check).

**AC3 — Job-event durable write via `PgStore`, including job-before-run**
- *Success:* `PgStore::apply_job_event` performs (1) a stub `INSERT INTO runs ... ON CONFLICT (id) DO NOTHING` and (2) the predicated job UPSERT, in that order. Job asymmetry (`Queued → Completed` invalid) is enforced by `predecessors_of(JobStatus::Completed) = [InProgress, Completed]`. Job-before-run delivery creates a stub run row (status `'Queued'`, minimal columns from the JobEventEnvelope), and a later real run event reconciles the stub via the run UPSERT predicate + `COALESCE` field merge. Two job UPSERTs for the same unknown run share one stub (idempotency). Broadcast remains a JobEvent — no synthetic RunEvent.
- *Failure:* `Queued → Completed` job webhook accepted by the SQL UPSERT, OR a job-before-run webhook fails with FK violation, OR the stub clobbers a real run row's fields.

**AC4 — Field-merge parity with in-memory store**
- *Success:* Sticky fields (`workflow_name`, `workflow_path`, `run_started_at`, `conclusion`, `runner_*`, `started_at`, `completed_at`) survive subsequent webhooks that omit them.
- *Failure:* A second webhook clobbers `workflow_name` to `NULL` when the envelope omits it.

**AC5 — Webhook handler dual-write composed with the seq mutex**
- *Success:* `webhook_handler` performs in-memory apply + seq bump + broadcast under the seq mutex; drops the mutex; then performs the PG write outside the mutex (only if in-memory apply succeeded and `pg_store.is_some()`). Integration test fires N webhooks; in-memory + PG agree at every step. A concurrency probe confirms the mutex does not hold across PG I/O (handler latencies on parallel webhooks overlap rather than serialize under a slow PG).
- *Failure:* PG call before in-memory apply, before broadcast, or in the `Err(_)` branch of the in-memory apply; OR PG write inside the seq mutex critical section (would cause concurrent webhooks to serialize on PG I/O).

**AC6 — In-memory mode behavioral invariance**
- *Success:* With `ATC_DATABASE_URL` unset (`pg_pool: None`, `pg_store: None`), the webhook handler exhibits the same observable behavior as before this PR: response is 200, broadcast fires on accepted events, in-memory snapshot reflects the event. All existing tests pass without modification. The absence of PG calls is proven by `pg_store: None` short-circuiting at the type level (the `if let Some(pg_store) = ...` branch is dead code in this configuration), not by a runtime "no PG attempt" assertion.
- *Failure:* Adding `pg_store` introduces a panic, log line, or behavioral difference in in-memory mode (response code, broadcast presence, snapshot content).

**AC7 — Drift observability via metrics**
- *Success:* `atc_shadow_pg_write_failures_total{kind="transient"|"parity"}` exists and increments under simulated DB outage and simulated state-divergence respectively.
- *Failure:* Drift goes unobserved (no metric, only ephemeral log lines).

**AC8 — Compile-time SQL via committed `backend/.sqlx/` cache**
- *Success:* All persist-module queries use `sqlx::query!` / `sqlx::query_as!`. `backend/.sqlx/` directory is committed at the Cargo workspace root; CI builds succeed with no `DATABASE_URL` set (sqlx 0.8 transparently falls back to the offline cache). `CONTRIBUTING.md` documents the `cargo sqlx prepare --workspace -- --tests` ritual (run from `backend/`).
- *Failure:* CI fails with "set `DATABASE_URL` to use query macros online, or run `cargo sqlx prepare`" — indicates the cache is missing or stale relative to the SQL in the diff.

**AC9 — `PersistentStore` trait abstraction**
- *Success:* Trait lives in atc-core; `StateStore` and `PgStore` both impl it; route handler holds `Arc<dyn PersistentStore + Send + Sync>` for `pg_store`. atc-core remains sqlx-free.
- *Failure:* Trait leaks sqlx-specific types; or trait is in atc-server (defeats the abstraction); or in-memory `StateStore` doesn't implement it (defeats the symmetry).

**AC10 — Documentation**
- *Success:* All five docs in the **Documents to Update** table change in lockstep with the source diff:
  - `docs/architecture/backend-server.md` describes the trait, dual-write path, shadow-mode error policy, and metric labels.
  - `backend/crates/atc-server/CLAUDE.md` adds `persist` to the module table and notes `PgStore`.
  - `backend/crates/atc-core/CLAUDE.md` adds `persist` to the module table and notes `predecessors_of` as a companion to `transition_to`.
  - `docs/architecture/state-externalization-research/rollout-and-implementation.md` flips Phase 2b status to "complete (PR #XX)" after merge.
  - `CONTRIBUTING.md` adds an "Updating SQL queries" section documenting the `cargo sqlx prepare --workspace -- --tests` workflow (run from `backend/`).
  The doc-staleness pre-push hook passes.
- *Failure:* `src/persist.rs` added but the mapped architecture doc (`docs/architecture/backend-server.md`) doesn't change — pre-push blocks.

## Affected Files

**New:**
- `backend/crates/atc-core/src/persist.rs` (trait + error type + StateStore impl)
- `backend/crates/atc-server/src/persist.rs` (PgStore + impl + ping)
- `backend/crates/atc-server/tests/persist_pg_tests.rs`
- `backend/crates/atc-server/tests/shadow_writes_tests.rs`
- `backend/.sqlx/` (Cargo-workspace-root directory of generated query metadata; NOT repo root)
- `docs/design-plans/2026-05-04-pg-shadow-writes.md` (canonical copy of this plan, on the feature branch)

**Modified:**
- `backend/crates/atc-core/src/run.rs` — `predecessors_of` impl + property test
- `backend/crates/atc-core/src/job.rs` — `predecessors_of` impl + property test
- `backend/crates/atc-core/src/lib.rs` — re-export `PersistentStore`, `PersistError`
- `backend/crates/atc-core/Cargo.toml` — add `async-trait` to deps; ensure `proptest` in dev-deps (likely already there)
- `backend/crates/atc-server/src/lib.rs` — `pub mod persist;`
- `backend/crates/atc-server/src/state.rs` — add `pg_store: Option<Arc<dyn PersistentStore + Send + Sync>>`
- `backend/crates/atc-server/src/main.rs` — build `PgStore` alongside `pg_pool`, wire into `AppState`
- `backend/crates/atc-server/src/routes.rs` — dual-write composed with the seq mutex (PG outside the mutex); classify metrics
- `backend/crates/atc-server/src/metrics.rs` — register `atc_shadow_pg_write_failures_total`
- `backend/crates/atc-server/Cargo.toml` — add `async-trait` to deps
- `backend/crates/atc-server/tests/common/mod.rs` — extend `AppState` test-helper builder with `pg_store: None` (or `Some(PgStore::new(pool))`) field
- `backend/crates/atc-server/tests/db_readyz_tests.rs` — minor: add `pg_store: None` (or `Some(PgStore::new(pool))`) to test `AppState` literals
- `backend/crates/atc-server/tests/metrics.rs`, `tests/state_tests.rs`, `tests/sidecar_tests.rs`, `tests/routes_tests.rs`, `tests/e2e_tests.rs`, `tests/ws_tests.rs`, `tests/config_tests.rs` — sweep with `rg 'AppState \{' backend/crates/atc-server/tests/` to update every literal with `pg_store: None`

**Documents to Update** (per `.ed3d/design-plan-guidance.md` rule 6):

| Document | What changes |
|---|---|
| `docs/architecture/backend-server.md` | Section on dual-write path, `PersistentStore` trait, shadow-mode error policy, metric labels |
| `backend/crates/atc-server/CLAUDE.md` | Module table: add `persist` module, note `PgStore`; testing prerequisites: `cargo sqlx prepare` after schema change |
| `backend/crates/atc-core/CLAUDE.md` | Module table: add `persist` module; note `predecessors_of` companion to `transition_to` |
| `docs/architecture/state-externalization-research/rollout-and-implementation.md` | Flip Phase 2b status to "complete (PR #XX)" after merge |
| `CONTRIBUTING.md` | New section: "Updating SQL queries" — `cargo sqlx prepare --workspace -- --tests` workflow, when to run it, why `.sqlx/` is committed |

**Untouched:**
- `frontend/**` — no contract changes in 2b
- `deploy/helm/atc/**` — no chart changes
- `backend/crates/atc-github/**` — webhook parsing unchanged

## Verification

End-to-end manual run (post-implementation):

```bash
export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock  # macOS/OrbStack

# 1. Tools
just setup                  # ensures sqlx-cli, hooks, etc.
docker info

# 2. Test suite (boots ephemeral PG, runs persist + shadow_writes integration tests)
just test

# 3. Cache regeneration sanity check (run when changing SQL)
docker run -d --rm --name atc-pg -e POSTGRES_PASSWORD=atc -p 5432:5432 postgres:16
DATABASE_URL=postgres://postgres:atc@localhost:5432/postgres \
  cargo sqlx migrate run --source backend/crates/atc-server/migrations
# Cargo workspace root is backend/, so run sqlx prepare from there:
(cd backend && DATABASE_URL=postgres://postgres:atc@localhost:5432/postgres \
  cargo sqlx prepare --workspace -- --tests)
git diff backend/.sqlx/   # should be empty if no SQL changed

# 4. Local dev with PG, fire webhook, verify dual-write
ATC_DATABASE_URL=postgres://postgres:atc@localhost:5432/postgres just dev
curl -X POST localhost:8080/v1/webhooks/github \
  -H "X-GitHub-Event: workflow_run" \
  -H "Content-Type: application/json" \
  -d @backend/crates/atc-github/tests/fixtures/workflow_run_requested.json
curl -s localhost:8080/v1/state | jq '.runs[] | {id, status}'
psql postgres://postgres:atc@localhost:5432/postgres -c "SELECT id, status FROM runs;"

# 5. Drift observation: poison PG, fire webhook, observe metric
psql postgres://postgres:atc@localhost:5432/postgres -c "UPDATE runs SET status='Completed' WHERE id=...;"
curl -X POST localhost:8080/v1/webhooks/github -H ... -d @workflow_run_in_progress.json
curl -s localhost:9090/metrics | grep atc_shadow_pg_write_failures_total
# Expect: kind="parity" counter incremented by 1

# 6. In-memory mode regression
unset ATC_DATABASE_URL && just dev
curl -X POST localhost:8080/v1/webhooks/github -H ... -d @fixture.json
# Expect: no PG attempt, no panic, broadcast still works

# 7. Doc-staleness gate
# Modify src/persist.rs without updating docs/architecture/backend-server.md;
# `git push` should be blocked by lefthook pre-push.
```

## Implementation Sequencing & Parallelism

The dependency chain is tight; true parallelism is limited.

1. **Track A (atc-core, blocks everything else):** Add `predecessors_of` to `RunStatus`/`JobStatus` with property tests. Define `PersistError` and `PersistentStore` trait. Implement `PersistentStore for StateStore`. Land first because (B) and (C) both depend on the trait shape.
2. **Track B (atc-server PgStore):** Implement `PgStore` with `query!` macros. Generate and commit `.sqlx/` cache. Add `persist_pg_tests.rs`. Depends on (A).
3. **Track C (atc-server route wiring + metrics):** `AppState` gains `pg_store`. Webhook handler dual-writes. Metrics registered. `shadow_writes_tests.rs` added. Test-helper `AppState { ... }` literals across `tests/*.rs` updated to include `pg_store: None`. Depends on (B).
4. **Track D (docs):** Update five docs in parallel with (C); minimal interaction.

Recommended team for the implementation phase (when `start-implementation-plan` runs after approval):
- **`task-implementor-fast` for (A)**, dispatched with `model: sonnet`. Apply `feedback_implementor_dispatch_standards.md` (inject feedback memories as `EXTRA_CONTEXT`; demand paste-the-failing-output RED gate). Apply `feedback_subagent_shortcut_patterns.md` (no `as any` casts — irrelevant here, but forbid fabricated AC labels and "pre-existing" dismissals).
- **`code-reviewer` checkpoint** between (A) and (B+C) so the trait surface and `predecessors_of` semantics commit to a verified baseline.
- **`task-implementor-fast` for (B+C)** — same dispatch standards. Splitting B from C generates integration churn against the persist module's API; better to land them together with the cache regenerated against the final schema.
- **`code-reviewer` after (B+C)** verifies dual-write is correctly composed with the seq mutex and metric labels are right.
- **`task-implementor-fast` for (D)** doc updates after (C) is green; these can be a single small PR-shaped commit at the end.

A single `team_name` (e.g., `pg-shadow-writes`) holds the four tracks across two implementor handoffs (A then B+C+D). The `using-git-worktrees` skill creates a worktree at branch creation; `just setup` is required in that worktree to install lefthook hooks.

## Out of Scope

Explicitly handed to a later sub-phase or a different decision:

- **Outbox table and transactional UPSERT+INSERT** — Phase 2c.
- **`LISTEN/NOTIFY` emission and listener stub** — Phase 2d. (The seq mutex still serializes 2b; 2c's BIGSERIAL replaces it.)
- **`ATC_DATABASE_LISTENER_URL` config** — Phase 2d.
- **Reading from PG (snapshot, WS forwarder)** — Phase 3c. (The trait covers writes only in 2b; reads are added to the trait in 3c.)
- **Cursor rename to `lastSeq`** — Phase 3a.
- **Pool stats moved to frontend** — Phase 3b. (Phase 2b leaves `state.store.pool_stats()` exactly as today.)
- **Helm chart changes** — Phase 4.
- **TTL eviction as SQL DELETE** — Phase 3c. (Phase 2b's PG rows accumulate without eviction; in-memory eviction continues for the read path.)
- **Persisting raw GitHub webhook JSON for audit** — ADR 0002 Out of scope; Phase 5.
- **Connection-pool tuning** — defer until 2b's load profile is observed.
- **Eviction of orphaned PG rows in shadow mode** — by design, PG accumulates rows that the in-memory store has TTL-evicted. Phase 3c reconciles this when PG becomes the read source. If 2b's accumulating PG rows are operationally awkward in dev/prod between 2b and 3c, a manual `DELETE` is acceptable; do not bake an eviction loop into 2b.
- **Collapsing `pg_pool` and `pg_store` in `AppState`** — Phase 4. Once reads also live behind the trait, `/readyz` calls a `health_check` method on the trait and `pg_pool` can be dropped from `AppState`.
- **Adding `health_check` to the `PersistentStore` trait** — Phase 4 (when reads cut over and `/readyz` becomes polymorphic).

## Project Deliverables (post-approval)

Per `.ed3d/design-plan-guidance.md` rule 8 and `feedback_pr_title_convention.md`:

1. Create branch `feat/pg-shadow-writes` (from `main`, in a worktree per project convention via `using-git-worktrees`).
2. Run `just setup` in the worktree (worktrees do not inherit lefthook hooks per `feedback_verify_lefthook_installed.md`).
3. Copy this plan to `docs/design-plans/2026-05-04-pg-shadow-writes.md` on the feature branch — the canonical, checked-in home. The `~/.claude/plans/` copy is the working draft only.
4. Implement per the sequencing above.
5. PR per project convention:
   - **Squash merge** — title scoped to the user-facing change, **no phase mechanics**. Suggested: `feat(server): persist workflow run and job state to PostgreSQL`. ("alongside in-memory store" was rejected in codex review as leaking phase mechanics.)
   - PR body = squash commit body (what will be / was implemented; no test plan in body), per `feedback_pr_body_convention.md`.
   - Test plan posted as the **first comment** on the PR, per `feedback_test_plans.md`.

## Forward-Compat Summary (Phase 2c readiness)

| Phase 2b decision | Phase 2c upgrade |
|---|---|
| `PersistentStore` trait wraps SQL inside `PgStore` (takes `&self`, runs against the pool) | **Trait is bypassed for the transactional path.** Phase 2c's route handler opens a `sqlx::Transaction` directly and inlines the UPSERT SQL alongside the outbox INSERT — `predecessors_of` and the SQL strings move out of `PgStore` into the route handler (or into a `pub(crate) fn` next to it). The trait does not gain a transactional method. The trait survives unchanged for shadow-mode writes and as the Phase 4 symmetric-backend abstraction slot. |
| PG write outside the seq mutex (shadow mode) | Seq mutex retires entirely; outbox `BIGSERIAL` becomes the cursor; the in-memory store retires. |
| `kind="parity"` returns 200 in routes.rs (shadow policy) | Transaction rolls back on either UPSERT failure or outbox failure; route returns 5xx; GitHub retries. |
| `kind="transient"` and `kind="parity"` metric labels | Survive — same instrument and labels, new HTTP/retry semantics. |
| `predecessors_of` returns `&'static [Self]` | Reused as-is — composes fine with both `&PgPool` and `&mut Transaction<'_, Postgres>` because the function is pure. |
| `query!` macros against committed `backend/.sqlx/` cache | Reused as-is — the outbox INSERT joins the cache via the same `cargo sqlx prepare --workspace -- --tests` ritual run from `backend/`. |
| `Arc<dyn PersistentStore>` for writes (shadow mode) | The trait stays alive but is no longer the write hot path. Phase 4 grows it to cover reads (`/readyz` adopts a `health_check` method on the trait) and `pg_pool` can be dropped from `AppState` in favor of the trait alone. |
