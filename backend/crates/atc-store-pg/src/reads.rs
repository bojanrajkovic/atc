//! Read-side helpers for PG state snapshot (`/v1/state`).
//!
//! These free functions read all runs and jobs from an open transaction.
//! They are called by `PgStore::read_snapshot` (Phase 3) and by the
//! current `routes::state_handler` PG path (until that phase lands).

use atc_core::{
    Job, JobConclusion, JobId, JobStatus, PersistError, RunConclusion, RunId, RunStatus,
    RunnerInfo, Step, WorkflowRun,
};

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
#[allow(dead_code)]
pub(crate) async fn read_all_runs(
    tx: &mut sqlx_tracing::Transaction<'_, sqlx::Postgres>,
) -> Result<Vec<WorkflowRun>, PersistError> {
    let rows = sqlx::query!(
        r#"
        SELECT id, org, repo, workflow_name, workflow_path, branch, head_sha,
               commit_message, event, display_title, status, conclusion,
               html_url, created_at, run_started_at, updated_at
          FROM runs
         WHERE placeholder = false
         ORDER BY id
        "#,
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
        });
    }
    Ok(runs)
}

/// Read all jobs ordered by id, reconstructing `RunnerInfo` and `steps`.
#[allow(dead_code)]
pub(crate) async fn read_all_jobs(
    tx: &mut sqlx_tracing::Transaction<'_, sqlx::Postgres>,
) -> Result<Vec<Job>, PersistError> {
    let rows = sqlx::query!(
        r#"
        SELECT id, run_id, name, status, conclusion,
               runner_id, runner_name, runner_group_name,
               labels, steps, created_at, started_at, completed_at
          FROM jobs
         ORDER BY id
        "#,
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
        });
    }
    Ok(jobs)
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
