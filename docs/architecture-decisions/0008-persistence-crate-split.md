# ADR 0008 — Persistence-layer crate split

Date: 2026-05-15
Status: Accepted

## Context

Until this issue, the entire persistence layer lived inside `atc-server`. The `PersistentStore` trait sat in `backend/crates/atc-server/src/persist/mod.rs`, the wire envelope `SeqEvent` and the REST baseline `StateSnapshot` sat in `backend/crates/atc-server/src/state.rs`, and both the in-memory store and the PG-backed store were sibling files under `backend/crates/atc-server/src/persist/`. PG-mode binaries compiled the in-memory state machine that they never executed, and in-memory-mode binaries compiled the entire sqlx-based PG path. Eviction (in-memory) and outbox retention (PG) had no place to live except the executable crate that consumed them.

[Issue #163](https://github.com/coderinserepeat/atc/issues/163) ("eviction task machinery should live inside the persistence store") deferred the crate-split question to [issue #169](https://github.com/coderinserepeat/atc/issues/169). A 7-worker / 3-critic exploration (orchestrated December 2025 through May 2026) examined wire-type placement, trait shape, async-trait promotion, error-shape resolution, test reorganization, and migration sequencing. The synthesis converged on a four-crate split.

[ADR 0005](0005-persistentstore-trait-relocation.md) placed the trait inside `atc-server::persist`. That geographic claim is **revised here**; the architectural reasoning ("trait owned by the layer that wires it") survives the split — the trait is now owned by `atc-persist`, which is the layer every consumer wires through.

[ADR 0006](0006-stores-own-background-task-lifecycle.md) extended the trait with `subscribe()` + `shutdown()` and put per-store background tasks under the store's ownership. The crate split preserves both decisions — the trait surface is unchanged.

## Decision

The persistence layer splits into **four crates** along the cycle constraints:

```
atc-core ─ atc-wire ─ atc-persist ─┬─ atc-store-pg
                                   └─ atc-store-mem
```

| Crate | Role | Direct deps allowed |
|---|---|---|
| **`atc-wire`** | Serializable types that cross the WS / REST boundary: `CommittedEvent`, `StateSnapshot`. Both derive `ts_rs::TS`. | `atc-core`, `atc-github`, `serde`, `ts-rs` |
| **`atc-persist`** | The `PersistentStore` trait, `LivenessError` (opaque-box variant), `PersistError` re-export, and the shared `join_with_timeout` shutdown helper. The interface waist. | `atc-core`, `atc-wire`, `async-trait`, `tokio` (constrained to `["sync", "time", "rt"]`), `tracing` |
| **`atc-store-pg`** | `PgStore`, listener + drain tasks, retention tasks, migrations, `PgMetrics`, `DbInitError`. PG-only. | `atc-core`, `atc-wire`, `atc-persist`, `atc-github`, `sqlx`, `opentelemetry`, `tokio`, `async-trait`, `tracing` |
| **`atc-store-mem`** | `InMemoryStore` + eviction task. NO sqlx. | `atc-core`, `atc-wire`, `atc-persist`, `atc-github`, `tokio`, `async-trait`, `tracing` |

This PR (the first follow-up to the pre-flight) lands `atc-wire` and `atc-persist` plus the `SeqEvent → CommittedEvent` rename, the `LivenessError` opaque-box fix, and the `async-trait` / `tracing` workspace-dep promotions. Subsequent PRs extract `atc-store-pg` and `atc-store-mem`.

### Wire types live in `atc-wire`, not `atc-persist`

The `PersistentStore` trait's `read_snapshot` returns `StateSnapshot`, and its `subscribe()` returns `broadcast::Receiver<CommittedEvent>`. The trait crate must therefore name both types. Keeping the types in the same crate as the trait — call it Option C, "wire types in `atc-persist`" — works technically but forces `atc-persist` to carry direct deps on `serde`, `ts-rs`, and `atc-github`. That pollutes the trait crate with serialization concerns and breaks the rule that the interface crate names interfaces only.

Promoting the types into a peer crate (`atc-wire`) avoids the pollution: `atc-persist` depends on `atc-wire`, so the trait can name both types without naming `serde` or `ts-rs` directly. The two-line cost is one extra crate in the dependency graph; the gain is a trait crate whose `[dependencies]` table is genuinely minimal — `atc-core`, `atc-wire`, `async-trait`, `tokio`, `tracing`.

### `SeqEvent` renamed to `CommittedEvent`

At the point of emission the event has been validated, applied to state, and assigned a monotonic `seq` by the store's commit-order allocator. It is a committed domain event, not a serialization shape — "Seq" described the structural feature, not the semantic role. The frontend rename is mechanical (4 production .ts files, 7 test files, 3 e2e tests, plus the exported `makeJobSeqEvent` harness helper). The `seq` field name is unchanged.

`StateSnapshot` is already well-named and stays.

### `LivenessError::DbUnreachable` carries an opaque-boxed inner error

Before the split, `LivenessError::DbUnreachable(sqlx::Error)` named a sqlx error directly. Moving the enum into `atc-persist` (which forbids sqlx) requires opaqueness:

```rust
pub enum LivenessError {
    DbUnreachable(Box<dyn std::error::Error + Send + Sync + 'static>),
    DrainStale { age_ms: i64 },
}

impl std::error::Error for LivenessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LivenessError::DbUnreachable(e) => Some(e.as_ref()),
            LivenessError::DrainStale { .. } => None,
        }
    }
}
```

`Error::source()` exposes the inner error to log formatters and `/readyz` diagnostics; the routes layer's `error = %e` capture continues to display the sqlx message via `Display` forwarding through the box.

### Workspace `async-trait` + `tracing`

`async-trait` is required by every `impl PersistentStore for *` site across three crates. Pinning it per-crate would diverge silently on a bump. Same for `tracing`: `atc-persist::join_with_timeout` logs on timeout / cancellation, and both store crates use `tracing::info_span!` / `#[tracing::instrument]` extensively. Both go into `[workspace.dependencies]` so the version is named once.

## Rejected alternatives

### Bus inversion — move `subscribe()` off the trait

Moving the broadcast channel ownership to `atc-server` (so the active store no longer surfaces `subscribe()` and `atc-server` constructs the `broadcast::Sender` and injects it) was considered. It would let `atc-persist` drop one trait method. But the store still needs the `Sender` to broadcast `CommittedEvent` from inside `apply_*_event`, so the wire-type precondition does not dissolve — `atc-persist` would still name `CommittedEvent` as a method-parameter type. And the bus capacity (`BROADCAST_CAPACITY = 256`) would become a server-layer concern, which is a regression from the current "store owns its bus" property in ADR 0006.

Rejected: equivalent runtime behavior at meaningfully higher coordination cost.

### Tuple decomposition — replace `broadcast::Receiver<CommittedEvent>` with `broadcast::Receiver<(u64, WebhookEvent)>`

A pure-tuple receiver avoids naming `CommittedEvent` in the trait surface. But `StateSnapshot` (`read_snapshot` return type) does not decompose to a tuple — it has four fields. So the cycle through `atc-github::WebhookEvent` would survive, and the trait crate would still need a peer-crate dep for the snapshot type. Doesn't help.

Rejected: mechanically viable but doesn't move the constraint that motivated the split.

### Wire types in `atc-core`

`atc-core` is the pure domain layer with no tokio, no I/O, no GitHub coupling. Putting `CommittedEvent` there would pull `atc-github::WebhookEvent` into `atc-core`, which inverts the existing dependency direction. `StateSnapshot.runner_pool_capacities` is operator config, not domain state, so it doesn't belong in the domain crate either.

Rejected: violates the existing layering and contaminates the pure-domain crate.

### Wire types stay in `atc-server::state` (status quo)

The reason this PR exists. Forces a Cargo cycle through `WebhookEvent` once the stores are split out of `atc-server` — the stores would need to name `atc_server::state::CommittedEvent`, and `atc-server` would need to name the store crates. Status-quo placement is fatal to the split.

Rejected: the constraint the whole exercise resolves.

## Consequences

- **Active source separation.** PG-mode binaries no longer compile the in-memory state machine (after the `atc-store-mem` extraction). In-memory-mode binaries no longer compile the sqlx PG path. Build times improve at the long tail of the dependency graph.
- **Trait crate is sqlx-free.** `atc-persist/Cargo.toml`'s `[dependencies]` contains exactly five entries: `atc-core`, `atc-wire`, `async-trait` (workspace), `tokio` (constrained to `["sync", "time", "rt"]`), `tracing` (workspace). No storage-library coupling, by construction.
- **One canonical `join_with_timeout`.** Lifted from `atc-server::shutdown.rs:50-68` into `atc-persist::join`. Both store crates consume the same copy; `atc-server::shutdown` imports it for its non-store joins (metrics collector, axum graceful drain).
- **Renames cascade to the frontend.** The TypeScript discriminated union renames from `SeqEvent.ts` to `CommittedEvent.ts`. The exported e2e harness helper `makeJobSeqEvent` becomes `makeJobCommittedEvent`. Operators reading the deployment runbook see `CommittedEvent` in the multi-replica smoke-test acceptance text.
- **Two new CLAUDE.md / AGENTS.md pairs.** `backend/crates/atc-wire/{CLAUDE,AGENTS}.md` and `backend/crates/atc-persist/{CLAUDE,AGENTS}.md` follow the two-tier convention (skeleton + reactive sharp edges) established by the pre-flight PR.
- **ADR-0005's geographic claim is superseded.** The trait no longer lives in `atc-server::persist`; the reasoning ("trait owned by the layer that wires it") survives in the form of the new `atc-persist` interface waist.
- **Subsequent PRs in this series** extract `atc-store-pg` (with `PG-B` file decomposition, `invariants.rs`, and `PgMetrics` extraction) and `atc-store-mem` (with `invariants.rs` and the self-ref dev-dep retirement). The trait + wire-type contract this ADR records is the foundation those PRs build on; the trait does not change shape again.

## References

- Issue: [#169 — refactor: extract atc-persist + atc-store-{pg,mem} crates](https://github.com/coderinserepeat/atc/issues/169)
- Design plan: [`docs/design-plans/2026-05-15-persistence-crate-split.md`](../design-plans/2026-05-15-persistence-crate-split.md)
- [ADR 0005](0005-persistentstore-trait-relocation.md) — superseded geographic claim about trait location
- [ADR 0006](0006-stores-own-background-task-lifecycle.md) — preserved: stores still own `subscribe()` + `shutdown()`
- [ADR 0007](0007-outbox-retention-policy.md) — outbox retention tasks owned by `PgStore`, unchanged by the split
