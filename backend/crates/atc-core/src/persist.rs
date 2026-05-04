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
    use super::*;

    // Compile-time proof that StateStore: PersistentStore
    #[allow(dead_code, clippy::used_underscore_items)]
    fn _assert_state_store_impls_trait() {
        fn _f<T: PersistentStore>() {}
        _f::<crate::StateStore>();
    }
}
