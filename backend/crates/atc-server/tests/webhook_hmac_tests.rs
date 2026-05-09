use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod common;

use common::{
    build_app_no_secret, build_app_with_secret, compute_signature, fixture_workflow_run_requested,
};

/// Valid signature with matching secret returns 200
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_valid_signature_returns_200() {
    let secret = "test-secret";
    let body = fixture_workflow_run_requested();
    let signature = compute_signature(secret.as_bytes(), &body);

    let (app, _) = build_app_with_secret(secret);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .header("x-hub-signature-256", signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "accepted");
    assert!(json["seq"].is_number(), "response must include numeric seq");
}

/// No secret configured + no signature header returns 200 (verification skipped)
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_no_secret_no_signature_returns_200() {
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_no_secret();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "accepted");
    assert!(json["seq"].is_number(), "response must include numeric seq");
}

/// Invalid signature returns 401
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_invalid_signature_returns_401() {
    let secret = "test-secret";
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_with_secret(secret);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .header(
                    "x-hub-signature-256",
                    "sha256=0000000000000000000000000000000000000000000000000000000000000000",
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["error"], "invalid signature");
}

/// Missing signature header when secret configured returns 401
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_missing_signature_header_returns_401() {
    let secret = "test-secret";
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_with_secret(secret);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["error"], "missing X-Hub-Signature-256 header");
}

/// SHA-1 signature rejected with 401
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_sha1_signature_rejected_returns_401() {
    let secret = "test-secret";
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_with_secret(secret);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .header(
                    "x-hub-signature-256",
                    "sha1=0000000000000000000000000000000000000000",
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
