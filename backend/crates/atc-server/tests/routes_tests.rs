use std::sync::Arc;

use atc_core::{StateStore, SystemClock};
use atc_server::state::{AppState, SeqEvent};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;
use std::time::Duration;
use tower::ServiceExt;

// Guard: PrometheusMetricLayer::pair() is called only once per test binary.
// Tests that use this must be marked with #[serial_test::serial] to avoid concurrent execution.
static PROMETHEUS_INIT: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

/// Helper to build and test the full app with API routes and asset fallback.
/// Must be used in tests marked with #[serial_test::serial] since pair() installs a global recorder.
fn build_full_app() -> axum::Router {
    let layer = PROMETHEUS_INIT.get_or_init(|| PrometheusMetricLayer::pair().0);
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel::<SeqEvent>(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
    });
    atc_server::routes::api_routes(layer.clone())
        .with_state(app_state)
        .fallback(atc_server::assets::fallback_handler())
}

#[tokio::test]
#[serial_test::serial]
async fn healthz_returns_ok() {
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify response body is valid JSON
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "ok");

    // Verify content-type header
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
}

#[tokio::test]
#[serial_test::serial]
async fn readyz_returns_ok() {
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify response body is valid JSON
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "ok");

    // Verify content-type header
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
}

#[tokio::test]
#[serial_test::serial]
async fn health_returns_404() {
    // Test that the full app (with fallback) returns 404 for /health, not SPA index.html.
    // This verifies AC3.3: unknown API paths return 404 at the app level.
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial_test::serial]
async fn state_endpoint_snapshot_uses_sorted_pool_stats() {
    // AC1.4: Regression guard: GET /v1/state returns poolStats sorted by labels
    // lexicographically. This is a route-level test verifying the HTTP contract.
    //
    // Fixture: three Job-Queued events with distinct label sets chosen so that
    // insertion order differs from alphabetical order.
    //
    // Note: LabelSet internally uses BTreeSet, which serializes elements in
    // lexicographic order. So:
    // - Input ["ubuntu-latest"] → serializes as ["ubuntu-latest"]
    // - Input ["self-hosted", "x86_64"] → serializes as ["self-hosted", "x86_64"]
    // - Input ["self-hosted", "linux"] → serializes as ["linux", "self-hosted"]
    //
    // Lexicographic sort order (by sorted labels):
    // 1. ["linux", "self-hosted"]      (first element "linux" < others)
    // 2. ["self-hosted", "x86_64"]     (first element "self-hosted", second "x86_64")
    // 3. ["ubuntu-latest"]             (first element "ubuntu" > all others)
    let app = build_full_app();

    // Helper to construct a minimal workflow_job Queued payload
    fn job_payload_queued(run_id: u64, job_id: u64, labels: &[&str]) -> Vec<u8> {
        let labels_json = labels
            .iter()
            .map(|l| format!("\"{}\"", l))
            .collect::<Vec<_>>()
            .join(",");

        let payload = format!(
            r#"{{
  "action": "queued",
  "workflow_job": {{
    "id": {},
    "run_id": {},
    "workflow_name": "TestWorkflow",
    "head_branch": "main",
    "run_url": "https://api.github.com/repos/test/repo/actions/runs/{}",
    "run_attempt": 1,
    "node_id": "CR_test",
    "head_sha": "abc123",
    "url": "https://api.github.com/repos/test/repo/actions/jobs/{}",
    "html_url": "https://github.com/test/repo/actions/runs/{}/job/{}",
    "status": "queued",
    "conclusion": null,
    "created_at": "2026-04-18T12:00:00Z",
    "started_at": "2026-04-18T12:00:00Z",
    "completed_at": null,
    "name": "test-job",
    "steps": [],
    "check_run_url": "https://api.github.com/repos/test/repo/check-runs/{}",
    "labels": [{}],
    "runner_id": null,
    "runner_name": null,
    "runner_group_id": null,
    "runner_group_name": null
  }},
  "repository": {{
    "id": 1,
    "node_id": "R_test",
    "name": "repo",
    "full_name": "test/repo",
    "private": false,
    "owner": {{
      "login": "test",
      "id": 1,
      "node_id": "U_test",
      "avatar_url": "https://example.com/avatar.jpg",
      "gravatar_id": "",
      "url": "https://api.github.com/users/test",
      "html_url": "https://github.com/test",
      "followers_url": "https://api.github.com/users/test/followers",
      "following_url": "https://api.github.com/users/test/following{{/other_user}}",
      "gists_url": "https://api.github.com/users/test/gists{{/gist_id}}",
      "starred_url": "https://api.github.com/users/test/starred{{/owner}}{{/repo}}",
      "subscriptions_url": "https://api.github.com/users/test/subscriptions",
      "organizations_url": "https://api.github.com/users/test/orgs",
      "repos_url": "https://api.github.com/users/test/repos",
      "events_url": "https://api.github.com/users/test/events{{/privacy}}",
      "received_events_url": "https://api.github.com/users/test/received_events",
      "type": "User",
      "user_view_type": "public",
      "site_admin": false
    }},
    "html_url": "https://github.com/test/repo",
    "description": "Test Repo",
    "fork": false,
    "url": "https://api.github.com/repos/test/repo",
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-04-18T12:00:00Z",
    "pushed_at": "2026-04-18T12:00:00Z",
    "git_url": "git://github.com/test/repo.git",
    "ssh_url": "git@github.com:test/repo.git",
    "clone_url": "https://github.com/test/repo.git",
    "svn_url": "https://svn.github.com/test/repo",
    "homepage": null,
    "size": 1,
    "stargazers_count": 0,
    "watchers_count": 0,
    "language": null,
    "has_issues": true,
    "has_projects": true,
    "has_downloads": true,
    "has_wiki": true,
    "has_pages": false,
    "forks_count": 0,
    "mirror_url": null,
    "open_issues_count": 0,
    "forks": 0,
    "open_issues": 0,
    "watchers": 0,
    "default_branch": "main"
  }}
}}"#,
            job_id, run_id, run_id, job_id, run_id, job_id, job_id, labels_json
        );
        payload.into_bytes()
    }

    // Step 1: Ingest a workflow_run event first (required for store to accept job events)
    let run_fixture = include_bytes!("../../atc-github/tests/fixtures/workflow_run_requested.json");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("X-GitHub-Event", "workflow_run")
                .body(Body::from(run_fixture.to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Step 2: POST three job events with distinct labels in non-sorted order
    // Order of insertion: ubuntu-latest, self-hosted+x86_64, self-hosted+linux
    let fixtures = vec![
        (
            "ubuntu-latest",
            job_payload_queued(100, 1, &["ubuntu-latest"]),
        ),
        (
            "self-hosted+x86_64",
            job_payload_queued(100, 2, &["self-hosted", "x86_64"]),
        ),
        (
            "self-hosted+linux",
            job_payload_queued(100, 3, &["self-hosted", "linux"]),
        ),
    ];

    for (_label_name, payload) in fixtures {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/webhooks/github")
                    .header("X-GitHub-Event", "workflow_job")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Step 3: GET /v1/state and verify poolStats are sorted by labels lexicographically
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");

    let pool_stats = json["poolStats"]
        .as_array()
        .expect("poolStats should be an array");

    // Assert we have three pools
    assert_eq!(
        pool_stats.len(),
        3,
        "should have three pool entries from three distinct label sets"
    );

    // Extract labels from each pool and verify lexicographic order
    // Expected sorted order:
    // 1. ["self-hosted", "linux"]
    // 2. ["self-hosted", "x86_64"]
    // 3. ["ubuntu-latest"]
    let labels_arrays: Vec<Vec<String>> = pool_stats
        .iter()
        .map(|pool| {
            pool["labels"]
                .as_array()
                .expect("labels should be an array")
                .iter()
                .map(|l| l.as_str().unwrap().to_string())
                .collect()
        })
        .collect();

    assert_eq!(
        labels_arrays[0],
        vec!["linux", "self-hosted"],
        "first pool should be ['linux', 'self-hosted'] (BTreeSet sorts internally)"
    );
    assert_eq!(
        labels_arrays[1],
        vec!["self-hosted", "x86_64"],
        "second pool should be ['self-hosted', 'x86_64']"
    );
    assert_eq!(
        labels_arrays[2],
        vec!["ubuntu-latest"],
        "third pool should be ['ubuntu-latest']"
    );

    // Sanity check: verify the labels array is indeed sorted (each pool's labels
    // are lexicographically >= the previous pool's labels)
    for i in 0..labels_arrays.len() - 1 {
        assert!(
            labels_arrays[i] < labels_arrays[i + 1],
            "labels at index {} should be < labels at index {}",
            i,
            i + 1
        );
    }
}
