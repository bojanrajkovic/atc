//! Read-side helpers for PG state snapshot (`/v1/state`).
//!
//! These free functions read all runs and jobs from an open transaction.
//! They are called by `PgStore::read_snapshot` (Phase 3) and by the
//! current `routes::state_handler` PG path (until that phase lands).

use std::collections::HashSet;

use atc_core::{
    Job, JobConclusion, JobId, JobStatus, PersistError, RepoId, RunConclusion, RunId, RunStatus,
    RunnerInfo, Step, WorkflowRun,
};
use chrono::{DateTime, Utc};

use crate::TracedPool;

/// Parse a SQL CHECK constraint string back to [`RunStatus`].
///
/// Inverse of the `SqlRepr::sql_repr` mapping in `pg.rs`. The persistence layer
/// writes only valid constraint strings, so an unrecognized value indicates
/// schema/code drift — surfaced as [`PersistError::Backend`].
pub(super) fn parse_run_status(s: &str) -> Result<RunStatus, PersistError> {
    match s {
        "Queued" => Ok(RunStatus::Queued),
        "InProgress" => Ok(RunStatus::InProgress),
        "Completed" => Ok(RunStatus::Completed),
        other => Err(PersistError::Backend(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(format!(
            "unknown run status from PG: {other}"
        )))),
    }
}

pub(super) fn parse_job_status(s: &str) -> Result<JobStatus, PersistError> {
    match s {
        "Queued" => Ok(JobStatus::Queued),
        "Waiting" => Ok(JobStatus::Waiting),
        "InProgress" => Ok(JobStatus::InProgress),
        "Completed" => Ok(JobStatus::Completed),
        other => Err(PersistError::Backend(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(format!(
            "unknown job status from PG: {other}"
        )))),
    }
}

pub(super) fn parse_run_conclusion(s: &str) -> Result<RunConclusion, PersistError> {
    match s {
        "Success" => Ok(RunConclusion::Success),
        "Failure" => Ok(RunConclusion::Failure),
        "Cancelled" => Ok(RunConclusion::Cancelled),
        "TimedOut" => Ok(RunConclusion::TimedOut),
        "ActionRequired" => Ok(RunConclusion::ActionRequired),
        "Stale" => Ok(RunConclusion::Stale),
        "Neutral" => Ok(RunConclusion::Neutral),
        "Skipped" => Ok(RunConclusion::Skipped),
        "StartupFailure" => Ok(RunConclusion::StartupFailure),
        other => Err(PersistError::Backend(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(format!(
            "unknown run conclusion from PG: {other}"
        )))),
    }
}

pub(super) fn parse_job_conclusion(s: &str) -> Result<JobConclusion, PersistError> {
    match s {
        "Success" => Ok(JobConclusion::Success),
        "Failure" => Ok(JobConclusion::Failure),
        "Cancelled" => Ok(JobConclusion::Cancelled),
        "TimedOut" => Ok(JobConclusion::TimedOut),
        "ActionRequired" => Ok(JobConclusion::ActionRequired),
        "Stale" => Ok(JobConclusion::Stale),
        "Neutral" => Ok(JobConclusion::Neutral),
        "Skipped" => Ok(JobConclusion::Skipped),
        other => Err(PersistError::Backend(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(format!(
            "unknown job conclusion from PG: {other}"
        )))),
    }
}

/// Read all real (non-placeholder) runs ordered by id.
///
/// Filters out FK-only stub rows created by `upsert_job_in_txn` for
/// job-before-run delivery. Used by the PG path to project the PG state
/// into the `StateSnapshot` wire contract.
///
/// `cutoff = Some(t)` hides completed rows whose `completed_at` is strictly
/// earlier than `t` (display-TTL gate). The predicate is permissive on
/// `completed_at IS NULL` so newly-deployed code can still surface a
/// completed row that has not yet been backfilled or has received no event
/// since the deploy; the `(status, completed_at)` composite index keeps
/// the filter cheap. Symmetric with `atc-store-mem`'s `run_passes_cutoff`.
#[allow(dead_code)]
pub(crate) async fn read_all_runs(
    tx: &mut sqlx_tracing::Transaction<'_, sqlx::Postgres>,
    cutoff: Option<DateTime<Utc>>,
) -> Result<Vec<WorkflowRun>, PersistError> {
    let rows = sqlx::query!(
        r#"
        SELECT id, org, repo, workflow_name, workflow_path, branch, head_sha,
               commit_message, event, display_title, status, conclusion,
               html_url, created_at, run_started_at, updated_at, completed_at,
               run_attempt, repo_id
          FROM runs
         WHERE placeholder = false
           AND ($1::timestamptz IS NULL
                OR status != 'Completed'
                OR completed_at IS NULL
                OR completed_at >= $1::timestamptz)
         ORDER BY id
        "#,
        cutoff,
    )
    .fetch_all(&mut tx.executor())
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    let mut runs = Vec::with_capacity(rows.len());
    for row in rows {
        let status = parse_run_status(&row.status)?;
        let conclusion = match row.conclusion {
            Some(s) => Some(parse_run_conclusion(&s)?),
            None => None,
        };
        runs.push(WorkflowRun {
            id: RunId(row.id),
            org: row.org,
            repo: row.repo,
            workflow_name: row.workflow_name,
            workflow_path: row.workflow_path,
            branch: row.branch,
            head_sha: row.head_sha,
            commit_message: row.commit_message,
            event: row.event,
            display_title: row.display_title,
            status,
            conclusion,
            html_url: row.html_url,
            created_at: row.created_at,
            run_started_at: row.run_started_at,
            updated_at: row.updated_at,
            completed_at: row.completed_at,
            run_attempt: row.run_attempt,
            repo_id: row.repo_id.map(RepoId),
        });
    }
    Ok(runs)
}

/// Read all jobs ordered by id, reconstructing `RunnerInfo` and `steps`.
///
/// `cutoff` filters jobs in two dimensions:
///   1. The job's own `(status, completed_at)` predicate, mirroring
///      [`read_all_runs`]'s shape and leveraging the existing
///      `jobs_status_completed_at_idx` composite index.
///   2. The parent run's cutoff predicate — a job whose run is itself
///      filtered out by the run-level cutoff is also excluded. Without
///      this gate, a completed run aged past the cutoff with a
///      non-`Completed` (or `completed_at IS NULL`) sub-job would produce
///      an orphan job on the wire. The frontend's runner-pool derivation
///      tolerates orphan jobs by treating them as live, so the orphan
///      would inflate pool capacity stats after the run had aged out of
///      view.
///
/// Placeholder parent runs are intentionally NOT excluded: when a job
/// webhook arrives before its run webhook, `upsert_job_in_txn` inserts a
/// stub run with `placeholder=true` to satisfy the FK. The wire contract
/// is that the job is still visible (the placeholder row itself is hidden
/// by `read_all_runs`). The parent-cutoff predicate keeps placeholder
/// runs because their status defaults to non-`Completed`.
#[allow(dead_code)]
pub(crate) async fn read_all_jobs(
    tx: &mut sqlx_tracing::Transaction<'_, sqlx::Postgres>,
    cutoff: Option<DateTime<Utc>>,
) -> Result<Vec<Job>, PersistError> {
    // Column annotations: sqlx 0.8's compile-time JOIN analysis is
    // conservative and would otherwise lose the NOT NULL guarantees on
    // `j.*` columns (since a JOIN could in principle produce NULL rows).
    // Explicit `as "name!"` and `as "name?"` annotations restore the
    // correct nullability the schema enforces, matching the row struct
    // the no-JOIN `read_all_runs` produces.
    let rows = sqlx::query!(
        r#"
        SELECT j.id            AS "id!",
               j.run_id        AS "run_id!",
               j.name          AS "name!",
               j.status        AS "status!",
               j.conclusion    AS "conclusion?",
               j.runner_id     AS "runner_id?",
               j.runner_name   AS "runner_name?",
               j.runner_group_name AS "runner_group_name?",
               j.labels        AS "labels!: Vec<String>",
               j.steps         AS "steps!: serde_json::Value",
               j.created_at    AS "created_at!",
               j.started_at    AS "started_at?",
               j.completed_at  AS "completed_at?",
               j.run_attempt   AS "run_attempt!"
          FROM jobs j
          JOIN runs r ON r.id = j.run_id
         WHERE j.run_attempt >= r.run_attempt
           AND ($1::timestamptz IS NULL
                -- A higher-attempt job's parent row is still the aged-out prior
                -- attempt; don't gate the fresh job on the stale run's cutoff.
                -- It self-heals once the run event advances the row.
                OR j.run_attempt > r.run_attempt
                OR r.status != 'Completed'
                OR r.completed_at IS NULL
                OR r.completed_at >= $1::timestamptz)
           AND ($1::timestamptz IS NULL
                OR j.status != 'Completed'
                OR j.completed_at IS NULL
                OR j.completed_at >= $1::timestamptz)
         ORDER BY j.id
        "#,
        cutoff,
    )
    .fetch_all(&mut tx.executor())
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    let mut jobs = Vec::with_capacity(rows.len());
    for row in rows {
        let status = parse_job_status(&row.status)?;
        let conclusion = match row.conclusion {
            Some(s) => Some(parse_job_conclusion(&s)?),
            None => None,
        };
        // RunnerInfo is composed if runner_id/runner_name are present together.
        // The PG schema does not enforce this pairing; if either is missing we
        // treat runner as None.
        let runner = match (row.runner_id, row.runner_name) {
            (Some(id), Some(name)) => Some(RunnerInfo {
                id,
                name,
                group_name: row.runner_group_name,
            }),
            _ => None,
        };
        let steps: Vec<Step> =
            serde_json::from_value(row.steps).map_err(|e| PersistError::Backend(Box::new(e)))?;
        jobs.push(Job {
            id: JobId(row.id),
            name: row.name,
            run_id: RunId(row.run_id),
            status,
            conclusion,
            runner,
            labels: row.labels,
            steps,
            created_at: row.created_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            run_attempt: row.run_attempt,
        });
    }
    Ok(jobs)
}

/// Every distinct `repo_id` any *real* run has ever recorded, read directly
/// off the pool (no transaction — this isn't part of `read_snapshot`'s
/// REPEATABLE READ contract, just a narrow projection).
///
/// Deliberately does not go through [`read_all_runs`]: that projects every
/// run column, which is the wide read `PublicRepoCache::refresh` used to do
/// just to throw away everything but `repo_id`. It does, however, keep
/// `read_all_runs`'s `placeholder = false` filter: a job-before-run FK stub
/// (see `upsert_job_in_txn`) carries a real `repo_id` but no real run data,
/// and `atc-store-mem` never creates an equivalent stub — including it here
/// would both leak a repo with zero actual runs into the public-repo check
/// and diverge from the in-memory backend for the same webhook sequence.
///
/// This is the first query to put `repo_id` in a `WHERE`/aggregation clause
/// — migration `0011_runs_repo_id.sql`'s "no index" comment predates this
/// caller. `EXPLAIN ANALYZE` against production (~10k runs, 12 distinct
/// `repo_id`s) measured a plain Seq Scan + HashAggregate at ~7ms, already
/// negligible next to the GitHub API round trips `PublicRepoCache::refresh`
/// makes with the result.
// ponytail: no index on runs.repo_id — add one (a partial btree, `WHERE
// repo_id IS NOT NULL`) only if `runs` grows large enough that this scan
// stops being noise.
pub(crate) async fn distinct_repo_ids(pool: &TracedPool) -> Result<HashSet<RepoId>, PersistError> {
    let rows = sqlx::query!(
        "SELECT DISTINCT repo_id FROM runs WHERE repo_id IS NOT NULL AND placeholder = false"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.repo_id.map(RepoId))
        .collect())
}

#[cfg(test)]
mod tests {
    use atc_core::{JobStatus, RunStatus};

    use super::*;

    #[test]
    fn parse_run_status_valid_variants() {
        assert!(matches!(parse_run_status("Queued"), Ok(RunStatus::Queued)));
        assert!(matches!(
            parse_run_status("InProgress"),
            Ok(RunStatus::InProgress)
        ));
        assert!(matches!(
            parse_run_status("Completed"),
            Ok(RunStatus::Completed)
        ));
    }

    #[test]
    fn parse_run_status_unknown_returns_backend_error() {
        let result = parse_run_status("Frobnicated");
        assert!(
            matches!(result, Err(PersistError::Backend(_))),
            "expected Backend error, got: {result:?}"
        );
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(
            err_str.contains("Frobnicated"),
            "error should mention the bad input; got {err_str}"
        );
    }

    #[test]
    fn parse_job_status_valid_variants() {
        assert!(matches!(parse_job_status("Queued"), Ok(JobStatus::Queued)));
        assert!(matches!(
            parse_job_status("Waiting"),
            Ok(JobStatus::Waiting)
        ));
        assert!(matches!(
            parse_job_status("InProgress"),
            Ok(JobStatus::InProgress)
        ));
        assert!(matches!(
            parse_job_status("Completed"),
            Ok(JobStatus::Completed)
        ));
    }

    #[test]
    fn parse_job_status_unknown_returns_backend_error() {
        let result = parse_job_status("Obliterated");
        assert!(
            matches!(result, Err(PersistError::Backend(_))),
            "expected Backend error, got: {result:?}"
        );
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(
            err_str.contains("Obliterated"),
            "error should mention the bad input; got {err_str}"
        );
    }

    #[test]
    fn parse_run_conclusion_valid_variants() {
        for s in [
            "Success",
            "Failure",
            "Cancelled",
            "TimedOut",
            "ActionRequired",
            "Stale",
            "Neutral",
            "Skipped",
            "StartupFailure",
        ] {
            assert!(parse_run_conclusion(s).is_ok(), "expected Ok for {s:?}");
        }
    }

    #[test]
    fn parse_run_conclusion_unknown_returns_backend_error() {
        let result = parse_run_conclusion("Exploded");
        assert!(
            matches!(result, Err(PersistError::Backend(_))),
            "expected Backend error, got: {result:?}"
        );
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(
            err_str.contains("Exploded"),
            "error should mention the bad input; got {err_str}"
        );
    }

    #[test]
    fn parse_job_conclusion_valid_variants() {
        for s in [
            "Success",
            "Failure",
            "Cancelled",
            "TimedOut",
            "ActionRequired",
            "Stale",
            "Neutral",
            "Skipped",
        ] {
            assert!(parse_job_conclusion(s).is_ok(), "expected Ok for {s:?}");
        }
    }

    #[test]
    fn parse_job_conclusion_unknown_returns_backend_error() {
        let result = parse_job_conclusion("Vaporized");
        assert!(
            matches!(result, Err(PersistError::Backend(_))),
            "expected Backend error, got: {result:?}"
        );
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(
            err_str.contains("Vaporized"),
            "error should mention the bad input; got {err_str}"
        );
    }
}
