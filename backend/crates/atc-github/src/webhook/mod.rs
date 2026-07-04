//! GitHub webhook parsing and verification.
//!
//! Two public entry points:
//! - [`verify_signature`] — HMAC-SHA256 signature verification
//! - [`parse_webhook`] — JSON deserialization + translation to domain events

mod translate;
pub(crate) mod types;
mod verify;

pub use verify::{VerifyError, verify_signature};

use atc_core::event::{JobEventEnvelope, RunEventEnvelope};
use atc_core::types::RepoId;

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
#[derive(Debug, Clone, serde::Serialize)]
pub enum ParseResult {
    /// Successfully parsed and translated to a domain event.
    Parsed(Box<WebhookEvent>),
    /// A GitHub `ping` event — a webhook connectivity check fired when the hook
    /// is created. Carries no payload; the delivery itself is the signal.
    Ping,
    /// Unrecognized event type — not an error, just not ATC's concern.
    Skipped {
        /// The event type string that was skipped.
        event_type: String,
    },
}

/// A parsed webhook event carrying a domain event envelope.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(tag = "type", content = "data")]
#[ts(export)]
pub enum WebhookEvent {
    /// A `workflow_run` event translated to a run event envelope.
    Run(RunEventEnvelope),
    /// A `workflow_job` event translated to a job event envelope.
    Job(JobEventEnvelope),
}

impl WebhookEvent {
    /// The wrapped envelope's `repo_id` — both variants carry one
    /// (post-#449). `None` covers a pre-migration outbox row or a
    /// staleness-sweep-synthesized completion that predates the field.
    #[must_use]
    pub fn repo_id(&self) -> Option<RepoId> {
        match self {
            Self::Run(env) => env.repo_id,
            Self::Job(env) => env.repo_id,
        }
    }
}

/// Parse a GitHub webhook payload into a domain event.
///
/// Accepts the event type string (from the `X-GitHub-Event` HTTP header)
/// and the raw JSON body. Returns [`ParseResult::Parsed`] for recognized
/// events (`workflow_run`, `workflow_job`), [`ParseResult::Ping`] for a
/// `ping` connectivity check, or [`ParseResult::Skipped`] for any other
/// event type.
///
/// # Errors
///
/// Returns [`ParseError`] if JSON deserialization fails or if the payload
/// contains unrecognized action/conclusion/status values.
#[tracing::instrument(
    name = "webhook.parse",
    skip(body),
    fields(
        webhook.event_type = event_type,
        webhook.action = tracing::field::Empty,
        webhook.repo = tracing::field::Empty,
        webhook.run_id = tracing::field::Empty,
        webhook.job_id = tracing::field::Empty,
    ),
)]
pub fn parse_webhook(event_type: &str, body: &[u8]) -> Result<ParseResult, ParseError> {
    match event_type {
        "workflow_run" => {
            let webhook: types::WorkflowRunWebhook = serde_json::from_slice(body)?;
            let envelope = translate::translate_run(webhook)?;
            let action_name = envelope.action.name();
            let repo = format!("{}/{}", envelope.org, envelope.repo);
            let run_id = envelope.run_id.0;
            let span = tracing::Span::current();
            span.record("webhook.action", action_name);
            span.record("webhook.repo", repo.as_str());
            span.record("webhook.run_id", run_id);
            tracing::debug!(
                event_type = "workflow_run",
                action = action_name,
                repo = repo,
                run_id = run_id,
                "parsed webhook"
            );
            Ok(ParseResult::Parsed(Box::new(WebhookEvent::Run(envelope))))
        }
        "workflow_job" => {
            let webhook: types::WorkflowJobWebhook = serde_json::from_slice(body)?;
            let envelope = translate::translate_job(webhook)?;
            let action_name = envelope.action.name();
            let repo = format!("{}/{}", envelope.org, envelope.repo);
            let run_id = envelope.run_id.0;
            let job_id = envelope.job_id.0;
            let span = tracing::Span::current();
            span.record("webhook.action", action_name);
            span.record("webhook.repo", repo.as_str());
            span.record("webhook.run_id", run_id);
            span.record("webhook.job_id", job_id);
            tracing::debug!(
                event_type = "workflow_job",
                action = action_name,
                repo = repo,
                run_id = run_id,
                job_id = job_id,
                "parsed webhook"
            );
            Ok(ParseResult::Parsed(Box::new(WebhookEvent::Job(envelope))))
        }
        "ping" => {
            tracing::debug!("ping webhook received");
            Ok(ParseResult::Ping)
        }
        _ => {
            tracing::debug!(event_type = event_type, "skipped webhook");
            Ok(ParseResult::Skipped {
                event_type: event_type.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_core::event::{JobEvent, RunEvent};
    use atc_core::test_support::{make_job_event, make_run_event};
    use atc_core::types::{JobId, RepoId, RunId};

    #[test]
    fn repo_id_reads_through_either_variant() {
        let run = WebhookEvent::Run(make_run_event(RunId(1), RunEvent::Requested));
        assert_eq!(run.repo_id(), Some(RepoId(1_296_269)));

        let job = WebhookEvent::Job(make_job_event(
            JobId(1),
            RunId(1),
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ));
        assert_eq!(job.repo_id(), Some(RepoId(1_296_269)));
    }

    #[test]
    fn test_parse_workflow_run_requested() {
        let fixture = include_str!("../../tests/fixtures/workflow_run_requested.json");
        let result =
            parse_webhook("workflow_run", fixture.as_bytes()).expect("should parse without error");

        match result {
            ParseResult::Parsed(event) => match *event {
                WebhookEvent::Run(ref envelope) => {
                    assert_eq!(envelope.org, "bojanrajkovic");
                    assert_eq!(envelope.repo, "atc");
                    assert_eq!(envelope.action, RunEvent::Requested);
                    assert_eq!(envelope.repo_id, Some(RepoId(1_190_105_052)));
                }
                WebhookEvent::Job(_) => panic!("expected Run variant"),
            },
            other => panic!("expected Parsed variant, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_workflow_job_queued() {
        let fixture = include_str!("../../tests/fixtures/workflow_job_queued.json");
        let result =
            parse_webhook("workflow_job", fixture.as_bytes()).expect("should parse without error");

        match result {
            ParseResult::Parsed(event) => match *event {
                WebhookEvent::Job(ref envelope) => {
                    assert_eq!(envelope.org, "bojanrajkovic");
                    assert_eq!(envelope.repo, "atc");
                    assert_eq!(envelope.repo_id, Some(RepoId(1_190_105_052)));
                    match envelope.action {
                        JobEvent::Queued { .. } => {}
                        _ => panic!("expected Queued action"),
                    }
                }
                WebhookEvent::Run(_) => panic!("expected Job variant"),
            },
            other => panic!("expected Parsed variant, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_unknown_event_skipped() {
        let result = parse_webhook("push", b"{}").expect("should return ParseResult::Skipped");

        match result {
            ParseResult::Skipped { event_type } => {
                assert_eq!(event_type, "push");
            }
            other => panic!("expected Skipped variant, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_unknown_event_type_skipped() {
        let result =
            parse_webhook("unknown_event", b"{}").expect("should return ParseResult::Skipped");

        match result {
            ParseResult::Skipped { event_type } => {
                assert_eq!(event_type, "unknown_event");
            }
            other => panic!("expected Skipped variant, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_ping_event() {
        let result = parse_webhook("ping", b"{\"zen\": \"Keep it simple.\", \"hook_id\": 1}")
            .expect("should return ParseResult::Ping");

        assert!(
            matches!(result, ParseResult::Ping),
            "ping must map to ParseResult::Ping, got {result:?}"
        );
    }

    #[test]
    fn test_parse_malformed_json() {
        let result = parse_webhook("workflow_run", b"not valid json{{{");

        assert!(matches!(result, Err(ParseError::InvalidJson(_))));
    }
}
