#![deny(missing_docs)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! ATC GitHub API integration.
//!
//! Provides webhook payload parsing and HMAC signature verification for
//! GitHub Actions `workflow_run` and `workflow_job` events.

mod webhook;

pub use webhook::{
    parse_webhook, ParseError, ParseResult, WebhookEvent,
    verify_signature, VerifyError,
};
