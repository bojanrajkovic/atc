# Human Test Plan: GitHub Webhook Parsing

**Implementation plan:** `docs/implementation-plans/2026-04-11-gh-webhooks/`
**Branch:** `feat/gh-webhooks`
**Generated:** 2026-04-11

## Prerequisites

- Rust 1.94.0 installed (via `mise`)
- Repository cloned and on branch `feat/gh-webhooks`
- Run `just setup` to bootstrap the environment
- Run `cd backend && cargo test -p atc-github -p atc-core` -- all 147+ tests should pass

## Phase 1: HMAC Constant-Time Verification (AC4.8)

| Step | Action | Expected |
|------|--------|----------|
| 1 | Open `backend/crates/atc-github/src/webhook/verify.rs` | File opens in editor |
| 2 | Locate the `verify_signature` function body | Function accepts `secret: &[u8]`, `body: &[u8]`, `signature: &str` |
| 3 | Verify HMAC creation uses `HmacSha256::new_from_slice(secret)` | Should see `let mut mac = HmacSha256::new_from_slice(secret).expect(...)` |
| 4 | Verify body is fed via `mac.update(body)` | Should see `mac.update(body);` |
| 5 | Verify comparison uses `mac.verify_slice(&expected_bytes)` | Should see `mac.verify_slice(&expected_bytes).map_err(...)` -- NOT `==`, `!=`, `PartialEq`, or direct byte comparison |
| 6 | Verify the `HmacSha256` type alias resolves to `Hmac<Sha256>` from the `hmac` crate | `type HmacSha256 = Hmac<Sha256>;` |
| 7 | Confirm the `hmac` crate is in `Cargo.toml` dependencies | Open `backend/crates/atc-github/Cargo.toml`, look for `hmac` under `[dependencies]` |
| 8 | Verify `hmac::Mac::verify_slice` uses constant-time comparison | The `hmac` crate uses the `subtle` crate's `ConstantTimeEq` internally. No direct byte comparison (`==`) should appear in the verify path. |

## Phase 2: Fixture Provenance Verification

| Step | Action | Expected |
|------|--------|----------|
| 1 | Open `backend/crates/atc-github/tests/fixtures/` | Directory should contain exactly 7 files |
| 2 | Open `workflow_run_requested.json` and visually inspect | Should look like a real GitHub webhook payload with realistic field values (timestamps, SHA hashes, URLs), not synthetic/minimal JSON |
| 3 | Open `workflow_job_waiting.json` and look for `_synthetic` annotation | The fixture is synthetic since `waiting` events require environment protection rules. Verify the `_synthetic` field explains its provenance. |
| 4 | Verify all 6 real fixtures have consistent `repository.owner.login` and `repository.name` values | All fixtures should reference the same repository |

## Phase 3: End-to-End Parse Pipeline

| Step | Action | Expected |
|------|--------|----------|
| 1 | Run `cd backend && cargo test -p atc-github -- --test-threads=1` | All 42 tests pass, no warnings |
| 2 | Run `cd backend && cargo test -p atc-core -- --test-threads=1` | All 105+ tests pass (including proptest cases), no warnings |
| 3 | Run `cd backend && cargo clippy -p atc-github -p atc-core -- -D warnings` | No clippy warnings or errors |
| 4 | Run `cd backend && cargo doc -p atc-github --no-deps` | Documentation builds without warnings; public API surfaces `verify_signature`, `VerifyError`, `parse_webhook`, `ParseError`, `ParseResult`, `WebhookEvent` |

## Human Verification Required

| Criterion | Why Manual | Steps |
|-----------|-----------|-------|
| gh-webhooks.AC4.8 | Constant-time behavior is a property of the `hmac` crate's implementation, not something automated tests can assert | See Phase 1 above |

## Traceability Matrix

| Acceptance Criterion | Automated Test | Manual Step |
|----------------------|----------------|-------------|
| gh-webhooks.AC1.1 | `types::tests::test_workflow_run_all_fields_populated` | -- |
| gh-webhooks.AC1.2 | `types::tests::test_null_head_commit` | -- |
| gh-webhooks.AC1.3 | `types::tests::test_null_workflow` | -- |
| gh-webhooks.AC1.4 | `types::tests::test_workflow_job_{queued,in_progress,completed,waiting}_fixture` | -- |
| gh-webhooks.AC1.5 | `types::tests::test_workflow_job_null_runner_fields` | -- |
| gh-webhooks.AC1.6 | `types::tests::test_unknown_fields_ignored` | -- |
| gh-webhooks.AC1.7 | `mod::tests::test_parse_malformed_json` | -- |
| gh-webhooks.AC2.1 | `translate::tests::test_translate_run_requested` | -- |
| gh-webhooks.AC2.2 | `translate::tests::test_translate_run_in_progress` | -- |
| gh-webhooks.AC2.3 | `translate::tests::test_translate_run_completed_success` | -- |
| gh-webhooks.AC2.4 | `translate::tests::test_translate_run_all_conclusions` | -- |
| gh-webhooks.AC2.5 | `translate::tests::test_translate_job_queued` | -- |
| gh-webhooks.AC2.6 | `translate::tests::test_translate_job_waiting` | -- |
| gh-webhooks.AC2.7 | `translate::tests::test_translate_job_in_progress_with_runner` | -- |
| gh-webhooks.AC2.8 | `translate::tests::test_translate_job_in_progress_no_runner` | -- |
| gh-webhooks.AC2.9 | `translate::tests::test_translate_job_completed` | -- |
| gh-webhooks.AC2.10 | `translate::tests::test_translate_job_with_steps` | -- |
| gh-webhooks.AC2.11 | `translate::tests::test_translate_run_requested` + `test_translate_run_with_null_workflow` | -- |
| gh-webhooks.AC2.12 | `translate::tests::test_unknown_action_workflow_{run,job}` | -- |
| gh-webhooks.AC2.13 | `translate::tests::test_missing_conclusion_workflow_{run,job}` | -- |
| gh-webhooks.AC2.14 | `translate::tests::test_unknown_conclusion_workflow_{run,job}` | -- |
| gh-webhooks.AC2.15 | `translate::tests::test_unknown_step_status` | -- |
| gh-webhooks.AC3.1 | `event_ingestion::test_ac3_1_waiting_variant_exists` | -- |
| gh-webhooks.AC3.2 | `event_ingestion::test_ac3_2_create_job_from_waiting` | -- |
| gh-webhooks.AC3.3 | `event_ingestion::test_ac3_3_queued_to_waiting_to_inprogress` | -- |
| gh-webhooks.AC3.4 | `event_ingestion::test_ac3_4_in_progress_with_no_runner` | -- |
| gh-webhooks.AC3.5 | All existing tests compile/pass with `Option<RunnerInfo>` | -- |
| gh-webhooks.AC3.6 | `event_ingestion::test_ac3_7_workflow_name_preservation_with_or` | -- |
| gh-webhooks.AC3.7 | `event_ingestion::test_ac3_7_workflow_name_preservation_with_or` | -- |
| gh-webhooks.AC3.8 | `event_ingestion::test_ac3_8_workflow_name_preservation_failure_mode` | -- |
| gh-webhooks.AC4.1 | `verify::tests::test_valid_signature_succeeds` | -- |
| gh-webhooks.AC4.2 | `verify::tests::test_tampered_body_fails` | -- |
| gh-webhooks.AC4.3 | `verify::tests::test_wrong_secret_fails` | -- |
| gh-webhooks.AC4.4 | `verify::tests::test_sha1_algorithm_rejected` | -- |
| gh-webhooks.AC4.5 | `verify::tests::test_unknown_algorithm_fails` | -- |
| gh-webhooks.AC4.6 | `verify::tests::test_invalid_hex_fails` | -- |
| gh-webhooks.AC4.7 | `verify::tests::test_no_equals_separator_fails` | -- |
| gh-webhooks.AC4.8 | -- | Phase 1: Code review of `verify.rs` |
| gh-webhooks.AC5.1 | `types::tests::test_workflow_run_{requested,in_progress,completed}_fixture` | -- |
| gh-webhooks.AC5.2 | `types::tests::test_workflow_job_{queued,in_progress,completed}_fixture` | -- |
| gh-webhooks.AC5.3 | `types::tests::test_workflow_job_waiting_fixture` | -- |
| gh-webhooks.AC5.4 | All 7 fixture tests call `.expect(...)` on deserialization | -- |
| gh-webhooks.AC6.1 | `mod::tests::test_parse_workflow_run_requested` | -- |
| gh-webhooks.AC6.2 | `mod::tests::test_parse_workflow_job_queued` | -- |
| gh-webhooks.AC6.3 | `mod::tests::test_parse_unknown_event_skipped` | -- |
| gh-webhooks.AC6.4 | `mod::tests::test_parse_unknown_event_type_skipped` | -- |
