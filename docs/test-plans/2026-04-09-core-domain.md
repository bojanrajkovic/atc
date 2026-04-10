# Human Test Plan: Core Domain Model

**Implementation plan:** `docs/implementation-plans/2026-04-09-core-domain/`
**Branch:** `docs/core-domain-design`
**Automated tests:** 99 passing (`cargo test -p atc-core`)

---

## Prerequisites

- Rust toolchain 1.94.0+ installed (pinned in `.mise.toml`)
- `cd backend && cargo test -p atc-core` passing (99 tests, 0 failures)
- Branch `docs/core-domain-design` checked out

---

## Phase 1: AC7.1 — No GitHub-specific Dependencies in atc-core

| Step | Action | Expected |
|------|--------|----------|
| 1 | Open `backend/crates/atc-core/Cargo.toml` | `[dependencies]` lists exactly: `chrono`, `serde`, `tokio`, `tracing`. No `octocrab`, `atc-github`, or any GitHub API crate. |
| 2 | Run `cd backend && cargo tree -p atc-core --depth 1` | Output shows only `chrono`, `serde`, `tokio`, and `tracing` as direct dependencies. |
| 3 | Run `cd backend && cargo tree -p atc-core` (full tree) | No crate with "github", "octocrab", or "octokit" appears anywhere in the transitive tree. |

## Phase 2: AC7.2 — Source-Agnostic Event Types

| Step | Action | Expected |
|------|--------|----------|
| 1 | Open `backend/crates/atc-core/src/event.rs` | Contains `RunEvent`, `RunEventEnvelope`, `JobEvent`, `JobEventEnvelope`. |
| 2 | Inspect imports at top of `event.rs` | Only imports from `crate::*` plus `chrono` and `serde`. No external GitHub-specific crates. |
| 3 | Inspect field names in `RunEventEnvelope` and `JobEventEnvelope` | All fields use domain types: `RunId`, `JobId`, `RunConclusion`, `JobConclusion`, `RunnerInfo`, `Step`, `DateTime<Utc>`, `String`. No GitHub webhook field names (no `installation_id`, `sender`, `check_suite_id`). |
| 4 | Inspect enum variants in `RunEvent` and `JobEvent` | Variants are domain-level (`Requested`, `InProgress`, `Completed`) carrying domain types only. |

## Phase 3: Structural Verification — Module Organization

| Step | Action | Expected |
|------|--------|----------|
| 1 | Open `backend/crates/atc-core/src/lib.rs` | Module declarations: `clock`, `event`, `job`, `run`, `store`, `types`. No GitHub adapter modules. |
| 2 | Verify `clock.rs` provides `Clock` trait + `TestClock` | `Clock` trait: `fn now(&self) -> DateTime<Utc>`. `TestClock` wraps `Mutex<DateTime<Utc>>` with `advance()`. |

## Phase 4: Full Test Suite Verification

| Step | Action | Expected |
|------|--------|----------|
| 1 | Run `cd backend && cargo test -p atc-core` | 99 tests pass, 0 failures. Property test generates 256 random cases. |
| 2 | Run `cd backend && cargo clippy -p atc-core -- -D warnings` | Zero warnings. |

---

## Human Verification Required

| Criterion | Why Manual | Steps |
|-----------|-----------|-------|
| AC7.1 (no GitHub dependency) | Structural property of dependency graph — fragile to test via code | Phase 1 above |
| AC7.2 (source-agnostic events) | Semantic intent of field names cannot be asserted at runtime | Phase 2 above |

## Traceability Matrix

| AC | Automated Test | Manual Step |
|----|---------------|-------------|
| AC1.1-1.5 | 19 type/field/serde tests | — |
| AC2.1-2.4 | 24 state transition tests | — |
| AC3.1-3.6 | 11 store ingestion tests | — |
| AC4.1-4.6 | 10 query/pool stats tests | — |
| AC5.1-5.6 | 6 TTL eviction tests | — |
| AC6.1-6.5 | 1 proptest (256 cases) + 8 edge case tests | — |
| AC7.1 | — | Phase 1 |
| AC7.2 | — | Phase 2 |
