//! GitHub webhook parsing and verification.

pub(crate) mod types;
mod verify;

pub use verify::{verify_signature, VerifyError};
