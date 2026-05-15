//! In-memory persistence implementation.
//!
//! `InMemoryStore` owns the full domain state: the HashMap tables, secondary
//! indexes, a monotonic seq counter, the clock for eviction, and the broadcast
//! sender. It uses `atc_core::apply_*_event` pure free functions for state
//! transitions, maintaining indexes locally on first-sight.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use atc_core::{
    Clock, Job, JobId, PersistError, RunId, WorkflowRun,
    event::{JobEventEnvelope, RunEventEnvelope},
};
use atc_github::WebhookEvent;
use atc_persist::{LivenessError, PersistentStore};
use atc_wire::{CommittedEvent, StateSnapshot};
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use atc_core::types::RepoKey;

#[cfg(any(test, feature = "test-support"))]
mod invariants;

/// Per-task shutdown timeout for the eviction sweep task. Public so the
/// shutdown-orchestration test in `atc-server` can include this value in its
/// aggregate-budget assertion alongside `atc-server::shutdown`'s remaining
/// timeouts.
pub const EVICTION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

/// Default broadcast channel capacity. Matches the production setting and the
/// `PgStore::start` capacity so both modes have identical lag semantics.
const DEFAULT_BROADCAST_CAPACITY: usize = 256;

// ---------------------------------------------------------------------------
// StateData — mutable state behind the RwLock
// ---------------------------------------------------------------------------

pub(crate) struct StateData {
    /// Primary map of runs by ID.
    pub(crate) runs: HashMap<RunId, WorkflowRun>,
    /// Primary map of jobs by ID.
    pub(crate) jobs: HashMap<JobId, Job>,
    /// Jobs grouped by parent run.
    pub(crate) jobs_by_run: HashMap<RunId, HashSet<JobId>>,
    /// Jobs grouped by repository.
    pub(crate) jobs_by_repo: HashMap<RepoKey, HashSet<JobId>>,
}

impl StateData {
    fn new() -> Self {
        Self {
            runs: HashMap::new(),
            jobs: HashMap::new(),
            jobs_by_run: HashMap::new(),
            jobs_by_repo: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// InMemoryStore
// ---------------------------------------------------------------------------

/// In-memory backend for [`PersistentStore`] (dev/test only).
///
/// Owns the full domain state: HashMaps, secondary indexes, seq counter,
/// clock, and the broadcast sender. State transitions delegate to the pure
/// free functions in `atc_core::state_machine`.
///
/// Thread-safe via `RwLock<StateData>` for state and `Mutex<u64>` for seq.
/// Wrap in `Arc` for sharing across async tasks and Axum handlers.
pub struct InMemoryStore {
    /// All domain state behind a read-write lock.
    pub(crate) state: RwLock<StateData>,
    /// Monotonic event counter. Locked before state writes so WS event order
    /// matches ingestion order, and across snapshot + seq read so the cursor
    /// matches snapshot content.
    pub(crate) seq: Mutex<u64>,
    /// Clock for determining current time during eviction.
    clock: Arc<dyn Clock>,
    /// How long to retain completed jobs before eviction.
    completed_ttl: Duration,
    /// Broadcast sender for pushing domain events to WebSocket clients.
    broadcast_tx: broadcast::Sender<CommittedEvent>,
    /// JoinHandle for the eviction task. `Some` until the first `shutdown()`
    /// call takes it. `None` after shutdown or when constructed via
    /// `new_for_test` (which never spawns the task).
    eviction_handle: StdMutex<Option<JoinHandle<()>>>,
}

impl InMemoryStore {
    /// Construct an [`InMemoryStore`] and spawn its eviction task.
    ///
    /// Returns `Arc<Self>` so the spawned task can hold a strong reference
    /// without forcing the caller to wrap manually. Synchronous — no `.await`
    /// at the call site.
    pub fn start(
        clock: Arc<dyn Clock>,
        completed_ttl: Duration,
        eviction_period: Duration,
        shutdown: CancellationToken,
    ) -> Arc<Self> {
        let (broadcast_tx, _sentinel) =
            broadcast::channel::<CommittedEvent>(DEFAULT_BROADCAST_CAPACITY);
        let store = Arc::new(Self {
            state: RwLock::new(StateData::new()),
            seq: Mutex::new(0u64),
            clock,
            completed_ttl,
            broadcast_tx,
            eviction_handle: StdMutex::new(None),
        });
        let handle = Arc::clone(&store).spawn_eviction(eviction_period, shutdown);
        *store
            .eviction_handle
            .lock()
            .expect("eviction_handle mutex poisoned") = Some(handle);
        store
    }

    /// Spawn the supervised background task that periodically evicts expired
    /// entries from this store.
    ///
    /// Returns a [`JoinHandle`] that resolves when the task exits cooperatively
    /// after `cancel` is cancelled. The first eviction runs after `interval`
    /// elapses (not immediately) — we consume the `tokio::time::interval`
    /// first tick to align the cadence. The `cancel` arm of the select is
    /// `biased;` first so cancellation is always honoured before the next
    /// tick fires, matching the issue #60 supervision pattern.
    ///
    /// No `.instrument(...)` task-lifetime root: each `evict_expired` call's
    /// `#[tracing::instrument(name = "eviction.sweep")]` becomes its own root
    /// span, so every sweep is one tidy trace that exports on tick rather
    /// than accumulating under a long-lived parent that only ends at process
    /// shutdown.
    fn spawn_eviction(
        self: Arc<Self>,
        interval: Duration,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            // First tick completes immediately — consume it to align cadence.
            ticker.tick().await;
            loop {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => break,
                    _ = ticker.tick() => self.evict_expired().await,
                }
            }
        })
    }

    /// Test-only constructor that allows a custom broadcast capacity and skips
    /// the eviction task. Used by lagging-client tests that need to trigger
    /// `RecvError::Lagged` with a small buffer instead of 256.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_for_test(
        clock: Arc<dyn Clock>,
        completed_ttl: Duration,
        broadcast_capacity: usize,
    ) -> Arc<Self> {
        let (broadcast_tx, _sentinel) = broadcast::channel::<CommittedEvent>(broadcast_capacity);
        Arc::new(Self {
            state: RwLock::new(StateData::new()),
            seq: Mutex::new(0u64),
            clock,
            completed_ttl,
            broadcast_tx,
            eviction_handle: StdMutex::new(None),
        })
    }

    /// Return a consistent snapshot of all state with the current seq.
    ///
    /// Locks seq then read-locks state so snapshot content and cursor
    /// describe the same point in time — concurrent writers cannot interleave.
    pub(crate) async fn read_snapshot_inner(&self) -> StateSnapshot {
        let seq_guard = self.seq.lock().await;
        let state = self.state.read().await;
        let last_seq = *seq_guard;
        let runs: Vec<WorkflowRun> = state.runs.values().cloned().collect();
        let jobs: Vec<Job> = state.jobs.values().cloned().collect();
        drop(seq_guard);
        StateSnapshot {
            last_seq,
            runs,
            jobs,
            // Operator-declared capacities live in `AppState`, not the store —
            // composed into the response by `routes::state_handler`.
            runner_pool_capacities: Vec::new(),
        }
    }

    /// Evict completed jobs that have exceeded the configured TTL.
    ///
    /// Holds a single write lock for the entire sweep to avoid a TOCTOU window
    /// between predicate evaluation and removal. Matching `state_machine.rs`
    /// semantics.
    #[tracing::instrument(
        name = "eviction.sweep",
        skip_all,
        fields(
            jobs.evicted = tracing::field::Empty,
            runs.evicted = tracing::field::Empty,
            elapsed.micros = tracing::field::Empty,
        ),
    )]
    pub async fn evict_expired(&self) {
        tracing::debug!("starting eviction sweep");
        let start = std::time::Instant::now();

        let now = self.clock.now();
        let mut state = self.state.write().await;

        // Find expired completed job IDs
        let expired_job_ids: Vec<JobId> = state
            .jobs
            .iter()
            .filter(|(_, job)| atc_core::state_machine::is_evictable(job, now, self.completed_ttl))
            .map(|(id, _)| *id)
            .collect();

        if expired_job_ids.is_empty() {
            #[allow(clippy::cast_possible_truncation)]
            let elapsed_us = start.elapsed().as_micros() as u64;
            let span = tracing::Span::current();
            span.record("jobs.evicted", 0u64);
            span.record("runs.evicted", 0u64);
            span.record("elapsed.micros", elapsed_us);
            tracing::debug!(elapsed_us, "eviction sweep complete, nothing to evict");
            return;
        }

        // Remove expired jobs from primary map, collect affected run IDs
        let mut affected_run_ids = HashSet::new();
        for job_id in &expired_job_ids {
            if let Some(job) = state.jobs.remove(job_id) {
                affected_run_ids.insert(job.run_id);
            }
        }

        // Remove from jobs_by_run index
        for run_id in &affected_run_ids {
            if let Some(set) = state.jobs_by_run.get_mut(run_id) {
                for job_id in &expired_job_ids {
                    set.remove(job_id);
                }
            }
        }

        // Remove from jobs_by_repo index
        for set in state.jobs_by_repo.values_mut() {
            for job_id in &expired_job_ids {
                set.remove(job_id);
            }
        }
        state.jobs_by_repo.retain(|_, set| !set.is_empty());
        state.jobs_by_run.retain(|_, set| !set.is_empty());

        // Evict runs with no remaining jobs
        let mut runs_evicted: u64 = 0;
        for run_id in &affected_run_ids {
            let has_jobs = state
                .jobs_by_run
                .get(run_id)
                .is_some_and(|set| !set.is_empty());
            if !has_jobs {
                state.runs.remove(run_id);
                state.jobs_by_run.remove(run_id);
                runs_evicted += 1;
            }
        }

        #[allow(clippy::cast_possible_truncation)]
        let elapsed_us = start.elapsed().as_micros() as u64;
        let jobs_evicted = expired_job_ids.len() as u64;
        let span = tracing::Span::current();
        span.record("jobs.evicted", jobs_evicted);
        span.record("runs.evicted", runs_evicted);
        span.record("elapsed.micros", elapsed_us);
        tracing::info!(
            jobs_evicted,
            runs_evicted,
            elapsed_us,
            "eviction sweep complete"
        );
    }
}

#[async_trait::async_trait]
impl PersistentStore for InMemoryStore {
    /// Return a consistent snapshot of all state with the current seq cursor.
    ///
    /// Locks seq then read-locks state so snapshot content and cursor describe
    /// the same point in time — concurrent writers cannot interleave.
    #[tracing::instrument(
        name = "persist.read.snapshot",
        skip_all,
        fields(last_seq = tracing::field::Empty, runs_count = tracing::field::Empty, jobs_count = tracing::field::Empty),
    )]
    async fn read_snapshot(&self) -> Result<StateSnapshot, PersistError> {
        let snap = self.read_snapshot_inner().await;
        let span = tracing::Span::current();
        span.record("last_seq", snap.last_seq);
        span.record("runs_count", snap.runs.len());
        span.record("jobs_count", snap.jobs.len());
        Ok(snap)
    }

    /// Always returns `Ok(())` — the in-memory store is always live.
    async fn liveness_check(&self) -> Result<(), LivenessError> {
        Ok(())
    }

    fn subscribe(&self) -> broadcast::Receiver<CommittedEvent> {
        self.broadcast_tx.subscribe()
    }

    async fn shutdown(&self) {
        // Drop the std::sync::Mutex guard BEFORE awaiting — holding it across
        // `.await` would `!Send` the future and break the `async_trait` bound.
        //
        // Callers must cancel the shutdown token they passed to `start()`
        // before invoking `shutdown()` — otherwise the eviction task never
        // observes cancellation and this method waits the full per-task
        // timeout before aborting. Stores constructed via `new_for_test`
        // hold `None` here and return immediately.
        let handle = self
            .eviction_handle
            .lock()
            .expect("eviction_handle mutex poisoned")
            .take();
        if let Some(handle) = handle {
            atc_persist::join_with_timeout(handle, EVICTION_SHUTDOWN_TIMEOUT, "eviction").await;
        }
    }

    /// Apply a run event to the in-memory state and broadcast.
    ///
    /// Acquires the seq mutex before the apply so that WS event order matches
    /// ingestion order. On invalid transition returns `Err(PersistError::InvalidTransition)`
    /// without incrementing seq or emitting a broadcast.
    #[tracing::instrument(
        name = "persist.apply.run_event",
        skip_all,
        fields(run_id = env.run_id.0, seq = tracing::field::Empty),
    )]
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError> {
        let mut seq_guard = self.seq.lock().await;
        let mut state = self.state.write().await;

        let existing = state.runs.get(&env.run_id).cloned();
        let run = atc_core::state_machine::apply_run_event(existing, env.clone()).map_err(|e| {
            tracing::warn!(error = %e, "state machine rejected run transition");
            atc_core::PersistError::from(e)
        })?;

        // Transition validated — commit via CoW remove-then-insert.
        state.runs.remove(&run.id);
        state.runs.insert(run.id, run);

        *seq_guard += 1;
        let allocated = *seq_guard;
        drop(state);

        tracing::Span::current().record("seq", allocated);
        let _ = self.broadcast_tx.send(CommittedEvent {
            seq: allocated,
            event: WebhookEvent::Run(env),
        });
        Ok(allocated)
    }

    /// Apply a job event to the in-memory state and broadcast.
    ///
    /// Same locking semantics as [`apply_run_event`]. Invalid transitions return
    /// `Err(PersistError::InvalidTransition)` without side effects.
    /// Secondary indexes are updated on first sight.
    #[tracing::instrument(
        name = "persist.apply.job_event",
        skip_all,
        fields(run_id = env.run_id.0, job_id = env.job_id.0, seq = tracing::field::Empty),
    )]
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError> {
        let mut seq_guard = self.seq.lock().await;
        let mut state = self.state.write().await;

        let job_id = env.job_id;
        let run_id = env.run_id;
        let org = env.org.clone();
        let repo = env.repo.clone();
        let is_new = !state.jobs.contains_key(&job_id);
        let existing = state.jobs.get(&job_id).cloned();

        let job = atc_core::state_machine::apply_job_event(existing, env.clone()).map_err(|e| {
            tracing::warn!(error = %e, "state machine rejected job transition");
            atc_core::PersistError::from(e)
        })?;

        // Transition validated — commit via CoW remove-then-insert.
        state.jobs.remove(&job_id);
        state.jobs.insert(job_id, job);

        // Update secondary indexes on first sight
        if is_new {
            let repo_key = RepoKey::new(org, repo);
            state.jobs_by_run.entry(run_id).or_default().insert(job_id);
            state
                .jobs_by_repo
                .entry(repo_key)
                .or_default()
                .insert(job_id);
        }

        *seq_guard += 1;
        let allocated = *seq_guard;
        drop(state);

        tracing::Span::current().record("seq", allocated);
        let _ = self.broadcast_tx.send(CommittedEvent {
            seq: allocated,
            event: WebhookEvent::Job(env),
        });
        Ok(allocated)
    }
}
