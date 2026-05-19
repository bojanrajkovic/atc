#![deny(missing_docs)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! ATC GitHub API integration.
//!
//! Provides:
//!
//! - Webhook payload parsing and HMAC signature verification for GitHub
//!   Actions `workflow_run` and `workflow_job` events
//!   (`parse_webhook`, `verify_signature`).
//! - An OAuth client for GitHub-App user-to-server tokens with PKCE
//!   (`oauth::OAuthClient`, `oauth::generate_pkce_pair`).

pub mod oauth;
mod webhook;

pub use webhook::{
    ParseError, ParseResult, VerifyError, WebhookEvent, parse_webhook, verify_signature,
};
