use super::*;
use atc_core::fixed_test_timestamp;

use crate::webhook::types::{
    HeadCommit, OwnerData, RepositoryData, StepData, WorkflowData, WorkflowJobData, WorkflowRunData,
};

// ===== Test helpers =====

/// Baseline `WorkflowRunData` with sensible defaults.
///
/// Override specific fields with struct-update syntax:
///
/// ```rust
/// let run = WorkflowRunData {
///     run_attempt: 2,
///     ..default_run_data(Some("cancelled"))
/// };
/// ```
fn default_run_data(conclusion: Option<&str>) -> WorkflowRunData {
    WorkflowRunData {
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
        created_at: fixed_test_timestamp(),
        run_started_at: Some(fixed_test_timestamp()),
        updated_at: fixed_test_timestamp(),
        run_attempt: 1,
    }
}

/// Create a minimal `WorkflowRunWebhook` with sensible defaults.
fn make_workflow_run_webhook(action: &str, conclusion: Option<&str>) -> WorkflowRunWebhook {
    let workflow_run = default_run_data(conclusion);

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
    let workflow_job = WorkflowJobData {
        id: 987_654,
        run_id: 123_456,
        name: "test-job".to_string(),
        status: "completed".to_string(),
        conclusion: conclusion.map(std::string::ToString::to_string),
        created_at: fixed_test_timestamp(),
        started_at: Some(fixed_test_timestamp()),
        completed_at: Some(fixed_test_timestamp()),
        steps: vec![],
        labels: vec!["ubuntu-latest".to_string()],
        runner_id: if runner { Some(1) } else { None },
        runner_name: if runner {
            Some("runner-1".to_string())
        } else {
            None
        },
        runner_group_name: if runner {
            Some("self-hosted".to_string())
        } else {
            None
        },
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

// ===== Run event translation tests =====

#[test]
fn test_translate_run_requested() {
    let webhook = make_workflow_run_webhook("requested", None);
    let result = translate_run(webhook).expect("should translate");

    assert_eq!(result.action, RunEvent::Requested);
    assert_eq!(result.org, "test-org");
    assert_eq!(result.repo, "test-repo");
    assert_eq!(result.run_id, RunId(123_456));
    assert_eq!(result.head_sha, "abc123def456");
    assert_eq!(result.workflow_name, Some("CI Workflow".to_string()));
    assert_eq!(
        result.workflow_path,
        Some(".github/workflows/ci.yml".to_string())
    );
}

#[test]
fn test_translate_run_in_progress() {
    let webhook = make_workflow_run_webhook("in_progress", None);
    let result = translate_run(webhook).expect("should translate");

    assert_eq!(result.action, RunEvent::InProgress);
}

#[test]
fn test_translate_run_completed_success() {
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
    let mut webhook = make_workflow_run_webhook("requested", None);
    webhook.workflow = None;
    let result = translate_run(webhook).expect("should translate");

    assert_eq!(result.workflow_name, None);
    assert_eq!(result.workflow_path, None);
}

#[test]
fn test_translate_run_completed_populates_completed_at_from_updated_at() {
    // GitHub does not surface a dedicated `completed_at` on workflow_run;
    // `updated_at` is the best-available proxy at the moment the run
    // transitions to `completed`.
    let webhook = make_workflow_run_webhook("completed", Some("success"));
    let expected_updated_at = webhook.workflow_run.updated_at;
    let result = translate_run(webhook).expect("should translate");

    assert_eq!(result.completed_at, Some(expected_updated_at));
}

#[test]
fn test_translate_run_non_completed_actions_leave_completed_at_none() {
    for action in ["requested", "in_progress"] {
        let webhook = make_workflow_run_webhook(action, None);
        let result = translate_run(webhook).expect("should translate");
        assert_eq!(
            result.completed_at, None,
            "non-completed action {action} must not stamp completed_at"
        );
    }
}

// ===== Job event translation tests =====

#[test]
fn test_translate_job_queued() {
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
    let webhook = make_workflow_job_webhook("in_progress", None, true);
    let result = translate_job(webhook).expect("should translate");

    match result.action {
        JobEvent::InProgress {
            runner,
            labels,
            steps,
        } => {
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
    let webhook = make_workflow_job_webhook("completed", Some("success"), true);
    let result = translate_job(webhook).expect("should translate");

    match result.action {
        JobEvent::Completed {
            conclusion,
            runner,
            labels,
            steps,
        } => {
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
    let steps = vec![
        StepData {
            number: 1,
            name: "Setup".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            started_at: Some(fixed_test_timestamp()),
            completed_at: Some(fixed_test_timestamp()),
        },
        StepData {
            number: 2,
            name: "Build".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            started_at: Some(fixed_test_timestamp()),
            completed_at: None,
        },
    ];

    let webhook_job = WorkflowJobData {
        id: 987_654,
        run_id: 123_456,
        name: "test-job".to_string(),
        status: "in_progress".to_string(),
        conclusion: None,
        created_at: fixed_test_timestamp(),
        started_at: Some(fixed_test_timestamp()),
        completed_at: None,
        steps,
        labels: vec!["ubuntu-latest".to_string()],
        runner_id: None,
        runner_name: None,
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

// ===== Error cases =====

#[test]
fn test_unknown_action_workflow_run() {
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
        created_at: fixed_test_timestamp(),
        started_at: Some(fixed_test_timestamp()),
        completed_at: None,
        steps,
        labels: vec![],
        runner_id: None,
        runner_name: None,
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

#[test]
fn make_runner_info_normalizes_empty_runner_group_name_to_none() {
    let workflow_job = WorkflowJobData {
        id: 987_654,
        run_id: 123_456,
        name: "test-job".to_string(),
        status: "in_progress".to_string(),
        conclusion: None,
        created_at: fixed_test_timestamp(),
        started_at: Some(fixed_test_timestamp()),
        completed_at: None,
        steps: vec![],
        labels: vec!["ubuntu-latest".to_string()],
        runner_id: Some(42),
        runner_name: Some("runner-42".to_string()),
        runner_group_name: Some(String::new()),
    };

    let result = make_runner_info(&workflow_job);

    assert!(result.is_some());
    let info = result.unwrap();
    assert_eq!(info.id, 42);
    assert_eq!(info.name, "runner-42");
    assert_eq!(info.group_name, None);
}
