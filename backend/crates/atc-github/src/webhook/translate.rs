//! Translation from GitHub webhook payload types to `atc-core` domain events.

use atc_core::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use atc_core::job::{JobConclusion, RunnerInfo, Step, StepStatus};
use atc_core::run::RunConclusion;
use atc_core::types::{JobId, RunId};

use super::types::{WorkflowJobWebhook, WorkflowRunWebhook};
use super::ParseError;

/// Translate a deserialized `workflow_run` webhook into a [`RunEventEnvelope`].
pub(crate) fn translate_run(
    webhook: WorkflowRunWebhook,
) -> Result<RunEventEnvelope, ParseError> {
    let action = match webhook.action.as_str() {
        "requested" => RunEvent::Requested,
        "in_progress" => RunEvent::InProgress,
        "completed" => {
            let conclusion_str =
                webhook.workflow_run.conclusion.as_deref().ok_or_else(|| {
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
        action,
    })
}

/// Translate a deserialized `workflow_job` webhook into a [`JobEventEnvelope`].
pub(crate) fn translate_job(
    webhook: WorkflowJobWebhook,
) -> Result<JobEventEnvelope, ParseError> {
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
                job.conclusion.as_deref().ok_or_else(|| {
                    ParseError::MissingConclusion {
                        event_type: "workflow_job".into(),
                        action: "completed".into(),
                    }
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
fn make_runner_info(
    job: &super::types::WorkflowJobData,
) -> Option<RunnerInfo> {
    let id = job.runner_id?;
    let name = job.runner_name.clone()?;
    Some(RunnerInfo {
        id,
        name,
        group_id: job.runner_group_id,
        group_name: job.runner_group_name.clone(),
    })
}

/// Translate step data from GitHub format to domain [`Step`] types.
fn translate_steps(
    steps: &[super::types::StepData],
) -> Result<Vec<Step>, ParseError> {
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

fn parse_step_conclusion(
    s: &str,
    step_name: &str,
) -> Result<JobConclusion, ParseError> {
    // Step conclusions use the same values as job conclusions.
    parse_job_conclusion(s).map_err(|_| ParseError::UnknownConclusion {
        event_type: format!("step '{step_name}'"),
        value: s.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // ===== Test helpers =====

    /// Create a minimal `WorkflowRunWebhook` with sensible defaults.
    fn make_workflow_run_webhook(
        action: &str,
        conclusion: Option<&str>,
    ) -> WorkflowRunWebhook {
        use super::super::types::{HeadCommit, OwnerData, RepositoryData, WorkflowData, WorkflowRunData};

        let workflow_run = WorkflowRunData {
            id: 123_456,
            status: "completed".to_string(),
            conclusion: conclusion.map(std::string::ToString::to_string),
            head_branch: Some("main".to_string()),
            head_sha: "abc123def456".to_string(),
            head_commit: Some(HeadCommit {
                message: "Test commit".to_string(),
            }),
            event: "push".to_string(),
            display_title: "Test Workflow".to_string(),
            html_url: "https://github.com/test/repo/runs/123456".to_string(),
            created_at: Utc::now(),
            run_started_at: Some(Utc::now()),
            updated_at: Utc::now(),
        };

        WorkflowRunWebhook {
            action: action.to_string(),
            workflow_run,
            workflow: Some(WorkflowData {
                name: "CI Workflow".to_string(),
                path: ".github/workflows/ci.yml".to_string(),
            }),
            repository: RepositoryData {
                owner: OwnerData {
                    login: "test-org".to_string(),
                },
                name: "test-repo".to_string(),
            },
        }
    }

    /// Create a minimal `WorkflowJobWebhook` with sensible defaults.
    fn make_workflow_job_webhook(
        action: &str,
        conclusion: Option<&str>,
        runner: bool,
    ) -> WorkflowJobWebhook {
        use super::super::types::{OwnerData, RepositoryData, WorkflowJobData};

        let workflow_job = WorkflowJobData {
            id: 987_654,
            run_id: 123_456,
            name: "test-job".to_string(),
            status: "completed".to_string(),
            conclusion: conclusion.map(std::string::ToString::to_string),
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            completed_at: Some(Utc::now()),
            steps: vec![],
            labels: vec!["ubuntu-latest".to_string()],
            runner_id: if runner { Some(1) } else { None },
            runner_name: if runner { Some("runner-1".to_string()) } else { None },
            runner_group_id: if runner { Some(100) } else { None },
            runner_group_name: if runner { Some("self-hosted".to_string()) } else { None },
        };

        WorkflowJobWebhook {
            action: action.to_string(),
            workflow_job,
            repository: RepositoryData {
                owner: OwnerData {
                    login: "test-org".to_string(),
                },
                name: "test-repo".to_string(),
            },
        }
    }

    // ===== Run event translation tests (AC2.1–AC2.4, AC2.11) =====

    #[test]
    fn test_translate_run_requested() {
        // AC2.1: Translate a `workflow_run` webhook with `action: "requested"`
        let webhook = make_workflow_run_webhook("requested", None);
        let result = translate_run(webhook).expect("should translate");

        assert_eq!(result.action, RunEvent::Requested);
        assert_eq!(result.org, "test-org");
        assert_eq!(result.repo, "test-repo");
        assert_eq!(result.run_id, RunId(123_456));
        assert_eq!(result.head_sha, "abc123def456");
        assert_eq!(result.workflow_name, Some("CI Workflow".to_string()));
        assert_eq!(result.workflow_path, Some(".github/workflows/ci.yml".to_string()));
    }

    #[test]
    fn test_translate_run_in_progress() {
        // AC2.2: Translate with `action: "in_progress"`
        let webhook = make_workflow_run_webhook("in_progress", None);
        let result = translate_run(webhook).expect("should translate");

        assert_eq!(result.action, RunEvent::InProgress);
    }

    #[test]
    fn test_translate_run_completed_success() {
        // AC2.3: Translate with `action: "completed"` and `conclusion: Some("success")`
        let webhook = make_workflow_run_webhook("completed", Some("success"));
        let result = translate_run(webhook).expect("should translate");

        match result.action {
            RunEvent::Completed { conclusion } => {
                assert_eq!(conclusion, RunConclusion::Success);
            }
            _ => panic!("expected Completed variant"),
        }
    }

    #[test]
    fn test_translate_run_all_conclusions() {
        // AC2.4: Test all 9 RunConclusion variants
        let conclusions = vec![
            ("success", RunConclusion::Success),
            ("failure", RunConclusion::Failure),
            ("cancelled", RunConclusion::Cancelled),
            ("timed_out", RunConclusion::TimedOut),
            ("action_required", RunConclusion::ActionRequired),
            ("stale", RunConclusion::Stale),
            ("neutral", RunConclusion::Neutral),
            ("skipped", RunConclusion::Skipped),
            ("startup_failure", RunConclusion::StartupFailure),
        ];

        for (conclusion_str, expected) in conclusions {
            let webhook = make_workflow_run_webhook("completed", Some(conclusion_str));
            let result = translate_run(webhook).expect("should translate");

            match result.action {
                RunEvent::Completed { conclusion } => {
                    assert_eq!(conclusion, expected, "failed for {conclusion_str}");
                }
                _ => panic!("expected Completed variant for {conclusion_str}"),
            }
        }
    }

    #[test]
    fn test_translate_run_with_null_workflow() {
        // AC2.11: Test with `workflow: None` and verify fields are `None`
        let mut webhook = make_workflow_run_webhook("requested", None);
        webhook.workflow = None;
        let result = translate_run(webhook).expect("should translate");

        assert_eq!(result.workflow_name, None);
        assert_eq!(result.workflow_path, None);
    }

    // ===== Job event translation tests (AC2.5–AC2.10) =====

    #[test]
    fn test_translate_job_queued() {
        // AC2.5: Translate `workflow_job` with `action: "queued"`
        let webhook = make_workflow_job_webhook("queued", None, false);
        let result = translate_job(webhook).expect("should translate");

        match result.action {
            JobEvent::Queued { labels, steps } => {
                assert_eq!(labels, vec!["ubuntu-latest"]);
                assert!(steps.is_empty());
            }
            _ => panic!("expected Queued variant"),
        }
    }

    #[test]
    fn test_translate_job_waiting() {
        // AC2.6: Translate with `action: "waiting"`
        let webhook = make_workflow_job_webhook("waiting", None, false);
        let result = translate_job(webhook).expect("should translate");

        match result.action {
            JobEvent::Waiting { labels, steps } => {
                assert_eq!(labels, vec!["ubuntu-latest"]);
                assert!(steps.is_empty());
            }
            _ => panic!("expected Waiting variant"),
        }
    }

    #[test]
    fn test_translate_job_in_progress_with_runner() {
        // AC2.7: Translate with runner fields populated
        let webhook = make_workflow_job_webhook("in_progress", None, true);
        let result = translate_job(webhook).expect("should translate");

        match result.action {
            JobEvent::InProgress { runner, labels, steps } => {
                assert!(runner.is_some());
                let runner_info = runner.unwrap();
                assert_eq!(runner_info.id, 1);
                assert_eq!(runner_info.name, "runner-1");
                assert_eq!(labels, vec!["ubuntu-latest"]);
                assert!(steps.is_empty());
            }
            _ => panic!("expected InProgress variant"),
        }
    }

    #[test]
    fn test_translate_job_in_progress_no_runner() {
        // AC2.8: Translate with all runner fields `None`
        let webhook = make_workflow_job_webhook("in_progress", None, false);
        let result = translate_job(webhook).expect("should translate");

        match result.action {
            JobEvent::InProgress { runner, .. } => {
                assert!(runner.is_none());
            }
            _ => panic!("expected InProgress variant"),
        }
    }

    #[test]
    fn test_translate_job_completed() {
        // AC2.9: Translate with `action: "completed"` with optional runner, labels, and steps
        let webhook = make_workflow_job_webhook("completed", Some("success"), true);
        let result = translate_job(webhook).expect("should translate");

        match result.action {
            JobEvent::Completed { conclusion, runner, labels, steps } => {
                assert_eq!(conclusion, JobConclusion::Success);
                assert!(runner.is_some());
                assert_eq!(labels, vec!["ubuntu-latest"]);
                assert!(steps.is_empty());
            }
            _ => panic!("expected Completed variant"),
        }
    }

    #[test]
    fn test_translate_job_with_steps() {
        // AC2.10: Create a webhook with steps and verify status/conclusion translation
        use super::super::types::{OwnerData, RepositoryData, StepData, WorkflowJobData};

        let steps = vec![
            StepData {
                number: 1,
                name: "Setup".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                started_at: Some(Utc::now()),
                completed_at: Some(Utc::now()),
            },
            StepData {
                number: 2,
                name: "Build".to_string(),
                status: "in_progress".to_string(),
                conclusion: None,
                started_at: Some(Utc::now()),
                completed_at: None,
            },
        ];

        let webhook_job = WorkflowJobData {
            id: 987_654,
            run_id: 123_456,
            name: "test-job".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            completed_at: None,
            steps,
            labels: vec!["ubuntu-latest".to_string()],
            runner_id: None,
            runner_name: None,
            runner_group_id: None,
            runner_group_name: None,
        };

        let webhook = WorkflowJobWebhook {
            action: "in_progress".to_string(),
            workflow_job: webhook_job,
            repository: RepositoryData {
                owner: OwnerData {
                    login: "test-org".to_string(),
                },
                name: "test-repo".to_string(),
            },
        };

        let result = translate_job(webhook).expect("should translate");

        match result.action {
            JobEvent::InProgress { steps, .. } => {
                assert_eq!(steps.len(), 2);
                assert_eq!(steps[0].status, StepStatus::Completed);
                assert_eq!(steps[0].conclusion, Some(JobConclusion::Success));
                assert_eq!(steps[1].status, StepStatus::InProgress);
                assert_eq!(steps[1].conclusion, None);
            }
            _ => panic!("expected InProgress variant"),
        }
    }

    // ===== Error cases (AC2.12–AC2.15) =====

    #[test]
    fn test_unknown_action_workflow_run() {
        // AC2.12: Translate with `action: "unknown_action"`
        let webhook = make_workflow_run_webhook("unknown_action", None);
        let err = translate_run(webhook).expect_err("should error");

        assert!(matches!(
            err,
            ParseError::UnknownAction {
                event_type,
                action,
            } if event_type == "workflow_run" && action == "unknown_action"
        ));
    }

    #[test]
    fn test_unknown_action_workflow_job() {
        // AC2.12: Repeat for workflow_job
        let webhook = make_workflow_job_webhook("unknown_action", None, false);
        let err = translate_job(webhook).expect_err("should error");

        assert!(matches!(
            err,
            ParseError::UnknownAction {
                event_type,
                action,
            } if event_type == "workflow_job" && action == "unknown_action"
        ));
    }

    #[test]
    fn test_missing_conclusion_workflow_run() {
        // AC2.13: Translate a `completed` workflow_run with `conclusion: None`
        let webhook = make_workflow_run_webhook("completed", None);
        let err = translate_run(webhook).expect_err("should error");

        assert!(matches!(
            err,
            ParseError::MissingConclusion {
                event_type,
                action,
            } if event_type == "workflow_run" && action == "completed"
        ));
    }

    #[test]
    fn test_missing_conclusion_workflow_job() {
        // AC2.13: Repeat for workflow_job
        let webhook = make_workflow_job_webhook("completed", None, false);
        let err = translate_job(webhook).expect_err("should error");

        assert!(matches!(
            err,
            ParseError::MissingConclusion {
                event_type,
                action,
            } if event_type == "workflow_job" && action == "completed"
        ));
    }

    #[test]
    fn test_unknown_conclusion_workflow_run() {
        // AC2.14: Translate with `conclusion: Some("bogus")`
        let webhook = make_workflow_run_webhook("completed", Some("bogus"));
        let err = translate_run(webhook).expect_err("should error");

        assert!(matches!(
            err,
            ParseError::UnknownConclusion {
                event_type,
                value,
            } if event_type == "workflow_run" && value == "bogus"
        ));
    }

    #[test]
    fn test_unknown_conclusion_workflow_job() {
        // AC2.14: Repeat for workflow_job
        let webhook = make_workflow_job_webhook("completed", Some("bogus"), false);
        let err = translate_job(webhook).expect_err("should error");

        assert!(matches!(
            err,
            ParseError::UnknownConclusion {
                event_type,
                value,
            } if event_type == "workflow_job" && value == "bogus"
        ));
    }

    #[test]
    fn test_unknown_step_status() {
        // AC2.15: Create a webhook with a step that has `status: "bogus"`
        use super::super::types::{OwnerData, RepositoryData, StepData, WorkflowJobData};

        let steps = vec![StepData {
            number: 1,
            name: "Setup".to_string(),
            status: "bogus".to_string(),
            conclusion: None,
            started_at: None,
            completed_at: None,
        }];

        let webhook_job = WorkflowJobData {
            id: 987_654,
            run_id: 123_456,
            name: "test-job".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            completed_at: None,
            steps,
            labels: vec![],
            runner_id: None,
            runner_name: None,
            runner_group_id: None,
            runner_group_name: None,
        };

        let webhook = WorkflowJobWebhook {
            action: "in_progress".to_string(),
            workflow_job: webhook_job,
            repository: RepositoryData {
                owner: OwnerData {
                    login: "test-org".to_string(),
                },
                name: "test-repo".to_string(),
            },
        };

        let err = translate_job(webhook).expect_err("should error");

        assert!(matches!(
            err,
            ParseError::UnknownStatus {
                context,
                value,
            } if context == "step 'Setup'" && value == "bogus"
        ));
    }
}
