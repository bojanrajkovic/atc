# ADR 0013 — Staleness sweep: synthetic terminal events over GitHub API reconciliation

Date: 2026-07-03
Status: Accepted

## Context

ATC is purely webhook-driven. If GitHub drops (or the drain never processes) the terminal `completed` webhook for a run or job, nothing ever moves the row out of `Queued`/`Waiting`/`InProgress`:

- `is_evictable` (`atc-core`) only applies TTL eviction to already-`Completed` jobs, and only the in-memory store wires it in.
- The Postgres display-TTL cutoff (`reads.rs`) hides old **completed** rows at read time; it never touches non-terminal rows.
- There was no admin endpoint and no reconciliation against the GitHub API.

Observed in production: 5 jobs across 4 runs sat `InProgress` for 10–15 days. The only fix was raw SQL against the `jobs`/`runs` tables, which bypasses the outbox/NOTIFY pipeline entirely — connected WebSocket clients never saw the correction, and the fix had to be repeated by hand for each stuck row.

Issue [#439](https://github.com/bojanrajkovic/atc/issues/439) is the ticket for this work.

## Decision

A **store-owned background staleness sweep**: after a configurable period with no observed webhook activity (default 48h), non-terminal rows are force-completed with conclusion `Stale` — a variant that already exists on both `RunConclusion` and `JobConclusion` (it is GitHub's own vocabulary for discarded work). The synthetic completion is written through the **existing per-event transaction path** (predicated UPSERT + outbox insert + `pg_notify`), so drain → broadcast → WebSocket fan-out all work unchanged and clients see the correction live.

No GitHub API reconciliation, no admin endpoint (see Alternatives).

### Why a synthetic event is safe

The state machine is forward-only (`Queued → InProgress → Completed`, same-status transitions idempotent — see `docs/architecture/backend-server.md` § Domain model and state-machine invariants). This gives two load-bearing properties:

1. **A synthetic completion can never reopen or regress a row.** `Completed → InProgress` is rejected by both stores' predicated writes.
2. **A false positive self-heals.** `Completed → Completed` is admitted, and `conclusion: conclusion.or(existing.conclusion)` means a later *real* `Completed{Success}` webhook overwrites the synthetic `Stale` — the incoming `Some` conclusion always wins over the existing one. The one wart: `completed_at` uses preserve-first semantics, so it keeps the sweep's timestamp rather than the real completion time.

The one thing the FSM does **not** protect against is the reverse race — a sweep writing `Stale` *after* a real `Success` landed. Same-status replay is admitted by design, and whichever conclusion arrives second would normally win. The sweep closes this itself: it takes a row lock (`SELECT ... FOR UPDATE SKIP LOCKED`) and re-checks the status inside the writing transaction before building the synthetic envelope.

### Sweep mechanics

No new background task: the staleness pass rides the existing outbox sweep task (ADR-0006: stores own their background tasks; ADR-0007: the structural template this clones — quiet first tick, `tokio::select!` on cancellation vs. a fixed interval, Rust-side clock cutoffs, never SQL `now()`). Both passes run on the identical 300s cadence, so the staleness sweep is piggybacked onto the outbox sweep's tick — the same pattern that task already uses for its own watermark cleanup — rather than spawning a second task with its own `JoinHandle` and shutdown-join step. `staleness_threshold: None` skips just the staleness pass each tick; the outbox sweep itself is unconditional.

Each tick, with `cutoff = clock.now() - staleness_threshold`:

- **Pass 1 — jobs.** Candidates: `status IN ('Queued', 'InProgress') AND GREATEST(created_at, COALESCE(started_at, created_at)) < cutoff`, batch-capped at 500. `workflow_job` webhooks only fire on the four status transitions, so this expression *is* last-observed-activity for a non-terminal job — no `jobs.updated_at` migration needed. `Waiting` is deliberately excluded: `JobStatus::transition_to` has no `Waiting -> Completed` arm (only `Queued`/`InProgress` reach `Completed`), so a `Waiting` job can never be force-completed — selecting one would only waste a row lock on a predicated UPSERT guaranteed to reject.
- **Pass 2 — runs.** Candidates: `status != 'Completed' AND placeholder = false AND updated_at < cutoff AND NOT EXISTS (non-terminal jobs for this run)`. `placeholder = false` excludes FK-stub rows (`0003_runs_placeholder.sql`) — `read_all_runs` already hides them from `/v1/state` regardless of status, so sweeping one would only spend work turning an invisible stub into an invisible `Stale` stub, and `upsert_run_in_txn` unconditionally flips `placeholder` to `false` on any write, which would be actively wrong to do to a stub. The `NOT EXISTS` guard matters: `runs.updated_at` only bumps on **run** webhooks, so a legitimately-running multi-day self-hosted job would otherwise starve its run's signal and get its parent falsely swept. Because jobs sweep first in the same tick, any *stale* jobs are already force-completed by the time pass 2 runs its `NOT EXISTS` query — any remaining non-terminal job is fresh and correctly shields its run.

Per candidate row, one transaction: `SELECT ... FOR UPDATE SKIP LOCKED` → re-check the row is still non-terminal (and, for runs, that it still has no live jobs) → if so, build a synthetic `RunEventEnvelope`/`JobEventEnvelope` (`Completed { conclusion: Stale }`, `completed_at = now`) sourced entirely from the locked row → write through the same `upsert_*_in_txn` + `insert_outbox_*_in_txn` + `notify_outbox_seq_in_txn` helpers the webhook handler uses → commit.

- **Race vs. a real webhook:** the `FOR UPDATE` + status re-check closes it. If the real `Completed{Success}` commits first, the sweep observes `Completed` and skips. If the sweep commits first, the real webhook later lands as an idempotent `Completed → Completed` and its conclusion wins.
- **Multi-replica:** every replica runs the sweep, no leader (ADR-0002). `SKIP LOCKED` partitions concurrent sweepers — a replica that loses the row lock gets `None` back immediately (no blocking) and moves on; it never re-checks or double-writes because it never acquired the lock in the first place.
- **Batch cap** (500/tick, crate const) bounds per-tick work; leftovers wait for the next tick. A run whose stale jobs were deferred by the cap is shielded until the jobs sweep catches up — self-correcting.

### In-memory store

The same policy is expressed as a pure predicate pair in `atc-core` beside `is_evictable` — `is_stale_job` / `is_stale_run` — and `atc-store-mem` wires them into its existing eviction-tick task (same interval, same tick, no separate task). It synthesizes the same envelopes and applies them through the normal `apply_*_event` path so seq allocation, indexing, and broadcast all behave identically to a real webhook. A concurrent real completion racing the sweep is resolved by `apply_*_event`'s own forward-only transition check rather than a row lock (there is no row lock concept in the in-memory store) — whichever call lands first wins, and the loser's write returns `Err(InvalidTransition)`, which the sweep logs at debug and ignores.

### Configuration

One new knob, `staleness_threshold: Option<Duration>` (`ATC_STALENESS_THRESHOLD`, humantime), default 48h, floor 6h (rejected at startup — below GitHub's own hosted-job ceiling, a shorter threshold would false-positive on every legitimate long-running hosted job). `null` disables the sweep — no task is spawned in either store. Restart-only, same reload posture as `outbox_retention` / `display_ttl` (ADR-0009's Decision 9 precedent); the config watcher already warn-logs changed scalars it won't apply live.

Sweep interval (300s) and batch cap (500) are crate consts, not operator-tunable — matching the retention sweep's stance that cadence isn't an operator concern (ADR-0007).

Threshold rationale: GitHub's own ceilings are 6h (hosted job), 24h (hosted queue wait), 5 days (self-hosted job), 35 days (run). 48h covers all hosted cases with wide margin; only multi-day self-hosted jobs can false-positive, and those self-heal on the real webhook. Operators with such workloads raise the threshold. A job stuck `Waiting` (e.g. an unattended deployment-approval gate) is never swept at all — see the jobs-pass exclusion above — so it surfaces indefinitely until a human resolves it or a real webhook advances it, matching GitHub's own "waiting" semantics rather than mislabeling human-pending work as `Stale`.

## Alternatives considered

- **GitHub API reconciliation before declaring dead.** Rejected. ATC has zero outbound GitHub surface today (webhooks + HMAC verification only — no `reqwest`/`octocrab` toward GitHub, no PAT/App story, no rate-limit handling, and a multi-org token-scoping question). The correctness gain is marginal given the self-heal property above. Revisit only if false positives are observed to hurt in practice.
- **Admin force-resolve endpoint.** Rejected for now. There is no in-process auth; a mutating endpoint would rely entirely on the operator's auth proxy (`docs/operator/authentication.md`). The sweep resolves the entire incident *class* automatically, which is strictly better than a manual per-incident verb. Can be layered on later if a "resolve *now*" need appears.
- **Raw-SQL runbook (status quo).** This was the bug: it bypasses the outbox/NOTIFY pipeline, so WS clients never see the correction, and it has to be repeated by hand for every future occurrence.
- **A `jobs.updated_at` migration.** Unnecessary: `workflow_job` webhooks only fire on status transitions, so `GREATEST(created_at, started_at)` already captures last activity without a schema change. Add the column only if jobs ever gain intermediate updates that don't carry a status transition.

## Consequences

### Positive

- **The incident class self-heals automatically**, including any rows already stuck at deploy time — the first sweep tick after rollout force-completes them, and the existing display-TTL cutoff ages them off `/v1/state` shortly after.
- **No new outbound dependency.** The sweep only ever writes through the existing transactional write path; ATC's webhook-only posture toward GitHub is unchanged.
- **Dev/prod parity.** The in-memory store gets the identical policy via a shared pure predicate, so the sweep's behavior doesn't diverge between local dev and production Postgres.

### Negative

- **A legitimately long-running self-hosted job's run is marked `Stale` in the dashboard until the real webhook lands.** This is a real, if narrow, UX wart for operators running multi-day self-hosted jobs at the default threshold. Mitigation: raise `staleness_threshold` for that workload profile.
- **`completed_at` preserve-first semantics mean a swept row's `completed_at` reflects sweep time, not real completion time**, even after the real webhook's conclusion overwrites the synthetic `Stale`. Acceptable — the row was, by definition, already past the operator's own staleness bar.
- **The PG outbox sweep task now does two jobs.** Piggybacking the staleness pass onto its tick (rather than a dedicated task) avoids a new `JoinHandle` and shutdown-join step, but a future contributor adding a third piggybacked concern to that task should watch its tick body for growing into an unrelated-responsibilities grab bag.
- **A run's non-terminal-jobs re-check under the row lock narrows, but cannot fully close, the race against a brand-new job arriving for that run.** The row lock blocks `upsert_job_in_txn`'s FK-stub insert from committing until the sweep's transaction finishes, but the sweep's own `EXISTS` snapshot was already taken — a run can commit as `Stale` moments before that fresh job's row lands. Self-heals the same way any post-sweep completion does (the job's own terminal event, or the re-run's, overwrites the synthetic conclusion per `upsert_run_in_txn`'s predicate), but the dashboard can show a `Stale` run with a live job underneath it for the width of one transaction.

## References

- Issue: [#439](https://github.com/bojanrajkovic/atc/issues/439)
- Design doc: Outline "439 — Staleness sweep for stuck non-terminal runs/jobs (design)"
- Implementation: `backend/crates/atc-core/src/state_machine.rs` (`is_stale_job`, `is_stale_run`); `backend/crates/atc-store-pg/src/store/staleness.rs`; `backend/crates/atc-store-mem/src/lib.rs` (`sweep_stale`)
- Operator surface: [`docs/architecture/deployment.md`](../architecture/deployment.md) § `ATC_STALENESS_THRESHOLD`
- Metrics: [`docs/architecture/metrics.md`](../architecture/metrics.md) § `atc_staleness_swept_total`
- Related ADRs: [0002](0002-state-externalization-postgres-outbox.md) (PG outbox externalisation, no-leader replicas), [0006](0006-stores-own-background-task-lifecycle.md) (store-owned lifecycle), [0007](0007-outbox-retention-policy.md) (the sweep-task structural template), [0009](0009-display-vs-data-retention.md) (display-TTL, restart-only scalar precedent).
