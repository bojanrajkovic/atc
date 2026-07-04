//! Domain event types for state store ingestion.
//!
//! These types are source-agnostic — they carry domain data, not raw
//! webhook payloads. The `atc-github` crate maps webhook JSON into these types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::job::{JobConclusion, RunnerInfo, Step};
use crate::run::RunConclusion;
use crate::types::{JobId, RepoId, RunId};

/// Action that occurred on a workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data")]
#[ts(export)]
pub enum RunEvent {
    /// New run appeared (maps to `workflow_run` `requested` action).
    Requested,
    /// Run started executing.
    InProgress,
    /// Run finished.
    Completed {
        /// The conclusion of the run.
        conclusion: RunConclusion,
    },
}

impl RunEvent {
    /// Bounded `&'static str` discriminant name, suitable for span attributes.
    ///
    /// Use this instead of `format!("{:?}", event)` — `Debug` recurses through
    /// payload fields (including `Vec<Step>` and `RunnerInfo`) and can produce
    /// multi-KB strings that risk being dropped past OTLP message-size limits.
    /// The names mirror GitHub's `workflow_run` action strings.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            RunEvent::Requested => "requested",
            RunEvent::InProgress => "in_progress",
            RunEvent::Completed { .. } => "completed",
        }
    }
}

/// Full run event data for state store ingestion.
///
/// Carries all fields needed to create or update a `WorkflowRun`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunEventEnvelope {
    /// Unique identifier for the run.
    pub run_id: RunId,
    /// Organization name.
    pub org: String,
    /// Repository name.
    pub repo: String,
    /// Workflow name from the `workflow` object. `None` when GitHub sends
    /// `workflow: null` (common on `in_progress` and `completed` events).
    pub workflow_name: Option<String>,
    /// Workflow file path. `None` when GitHub sends `workflow: null`.
    pub workflow_path: Option<String>,
    /// Branch name, if applicable.
    pub branch: Option<String>,
    /// Head commit SHA.
    pub head_sha: String,
    /// Head commit message.
    pub commit_message: Option<String>,
    /// Event that triggered the run (e.g., `push`, `pull_request`).
    pub trigger_event: String,
    /// Display title for the run.
    pub display_title: String,
    /// URL to the run on GitHub.
    pub html_url: String,
    /// When the run was created.
    pub created_at: DateTime<Utc>,
    /// When the run started executing.
    pub run_started_at: Option<DateTime<Utc>>,
    /// When the run was last updated.
    pub updated_at: DateTime<Utc>,
    /// When the run reached its terminal state. Populated by the GitHub
    /// translation layer from `workflow_run.updated_at` ONLY when the action
    /// is `Completed` (GitHub does not surface a dedicated `completed_at`
    /// field on `workflow_run` — see `atc-github/src/webhook/translate.rs`).
    /// `None` for non-completed actions; carried into `WorkflowRun.completed_at`
    /// by `apply_run_event` with preserve-first semantics (`.or(existing)`).
    ///
    /// `#[ts(optional)]` + `#[serde(skip_serializing_if)]` keep the TS
    /// type honest against the wire shape; mirrors `WorkflowRun.completed_at`.
    /// Pre-feature replicas may emit `RunEventEnvelope` over WS without
    /// this field during a rolling deploy.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub completed_at: Option<DateTime<Utc>>,
    /// Attempt number for this run (1 for the initial run, 2+ for re-runs).
    /// Used by the persistence layer to detect when a new attempt supersedes a
    /// completed/cancelled run and should reset its state.
    ///
    /// Defaults to 1 on deserialization: the PG drain decodes persisted
    /// `outbox.payload` rows back into this type, and rows written before this
    /// field existed carry no `run_attempt`. Without the default they would
    /// fail to deserialize and be silently dropped from the drain during a
    /// rolling deploy / backlog drain. Mirrors the webhook parser's default.
    #[serde(default = "default_run_attempt")]
    pub run_attempt: i32,
    /// GitHub's immutable numeric repository identifier. Live webhook
    /// translation always populates `Some`. `None` covers two cases: a
    /// persisted `outbox.payload` row written before this field existed, and
    /// a staleness-sweep-synthesized completion (the sweep stores don't yet
    /// carry a repo id to attach — see issue #475). Optional (rather than
    /// required) for the rolling-deploy decode reason: the PG drain decodes
    /// historical outbox rows back into this type, and a required field with
    /// no default would fail to deserialize and be silently dropped from the
    /// drain.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub repo_id: Option<RepoId>,
    /// The action that occurred.
    pub action: RunEvent,
}

/// Default `run_attempt` for envelopes deserialized without the field
/// (pre-feature persisted outbox rows): the first attempt.
fn default_run_attempt() -> i32 {
    1
}

/// Action that occurred on a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data")]
#[ts(export)]
pub enum JobEvent {
    /// Job entered the queue.
    Queued {
        /// Runner labels requested by this job.
        labels: Vec<String>,
        /// Current step snapshot.
        steps: Vec<Step>,
    },
    /// A job is waiting for approval (environment protection rule, required reviewer).
    Waiting {
        /// Runner labels requested by the job.
        labels: Vec<String>,
        /// Steps defined in the job at the time of the event.
        steps: Vec<Step>,
    },
    /// Job started executing on a runner.
    InProgress {
        /// Runner assigned to the job. `None` when GitHub fires `in_progress`
        /// before runner assignment is complete.
        runner: Option<RunnerInfo>,
        /// Runner labels.
        labels: Vec<String>,
        /// Current step snapshot.
        steps: Vec<Step>,
    },
    /// Job finished executing.
    Completed {
        /// The conclusion of the job.
        conclusion: JobConclusion,
        /// Runner that executed the job, if known.
        runner: Option<RunnerInfo>,
        /// Runner labels.
        labels: Vec<String>,
        /// Final step snapshot.
        steps: Vec<Step>,
    },
}

impl JobEvent {
    /// Bounded `&'static str` discriminant name, suitable for span attributes.
    ///
    /// See [`RunEvent::name`] for the rationale.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            JobEvent::Queued { .. } => "queued",
            JobEvent::Waiting { .. } => "waiting",
            JobEvent::InProgress { .. } => "in_progress",
            JobEvent::Completed { .. } => "completed",
        }
    }
}

/// Full job event data for state store ingestion.
///
/// Carries all fields needed to create or update a `Job`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobEventEnvelope {
    /// Unique identifier for the job.
    pub job_id: JobId,
    /// Back-reference to the parent run.
    pub run_id: RunId,
    /// Organization name.
    pub org: String,
    /// Repository name.
    pub repo: String,
    /// Job name.
    pub name: String,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job started executing.
    pub started_at: Option<DateTime<Utc>>,
    /// When the job finished executing.
    pub completed_at: Option<DateTime<Utc>>,
    /// Attempt number of the parent run this job belongs to. GitHub assigns
    /// fresh job IDs per attempt but reuses the run ID; the store filters jobs
    /// to the run's current attempt so a re-run's card doesn't mix attempts.
    ///
    /// Defaults to 1 on deserialization for the same reason as
    /// [`RunEventEnvelope::run_attempt`] — the PG drain decodes pre-feature
    /// `outbox.payload` rows that carry no `run_attempt`.
    #[serde(default = "default_run_attempt")]
    pub run_attempt: i32,
    /// GitHub's immutable numeric repository identifier. See
    /// [`RunEventEnvelope::repo_id`] for why this is `Option`, not required.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub repo_id: Option<RepoId>,
    /// The action that occurred.
    pub action: JobEvent,
}

#[cfg(test)]
mod name_tests {
    use super::*;
    use crate::job::JobConclusion;
    use crate::run::RunConclusion;

    // The exhaustive match in each variant table pins the variant set: a new
    // RunEvent / JobEvent variant added without updating name() AND this test
    // produces a compile-time non-exhaustive-match warning at name() and a
    // missing-variant assertion failure here.

    #[test]
    fn run_event_names() {
        for (ev, expected) in [
            (RunEvent::Requested, "requested"),
            (RunEvent::InProgress, "in_progress"),
            (
                RunEvent::Completed {
                    conclusion: RunConclusion::Success,
                },
                "completed",
            ),
        ] {
            assert_eq!(ev.name(), expected);
        }
    }

    #[test]
    fn job_event_names() {
        for (ev, expected) in [
            (
                JobEvent::Queued {
                    labels: vec![],
                    steps: vec![],
                },
                "queued",
            ),
            (
                JobEvent::Waiting {
                    labels: vec![],
                    steps: vec![],
                },
                "waiting",
            ),
            (
                JobEvent::InProgress {
                    runner: None,
                    labels: vec![],
                    steps: vec![],
                },
                "in_progress",
            ),
            (
                JobEvent::Completed {
                    conclusion: JobConclusion::Success,
                    runner: None,
                    labels: vec![],
                    steps: vec![],
                },
                "completed",
            ),
        ] {
            assert_eq!(ev.name(), expected);
        }
    }
}
