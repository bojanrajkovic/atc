# CLAUDE.md — atc-wire

Last verified: 2026-05-23

> Canonical documentation lives in `docs/architecture/backend-server.md` (CommittedEvent Wire Contract section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Serializable wire types that cross the WebSocket and REST boundary to the frontend: `CommittedEvent` (the broadcast envelope a store emits after committing a write) and `StateSnapshot` (the REST baseline payload returned by `GET /v1/state`). Both derive `ts_rs::TS` with `#[ts(export)]` so `just types` regenerates the matching TypeScript modules under `frontend/src/lib/types/generated/`.

This crate sits above `atc-core` and `atc-github` (it names `WebhookEvent`) and below `atc-persist` (the trait crate names both types in its `read_snapshot` return type and `subscribe()` receiver). Lifting these types out of `atc-server` is what lets `atc-persist` stay free of `serde`, `ts-rs`, and `atc-github` direct deps (ADR-0008).

## Key References

- Architecture: `docs/architecture/backend-server.md` § CommittedEvent Wire Contract
- ADR-0008: `docs/architecture-decisions/0008-persistence-crate-split.md`
