//! Staleness sweep — force-completes non-terminal runs/jobs GitHub never
//! sent a terminal webhook for.
//!
//! Runs on the same cadence and inside the same task as the outbox sweep
//! (`retention::spawn_outbox_sweep`) — see that function's doc comment for
//! why this piggybacks rather than spawning its own task. Each tick sweeps
//! jobs first, then runs: a run's `NOT EXISTS` non-terminal-jobs guard only
//! holds if any of its stale jobs were already force-completed earlier in
//! the same tick. See ADR-0013 for the full design rationale (synthetic
//! terminal events over GitHub API reconciliation).

use std::time::Duration;

use atc_core::{
    Clock, JobConclusion, JobId, PersistError, RunConclusion, RunId, RunnerInfo, Step,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
};

use crate::TracedPool;
use crate::metrics::PgMetrics;

use super::STALENESS_SWEEP_BATCH_CAP;
use super::writes::{
    insert_outbox_job_in_txn, insert_outbox_run_in_txn, notify_outbox_seq_in_txn,
    upsert_job_in_txn, upsert_run_in_txn,
};

/// Single iteration of the staleness sweep: jobs pass, then runs pass.
/// Returns `(jobs_swept, runs_swept)`. Cutoff is bound Rust-side from
/// `Clock::now()` (no SQL `now()`) per ADR 0007's clock discipline.
#[tracing::instrument(
    name = "staleness.sweep.tick",
    skip_all,
    fields(
        threshold_seconds = threshold.as_secs(),
        jobs_swept = tracing::field::Empty,
        runs_swept = tracing::field::Empty,
    ),
)]
pub(crate) async fn staleness_sweep_tick(
    clock: &dyn Clock,
    pool: &TracedPool,
    threshold: Duration,
    metrics: &PgMetrics,
) -> Result<(u64, u64), PersistError> {
    let now = clock.now();
    let cutoff = now
        - chrono::Duration::from_std(threshold).expect("staleness threshold fits chrono::Duration");

    let jobs_swept = sweep_stale_jobs(now, cutoff, pool, metrics).await?;
    let runs_swept = sweep_stale_runs(now, cutoff, pool, metrics).await?;

    let span = tracing::Span::current();
    span.record("jobs_swept", jobs_swept);
    span.record("runs_swept", runs_swept);
    if jobs_swept > 0 || runs_swept > 0 {
        tracing::info!(jobs_swept, runs_swept, "staleness sweep completed");
    }

    Ok((jobs_swept, runs_swept))
}

// ---------------------------------------------------------------------------
// Jobs pass
// ---------------------------------------------------------------------------

async fn sweep_stale_jobs(
    now: chrono::DateTime<chrono::Utc>,
    cutoff: chrono::DateTime<chrono::Utc>,
    pool: &TracedPool,
    metrics: &PgMetrics,
) -> Result<u64, PersistError> {
    // `workflow_job` webhooks only fire on status transitions, so
    // GREATEST(created_at, started_at) already captures last-observed
    // activity for a non-terminal job — no `jobs.updated_at` migration
    // needed. See ADR-0013.
    //
    // Excludes `Waiting`: mirrors `is_stale_job` (atc-core) — `Waiting ->
    // Completed` is not a valid FSM transition, so a `Waiting` job can never
    // be force-completed; selecting one here would only waste a row lock on
    // a predicated UPSERT that's guaranteed to reject.
    let candidate_ids: Vec<i64> = sqlx::query_scalar!(
        r#"
        SELECT id FROM jobs
         WHERE status = ANY(ARRAY['Queued', 'InProgress'])
           AND GREATEST(created_at, COALESCE(started_at, created_at)) < $1
         ORDER BY id
         LIMIT $2
        "#,
        cutoff,
        STALENESS_SWEEP_BATCH_CAP,
    )
    .fetch_all(pool)
    .await
    .map_err(backend)?;

    let mut swept = 0u64;
    for job_id in candidate_ids {
        // A per-row failure (e.g. a race this function doesn't anticipate)
        // must not abort the rest of the batch or the runs pass after it —
        // log and continue, mirroring `atc-store-mem`'s per-row resilience.
        match sweep_one_job(now, cutoff, job_id, pool, metrics).await {
            Ok(true) => swept += 1,
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(job_id, error.message = ?e, "staleness sweep: job row skipped");
            }
        }
    }
    if swept > 0 {
        metrics.staleness_swept_job(swept);
    }
    Ok(swept)
}

/// Lock, re-check, and (if still non-terminal and still stale) force-complete
/// a single job with conclusion `Stale`. Returns `true` if the row was swept.
///
/// `FOR UPDATE OF j SKIP LOCKED` locks only the `jobs` row (not the joined
/// `runs` row); if another replica already holds the lock, this returns
/// `Ok(false)` without waiting — the other replica's transaction is the one
/// that resolves the row. Two conditions are re-checked after acquiring the
/// lock, both against data freshly read under it:
///
/// - **Status.** Closes the race against a real *completion* webhook:
///   whichever transaction commits first wins, and the loser observes
///   `Completed` and skips.
/// - **Age (`GREATEST(created_at, started_at) < cutoff`).** Closes the race
///   against a real *progress* webhook that doesn't complete the job but
///   does move its activity forward — e.g. `Queued -> InProgress` bumping
///   `started_at`. Without this recheck, a job that started moments ago
///   would still be force-completed as `Stale`, because "not yet
///   `Completed`" alone doesn't mean "still past the threshold": the row we
///   just locked can be fresher than the snapshot that made it a candidate.
async fn sweep_one_job(
    now: chrono::DateTime<chrono::Utc>,
    cutoff: chrono::DateTime<chrono::Utc>,
    job_id: i64,
    pool: &TracedPool,
    metrics: &PgMetrics,
) -> Result<bool, PersistError> {
    let mut tx = pool.begin().await.map_err(backend)?;

    let row = sqlx::query!(
        r#"
        SELECT j.id              AS "id!",
               j.run_id          AS "run_id!",
               j.name            AS "name!",
               j.status          AS "status!",
               j.labels          AS "labels!: Vec<String>",
               j.steps           AS "steps!: serde_json::Value",
               j.runner_id       AS "runner_id?",
               j.runner_name     AS "runner_name?",
               j.runner_group_name AS "runner_group_name?",
               j.created_at      AS "created_at!",
               j.started_at      AS "started_at?",
               j.run_attempt     AS "run_attempt!",
               r.org             AS "org!",
               r.repo            AS "repo!"
          FROM jobs j
          JOIN runs r ON r.id = j.run_id
         WHERE j.id = $1
         FOR UPDATE OF j SKIP LOCKED
        "#,
        job_id,
    )
    .fetch_optional(&mut tx.executor())
    .await
    .map_err(backend)?;

    let Some(row) = row else {
        // Locked by another replica's in-flight sweep, or the job no longer
        // exists (evicted in in-memory mode's analog doesn't apply to PG,
        // but a future deletion path could race here). Either way, skip.
        return Ok(false);
    };

    if row.status == "Completed" {
        // A real webhook won the race between candidate-select and this
        // lock. Nothing to do — the transaction has no writes, so COMMIT
        // and ROLLBACK are equivalent; COMMIT avoids an extra round trip.
        tx.commit().await.map_err(backend)?;
        return Ok(false);
    }

    let last_activity = row.started_at.unwrap_or(row.created_at).max(row.created_at);
    if last_activity >= cutoff {
        // A real progress webhook (e.g. Queued -> InProgress) landed between
        // candidate-select and this lock, bumping started_at. The job is no
        // longer stale — the candidate-select snapshot is out of date, and
        // "not yet Completed" alone doesn't mean "still past the threshold".
        tx.commit().await.map_err(backend)?;
        return Ok(false);
    }

    // Propagate (not swallow) a malformed `steps` blob — matches
    // `reads.rs::read_all_jobs`. `upsert_job_in_txn` writes `steps` back
    // unconditionally (snapshot replacement, not COALESCE), so defaulting to
    // `vec![]` here would permanently erase the job's real step history.
    let steps: Vec<Step> = serde_json::from_value(row.steps).map_err(backend_json)?;
    let runner = match (row.runner_id, row.runner_name) {
        (Some(id), Some(name)) => Some(RunnerInfo {
            id,
            name,
            group_name: row.runner_group_name,
        }),
        _ => None,
    };

    let env = JobEventEnvelope {
        job_id: JobId(row.id),
        run_id: RunId(row.run_id),
        org: row.org,
        repo: row.repo,
        name: row.name,
        created_at: row.created_at,
        started_at: row.started_at,
        completed_at: Some(now),
        run_attempt: row.run_attempt,
        repo_id: None,
        action: JobEvent::Completed {
            conclusion: JobConclusion::Stale,
            runner,
            labels: row.labels,
            steps,
        },
    };

    upsert_job_in_txn(&mut tx, &env).await?;
    let seq = insert_outbox_job_in_txn(&mut tx, &env).await?;
    notify_outbox_seq_in_txn(&mut tx, "job", seq).await?;
    tx.commit().await.map_err(backend)?;
    // Emit AFTER commit, same as `PgStore::apply_job_event`: PG delivers
    // NOTIFYs on COMMIT, and the listener/drain will process this row's
    // outbox entry regardless of who wrote it — the emitted counter must
    // count it too, or emitted/received parity dashboards under-report.
    metrics.notify_emitted_job();

    tracing::debug!(job_id = row.id, "staleness sweep force-completed job");
    Ok(true)
}

// ---------------------------------------------------------------------------
// Runs pass
// ---------------------------------------------------------------------------

async fn sweep_stale_runs(
    now: chrono::DateTime<chrono::Utc>,
    cutoff: chrono::DateTime<chrono::Utc>,
    pool: &TracedPool,
    metrics: &PgMetrics,
) -> Result<u64, PersistError> {
    // `placeholder = false` excludes FK-stub rows (see `0003_runs_placeholder.sql`)
    // — a stub's `updated_at` is stamped at job-arrival time and never
    // otherwise advances, so it would otherwise accumulate as a permanent
    // candidate. `read_all_runs` already hides placeholders from `/v1/state`
    // regardless of status, so sweeping one would only spend work turning an
    // invisible stub into an invisible Stale stub — filtering it out here is
    // strictly cheaper and avoids ever flipping `placeholder` to `false` on a
    // stub run (which `upsert_run_in_txn` does unconditionally on any write).
    //
    // `jobs.run_attempt >= runs.run_attempt` mirrors `read_all_jobs`'s filter
    // (reads.rs): a re-run's superseded lower-attempt jobs stay in the table
    // forever and must not count as "live" for the CURRENT attempt. Without
    // this, a zombie `Waiting` job left over from attempt 1 (jobs sweep never
    // completes `Waiting` — see `sweep_stale_jobs`) would permanently shield
    // attempt 2's run row from ever being swept, even once attempt 2 itself
    // goes stale.
    let candidate_ids: Vec<i64> = sqlx::query_scalar!(
        r#"
        SELECT id FROM runs
         WHERE status != 'Completed'
           AND placeholder = false
           AND updated_at < $1
           AND NOT EXISTS (
               SELECT 1 FROM jobs
                WHERE jobs.run_id = runs.id
                  AND jobs.status != 'Completed'
                  AND jobs.run_attempt >= runs.run_attempt
           )
         ORDER BY id
         LIMIT $2
        "#,
        cutoff,
        STALENESS_SWEEP_BATCH_CAP,
    )
    .fetch_all(pool)
    .await
    .map_err(backend)?;

    let mut swept = 0u64;
    for run_id in candidate_ids {
        match sweep_one_run(now, cutoff, run_id, pool, metrics).await {
            Ok(true) => swept += 1,
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(run_id, error.message = ?e, "staleness sweep: run row skipped");
            }
        }
    }
    if swept > 0 {
        metrics.staleness_swept_run(swept);
    }
    Ok(swept)
}

/// Lock, re-check, and (if still non-terminal, still stale, and with no live
/// jobs) force-complete a single run with conclusion `Stale`. Returns `true`
/// if the row was swept.
///
/// Three conditions are re-checked after acquiring the row lock, all against
/// data freshly read under it:
///
/// - **Status** (closes the race against a real completion webhook, same as
///   [`sweep_one_job`]).
/// - **Age (`updated_at < cutoff`)** — closes the analogous race to
///   `sweep_one_job`'s age recheck: a real run-level webhook (e.g. a
///   `workflow_run` event that doesn't complete the run) can bump
///   `updated_at` between candidate-select and this lock, and "not yet
///   `Completed`" alone doesn't mean "still past the threshold".
/// - **Non-terminal-jobs guard** (shrinks, but cannot fully close, the race
///   against a fresh job arriving — e.g. a queued re-run — after this check
///   but before commit: `upsert_job_in_txn`'s FK-stub `INSERT ... ON
///   CONFLICT` blocks on our `FOR UPDATE` lock, so the job row itself only
///   commits after we do, but its presence is invisible to *our*
///   already-taken `EXISTS` snapshot. The window is one transaction's
///   duration, and it self-heals the same way a real race does: the job's
///   own eventual completion (same attempt) or the re-run's terminal event
///   (higher attempt) is accepted by `upsert_run_in_txn`'s predicate and
///   overwrites the synthetic `Stale` conclusion — see ADR-0013).
async fn sweep_one_run(
    now: chrono::DateTime<chrono::Utc>,
    cutoff: chrono::DateTime<chrono::Utc>,
    run_id: i64,
    pool: &TracedPool,
    metrics: &PgMetrics,
) -> Result<bool, PersistError> {
    let mut tx = pool.begin().await.map_err(backend)?;

    let row = sqlx::query!(
        r#"
        SELECT id, org, repo, workflow_name, workflow_path, branch, head_sha,
               commit_message, event, display_title, html_url, status,
               created_at, run_started_at, run_attempt, updated_at
          FROM runs
         WHERE id = $1
         FOR UPDATE SKIP LOCKED
        "#,
        run_id,
    )
    .fetch_optional(&mut tx.executor())
    .await
    .map_err(backend)?;

    let Some(row) = row else {
        return Ok(false);
    };

    if row.status == "Completed" {
        tx.commit().await.map_err(backend)?;
        return Ok(false);
    }

    if row.updated_at >= cutoff {
        // A real run-level webhook landed between candidate-select and this
        // lock, bumping updated_at without completing the run. No longer
        // stale — the candidate-select snapshot is out of date.
        tx.commit().await.map_err(backend)?;
        return Ok(false);
    }

    // `jobs.run_attempt >= runs.run_attempt` — see the candidate query's
    // comment above for why prior-attempt jobs must not count as live.
    let has_non_terminal_jobs: bool = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM jobs
             WHERE jobs.run_id = $1
               AND jobs.status != 'Completed'
               AND jobs.run_attempt >= $2
        ) AS "exists!"
        "#,
        run_id,
        row.run_attempt,
    )
    .fetch_one(&mut tx.executor())
    .await
    .map_err(backend)?;

    if has_non_terminal_jobs {
        // A fresh job arrived (e.g. a queued re-run) after candidate-select.
        // The run is shielded — nothing to do.
        tx.commit().await.map_err(backend)?;
        return Ok(false);
    }

    let env = RunEventEnvelope {
        run_id: RunId(row.id),
        org: row.org,
        repo: row.repo,
        workflow_name: row.workflow_name,
        workflow_path: row.workflow_path,
        branch: row.branch,
        head_sha: row.head_sha,
        commit_message: row.commit_message,
        trigger_event: row.event,
        display_title: row.display_title,
        html_url: row.html_url,
        created_at: row.created_at,
        run_started_at: row.run_started_at,
        updated_at: now,
        completed_at: Some(now),
        run_attempt: row.run_attempt,
        repo_id: None,
        action: RunEvent::Completed {
            conclusion: RunConclusion::Stale,
        },
    };

    upsert_run_in_txn(&mut tx, &env).await?;
    let seq = insert_outbox_run_in_txn(&mut tx, &env).await?;
    notify_outbox_seq_in_txn(&mut tx, "run", seq).await?;
    tx.commit().await.map_err(backend)?;
    // Emit AFTER commit — see the matching comment in `sweep_one_job`.
    metrics.notify_emitted_run();

    tracing::debug!(run_id = row.id, "staleness sweep force-completed run");
    Ok(true)
}

/// Wrap a raw sqlx error as [`PersistError::Backend`], so every fallible
/// step in this module (`writes.rs` helpers included) shares one error type.
fn backend(e: sqlx::Error) -> PersistError {
    PersistError::Backend(Box::new(e))
}

/// Wrap a `steps` JSON deserialization error as [`PersistError::Backend`].
fn backend_json(e: serde_json::Error) -> PersistError {
    PersistError::Backend(Box::new(e))
}
