//! GitHub webhook parsing and verification.

mod verify;

pub use verify::{verify_signature, VerifyError};
