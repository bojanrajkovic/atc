# Issue #172 — Hot-reload runner_pools config without restart

## Context

Issue #16 (closed by PR #177, design at `docs/design-plans/2026-05-13-issue-16-runner-pool-capacity.md`) landed file-based `runner_pools` config: operators declare pools in `/etc/atc/config.yaml`, the figment chain `defaults → Yaml::file → Env::prefixed("ATC_").split("__")` loads it, validation rejects zero capacity / empty labels / duplicate canonicalized label sets, and `AppState.runner_pool_capacities: Vec<RunnerPoolCapacity>` is built once at startup and composed into every `StateSnapshot` at the route layer (`routes::state_handler`, `routes.rs:96`).

Today, edits to the ConfigMap propagate to the pod's filesystem within ~60s via kubelet sync, but ATC ignores them — operators have to roll the Deployment to pick up new capacities. The Out-of-Scope section of the #16 plan calls this out and links here (#172): the ask is a `notify`-based watcher that re-reads the file on change and pushes new capacities to open browsers over the WS without a reload.

Three structural facts shape the design:

1. **Kubernetes ConfigMap mounts use the `..data` symlink swap pattern, but `subPath` blocks updates.** The chart today mounts the ConfigMap via `subPath: config.yaml` (`deploy/helm/atc/templates/deployment.yaml:166–168`). Kubernetes explicitly documents that `subPath` ConfigMap mounts do **not** receive updates — kubelet refreshes the projected directory, not the subPath alias. Hot-reload therefore requires switching to a directory mount.
2. **The current wire envelope (`atc-wire::CommittedEvent`) wraps `atc-github::WebhookEvent` with a monotonic `seq`.** A `ConfigUpdate` event is not store-derived, has no sequence number, and doesn't fit the GitHub crate semantically. The chosen wire approach (decided in clarification) is an outer `kind` discriminator framed only at the `ws.rs` boundary — keeping `CommittedEvent` and `WebhookEvent` untouched and confining the new wire concept to the WS handler.
3. **`ConnectionManager` buffers `CommittedEvent`s by seq for pre-snapshot replay.** `frontend/src/lib/connection.ts:23` carries a `preConnectBuffer: CommittedEvent[]`, replayed against `snapshotLastSeq` at line 122–126. New non-seq frames (`ConfigUpdate`, `ConfigReloadError`) need explicit pre-snapshot semantics: they cannot be deduped by seq, and silently buffering them with no replay rule would either drop or duplicate them.

## Definition of Done

### Schema

Edits to `/etc/atc/config.yaml` (or whatever path `$ATC_CONFIG_FILE` points at) are detected within ~1s of kubelet completing the symlink swap. The file is re-loaded through the same validation path used at startup. If validation passes and the new capacities differ from the current ones, `AppState.runner_pool_capacities` is replaced atomically and a `ConfigUpdate` WS frame is broadcast to all open clients. If validation fails (zero capacity, duplicate pool, malformed YAML, read error, missing file), the old capacities are kept, a structured error is logged, a counter metric increments, and a `ConfigReloadError { reason }` WS frame is broadcast (frontend UI surfacing deferred to #203).

### Deliverables

1. **Helm chart: drop `subPath` from the runner-pool-config volume mount.** Change `deploy/helm/atc/templates/deployment.yaml:165–169` from `mountPath: /etc/atc/config.yaml` + `subPath: config.yaml` to `mountPath: /etc/atc` (directory mount). The ConfigMap key `config.yaml` becomes the file `/etc/atc/config.yaml` inside the mounted projected directory. This enables kubelet's ConfigMap update propagation that `subPath` mounts disable. Update helm-unittest specs accordingly. No `values.yaml` / `values.schema.json` shape change.
2. **`config_watcher` module** (new file `backend/crates/atc-server/src/config_watcher.rs`). Long-lived async task spawned from `main.rs`. Watches the parent directory of `Config::config_path()` using `notify-debouncer-full` (500ms debounce). On debounced event whose tracked path includes the configured file, calls `config::reload_runner_pools(path) -> Result<Vec<RunnerPoolCapacity>, ReloadError>`, then writes through to `AppState` and broadcasts. Wires into the existing `shutdown::run_shutdown_orchestration` join chain.
3. **`AppState.runner_pool_capacities` mutability change.** Field becomes `tokio::sync::RwLock<Vec<RunnerPoolCapacity>>` (no new dep — `tokio` already provides; Tokio's `RwLock` is write-preferring per its docs, so reader starvation is not a concern). `routes::state_handler` updates to `state.runner_pool_capacities.read().await.clone()`. The watcher writer takes `.write().await` and replaces, with the equality check performed **inside** the write guard (TOCTOU-safe).
4. **WS framing extension.** New broadcast channel on `AppState`: `config_events_tx: broadcast::Sender<ConfigEvent>` where `ConfigEvent` is a small enum (`Update(Vec<RunnerPoolCapacity>)` and `ReloadError { reason: String }`) defined in `config_watcher.rs`. WS handler `tokio::select!`s on both `committed` and `config` receivers. Both receivers close the socket on `RecvError::Lagged` (matching today's behavior — the client reconnects and re-fetches `/v1/state`; symmetric handling avoids the silent-drop trap). Before serializing, the WS handler wraps each event in a `WireFrame` enum defined in `ws.rs`: `#[derive(Serialize, ts_rs::TS)] #[serde(tag = "kind", rename_all = "camelCase")]` so the frontend gets a generated TS union with camelCase field names matching `CommittedEvent`'s existing convention.
5. **Frontend changes:**
   - `dispatcher.ts`: outer-`kind` switch (Committed → existing Run/Job path; ConfigUpdate → `runStore.applyConfigUpdate`; ConfigReloadError → `console.warn` with reference to #203).
   - `connection.ts`: parses the outer frame as `WireFrame`. Pre-snapshot semantics:
     - Existing `preConnectBuffer: CommittedEvent[]` stays as-is (seq-keyed replay).
     - **New**: a single `pendingConfigUpdate: RunnerPoolCapacity[] | null` slot, overwritten by each pre-snapshot ConfigUpdate frame (only the latest matters — `ConfigUpdate` carries the full current capacities, not a delta). On snapshot ready (after `runStore.loadSnapshot`), if `pendingConfigUpdate !== null`, apply it on top of the snapshot's capacities and clear the slot. This handles the race where a ConfigUpdate fires between snapshot generation and snapshot fetch.
     - Pre-snapshot `ConfigReloadError` frames are dropped (informational only; the next reload event will repaint state if needed).
   - `runs.svelte.ts`: add `applyConfigUpdate(capacities: RunnerPoolCapacity[])` method (assigns to the existing `runnerPoolCapacities` slice).
6. **Metrics** registered in `backend/crates/atc-server/src/metrics.rs` (or appropriate module), following `docs/architecture/metrics.md` § "Metric and span authoring contract":
   - `atc_config_reload_total{result="success"|"failure",reason?}` — sync `Counter<u64>` (events, monotonic).
   - `atc_config_runner_pools` — `ObservableGauge<f64>` with callback reading `Arc<AtomicI64>` (current count of loaded pools). Atomic is updated by the watcher on each successful reload; gauge callback re-reports on every collection cycle. Matches the existing convention for `atc_pg_broadcast_watermark` et al.
7. **Tests** per the Implementation Phases below, including:
   - Backend unit tests for `reload_runner_pools` (all error variants + happy path), `#[serial_test::serial]` + the existing `EnvGuard` + `write_yaml` pattern.
   - Backend integration tests for the watcher (atomic-rename, bad reload, no-op skip, K8s `..data` symlink swap simulation, missing-parent-dir, shutdown integration).
   - Backend integration tests for WS framing of both event types.
   - Frontend `ConnectionManager` tests using `msw/node` (per `docs/implementation-guidance.md:51–53`) — intercept WS at the network level, send synthetic frames, assert dispatch behavior. Direct dispatcher tests for the switch logic.
   - E2E test asserting capacity updates without page reload.
   - `helm-unittest` specs for the directory-mount change.
8. **Documentation updates** per the Documents to Update section.

### Success criteria

- Edit ConfigMap → wait ≤90s (kubelet sync + 500ms debounce) → an existing open browser shows the new `running/capacity` values without reload, with no console errors.
- Edit ConfigMap to introduce a duplicate pool → backend logs error, `atc_config_reload_total{result="failure",reason="validate"}` increments, the WS broadcasts a `ConfigReloadError`, and `/v1/state` still returns the previous (valid) capacities.
- Delete the config file → backend treats as ReloadError::Read (deliberate divergence from startup, which tolerates missing file); AppState capacities unchanged; operator sees the error in logs/metrics/WS.
- Process restart re-establishes the watcher; behavior unchanged from steady-state otherwise.
- Dev/in-memory mode (no config file, no `ATC_CONFIG_FILE` override) continues to boot cleanly; the watcher arms on the parent directory if it exists; if the parent dir also doesn't exist (e.g., bare-metal dev box with no `/etc/atc/`), the watcher is skipped with a warning log; in neither case does boot fail.
- Process shutdown via `CancellationToken` causes the watcher task to exit cleanly within a bounded budget (`SHUTDOWN_TIMEOUT_CONFIG_WATCHER`); joined explicitly by `run_shutdown_orchestration` before `otel::shutdown` per the "no live emitter when shutdown fires" invariant.
- Existing WS clients (browsers loaded before this deploy) will fail to parse the new outer-`kind` frame shape. **Documented caveat:** during the rolling-deploy window of this release, operators should expect to reload tabs once. Future releases that change wire shape should bump the WS endpoint version (`/v2/ws`) instead; this PR accepts the one-time cost.
- `just lint`, `just test`, `just types` pass. E2E green. `helm lint` + helm-unittest green. Pre-push doc-staleness gate passes — every source file change has its corresponding architecture doc update.

## Locked Decisions

1. **Wire shape: outer `kind` discriminator, framed only at the `ws.rs` boundary.** `WireFrame` is a `pub` enum in `ws.rs` (ts-rs requires at least crate-visibility for `#[ts(export)]` to emit). It carries `#[serde(rename_all = "camelCase")]` to match `CommittedEvent` / `StateSnapshot` convention. `CommittedEvent` and `WebhookEvent` are not modified.
2. **Mutability: `tokio::sync::RwLock<Vec<RunnerPoolCapacity>>`.** No new dep, async-aware, write-preferring (Tokio docs confirm — no starvation), trivial cost at ATC's snapshot-read rate.
3. **Bad-reload behavior: keep old config, log + metric + broadcast `ConfigReloadError`.** Graceful degradation. Frontend banner surfacing deferred to **#203** (filed).
4. **File watcher: `notify-debouncer-full` (500ms window), no version pin.** `notify-debouncer-full` (not `-mini`) is chosen explicitly because it tracks rename/path changes, which is what the K8s `..data` symlink swap pattern produces. `notify-debouncer-mini` only dedupes raw paths and does not track renames — its event `paths` field is best-effort and backend-dependent. `-full` exists precisely for this use case. Per `docs/implementation-guidance.md` "never pin library versions," the Cargo.toml entry omits a version and Renovate manages bumps.
5. **Watch the parent directory.** The K8s `..data` symlink swap requires parent-dir watching. The `notify-debouncer-full` tracker reports the swap event with a path resolving to (or containing) the configured filename; the watcher reloads on any tracked event under the parent — the basename check is best-effort (a non-`config.yaml` rename could trigger a no-op reload, harmless because of the equality check in Decision 7).
6. **Watcher always spawns when the parent dir exists.** Even in dev/in-memory mode with no file present. If the parent dir itself doesn't exist (e.g., dev box with no `/etc/atc/`), the watcher is skipped (warn-logged); no failure to boot.
7. **Equality check inside the write guard.** Take `.write().await`, compare current to new, if equal release without broadcast. If different, replace and broadcast while still holding the guard. This avoids the read-then-write TOCTOU even though we have a single watcher (defensive against future racy callers).
8. **No initial broadcast at startup.** First-time caps are delivered via the snapshot rail (`/v1/state`) on connect. The WS channel carries deltas only.
9. **`runner_pools` is the only reloadable field.** `reload_runner_pools` parses a **narrow schema** — a struct that only declares `runner_pools` — so the watcher never observes scalar fields (`http_addr`, DB URLs, log settings, retention) on the file. This eliminates the "silent scalar ignore" footgun: an operator editing a scalar in the YAML doesn't *appear* to have its change accepted-then-discarded; the watcher simply isn't looking at that field. The watcher emits a `tracing::warn!` on each reload if the full-Config parse (used only for tracing-side detection, not behavior) shows scalar drift from the startup snapshot, naming the changed fields so the operator knows to roll the deployment. Out-of-scope for v1: hot-reloading scalar fields, which requires per-field reload safety analysis.
10. **Missing file on reload = `ReloadError::Read` (documented divergence from startup).** `Config::load()` at startup tolerates a missing file (figment's `Yaml::file()` is auto-optional, yielding `runner_pools: []`). On reload, missing-file is treated as an error — an operator who deletes the file mid-deploy almost certainly didn't intend to clear all pool capacities. Old caps are kept; a `ConfigReloadError { reason: "read" }` is broadcast.
11. **Lagged on either WS channel closes the socket.** Symmetric with today's `CommittedEvent` handling (`backend/crates/atc-server/CLAUDE.md` "Broadcast semantics" + `ws.rs:82`). The client reconnects, refetches `/v1/state`, and re-establishes both seq cursor and capacities. No silent drops on either channel.
12. **`subPath` removed in this PR.** The chart change is part of this design, not a follow-up. Without it, the rest is non-functional in K8s.
13. **Frontend assumes the new `WireFrame` shape, no legacy fallback.** Browsers loaded with the pre-deploy bundle will fail to parse new frames. Documented as a one-time release caveat; users reload. Future wire-shape changes should use a `/v2/ws` path; this PR accepts the cost as a deliberate "single-tenant first-party app" trade-off.

## Architecture

### Helm chart change

- **File:** `deploy/helm/atc/templates/deployment.yaml:162–179`
- **Today** (lines 165–169):
  ```yaml
  {{- if gt (len .Values.runnerPools) 0 }}
  - name: runner-pool-config
    mountPath: /etc/atc/config.yaml
    subPath: config.yaml
    readOnly: true
  {{- end }}
  ```
- **Change:**
  ```yaml
  {{- if gt (len .Values.runnerPools) 0 }}
  - name: runner-pool-config
    mountPath: /etc/atc
    readOnly: true
  {{- end }}
  ```
  Directory mount preserves the `..data` symlink swap pattern and lets kubelet propagate ConfigMap updates. The ConfigMap key `config.yaml` becomes `/etc/atc/config.yaml` in the projected directory — unchanged from the operator's perspective. The `..data` symlink lives at `/etc/atc/..data`, which is read-only and ignored by `Config::load()` (only `$ATC_CONFIG_FILE` is read, default `/etc/atc/config.yaml`). With `readOnlyRootFilesystem: true`, the projected ConfigMap volume is mountable read-write at the volume level and the mount is read-only at the container level.
- **helm-unittest update** (`deploy/helm/atc/tests/unit/`): the existing test asserting the `subPath: config.yaml` line is changed to assert the directory mount + the absence of `subPath`. Add a test that the read-only mount and the `defaultMode: 0444` ConfigMap-volume settings remain unchanged.
- **Documentation:** `deploy/helm/atc/CLAUDE.md` "Runner-pool capacities gating" bullet is updated to drop the `subPath` reference and explain why (kubelet propagation requires directory mount).

### Backend — config_watcher module

- **New file:** `backend/crates/atc-server/src/config_watcher.rs`
- **Cargo.toml** dep added (no version pin): `notify-debouncer-full = { workspace = false }` — actual entry follows the repo's existing Cargo.toml style; Renovate manages version.
- **Public API:**
  ```rust
  pub fn spawn_config_watcher(
      config_path: PathBuf,
      app_state: Arc<AppState>,
      config_events_tx: broadcast::Sender<ConfigEvent>,
      shutdown: CancellationToken,
  ) -> Option<JoinHandle<()>>;

  #[derive(Debug, Clone)]
  pub enum ConfigEvent {
      Update(Vec<RunnerPoolCapacity>),
      ReloadError { reason: String },
  }
  ```
  Returns `None` (with `warn!`) if `config_path.parent()` doesn't exist. Otherwise spawns the watcher task and returns its `JoinHandle`. The returned handle is required by `run_shutdown_orchestration` (no `let _ = …`).

- **Watcher loop:**
  1. Build `notify_debouncer_full::new_debouncer(Duration::from_millis(500), None, callback)` where `callback: impl Fn(DebounceEventResult)`. The callback runs in notify's thread pool and forwards a unit signal into a bounded `tokio::sync::mpsc::Sender<()>` (collapse all events into a single "something happened" signal since we re-read the whole file anyway).
  2. Watch `config_path.parent()` with `RecursiveMode::NonRecursive`.
  3. Run a `tokio::select!` loop on `(mpsc_rx.recv(), shutdown.cancelled())`. Each mpsc tick triggers `reload_runner_pools(&config_path)`.
  4. `reload_runner_pools` reads + parses + validates: parses a **narrow schema** (`struct ReloadPayload { runner_pools: Vec<RunnerPoolConfig> }`) from the YAML file, runs the same canonicalization + validation as `Config::validate_runner_pools` (extracted into a shared helper called by both `Config::load` and `reload_runner_pools`), converts to `Vec<RunnerPoolCapacity>`. Returns `Result<Vec<RunnerPoolCapacity>, ReloadError>` where `ReloadError` carries a `reason: String` and a categorized variant (`Read`, `Parse`, `Validate`).
  5. On `Ok(new_caps)`: take `app_state.runner_pool_capacities.write().await`. Compare to `new_caps`. If equal, drop the guard, increment `atc_config_reload_total{result="success",reason="noop"}`, return (no broadcast). If different, replace, drop the guard, update the `Arc<AtomicI64>` backing `atc_config_runner_pools`, increment `atc_config_reload_total{result="success",reason="applied"}`, broadcast `ConfigEvent::Update(new_caps)`. (The "compare and replace inside the same guard" detail in Decision 7: the comparison and replacement both happen while holding the write lock, so a concurrent writer can't squeeze in between.)
  6. On `Err(reload_err)`: increment `atc_config_reload_total{result="failure",reason=<category>}`, log structured error, broadcast `ConfigEvent::ReloadError { reason: reload_err.to_string() }`. State unchanged.
  7. Additionally, on each reload attempt, parse a **full** `Config` from the same file (suppress errors), diff its scalar fields against a `startup_scalars: ScalarSnapshot` captured at spawn time, and `tracing::warn!` per changed scalar field listing the field name. Pure diagnostic — does not affect AppState or broadcasts. Implements Decision 9's "no silent scalar ignore" requirement.

- **Filename filtering:** The debouncer's event includes a `Vec<DebouncedEvent>` each with `paths: Vec<PathBuf>`. Filter best-effort: react if **any** path in **any** event matches `config_path.file_name()` (or `..data` for the K8s symlink case, which `notify-debouncer-full` tracks). Even if the filter misfires (matches an unrelated rename), the equality check in step 5 makes the reload idempotent.

### Backend — AppState change

- **File:** `backend/crates/atc-server/src/state.rs:31`
- **Today:** `pub runner_pool_capacities: Vec<RunnerPoolCapacity>` (immutable; built once in `main.rs`).
- **Change:** `pub runner_pool_capacities: tokio::sync::RwLock<Vec<RunnerPoolCapacity>>`. Initialize in `main.rs` as `RwLock::new(initial_capacities)`. Add a new field `pub config_events_tx: broadcast::Sender<ConfigEvent>`. AppState gains 7 fields total (per backend CLAUDE.md): existing 5 + the lock-wrapped capacities (no field count change since it replaces the existing slot) + `config_events_tx`.
- **Route handler update** (`backend/crates/atc-server/src/routes.rs:96`):
  - From: `snap.runner_pool_capacities = state.runner_pool_capacities.clone();`
  - To: `snap.runner_pool_capacities = state.runner_pool_capacities.read().await.clone();`

### Backend — main.rs + shutdown integration

- **File:** `backend/crates/atc-server/src/main.rs`
- **Changes:**
  - Capture a `startup_scalars: ScalarSnapshot` from `Config` before constructing AppState (cheap clone of relevant scalar fields).
  - Create the broadcast channel: `let (config_events_tx, _) = broadcast::channel::<ConfigEvent>(256)`. Capacity 256 matches the existing `CommittedEvent` channel (per `atc-server/CLAUDE.md` "Broadcast semantics") and gives slow clients reasonable headroom against a debugging operator who edits the file repeatedly.
  - Extract `let config_path: PathBuf = config::config_path()` (new helper in `config.rs` that returns the same path `Config::load()` reads from; refactor `Config::load()` to use the helper for consistency).
  - Build AppState with the new `RwLock` and `config_events_tx`.
  - After AppState is `Arc`'d: `let config_watcher_handle = config_watcher::spawn_config_watcher(config_path, Arc::clone(&app_state), config_events_tx.clone(), shutdown.clone());`
  - Pass `config_watcher_handle` (an `Option<JoinHandle<()>>`) into `run_shutdown_orchestration` (new parameter).
- **File:** `backend/crates/atc-server/src/shutdown.rs`
  - Add `pub const SHUTDOWN_TIMEOUT_CONFIG_WATCHER: Duration = Duration::from_secs(1);` (the watcher only has to drop its debouncer and exit; 1s is generous).
  - Add `config_watcher_handle: Option<JoinHandle<()>>` parameter to `run_shutdown_orchestration`.
  - Join it in Step 4 (between `persist.shutdown()` and `metrics_handle.shutdown()`) via `join_with_timeout(handle, SHUTDOWN_TIMEOUT_CONFIG_WATCHER, "config_watcher").await` when `Some`.
  - Extend the "no live emitter when shutdown fires" emitter-categories comment at `shutdown.rs:202–214` to include: `4. config_watcher (the file-watcher task — joined via the new parameter)`.

### Backend — WS handler

- **File:** `backend/crates/atc-server/src/ws.rs`
- **Today:** Subscribes to `state.persist.subscribe()` (`broadcast::Receiver<CommittedEvent>`), forwards each as a JSON text frame of `CommittedEvent`'s Serialize output. Closes the socket on `RecvError::Lagged` (per backend CLAUDE.md "Broadcast semantics").
- **Changes:**
  - Define a `pub` `WireFrame` enum:
    ```rust
    #[derive(serde::Serialize, ts_rs::TS)]
    #[serde(tag = "kind", rename_all = "camelCase")]
    #[ts(export)]
    pub enum WireFrame {
        Committed(CommittedEvent),
        ConfigUpdate { runner_pool_capacities: Vec<RunnerPoolCapacity> },
        ConfigReloadError { reason: String },
    }
    ```
    ts-rs exports it to `frontend/src/lib/types/generated/WireFrame.ts`. `rename_all = "camelCase"` ensures `runner_pool_capacities` serializes as `runnerPoolCapacities`, matching the existing `StateSnapshot.runnerPoolCapacities` convention.
  - Subscribe to both channels.
  - Replace the existing single-channel loop with `tokio::select!` on both receivers. Each branch maps the received event into the appropriate `WireFrame` variant, serializes via `serde_json::to_string`, and sends as `Message::Text`. **Both branches close the socket on `Err(RecvError::Lagged(_))`**, matching today's behavior — log a warning, break the loop. Client reconnects via the existing `ConnectionManager` reconnect path.

### Frontend — dispatcher + connection refactor

- **dispatcher.ts** — accepts `WireFrame` instead of `CommittedEvent`. Outer-kind switch:
  ```ts
  switch (frame.kind) {
    case 'Committed':
      // Existing RAF-coalesced path on frame.event.type
      this.bufferOrDispatch(frame)
      break
    case 'ConfigUpdate':
      runStore.applyConfigUpdate(frame.runnerPoolCapacities)
      break
    case 'ConfigReloadError':
      console.warn(`Config reload failed on server: ${frame.reason}. See https://github.com/bojanrajkovic/atc/issues/203 for UI surfacing.`)
      break
    default: { /* unknown-kind, log once */ }
  }
  ```
  `ConfigUpdate` / `ConfigReloadError` are out-of-band (not buffered through RAF) — low volume, deserves prompt application.
- **connection.ts** — pre-snapshot frame handling:
  - Existing `preConnectBuffer: CommittedEvent[]` stays; replay is unchanged (`seq > snapshotLastSeq`).
  - New `pendingConfigUpdate: RunnerPoolCapacity[] | null = null`. On pre-snapshot `ConfigUpdate` frame: overwrite the slot (latest wins; full state, not delta).
  - In the snapshot-ready step (after `runStore.loadSnapshot(...)`), if `pendingConfigUpdate !== null`, call `runStore.applyConfigUpdate(pendingConfigUpdate); pendingConfigUpdate = null;`.
  - Pre-snapshot `ConfigReloadError` frames are dropped (next reload will repaint state if needed).
  - `onmessage` parses the outer `WireFrame` shape; the existing `jsonReviver` for bigint fields continues to apply to nested `CommittedEvent.seq`.
- **runs.svelte.ts** — add `applyConfigUpdate(capacities: RunnerPoolCapacity[])`: `this.runnerPoolCapacities = capacities`. Svelte 5 runes propagate; `computePoolStats` recomputes.

### Doc-mapping

- **File:** `scripts/doc-mapping.sh`
- **Change:** add a straddler entry for `config_watcher.rs` **above** the `backend/crates/atc-server/src/*` catch-all at line 86. Maps the file to both `docs/architecture/backend-server.md` and `docs/architecture/metrics.md` so the staleness gate enforces both updates when the watcher changes.

### Why these choices

- **`notify-debouncer-full` over `-mini`:** `-mini` only dedupes raw paths; `-full` tracks rename/path changes, which is what the K8s `..data` symlink swap produces. Codex review (the project's external review skill) specifically flagged the `-mini` choice as relying on backend-specific notify behavior. `-full` is the explicit answer for production K8s use.
- **Anonymous `WireFrame` in `ws.rs` over promoting to `atc-wire`:** Per Decision 1 (user-confirmed), keep WS framing local. Stores remain pure event sources.
- **Equality check inside the write guard:** TOCTOU-safe; cheaper than separate read+write phases; no concurrency-policy hand-waving.
- **Narrow-schema reload + scalar-drift warn log:** Eliminates the "silent ignore" footgun without re-architecting hot-reload across all config surfaces.
- **`subPath` removal in this PR:** Without it, hot-reload literally cannot work. Codex flagged "no chart change" as a deal-breaker; the fix is in scope.

## Implementation Phases

Each phase has Step 1 (failing tests) and Step 2 (make them pass).

### Phase 0 — Branch + plan commit (per `docs/planning-workflow.md` Phase 7)

Create feature branch `feat/172-hot-reload-runner-pools` (or matching naming convention). Copy this plan from `~/.claude/plans/bright-gliding-iverson.md` to `docs/design-plans/2026-05-15-issue-172-hot-reload-runner-pools.md`. Commit on the feature branch with message `docs(design): hot-reload runner_pools (#172)`. This is the artifact the rest of the context reads from.

### Phase 1 — Helm chart: drop `subPath`

**Step 1: Failing test.** Update `deploy/helm/atc/tests/unit/`: change the existing "runner-pool-config subPath" assertion to expect a directory mount and the absence of `subPath`. Add a new test confirming the ConfigMap volume's `defaultMode: 0444` and the container-level `readOnly: true` remain set.

**Step 2: Implementation.** Edit `deploy/helm/atc/templates/deployment.yaml:165–169` per the Architecture section. Run `helm lint` + `helm unittest` to confirm green.

### Phase 2 — Config reload helper + scalar-snapshot

**Step 1: Failing tests** in `backend/crates/atc-server/src/config.rs` (each `#[serial_test::serial]`, using the existing `EnvGuard` + `write_yaml` pattern):
- `reload_runner_pools_returns_caps_from_yaml` — happy path, two pools, assert returned `Vec<RunnerPoolCapacity>`.
- `reload_runner_pools_rejects_zero_capacity` → `Err(ReloadError::Validate(_))`.
- `reload_runner_pools_rejects_duplicate_pool` → `Err(ReloadError::Validate(_))`.
- `reload_runner_pools_rejects_malformed_yaml` → `Err(ReloadError::Parse(_))`.
- `reload_runner_pools_treats_missing_file_as_read_error` → `Err(ReloadError::Read(_))`. Document the divergence from startup in the test docstring.
- `reload_runner_pools_uses_narrow_schema_no_scalar_leakage` — write a YAML with both `runner_pools` and `http_addr: "0.0.0.0:9999"`. Assert the reload returns the pools without complaint and without affecting any global state.
- `scalar_snapshot_diff_detects_changed_field` — unit test for the `ScalarSnapshot::diff` helper used by the warn-log diagnostic.

**Step 2: Implementation.** Add `pub fn reload_runner_pools(path: &Path) -> Result<Vec<RunnerPoolCapacity>, ReloadError>` to `config.rs`. Implement via a narrow `ReloadPayload` struct that derives only `runner_pools`. Add `pub fn config_path() -> PathBuf` helper sourcing from `$ATC_CONFIG_FILE` (refactor `Config::load()` to use it). Add `ScalarSnapshot { http_addr, database_url, … }` + `fn diff(&self, other: &Self) -> Vec<&'static str>`. Extract the canonicalization/validation logic from `validate_runner_pools` into a shared helper consumed by both `Config::load` and `reload_runner_pools`.

### Phase 3 — AppState mutability + state_handler

**Step 1: Failing tests** in `backend/crates/atc-server/tests/integration/`:
- Update existing tests that construct `AppState` (compiler will name them — `ws_tests.rs:38`, `common/mod.rs:368`, others) to use `RwLock::new(vec![])` for the capacities field and to add the `config_events_tx` field.
- New test `mutating_app_state_capacities_reflects_in_next_snapshot` — mutate via `state.runner_pool_capacities.write().await` then call the state route, assert snapshot reflects new caps.

**Step 2: Implementation.** Change `AppState.runner_pool_capacities` to `RwLock<Vec<_>>`. Add `config_events_tx`. Update `main.rs` construction. Update `routes.rs:96` to `read().await.clone()`. No behavior change yet — just the shape.

### Phase 4 — config_watcher module + shutdown integration

**Step 1: Failing tests** (new `backend/crates/atc-server/tests/integration/config_watcher_tests.rs`):
- `watcher_detects_atomic_rename_and_updates_state` — spawn watcher on tempdir, write initial valid file, snapshot via `state_handler` confirms initial state; atomic-rename a new file with different pools; subscribe to `config_events_tx`, wait (bounded timeout) for `ConfigEvent::Update`; assert AppState updated.
- `watcher_emits_reload_error_on_bad_file` — same setup, rewritten file has `capacity: 0`; assert `ConfigEvent::ReloadError` arrives; AppState unchanged.
- `watcher_skips_broadcast_on_identical_content` — write file, wait for first event, rewrite identical content; assert no second `ConfigEvent::Update` within bounded timeout. (A `noop` success metric increment is allowed.)
- `watcher_treats_file_deletion_as_reload_error` — delete the file; assert `ConfigEvent::ReloadError { reason: contains "read" }` arrives; AppState unchanged.
- `watcher_handles_kubernetes_symlink_swap` — create tempdir with the `..data → ..data_TS/`, `config.yaml → ..data/config.yaml` pattern; create new `..data_TS2/config.yaml` with different content; atomic-rename `..data → ..data_TS2`. Assert watcher fires `ConfigEvent::Update` matching the new content. Use `std::os::unix::fs::symlink`. Document in the test that this approximates but does not fully reproduce kubelet's behavior; the real validation is in the K8s smoke test in Phase 7 docs.
- `watcher_skip_when_parent_dir_missing` — call `spawn_config_watcher` with a nonexistent parent; assert `None` returned + warn log captured.
- `watcher_warn_logs_scalar_drift` — write a file with both `runner_pools` and a non-default `http_addr`; subscribe to a `tracing` capture; assert a warn log naming `http_addr` is emitted; assert AppState pools are updated and no error broadcast.
- `watcher_joined_in_shutdown_orchestration` — spawn watcher, immediately fire `shutdown.cancel()`; assert the watcher's `JoinHandle` resolves within `SHUTDOWN_TIMEOUT_CONFIG_WATCHER`. Reuse the `run_shutdown_orchestration` test pattern from `shutdown.rs:272–314`.

**Step 2: Implementation.** Add `notify-debouncer-full` to `Cargo.toml` (no version pin). Create `config_watcher.rs` per Architecture. Add `SHUTDOWN_TIMEOUT_CONFIG_WATCHER` to `shutdown.rs`; extend `run_shutdown_orchestration` signature with `config_watcher_handle: Option<JoinHandle<()>>`; wire it into the join chain at Step 4; update the emitter-categories comment. Wire from `main.rs`.

### Phase 5 — WS framing + frontend dispatcher

**Step 1: Failing tests:**
- Backend integration test (extend `ws_tests.rs`): connect WS client, write new config file, assert client receives `{"kind":"ConfigUpdate","runnerPoolCapacities":[...]}` (camelCase! verify casing). Second case: write bad file, assert `{"kind":"ConfigReloadError","reason":...}`. Third case: a `Committed` frame from a webhook arrives as `{"kind":"Committed",...}` wrapping the existing `CommittedEvent` shape.
- Backend test for **Lagged-on-config-channel close behavior**: subscribe a slow WS client, fire enough config reloads to exceed the channel capacity, assert the WS connection closes (matching existing `CommittedEvent`-Lagged behavior).
- Frontend `ConnectionManager` tests using `msw/node` per `docs/implementation-guidance.md:51–53`:
  - `pre_snapshot_config_update_applied_after_snapshot` — open mock WS, server sends a `ConfigUpdate` frame before the `/v1/state` response; assert that after the snapshot loads, `runStore.runnerPoolCapacities` reflects the pending update, not the snapshot's initial value.
  - `pre_snapshot_config_reload_error_dropped` — server sends a `ConfigReloadError` pre-snapshot; assert no UI state change, no thrown error, console.warn fired exactly once.
  - `post_snapshot_config_update_applied_immediately` — server sends `ConfigUpdate` after the snapshot; assert immediate application without RAF batching.
  - `lagged_config_channel_triggers_reconnect` — server-sent close frame after a Lagged simulation; assert reconnect cycle starts.
- Frontend dispatcher unit test (`frontend/src/lib/dispatcher.test.ts`): given a `Committed`-kind frame, asserts existing Run/Job path invoked. Given `ConfigUpdate`, asserts `runStore.applyConfigUpdate` called with capacities. Given `ConfigReloadError`, asserts `console.warn` called once with `#203` reference.

**Step 2: Implementation.** Define `WireFrame` in `ws.rs` per Architecture (with `rename_all = "camelCase"`). Add second `tokio::select!` arm with Lagged-close discipline. Run `just types` to regenerate `WireFrame.ts`. Update `dispatcher.ts` (outer-kind switch). Update `connection.ts` (`pendingConfigUpdate` slot, snapshot-step drain, `ConfigReloadError` pre-snapshot drop). Add `runStore.applyConfigUpdate(capacities)`.

### Phase 6 — Metrics + observability

**Step 1: Failing tests.** Backend integration test:
- Trigger a successful (changed) reload → assert `atc_config_reload_total{result="success",reason="applied"}` increments by 1; `atc_config_runner_pools` gauge reads the new pool count.
- Trigger a no-op reload (identical content) → assert `atc_config_reload_total{result="success",reason="noop"}` increments by 1; gauge unchanged.
- Trigger a bad reload → assert `atc_config_reload_total{result="failure",reason="validate"}` increments by 1; gauge unchanged.

**Step 2: Implementation.** Register the counter in `metrics.rs` (cached `Counter<u64>` per the cached-instrument convention in `metrics.md`). Register `atc_config_runner_pools` as an `ObservableGauge<f64>` with a callback over an `Arc<AtomicI64>` (matching the pattern in `metrics.md:97` and `atc_pg_broadcast_watermark`); the watcher updates the atomic on each successful applied reload, the gauge callback re-reports on every collection cycle. Add the metric to the cached-instrument struct in `metrics.rs`. Update `docs/architecture/metrics.md` § "Metric and span authoring contract" with the seven-element interpretation block for each new metric.

### Phase 7 — E2E + Documentation

**Step 1: Failing test.** Add `frontend/e2e/config-hot-reload.test.ts`:
- Mock `/v1/state` with one pool at `running: 3, capacity: 10`. Open the page; assert `CapacityBar` reads `3/10`.
- Send a `{"kind":"ConfigUpdate","runnerPoolCapacities":[{labels:["self-hosted","linux","x64"], capacity:20}]}` via the existing `e2e/lib/ws-mock.ts`.
- Assert the bar re-renders at `3/20` without a page reload.
- Send a `{"kind":"ConfigReloadError","reason":"..."}` frame; assert console.warn (no UI surfacing per #203).

**Step 2: Documentation.** Update:
- `docs/architecture/deployment.md` — append "Hot-reload" subsection: watcher behavior, 500ms debounce, kubelet propagation timing, **directory-mount requirement** (subPath would break propagation), graceful-failure semantics, missing-file = ReloadError, scalar-drift warning, single hot-reloadable field (`runner_pools`).
- `docs/architecture/backend-server.md` — `config_watcher` module in module map; `WireFrame` framing at `ws.rs` boundary; second broadcast channel on AppState; `runner_pool_capacities` now a `RwLock`; Lagged-on-config-channel behavior matches existing CommittedEvent close-on-lag.
- `docs/architecture/frontend-app.md` — dispatcher outer-kind switch; `runStore.applyConfigUpdate`; ConnectionManager `pendingConfigUpdate` semantics + msw-based testing.
- `docs/architecture/metrics.md` — both new metrics with the seven-element interpretation block.
- `backend/crates/atc-server/CLAUDE.md` — module map entry for `config_watcher`; AppState field changes (the lock + the second tx); shutdown emitter category.
- `deploy/helm/atc/CLAUDE.md` — update "Runner-pool capacities gating" bullet: drop subPath reference, explain directory-mount + kubelet propagation rationale.
- `frontend/CLAUDE.md` — dispatcher outer-kind switch; `runStore.applyConfigUpdate`; ConnectionManager `pendingConfigUpdate` slot.

## Acceptance Criteria

- **AC1.** Editing the YAML file at `$ATC_CONFIG_FILE` (with a valid new `runner_pools` block) results in `AppState.runner_pool_capacities` being replaced within 1s of the OS-level write completing.
- **AC2.** A WS client connected during the reload receives exactly one JSON frame `{"kind":"ConfigUpdate","runnerPoolCapacities":[...]}` matching the new content. Field names are camelCase, matching existing conventions.
- **AC3.** A WS client connected during a *bad* reload (zero capacity, duplicate pool, malformed YAML, **missing file**, read error) receives exactly one `{"kind":"ConfigReloadError","reason":...}` frame and AppState capacities remain unchanged.
- **AC4.** `/v1/state` after a successful reload returns the new capacities. After a failed reload, returns the previous (valid) capacities.
- **AC5.** Rewriting the file with content identical to the current AppState capacities produces no WS broadcast; metric `atc_config_reload_total{result="success",reason="noop"}` increments.
- **AC6.** Kubernetes ConfigMap atomic-rename pattern (`..data` symlink swap) is detected and triggers a reload, verified by Phase 4 test `watcher_handles_kubernetes_symlink_swap`. The Helm chart's directory mount (no subPath) is what enables this in real K8s.
- **AC7.** Process boot with no config file present succeeds; the watcher arms on the parent dir if it exists; if the parent dir also doesn't exist, the watcher is skipped with a warn log; in neither case does boot fail.
- **AC8.** Process shutdown via `CancellationToken` causes the watcher task to be joined explicitly by `run_shutdown_orchestration` within `SHUTDOWN_TIMEOUT_CONFIG_WATCHER` (1s), before `otel::shutdown` is called. The emitter-categories comment in `shutdown.rs` is extended to include `config_watcher`.
- **AC9.** `atc_config_reload_total{result,reason}` counter and `atc_config_runner_pools` observable gauge are registered. Counter uses the cached-instrument pattern; observable gauge uses the atomic-callback pattern (`Arc<AtomicI64>`) per `metrics.md`.
- **AC10.** `dispatcher.ts` correctly dispatches all three `kind` cases (Committed → existing Run/Job path; ConfigUpdate → `runStore.applyConfigUpdate`; ConfigReloadError → console.warn referencing #203). Verified by both unit tests (direct dispatcher calls) and `ConnectionManager` msw-based network-level tests.
- **AC11.** `ConnectionManager` correctly handles pre-snapshot frames: pending `ConfigUpdate` is applied after `loadSnapshot` (latest wins); pre-snapshot `ConfigReloadError` is dropped silently (warn only, no UI change). Verified by msw-based tests.
- **AC12.** Both WS receivers (committed + config) close the socket on `RecvError::Lagged`, matching today's CommittedEvent behavior. Client reconnects via the existing path.
- **AC13.** `notify-debouncer-full` is used (not `-mini`) and is added to `Cargo.toml` without a pinned version, per `docs/implementation-guidance.md` "never pin library versions."
- **AC14.** Helm chart mounts the ConfigMap as a directory (`mountPath: /etc/atc`, no `subPath`) when `runnerPools` is non-empty. `helm-unittest` confirms the absence of `subPath` and the presence of the directory mount.
- **AC15.** `scripts/doc-mapping.sh` has an explicit straddler entry for `config_watcher.rs` mapping to both `docs/architecture/backend-server.md` and `docs/architecture/metrics.md`, placed above the existing backend catch-all.
- **AC16.** Reload tolerates a YAML file containing scalar fields (`http_addr`, etc.) without affecting AppState. A `tracing::warn!` per changed scalar field is emitted on each reload (diagnostic only). Verified by Phase 2 + Phase 4 tests.
- **AC17.** `just lint`, `just test`, `just types` pass. E2E green. `helm lint` + helm-unittest green. Pre-push doc-staleness gate passes — every source file change has its corresponding architecture doc update.
- **AC18.** Issue #203 (frontend admin-alert banner) is filed before this PR opens (already filed during planning) and is referenced from the `ConfigReloadError` console.warn fallback as the canonical follow-up.

## Documents to Update

| Doc | Change |
|---|---|
| `docs/architecture/deployment.md` | New "Hot-reload" subsection: watcher behavior, 500ms debounce, kubelet propagation timing, **directory-mount requirement** (subPath blocks propagation), graceful-failure semantics (keep-last-good), missing-file = ReloadError, scalar-drift warn-log, single hot-reloadable field. |
| `docs/architecture/backend-server.md` | Add `config_watcher` module to module map. Document `WireFrame` framing at `ws.rs` boundary, second broadcast channel on AppState (`config_events_tx`), `runner_pool_capacities` is now a `RwLock`. Note Lagged-on-config-channel close-and-reconnect parallels CommittedEvent path. |
| `docs/architecture/frontend-app.md` | Document dispatcher outer-kind switch (Committed/ConfigUpdate/ConfigReloadError), `runStore.applyConfigUpdate`, ConnectionManager's `pendingConfigUpdate` semantics, msw-based testing for the new path. |
| `docs/architecture/metrics.md` | Add `atc_config_reload_total{result,reason}` and `atc_config_runner_pools` with the seven-element interpretation block each. Note the observable-gauge atomic-callback wiring. |
| `backend/crates/atc-server/CLAUDE.md` | Add `config_watcher` module map entry; note AppState changes (lock + second tx); add `config_watcher` to the shutdown emitter category list (Contracts section). |
| `deploy/helm/atc/CLAUDE.md` | Update "Runner-pool capacities gating" bullet: drop subPath reference, explain why (directory mount enables kubelet propagation). |
| `frontend/CLAUDE.md` | Dispatcher outer-kind switch; `runStore.applyConfigUpdate`; ConnectionManager `pendingConfigUpdate` slot. |
| `scripts/doc-mapping.sh` | Add explicit straddler entry for `backend/crates/atc-server/src/config_watcher.rs` → both `backend-server.md` and `metrics.md`, above the backend catch-all at line 86. |

## Out of Scope

Tracked as follow-up issues:

- **#203 — Frontend admin-alert banner for `ConfigReloadError`.** Filed during planning. This PR logs to console only; visible UI banner deferred.
- Hot-reload of scalar config fields. The watcher warns on detected scalar drift but does not apply it; lifting that requires per-field reload safety analysis.

Not tracked as follow-ups:

- WS endpoint versioning (`/v2/ws`) for future wire-shape changes. This PR accepts the one-time rolling-deploy mismatch cost (Decision 13). If a future wire-shape change is needed, file a follow-up at that time to introduce the versioning pattern.

## Glossary

- **ConfigMap atomic swap (`..data` pattern):** Kubernetes mounts ConfigMaps via a symlink dance. `/etc/atc/config.yaml → ..data/config.yaml`; `..data → ..data_TIMESTAMP/`. On update, a new `..data_TIMESTAMP2/` is created and `..data` is atomically renamed to it. Watching the file directly misses the new content; watching the parent directory and reacting to filename matches works correctly.
- **`subPath` mount (Kubernetes):** A pod volume mount that projects a single key from a ConfigMap into a file path. Kubernetes documents that ConfigMap updates do NOT propagate through subPath mounts; the kubelet refreshes the projected directory but not the subPath alias. Hot-reload requires a directory mount instead.
- **`notify-debouncer-full`:** A debouncing wrapper over the `notify` crate that tracks rename/path changes (unlike `notify-debouncer-mini` which only dedupes raw paths). The right choice for K8s `..data` symlink swap handling.
- **`WireFrame` (in `ws.rs`):** Public serde-tagged enum (`#[serde(tag = "kind", rename_all = "camelCase")]`) that wraps WS messages with an outer discriminator. Variants: `Committed(CommittedEvent)`, `ConfigUpdate { runner_pool_capacities }`, `ConfigReloadError { reason }`. TS-exported via ts-rs for the frontend dispatcher.
- **`ConfigEvent` (in `config_watcher.rs`):** Internal enum carried over the `config_events_tx` broadcast channel from the watcher task to the WS handler. The WS handler wraps each variant in the appropriate `WireFrame`. Two layers exist because `config_events_tx` is a Rust-internal channel and `WireFrame` is the wire shape.
- **`ObservableGauge<f64>` (OTel):** A gauge instrument whose value is read from a callback on each metric collection cycle. The convention in this repo (per `docs/architecture/metrics.md:97`) is to close the callback over an `Arc<AtomicI64>` that production code mutates; the atomic update IS the metric update — no explicit `record()` call.
- **`ReloadError`:** Categorized error from `reload_runner_pools`. Variants: `Read` (file I/O failure or missing file — the latter is treated as an error on reload, diverging from startup behavior), `Parse` (YAML deserialization failure), `Validate` (zero capacity, empty labels, or duplicate canonicalized label set).
- **Scalar drift:** The state in which a YAML file's scalar fields (`http_addr`, `database_url`, etc.) have changed since process startup. The watcher emits a `tracing::warn!` per changed field but does not apply scalar changes — hot-reload is limited to `runner_pools` by design (Decision 9).
