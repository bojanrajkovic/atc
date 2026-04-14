# Human Test Plan: Server Wiring

Generated from implementation plan `docs/implementation-plans/2026-04-11-server-wiring/`.

## Prerequisites

- `just setup` has been run in the project root
- `just test` passes (all 36+ `atc-server` tests green)
- `just build` succeeds
- A terminal available for running the server and sending HTTP requests
- `curl` and `websocat` (or equivalent WebSocket CLI client) installed

## Phase 1: Config and Startup

| Step | Action | Expected |
|------|--------|----------|
| 1.1 | Run `cargo run -p atc-server` from `backend/` with no `ATC_*` env vars set | Server starts, logs `listening on http://0.0.0.0:8080` and `metrics listening on http://0.0.0.0:9090`. No panics or errors. |
| 1.2 | Run `curl http://localhost:8080/healthz` | Returns `{"status":"ok"}` with HTTP 200 |
| 1.3 | Run `curl http://localhost:8080/readyz` | Returns `{"status":"ok"}` with HTTP 200 |
| 1.4 | Stop server. Set `ATC_HTTP_ADDR=127.0.0.1:9999` and `ATC_METRICS_ADDR=127.0.0.1:7777`, restart | Server logs `listening on http://127.0.0.1:9999` and `metrics listening on http://127.0.0.1:7777` |
| 1.5 | Stop server. Set `ATC_HTTP_ADDR=not-a-socket-addr`, attempt to start | Server exits with non-zero code and prints a configuration error to stderr |

## Phase 2: Webhook HMAC Verification

| Step | Action | Expected |
|------|--------|----------|
| 2.1 | Start server with `ATC_GITHUB__WEBHOOK_SECRET=test-secret`. Send `curl -X POST http://localhost:8080/v1/webhooks/github -H "X-GitHub-Event: workflow_run" -d '{}'` (no signature header) | Returns HTTP 401 with `{"error":"missing X-Hub-Signature-256 header"}` |
| 2.2 | Send same request with `-H "X-Hub-Signature-256: sha256=0000000000000000000000000000000000000000000000000000000000000000"` | Returns HTTP 401 with `{"error":"invalid signature"}` |
| 2.3 | Stop server. Restart WITHOUT `ATC_GITHUB__WEBHOOK_SECRET`. Send `curl -X POST http://localhost:8080/v1/webhooks/github -H "X-GitHub-Event: push" -d '{}'` (no signature) | Returns HTTP 200 with `{"status":"skipped"}` (verification skipped, event type unknown) |

## Phase 3: Webhook Ingestion and WebSocket Streaming

| Step | Action | Expected |
|------|--------|----------|
| 3.1 | Start server (no webhook secret). In one terminal, connect WebSocket: `websocat ws://localhost:8080/v1/ws` | Connection opens, no immediate output (waiting for events) |
| 3.2 | In another terminal, POST `curl -X POST http://localhost:8080/v1/webhooks/github -H "X-GitHub-Event: workflow_run" -H "Content-Type: application/json" -d @backend/crates/atc-github/tests/fixtures/workflow_run_requested.json` | curl returns HTTP 200 with `{"status":"processed"}`. The WebSocket terminal displays a JSON message containing `"seq":0` and a `"Run"` event variant. |
| 3.3 | POST the job fixture: `curl -X POST http://localhost:8080/v1/webhooks/github -H "X-GitHub-Event: workflow_job" -H "Content-Type: application/json" -d @backend/crates/atc-github/tests/fixtures/workflow_job_queued.json` | curl returns HTTP 200. WebSocket terminal displays a second JSON message with `"seq":1` and a `"Job"` event variant. |
| 3.4 | Send `curl http://localhost:8080/v1/state` | Returns JSON with `"seq":2`, a `runs` array containing 1 entry (run_id 24290980517), a `jobs` array containing 1 entry, and a non-empty `pool_stats` array. |

## Phase 4: REST State Snapshot Consistency

| Step | Action | Expected |
|------|--------|----------|
| 4.1 | Start a fresh server (no webhook secret). Send `curl http://localhost:8080/v1/state` immediately | Returns `{"seq":0,"runs":[],"jobs":[],"pool_stats":[]}` |
| 4.2 | POST the `workflow_run_requested.json` fixture, then GET `/v1/state` | `seq` is 1, `runs` has 1 entry, `jobs` and `pool_stats` are empty |
| 4.3 | POST the `workflow_job_queued.json` fixture, then GET `/v1/state` | `seq` is 2, `runs` has 1 entry, `jobs` has 1 entry, `pool_stats` is non-empty |

## End-to-End: Full Webhook-to-Client Flow

1. Start a fresh server with no webhook secret configured.
2. Open two WebSocket connections: `websocat ws://localhost:8080/v1/ws` in two terminals.
3. POST `workflow_run_requested.json` with `X-GitHub-Event: workflow_run`.
4. Verify both WebSocket terminals show a JSON frame with `"seq":0` and a `Run` variant.
5. POST `workflow_job_queued.json` with `X-GitHub-Event: workflow_job`.
6. Verify both WebSocket terminals show `"seq":1` with a `Job` variant.
7. Close one WebSocket terminal (Ctrl+C).
8. POST `workflow_job_in_progress.json` with `X-GitHub-Event: workflow_job`.
9. Verify the remaining WebSocket terminal shows `"seq":2` with a `Job` variant. Server logs show no panics or errors related to the disconnected client.
10. GET `/v1/state` and verify `seq` is 3, `runs` has 1 entry, `jobs` has 2 entries with different job_ids, `pool_stats` is non-empty.

## End-to-End: Backward State Transition

1. Start a fresh server. Connect a WebSocket client.
2. POST `workflow_run_completed.json` with `X-GitHub-Event: workflow_run`.
3. Verify WebSocket receives `"seq":0` with a `Run` variant.
4. POST `workflow_run_in_progress.json` with `X-GitHub-Event: workflow_run` (same run_id, backward transition).
5. Verify curl returns HTTP 200 with `{"status":"processed"}`, but the WebSocket terminal does NOT receive a second event. Server logs should show a warning about the rejected transition.
6. GET `/v1/state` -- `seq` should be 1 (only one event was committed), run should still show completed status.

## Human Verification Required

### AC6.3: StateStore eviction task is started and runs in background

The eviction task runs on a 60-second timer via `tokio::spawn`. Automated testing would require either waiting for TTL expiry (too slow for CI) or mocking the clock.

**Code inspection:**
1. Verify `main.rs` line 72 calls `store.start_eviction_task(Duration::from_secs(60))` and stores the `JoinHandle` in `eviction_handle`.
2. Verify `main.rs` line 151 calls `eviction_handle.abort()` in the shutdown path after the `tokio::select!` block completes.

**Runtime verification:**
3. Start the server with `ATC_LOG_FILTER=debug cargo run -p atc-server` from `backend/`.
4. Observe no panics or errors in logs mentioning eviction.

## Traceability

| Acceptance Criterion | Automated Test | Manual Step |
|----------------------|----------------|-------------|
| AC1.1 | `webhook_hmac_tests::webhook_hmac_valid_signature_returns_200` | 2.3 (no secret variant) |
| AC1.2 | `webhook_hmac_tests::webhook_hmac_no_secret_no_signature_returns_200` | 2.3 |
| AC1.3 | `webhook_hmac_tests::webhook_hmac_invalid_signature_returns_401` | 2.2 |
| AC1.4 | `webhook_hmac_tests::webhook_hmac_missing_signature_header_returns_401` | 2.1 |
| AC1.5 | `webhook_hmac_tests::webhook_hmac_sha1_signature_rejected_returns_401` | -- |
| AC2.1 | `webhook_ingestion_tests::webhook_ingestion_workflow_run_returns_processed` | 3.2 |
| AC2.2 | `webhook_ingestion_tests::webhook_ingestion_workflow_job_returns_processed` | 3.3 |
| AC2.3 | `webhook_ingestion_tests::webhook_ingestion_unknown_event_returns_skipped` | 2.3 |
| AC2.4 | `webhook_ingestion_tests::webhook_ingestion_missing_event_header_returns_400` | -- |
| AC2.5 | `webhook_ingestion_tests::webhook_ingestion_malformed_json_returns_422` | -- |
| AC2.6 | `webhook_ingestion_tests::webhook_ingestion_backward_transition_returns_200_no_broadcast` | E2E: Backward State Transition |
| AC2.7 | `webhook_ingestion_tests::webhook_ingestion_broadcast_single_event_with_seq` | 3.2 |
| AC2.8 | `webhook_ingestion_tests::webhook_ingestion_broadcast_consecutive_events_increasing_seq` | 3.2, 3.3 |
| AC3.1 | `ws_tests::ac3_1_ws_upgrade_succeeds` | 3.1 |
| AC3.2 | `ws_tests::ac3_2_ws_receives_webhook_event` | 3.2 |
| AC3.3 | `ws_tests::ac3_3_multiple_clients_receive_same_event` | E2E: Full Flow steps 3-4 |
| AC3.4 | `ws_tests::ac3_4_disconnect_does_not_crash_server` | E2E: Full Flow steps 7-9 |
| AC3.5 | `ws_tests::ac3_5_lagging_client_continues_receiving` | -- |
| AC4.1 | `state_tests::test_ac4_1_empty_state` | 4.1 |
| AC4.2 | `state_tests::test_ac4_2_state_after_run_event` | 4.2 |
| AC4.3 | `state_tests::test_ac4_3_state_with_pool_stats` | 4.3 |
| AC4.4 | `state_tests::test_ac4_4_state_seq_consistency` | E2E: Full Flow step 10 |
| AC5.1 | `e2e_tests::ac5_1_webhook_to_rest_state` | 4.2 |
| AC5.2 | `e2e_tests::ac5_2_webhook_to_websocket` | 3.2 |
| AC5.3 | `e2e_tests::ac5_3_multi_event_sequence` | E2E: Full Flow steps 3-10 |
| AC6.1 | `config_tests::config_github_webhook_secret_set` | 1.1 (implicit) |
| AC6.2 | `config_tests::config_github_webhook_secret_none` | 1.1 |
| AC6.3 | N/A (human verification) | Human Verification: AC6.3 |
