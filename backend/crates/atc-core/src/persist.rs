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
    use super::*;
    use crate::run::InvalidRunTransition;
    use crate::state_machine::StateMachineError;

    /// `StateMachineError` converts via `From` to `PersistError::InvalidTransition`.
    ///
    /// Constructs a `StateMachineError` directly rather than driving a full
    /// state machine — the conversion is purely structural.
    #[test]
    fn state_machine_error_maps_to_invalid_transition() {
        // Build an InvalidRunTransition directly — the exact variant is irrelevant.
        use crate::run::RunStatus;
        let transition_err = InvalidRunTransition {
            from: RunStatus::Completed,
            to: RunStatus::InProgress,
        };
        let sm_err = StateMachineError::InvalidRunTransition(transition_err);
        let persist_err = PersistError::from(sm_err);
        assert!(
            matches!(persist_err, PersistError::InvalidTransition),
            "expected InvalidTransition, got {persist_err:?}"
        );
    }
}
