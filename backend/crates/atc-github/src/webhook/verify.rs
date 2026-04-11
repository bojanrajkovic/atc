//! HMAC signature verification for GitHub webhook payloads.

/// Errors from webhook signature verification.
#[derive(Debug)]
pub enum VerifyError {}

/// Verify a GitHub webhook signature against the raw request body.
///
/// # Errors
///
/// Returns [`VerifyError`] if signature verification fails.
pub fn verify_signature(_secret: &[u8], _body: &[u8], _signature: &str) -> Result<(), VerifyError> {
    todo!("implemented in Task 2")
}
