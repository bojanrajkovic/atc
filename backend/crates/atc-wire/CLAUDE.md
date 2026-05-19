# CLAUDE.md — atc-wire

Last verified: 2026-05-18

> Canonical documentation lives in `docs/architecture/backend-server.md` (CommittedEvent Wire Contract section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Serializable wire types that cross the WebSocket and REST boundary to the frontend: `CommittedEvent` (the broadcast envelope a store emits after committing a write) and `StateSnapshot` (the REST baseline payload returned by `GET /v1/state`). Both derive `ts_rs::TS` with `#[ts(export)]` so `just types` regenerates the matching TypeScript modules under `frontend/src/lib/types/generated/`.

This crate sits above `atc-core` and `atc-github` (it names `WebhookEvent`) and below `atc-persist` (the trait crate names both types in its `read_snapshot` return type and `subscribe()` receiver). Lifting these types out of `atc-server` is what lets `atc-persist` stay free of `serde`, `ts-rs`, and `atc-github` direct deps (ADR-0008).

## Sharp edges

**`StateSnapshot.accessible_repos_count` is composed at the handler, not the store.** The persistent store constructs every snapshot with `accessible_repos_count: 0`. `routes::state_handler` overwrites the field from the resolved `AccessScope` after the persist call returns — same pattern as `runner_pool_capacities`. The field is `#[serde(default)]` for rolling-deploy tolerance (a snapshot from a replica that lacks the field deserializes to `0`). When you add a new `StateSnapshot` literal anywhere in the workspace — Rust fixture, e2e test, MSW mock — initialize the field explicitly; `serde(default)` only applies on the deserialization path.

## Key References

- Architecture: `docs/architecture/backend-server.md` § CommittedEvent Wire Contract
- ADR-0008: `docs/architecture-decisions/0008-persistence-crate-split.md`
