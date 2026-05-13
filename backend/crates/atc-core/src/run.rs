//! Workflow run types and status enums.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::{RepoKey, RunId};

/// Status of a workflow run in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "PascalCase")]
#[ts(export)]
pub enum RunStatus {
    /// Run is waiting to be processed.
    Queued,
    /// Run is currently executing.
    InProgress,
    /// Run has finished executing.
    Completed,
}

/// Conclusion of a completed workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "PascalCase")]
#[ts(export)]
pub enum RunConclusion {
    /// All jobs succeeded.
    Success,
    /// One or more jobs failed.
    Failure,
    /// Run was cancelled.
    Cancelled,
    /// Run exceeded time limit.
    TimedOut,
    /// Run requires manual intervention.
    ActionRequired,
    /// Run became stale.
    Stale,
    /// Run completed with neutral result.
    Neutral,
    /// Run was skipped.
    Skipped,
    /// Run failed during startup.
    StartupFailure,
}

/// A workflow run in the ATC domain model.
///
/// Top-level container that groups related jobs. Created and updated
/// by `RunEvent`s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkflowRun {
    /// Unique identifier for this run.
    pub id: RunId,
    /// GitHub organization or user name.
    pub org: String,
    /// Repository name.
    pub repo: String,
    /// Name of the workflow. `None` until an event supplies it; once set, preserved via `.or()` across subsequent events.
    pub workflow_name: Option<String>,
    /// Path to the workflow file. `None` until an event supplies it; once set, preserved via `.or()` across subsequent events.
    pub workflow_path: Option<String>,
    /// Branch that triggered the run, if applicable.
    pub branch: Option<String>,
    /// Git commit SHA at the head of the run.
    pub head_sha: String,
    /// Commit message, if available.
    pub commit_message: Option<String>,
    /// Event that triggered the run (e.g., `push`, `pull_request`).
    pub event: String,
    /// Human-readable display title for the run.
    pub display_title: String,
    /// Current lifecycle status.
    pub status: RunStatus,
    /// Final conclusion, populated when status is `Completed`.
    pub conclusion: Option<RunConclusion>,
    /// URL to the run on GitHub.
    pub html_url: String,
    /// When the run was created.
    pub created_at: DateTime<Utc>,
    /// When the run started executing, if it has started.
    pub run_started_at: Option<DateTime<Utc>>,
    /// When the run was last updated.
    pub updated_at: DateTime<Utc>,
}

impl WorkflowRun {
    /// Returns the repository key for this run.
    #[must_use]
    pub fn repo_key(&self) -> RepoKey {
        RepoKey::new(self.org.clone(), self.repo.clone())
    }
}

use std::fmt;

/// Error returned when an invalid run status transition is attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidRunTransition {
    /// The current status.
    pub from: RunStatus,
    /// The attempted target status.
    pub to: RunStatus,
}

impl fmt::Display for InvalidRunTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid run transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for InvalidRunTransition {}

impl RunStatus {
    /// Attempt to transition to the target status.
    ///
    /// Returns the new status on success, or `InvalidRunTransition` if
    /// the transition is not allowed. Same-status transitions are
    /// idempotent and always succeed.
    ///
    /// # Valid transitions
    ///
    /// - `Queued` -> `InProgress` | `Completed`
    /// - `InProgress` -> `Completed`
    ///
    /// # Errors
    ///
    /// Returns `InvalidRunTransition` for any transition not listed above
    /// (excluding idempotent same-status).
    pub fn transition_to(self, target: Self) -> Result<Self, InvalidRunTransition> {
        if self == target {
            return Ok(self);
        }
        match (self, target) {
            (Self::Queued, Self::InProgress | Self::Completed)
            | (Self::InProgress, Self::Completed) => Ok(target),
            _ => Err(InvalidRunTransition {
                from: self,
                to: target,
            }),
        }
    }

    /// Returns the set of statuses that can validly transition to `target`,
    /// inclusive of `target` itself (so same-status replay is admitted).
    /// Consistent with `transition_to` — verified by property test.
    #[must_use]
    pub fn predecessors_of(target: Self) -> &'static [Self] {
        match target {
            Self::Queued => &[Self::Queued],
            Self::InProgress => &[Self::Queued, Self::InProgress],
            Self::Completed => &[Self::Queued, Self::InProgress, Self::Completed],
        }
    }
}

#[cfg(test)]
mod arb {
    use super::RunStatus;
    use proptest::prelude::*;

    impl Arbitrary for RunStatus {
        type Parameters = ();
        type Strategy = proptest::strategy::BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            prop_oneof![
                Just(RunStatus::Queued),
                Just(RunStatus::InProgress),
                Just(RunStatus::Completed),
            ]
            .boxed()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::fixed_test_timestamp;
    use proptest::prelude::*;

    #[test]
    fn test_workflow_run_fields() {
        let now = fixed_test_timestamp();
        let run = WorkflowRun {
            id: RunId(123),
            org: "octocat".to_string(),
            repo: "Hello-World".to_string(),
            workflow_name: Some("CI".to_string()),
            workflow_path: Some(".github/workflows/ci.yml".to_string()),
            branch: Some("main".to_string()),
            head_sha: "abc123def456".to_string(),
            commit_message: Some("Fix bug".to_string()),
            event: "push".to_string(),
            display_title: "Triggered by push".to_string(),
            status: RunStatus::InProgress,
            conclusion: None,
            html_url: "https://github.com/octocat/Hello-World/actions/runs/123".to_string(),
            created_at: now,
            run_started_at: Some(now),
            updated_at: now,
        };

        assert_eq!(run.id, RunId(123));
        assert_eq!(run.org, "octocat");
        assert_eq!(run.repo, "Hello-World");
        assert_eq!(run.workflow_name, Some("CI".to_string()));
        assert_eq!(
            run.workflow_path,
            Some(".github/workflows/ci.yml".to_string())
        );
        assert_eq!(run.branch, Some("main".to_string()));
        assert_eq!(run.head_sha, "abc123def456");
        assert_eq!(run.commit_message, Some("Fix bug".to_string()));
        assert_eq!(run.event, "push");
        assert_eq!(run.display_title, "Triggered by push");
        assert_eq!(run.status, RunStatus::InProgress);
        assert_eq!(run.conclusion, None);
        assert_eq!(
            run.html_url,
            "https://github.com/octocat/Hello-World/actions/runs/123"
        );
        assert_eq!(run.created_at, now);
        assert_eq!(run.run_started_at, Some(now));
        assert_eq!(run.updated_at, now);
    }

    #[test]
    fn test_workflow_run_repo_key() {
        let now = fixed_test_timestamp();
        let run = WorkflowRun {
            id: RunId(456),
            org: "github".to_string(),
            repo: "example".to_string(),
            workflow_name: Some("Test".to_string()),
            workflow_path: Some(".github/workflows/test.yml".to_string()),
            branch: None,
            head_sha: "xyz789".to_string(),
            commit_message: None,
            event: "pull_request".to_string(),
            display_title: "PR".to_string(),
            status: RunStatus::Queued,
            conclusion: None,
            html_url: "https://github.com/github/example/actions/runs/456".to_string(),
            created_at: now,
            run_started_at: None,
            updated_at: now,
        };

        let repo_key = run.repo_key();
        assert_eq!(repo_key.org, "github");
        assert_eq!(repo_key.repo, "example");
    }

    #[test]
    fn test_run_status_serialization() {
        let queued_json = serde_json::to_string(&RunStatus::Queued).expect("serialize");
        assert_eq!(queued_json, "\"Queued\"");

        let in_progress_json = serde_json::to_string(&RunStatus::InProgress).expect("serialize");
        assert_eq!(in_progress_json, "\"InProgress\"");

        let completed_json = serde_json::to_string(&RunStatus::Completed).expect("serialize");
        assert_eq!(completed_json, "\"Completed\"");
    }

    #[test]
    fn test_run_conclusion_serialization() {
        let success_json = serde_json::to_string(&RunConclusion::Success).expect("serialize");
        assert_eq!(success_json, "\"Success\"");

        let failure_json = serde_json::to_string(&RunConclusion::Failure).expect("serialize");
        assert_eq!(failure_json, "\"Failure\"");

        let timed_out_json = serde_json::to_string(&RunConclusion::TimedOut).expect("serialize");
        assert_eq!(timed_out_json, "\"TimedOut\"");
    }

    #[test]
    fn test_workflow_run_round_trip_json() {
        let now = fixed_test_timestamp();
        let original = WorkflowRun {
            id: RunId(789),
            org: "myorg".to_string(),
            repo: "myrepo".to_string(),
            workflow_name: Some("Deploy".to_string()),
            workflow_path: Some(".github/workflows/deploy.yml".to_string()),
            branch: Some("release".to_string()),
            head_sha: "fedcba987654".to_string(),
            commit_message: Some("Release v1.0".to_string()),
            event: "push".to_string(),
            display_title: "Deploy to production".to_string(),
            status: RunStatus::Completed,
            conclusion: Some(RunConclusion::Success),
            html_url: "https://github.com/myorg/myrepo/actions/runs/789".to_string(),
            created_at: now,
            run_started_at: Some(now),
            updated_at: now,
        };

        let json_str = serde_json::to_string(&original).expect("serialize to JSON");
        let deserialized: WorkflowRun =
            serde_json::from_str(&json_str).expect("deserialize from JSON");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_workflow_run_with_optional_fields_none() {
        let now = fixed_test_timestamp();
        let run = WorkflowRun {
            id: RunId(999),
            org: "testorg".to_string(),
            repo: "testrepo".to_string(),
            workflow_name: Some("TestFlow".to_string()),
            workflow_path: Some(".github/workflows/test.yml".to_string()),
            branch: None,
            head_sha: "111222333444".to_string(),
            commit_message: None,
            event: "schedule".to_string(),
            display_title: "Scheduled run".to_string(),
            status: RunStatus::Queued,
            conclusion: None,
            html_url: "https://github.com/testorg/testrepo/actions/runs/999".to_string(),
            created_at: now,
            run_started_at: None,
            updated_at: now,
        };

        assert_eq!(run.branch, None);
        assert_eq!(run.commit_message, None);
        assert_eq!(run.conclusion, None);
        assert_eq!(run.run_started_at, None);
    }

    #[test]
    fn test_run_transition_queued_to_in_progress() {
        let result = RunStatus::Queued.transition_to(RunStatus::InProgress);
        assert_eq!(result, Ok(RunStatus::InProgress));
    }

    #[test]
    fn test_run_transition_in_progress_to_completed() {
        let result = RunStatus::InProgress.transition_to(RunStatus::Completed);
        assert_eq!(result, Ok(RunStatus::Completed));
    }

    #[test]
    fn test_run_transition_completed_to_in_progress_fails() {
        let result = RunStatus::Completed.transition_to(RunStatus::InProgress);
        assert_eq!(
            result,
            Err(InvalidRunTransition {
                from: RunStatus::Completed,
                to: RunStatus::InProgress,
            })
        );
    }

    // Queued -> Completed is valid: GitHub can skip runs directly to completed
    // (e.g., skipped workflows, cancelled-before-start)
    #[test]
    fn test_run_transition_queued_to_completed() {
        let result = RunStatus::Queued.transition_to(RunStatus::Completed);
        assert_eq!(result, Ok(RunStatus::Completed));
    }

    #[test]
    fn test_run_transition_completed_to_queued_fails() {
        let result = RunStatus::Completed.transition_to(RunStatus::Queued);
        assert_eq!(
            result,
            Err(InvalidRunTransition {
                from: RunStatus::Completed,
                to: RunStatus::Queued,
            })
        );
    }

    #[test]
    fn test_run_transition_queued_to_queued_idempotent() {
        let result = RunStatus::Queued.transition_to(RunStatus::Queued);
        assert_eq!(result, Ok(RunStatus::Queued));
    }

    #[test]
    fn test_run_transition_in_progress_to_in_progress_idempotent() {
        let result = RunStatus::InProgress.transition_to(RunStatus::InProgress);
        assert_eq!(result, Ok(RunStatus::InProgress));
    }

    #[test]
    fn test_run_transition_completed_to_completed_idempotent() {
        let result = RunStatus::Completed.transition_to(RunStatus::Completed);
        assert_eq!(result, Ok(RunStatus::Completed));
    }

    // predecessors_of: one assertion per (target, expected-slice) pair
    #[test]
    fn test_predecessors_of_queued() {
        assert_eq!(
            RunStatus::predecessors_of(RunStatus::Queued),
            &[RunStatus::Queued]
        );
    }

    #[test]
    fn test_predecessors_of_in_progress() {
        assert_eq!(
            RunStatus::predecessors_of(RunStatus::InProgress),
            &[RunStatus::Queued, RunStatus::InProgress]
        );
    }

    #[test]
    fn test_predecessors_of_completed() {
        assert_eq!(
            RunStatus::predecessors_of(RunStatus::Completed),
            &[
                RunStatus::Queued,
                RunStatus::InProgress,
                RunStatus::Completed
            ]
        );
    }

    // Proptest: from.transition_to(to).is_ok() ⟺ predecessors_of(to).contains(&from)
    proptest! {
        #[test]
        fn prop_predecessors_of_consistent_with_transition_to(
            from: RunStatus,
            to: RunStatus,
        ) {
            let transition_ok = from.transition_to(to).is_ok();
            let in_predecessors = RunStatus::predecessors_of(to).contains(&from);
            prop_assert_eq!(
                transition_ok,
                in_predecessors,
                "from={:?} to={:?}: transition_to={} predecessors={}",
                from, to, transition_ok, in_predecessors
            );
        }
    }
}
