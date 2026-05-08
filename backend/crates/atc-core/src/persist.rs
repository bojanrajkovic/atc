//! Persistence error types for domain event stores.
//!
//! Defines [`PersistError`] and its conversion from [`StateMachineError`].
//! The persistence trait and its backends live in `atc-server::persist` (ADR 0005).

use crate::StateMachineError;

/// Errors that can occur during persistent store operations.
#[derive(Debug)]
pub enum PersistError {
    /// PG `0 rows affected` on the predicated UPDATE,
    /// or in-memory `transition_to` rejection.
    InvalidTransition,
    /// Any backend-specific error (`sqlx::Error` for `PgStore`).
    Backend(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl From<StateMachineError> for PersistError {
    fn from(_e: StateMachineError) -> Self {
        PersistError::InvalidTransition
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::Utc;

    use super::*;
    use crate::event::{RunEvent, RunEventEnvelope};
    use crate::types::RunId;
    use crate::{RunConclusion, RunStateMachine, SystemClock};

    fn make_machine() -> RunStateMachine {
        RunStateMachine::new(Arc::new(SystemClock), Duration::from_secs(3600))
    }

    fn run_env(action: RunEvent) -> RunEventEnvelope {
        RunEventEnvelope {
            run_id: RunId(1),
            org: "org".into(),
            repo: "repo".into(),
            workflow_name: None,
            workflow_path: None,
            branch: Some("main".into()),
            head_sha: "abc".into(),
            commit_message: None,
            trigger_event: "push".into(),
            display_title: "run".into(),
            html_url: "https://github.com/".into(),
            created_at: Utc::now(),
            run_started_at: None,
            updated_at: Utc::now(),
            action,
        }
    }

    /// `StateMachineError` converts via `From` to `PersistError::InvalidTransition`.
    #[tokio::test]
    async fn state_machine_error_maps_to_invalid_transition() {
        let machine = make_machine();
        machine
            .apply_run_event(run_env(RunEvent::Requested))
            .await
            .unwrap();
        machine
            .apply_run_event(run_env(RunEvent::Completed {
                conclusion: RunConclusion::Success,
            }))
            .await
            .unwrap();
        // Completed → InProgress is rejected; StateMachineError converts via From to InvalidTransition
        let err = machine
            .apply_run_event(run_env(RunEvent::InProgress))
            .await
            .unwrap_err();
        let persist_err = PersistError::from(err);
        assert!(
            matches!(persist_err, PersistError::InvalidTransition),
            "expected InvalidTransition, got {persist_err:?}"
        );
    }
}
