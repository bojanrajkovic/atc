//! GitHub webhook parsing and verification.
//!
//! Two public entry points:
//! - [`verify_signature`] — HMAC-SHA256 signature verification
//! - [`parse_webhook`] — JSON deserialization + translation to domain events

mod verify;
pub(crate) mod types;
mod translate;

pub use verify::{verify_signature, VerifyError};

use atc_core::event::{JobEventEnvelope, RunEventEnvelope};

/// Errors from webhook parsing and translation.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// JSON deserialization failed.
    #[error("invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    /// The `action` field contains an unrecognized value.
    #[error("unknown {event_type} action: {action}")]
    UnknownAction {
        /// The webhook event type (e.g., `"workflow_run"`).
        event_type: String,
        /// The unrecognized action string.
        action: String,
    },

    /// A `completed` event arrived without a `conclusion` field.
    #[error("missing conclusion on {event_type} {action} event")]
    MissingConclusion {
        /// The webhook event type.
        event_type: String,
        /// The action (always `"completed"`).
        action: String,
    },

    /// The `conclusion` field contains an unrecognized value.
    #[error("unknown {event_type} conclusion: {value}")]
    UnknownConclusion {
        /// The webhook event type.
        event_type: String,
        /// The unrecognized conclusion string.
        value: String,
    },

    /// A step's `status` field contains an unrecognized value.
    #[error("unknown step status in {context}: {value}")]
    UnknownStatus {
        /// Context identifying the step (e.g., `"step 'Setup Node'"`).
        context: String,
        /// The unrecognized status string.
        value: String,
    },
}

/// Three-way result from [`parse_webhook`].
#[derive(Debug)]
pub enum ParseResult {
    /// Successfully parsed and translated to a domain event.
    Parsed(Box<WebhookEvent>),
    /// Unrecognized event type — not an error, just not ATC's concern.
    Skipped {
        /// The event type string that was skipped.
        event_type: String,
    },
}

/// A parsed webhook event carrying a domain event envelope.
#[derive(Debug)]
pub enum WebhookEvent {
    /// A `workflow_run` event translated to a run event envelope.
    Run(RunEventEnvelope),
    /// A `workflow_job` event translated to a job event envelope.
    Job(JobEventEnvelope),
}

/// Parse a GitHub webhook payload into a domain event.
///
/// # Errors
///
/// Returns [`ParseError`] if deserialization or translation fails.
pub fn parse_webhook(_event_type: &str, _body: &[u8]) -> Result<ParseResult, ParseError> {
    todo!("implemented in Task 3")
}
