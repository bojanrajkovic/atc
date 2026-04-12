//! HMAC signature verification for GitHub webhook payloads.
//!
//! GitHub signs webhook payloads with a secret configured by the user. The
//! signature arrives in the `X-Hub-Signature-256` HTTP header as
//! `sha256=<hex>`. This module verifies that signature against the raw
//! request body.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// HMAC-SHA256 type alias used for webhook signature verification.
type HmacSha256 = Hmac<Sha256>;

/// Errors from webhook signature verification.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// Signature string has no `algorithm=hex` structure.
    #[error("invalid signature format: expected 'algorithm=hex'")]
    InvalidFormat,

    /// Algorithm is recognized but not accepted (e.g., `sha1`).
    #[error("rejected signature algorithm: sha1 is not accepted, use sha256")]
    RejectedAlgorithm,

    /// Algorithm prefix is not recognized — ATC may need an update.
    #[error("unknown signature algorithm")]
    UnknownAlgorithm,

    /// Hex portion of the signature could not be decoded.
    #[error("invalid hex in signature digest")]
    InvalidHex,

    /// HMAC digest did not match. Constant-time comparison was used.
    #[error("signature mismatch")]
    SignatureMismatch,
}

/// Verify a GitHub webhook signature against the raw request body.
///
/// # Arguments
///
/// * `secret` — The webhook secret configured in GitHub, as bytes.
/// * `body` — The raw HTTP request body.
/// * `signature` — The value of the `X-Hub-Signature-256` header
///   (e.g., `"sha256=abc123..."`).
///
/// # Errors
///
/// Returns [`VerifyError`] if the signature format is invalid, the
/// algorithm is rejected/unknown, hex decoding fails, or the digest
/// does not match.
///
/// # Panics
///
/// This function never panics. The call to `expect()` on `new_from_slice`
/// is safe because HMAC-SHA256 accepts keys of any length.
#[tracing::instrument(skip(secret, body))]
pub fn verify_signature(secret: &[u8], body: &[u8], signature: &str) -> Result<(), VerifyError> {
    // Split on first '=' to separate algorithm tag from hex digest.
    let (algorithm, hex_digest) = signature
        .split_once('=')
        .ok_or(VerifyError::InvalidFormat)?;

    // Only sha256 is accepted. sha1 is explicitly rejected.
    match algorithm {
        "sha256" => {}
        "sha1" => return Err(VerifyError::RejectedAlgorithm),
        _ => return Err(VerifyError::UnknownAlgorithm),
    }

    // Decode the hex digest to raw bytes.
    let expected_bytes = const_hex::decode(hex_digest).map_err(|_| VerifyError::InvalidHex)?;

    // Compute HMAC-SHA256 over the body with the shared secret.
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts any key length");
    mac.update(body);

    // Constant-time comparison via `verify_slice`.
    mac.verify_slice(&expected_bytes)
        .map_err(|_| VerifyError::SignatureMismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to compute a valid HMAC-SHA256 signature for a given secret and body.
    fn compute_signature(secret: &[u8], body: &[u8]) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts any key length");
        mac.update(body);
        let digest = mac.finalize();
        format!("sha256={}", const_hex::encode(digest.into_bytes()))
    }

    /// gh-webhooks.AC4.1: Valid `sha256=<hex>` signature with correct secret passes
    #[test]
    fn test_valid_signature_succeeds() {
        let secret = b"my-secret";
        let body = b"test payload";
        let signature = compute_signature(secret, body);

        let result = verify_signature(secret, body, &signature);

        assert!(result.is_ok());
    }

    /// gh-webhooks.AC4.2: Tampered body with valid signature format returns `SignatureMismatch`
    #[test]
    fn test_tampered_body_fails() {
        let secret = b"my-secret";
        let original_body = b"test payload";
        let tampered_body = b"tampered payload";
        let signature = compute_signature(secret, original_body);

        let result = verify_signature(secret, tampered_body, &signature);

        assert!(matches!(result, Err(VerifyError::SignatureMismatch)));
    }

    /// gh-webhooks.AC4.3: Wrong secret returns `SignatureMismatch`
    #[test]
    fn test_wrong_secret_fails() {
        let secret = b"my-secret";
        let wrong_secret = b"wrong-secret";
        let body = b"test payload";
        let signature = compute_signature(secret, body);

        let result = verify_signature(wrong_secret, body, &signature);

        assert!(matches!(result, Err(VerifyError::SignatureMismatch)));
    }

    /// gh-webhooks.AC4.4: `sha1=<hex>` returns `RejectedAlgorithm`
    #[test]
    fn test_sha1_algorithm_rejected() {
        let secret = b"my-secret";
        let body = b"test payload";
        let signature = compute_signature(secret, body);
        let sha1_signature = signature.replace("sha256=", "sha1=");

        let result = verify_signature(secret, body, &sha1_signature);

        assert!(matches!(result, Err(VerifyError::RejectedAlgorithm)));
    }

    /// gh-webhooks.AC4.5: Unknown algorithm prefix (e.g., `sha512=`) returns `UnknownAlgorithm`
    #[test]
    fn test_unknown_algorithm_fails() {
        let secret = b"my-secret";
        let body = b"test payload";
        let signature = compute_signature(secret, body);
        let unknown_signature = signature.replace("sha256=", "sha512=");

        let result = verify_signature(secret, body, &unknown_signature);

        assert!(matches!(result, Err(VerifyError::UnknownAlgorithm)));
    }

    /// gh-webhooks.AC4.6: Invalid hex after prefix returns `InvalidHex`
    #[test]
    fn test_invalid_hex_fails() {
        let secret = b"my-secret";
        let body = b"test payload";

        let result = verify_signature(secret, body, "sha256=not-valid-hex!!!");

        assert!(matches!(result, Err(VerifyError::InvalidHex)));
    }

    /// gh-webhooks.AC4.7: Signature without `=` separator returns `InvalidFormat`
    #[test]
    fn test_no_equals_separator_fails() {
        let secret = b"my-secret";
        let body = b"test payload";

        let result = verify_signature(secret, body, "no-equals-separator");

        assert!(matches!(result, Err(VerifyError::InvalidFormat)));
    }
}
