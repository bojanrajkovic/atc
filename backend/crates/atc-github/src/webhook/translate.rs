//! Translation from GitHub webhook payload types to `atc-core` domain events.

use atc_core::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use atc_core::job::{JobConclusion, RunnerInfo, Step, StepStatus};
use atc_core::run::RunConclusion;
use atc_core::types::{JobId, RunId};

use super::ParseError;
use super::types::{WorkflowJobWebhook, WorkflowRunWebhook};

/// Translate a deserialized `workflow_run` webhook into a [`RunEventEnvelope`].
pub(crate) fn translate_run(webhook: WorkflowRunWebhook) -> Result<RunEventEnvelope, ParseError> {
    let action = match webhook.action.as_str() {
        "requested" => RunEvent::Requested,
        "in_progress" => RunEvent::InProgress,
        "completed" => {
            let conclusion_str = webhook.workflow_run.conclusion.as_deref().ok_or_else(|| {
                ParseError::MissingConclusion {
                    event_type: "workflow_run".into(),
                    action: "completed".into(),
                }
            })?;
            let conclusion = parse_run_conclusion(conclusion_str)?;
            RunEvent::Completed { conclusion }
        }
        other => {
            return Err(ParseError::UnknownAction {
                event_type: "workflow_run".into(),
                action: other.into(),
            });
        }
    };

    // GitHub's `workflow_run` payload has no dedicated `completed_at` (unlike
    // `workflow_job`); on the `completed` action, `updated_at` is the
    // best-available proxy because GitHub writes the row at the moment the
    // run completes. For other actions the field stays `None` so the
    // `apply_run_event` preserve-first semantics (`.or(existing)`) leave
    // any prior `completed_at` alone — important under out-of-order
    // delivery where a late non-completed event must not clobber an
    // already-recorded terminal timestamp.
    let completed_at =
        matches!(action, RunEvent::Completed { .. }).then_some(webhook.workflow_run.updated_at);

    Ok(RunEventEnvelope {
        run_id: RunId(webhook.workflow_run.id),
        org: webhook.repository.owner.login,
        repo: webhook.repository.name,
        workflow_name: webhook.workflow.as_ref().map(|w| w.name.clone()),
        workflow_path: webhook.workflow.as_ref().map(|w| w.path.clone()),
        branch: webhook.workflow_run.head_branch,
        head_sha: webhook.workflow_run.head_sha,
        commit_message: webhook.workflow_run.head_commit.map(|c| c.message),
        trigger_event: webhook.workflow_run.event,
        display_title: webhook.workflow_run.display_title,
        html_url: webhook.workflow_run.html_url,
        created_at: webhook.workflow_run.created_at,
        run_started_at: webhook.workflow_run.run_started_at,
        updated_at: webhook.workflow_run.updated_at,
        completed_at,
        action,
    })
}

/// Translate a deserialized `workflow_job` webhook into a [`JobEventEnvelope`].
pub(crate) fn translate_job(webhook: WorkflowJobWebhook) -> Result<JobEventEnvelope, ParseError> {
    let job = webhook.workflow_job;
    let steps = translate_steps(&job.steps)?;
    let runner = make_runner_info(&job);
    let labels = job.labels;

    let action = match webhook.action.as_str() {
        "queued" => JobEvent::Queued { labels, steps },
        "waiting" => JobEvent::Waiting { labels, steps },
        "in_progress" => JobEvent::InProgress {
            runner,
            labels,
            steps,
        },
        "completed" => {
            let conclusion_str =
                job.conclusion
                    .as_deref()
                    .ok_or_else(|| ParseError::MissingConclusion {
                        event_type: "workflow_job".into(),
                        action: "completed".into(),
                    })?;
            let conclusion = parse_job_conclusion(conclusion_str)?;
            JobEvent::Completed {
                conclusion,
                runner,
                labels,
                steps,
            }
        }
        other => {
            return Err(ParseError::UnknownAction {
                event_type: "workflow_job".into(),
                action: other.into(),
            });
        }
    };

    Ok(JobEventEnvelope {
        job_id: JobId(job.id),
        run_id: RunId(job.run_id),
        org: webhook.repository.owner.login,
        repo: webhook.repository.name,
        name: job.name,
        created_at: job.created_at,
        started_at: job.started_at,
        completed_at: job.completed_at,
        action,
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Construct [`RunnerInfo`] from nullable job fields.
///
/// Returns `None` if `runner_id` or `runner_name` is missing (GitHub sends
/// null runners on early `in_progress` events before assignment completes).
fn make_runner_info(job: &super::types::WorkflowJobData) -> Option<RunnerInfo> {
    let id = job.runner_id?;
    let name = job.runner_name.clone()?;
    Some(RunnerInfo {
        id,
        name,
        group_name: job.runner_group_name.clone().filter(|s| !s.is_empty()),
    })
}

/// Translate step data from GitHub format to domain [`Step`] types.
fn translate_steps(steps: &[super::types::StepData]) -> Result<Vec<Step>, ParseError> {
    steps
        .iter()
        .map(|s| {
            let status = parse_step_status(&s.status, &s.name)?;
            let conclusion = s
                .conclusion
                .as_deref()
                .map(|c| parse_step_conclusion(c, &s.name))
                .transpose()?;
            Ok(Step {
                number: i64::from(s.number),
                name: s.name.clone(),
                status,
                conclusion,
                started_at: s.started_at,
                completed_at: s.completed_at,
            })
        })
        .collect()
}

fn parse_run_conclusion(s: &str) -> Result<RunConclusion, ParseError> {
    match s {
        "success" => Ok(RunConclusion::Success),
        "failure" => Ok(RunConclusion::Failure),
        "cancelled" => Ok(RunConclusion::Cancelled),
        "timed_out" => Ok(RunConclusion::TimedOut),
        "action_required" => Ok(RunConclusion::ActionRequired),
        "stale" => Ok(RunConclusion::Stale),
        "neutral" => Ok(RunConclusion::Neutral),
        "skipped" => Ok(RunConclusion::Skipped),
        "startup_failure" => Ok(RunConclusion::StartupFailure),
        other => Err(ParseError::UnknownConclusion {
            event_type: "workflow_run".into(),
            value: other.into(),
        }),
    }
}

fn parse_job_conclusion(s: &str) -> Result<JobConclusion, ParseError> {
    match s {
        "success" => Ok(JobConclusion::Success),
        "failure" => Ok(JobConclusion::Failure),
        "cancelled" => Ok(JobConclusion::Cancelled),
        "timed_out" => Ok(JobConclusion::TimedOut),
        "action_required" => Ok(JobConclusion::ActionRequired),
        "stale" => Ok(JobConclusion::Stale),
        "neutral" => Ok(JobConclusion::Neutral),
        "skipped" => Ok(JobConclusion::Skipped),
        other => Err(ParseError::UnknownConclusion {
            event_type: "workflow_job".into(),
            value: other.into(),
        }),
    }
}

fn parse_step_status(s: &str, step_name: &str) -> Result<StepStatus, ParseError> {
    match s {
        "queued" => Ok(StepStatus::Queued),
        "in_progress" => Ok(StepStatus::InProgress),
        "completed" => Ok(StepStatus::Completed),
        other => Err(ParseError::UnknownStatus {
            context: format!("step '{step_name}'"),
            value: other.into(),
        }),
    }
}

fn parse_step_conclusion(s: &str, step_name: &str) -> Result<JobConclusion, ParseError> {
    // Step conclusions use the same values as job conclusions.
    parse_job_conclusion(s).map_err(|_| ParseError::UnknownConclusion {
        event_type: format!("step '{step_name}'"),
        value: s.into(),
    })
}

#[cfg(test)]
mod tests;
