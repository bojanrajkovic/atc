//! Persistence abstraction for domain event stores.
//!
//! Defines [`PersistentStore`], a common interface over any backend that can
//! durably apply domain events. [`StateStore`](crate::StateStore) implements
//! this trait so it can be used wherever a `PersistentStore` is expected.

use crate::{JobEventEnvelope, RunEventEnvelope, StoreError};

/// Errors that can occur during persistent store operations.
#[derive(Debug)]
pub enum PersistError {
    /// PG `0 rows affected` on the predicated UPDATE,
    /// or in-memory `transition_to` rejection.
    InvalidTransition,
    /// Any backend-specific error (`sqlx::Error` for `PgStore`).
    Backend(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl From<StoreError> for PersistError {
    fn from(_e: StoreError) -> Self {
        PersistError::InvalidTransition
    }
}

/// A store that can durably apply domain events.
///
/// Implementations must be `Send + Sync` for use behind `Arc` in async contexts.
#[async_trait::async_trait]
pub trait PersistentStore: Send + Sync {
    /// Apply a run event envelope, creating or updating the corresponding
    /// [`WorkflowRun`](crate::WorkflowRun).
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<(), PersistError>;

    /// Apply a job event envelope, creating or updating the corresponding
    /// [`Job`](crate::job::Job).
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<(), PersistError>;
}

#[async_trait::async_trait]
impl PersistentStore for crate::StateStore {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<(), PersistError> {
        crate::StateStore::apply_run_event(self, env)
            .await
            .map_err(Into::into)
    }

    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<(), PersistError> {
        crate::StateStore::apply_job_event(self, env)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::Utc;

    use super::*;
    use crate::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
    use crate::types::{JobId, RunId};
    use crate::{RunConclusion, StateStore, SystemClock};

    // Compile-time proof that StateStore: PersistentStore
    #[allow(dead_code, clippy::used_underscore_items)]
    fn _assert_state_store_impls_trait() {
        fn _f<T: PersistentStore>() {}
        _f::<crate::StateStore>();
    }

    fn make_store() -> StateStore {
        StateStore::new(Arc::new(SystemClock), Duration::from_secs(3600))
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

    fn job_queued_env() -> JobEventEnvelope {
        JobEventEnvelope {
            job_id: JobId(1),
            run_id: RunId(1),
            org: "org".into(),
            repo: "repo".into(),
            name: "job".into(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            action: JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        }
    }

    #[tokio::test]
    async fn trait_delegation_apply_run_event_ok() {
        let store = make_store();
        let store_ref: &dyn PersistentStore = &store;
        let result = store_ref
            .apply_run_event(run_env(RunEvent::Requested))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn trait_delegation_apply_job_event_ok() {
        let store = make_store();
        let store_ref: &dyn PersistentStore = &store;
        let result = store_ref.apply_job_event(job_queued_env()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn from_store_error_maps_to_invalid_transition() {
        let store = make_store();
        let store_ref: &dyn PersistentStore = &store;
        store_ref
            .apply_run_event(run_env(RunEvent::Requested))
            .await
            .unwrap();
        store_ref
            .apply_run_event(run_env(RunEvent::Completed {
                conclusion: RunConclusion::Success,
            }))
            .await
            .unwrap();
        // Completed → InProgress is rejected; StoreError converts via From to InvalidTransition
        let result = store_ref
            .apply_run_event(run_env(RunEvent::InProgress))
            .await;
        assert!(
            matches!(result, Err(PersistError::InvalidTransition)),
            "expected InvalidTransition, got {result:?}"
        );
    }
}
