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
    Clock, Job, JobConclusion, JobId, PersistError, RunConclusion, RunId, WorkflowRun,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
};
use atc_github::WebhookEvent;
use atc_persist::{LivenessError, PersistentStore};
use atc_wire::{CommittedEvent, StateSnapshot};
use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use atc_core::types::RepoKey;
use atc_core::{JobStatus, RunStatus};

#[cfg(any(test, feature = "test-support"))]
mod invariants;

/// Display-TTL filter for runs: keep the row when the operator has not
/// armed a cutoff, when the run is not yet `Completed`, when its
/// `completed_at` is `None` (permissive — pre-migration or pre-feature
/// rows stay visible), or when its `completed_at` is at or beyond the
/// cutoff. Mirrors the SQL `WHERE` in `atc-store-pg::reads`.
fn run_passes_cutoff(run: &WorkflowRun, cutoff: Option<DateTime<Utc>>) -> bool {
    let Some(cutoff) = cutoff else {
        return true;
    };
    if run.status != RunStatus::Completed {
        return true;
    }
    run.completed_at.is_none_or(|t| t >= cutoff)
}

/// Display-TTL filter for jobs. Same predicate shape as `run_passes_cutoff`
/// — split into a sibling function for clarity at the call site and so the
/// status enum can stay strongly-typed.
fn job_passes_cutoff(job: &atc_core::Job, cutoff: Option<DateTime<Utc>>) -> bool {
    let Some(cutoff) = cutoff else {
        return true;
    };
    if job.status != JobStatus::Completed {
        return true;
    }
    job.completed_at.is_none_or(|t| t >= cutoff)
}

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

    /// Whether `run_id` currently has at least one non-`Completed` job at
    /// `run_attempt` or higher. Shared by the staleness sweep's initial
    /// candidate filter and its immediately-before-apply recheck (see
    /// `sweep_stale`).
    ///
    /// The `run_attempt` filter mirrors `read_snapshot_inner`'s
    /// `j.run_attempt >= r.run_attempt` filter: a re-run's superseded
    /// lower-attempt jobs stay in the map forever (nothing evicts a
    /// non-`Completed` job) and must not count as "live" for the current
    /// attempt. Without it, a job stuck `Waiting` from a stale attempt
    /// (`sweep_stale`'s jobs pass never completes `Waiting` jobs) would
    /// permanently shield the current attempt's run from ever being swept.
    fn run_has_non_terminal_jobs(&self, run_id: RunId, run_attempt: i32) -> bool {
        self.jobs_by_run.get(&run_id).is_some_and(|ids| {
            ids.iter().any(|id| {
                self.jobs.get(id).is_some_and(|j| {
                    j.status != JobStatus::Completed && j.run_attempt >= run_attempt
                })
            })
        })
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
        staleness_threshold: Option<Duration>,
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
        let handle =
            Arc::clone(&store).spawn_eviction(eviction_period, staleness_threshold, shutdown);
        *store
            .eviction_handle
            .lock()
            .expect("eviction_handle mutex poisoned") = Some(handle);
        store
    }

    /// Spawn the supervised background task that periodically evicts expired
    /// entries and sweeps stale non-terminal entries from this store.
    ///
    /// Returns a [`JoinHandle`] that resolves when the task exits cooperatively
    /// after `cancel` is cancelled. The first tick runs after `interval`
    /// elapses (not immediately) — we consume the `tokio::time::interval`
    /// first tick to align the cadence. The `cancel` arm of the select is
    /// `biased;` first so cancellation is always honoured before the next
    /// tick fires, matching the issue #60 supervision pattern.
    ///
    /// `staleness_threshold = None` skips the staleness sweep entirely (the
    /// operator disabled it) — every tick still runs eviction. See ADR-0013.
    ///
    /// No `.instrument(...)` task-lifetime root: each `evict_expired` /
    /// `sweep_stale` call's own `#[tracing::instrument]` becomes its own root
    /// span, so every sweep is one tidy trace that exports on tick rather
    /// than accumulating under a long-lived parent that only ends at process
    /// shutdown.
    fn spawn_eviction(
        self: Arc<Self>,
        interval: Duration,
        staleness_threshold: Option<Duration>,
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
                    _ = ticker.tick() => {
                        self.evict_expired().await;
                        if let Some(threshold) = staleness_threshold {
                            self.sweep_stale(threshold).await;
                        }
                    }
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
    ///
    /// `cutoff = Some(t)` excludes completed runs and jobs whose
    /// `completed_at` is set and earlier than `t`. Completed entries with
    /// `completed_at == None` are kept (permissive) — symmetric with the PG
    /// `WHERE` clause. Active (non-completed) entries are always kept
    /// regardless of timestamp.
    ///
    /// Note that in-memory mode also retains the existing eviction TTL
    /// (currently 1h, hardcoded in `main.rs`). When `cutoff > now - 1h`, the
    /// eviction sweep wins: rows older than the eviction TTL have already
    /// been removed and cannot be surfaced via the cutoff filter. This is
    /// acceptable because in-memory mode is dev-only — production uses PG,
    /// where eviction and display-TTL are independent concerns. See
    /// ADR-0009.
    pub(crate) async fn read_snapshot_inner(&self, cutoff: Option<DateTime<Utc>>) -> StateSnapshot {
        let seq_guard = self.seq.lock().await;
        let state = self.state.read().await;
        let last_seq = *seq_guard;
        // Filter pre-collect so the snapshot vectors never carry rows the
        // display-TTL gate has rejected. The predicate is symmetric with the
        // PG `WHERE` clause: cutoff absent → keep; status != Completed →
        // keep; completed_at missing → keep (permissive); else keep iff
        // completed_at >= cutoff.
        //
        // Jobs additionally drop when their parent run is filtered by the
        // cutoff — matching the PG `JOIN runs r ON ...` predicate. Without
        // this gate, a completed run aged past the cutoff with a
        // non-`Completed` sub-job would produce an orphan job in the
        // snapshot. A job whose parent run does not yet exist in
        // `state.runs` (out-of-order job-before-run delivery — in-memory
        // has no FK stub, see PG's `placeholder` runs) is treated as live
        // and kept: the parent webhook has not arrived yet, so the cutoff
        // semantics do not yet apply to it.
        let runs: Vec<WorkflowRun> = state
            .runs
            .values()
            .filter(|r| run_passes_cutoff(r, cutoff))
            .cloned()
            .collect();
        let jobs: Vec<Job> = state
            .jobs
            .values()
            .filter(|j| {
                // Filter out prior-attempt jobs. A re-run reuses the run_id
                // with fresh job IDs at a higher attempt; jobs from a *lower*
                // attempt than the parent run are stale and drop out. Jobs at
                // the current OR a higher attempt are kept — a re-run's queued
                // jobs can arrive (with the higher attempt) before the run row
                // advances, and must stay visible. When the parent run is
                // absent (job-before-run stub), keep the job. Mirrors the PG
                // read's `j.run_attempt >= r.run_attempt` predicate.
                let parent = state.runs.get(&j.run_id);
                let attempt_current = parent.is_none_or(|r| j.run_attempt >= r.run_attempt);
                // A higher-attempt job's parent row is still the aged-out prior
                // attempt; don't gate the fresh job on the stale run's cutoff.
                let parent_alive = parent
                    .is_none_or(|r| j.run_attempt > r.run_attempt || run_passes_cutoff(r, cutoff));
                attempt_current && parent_alive && job_passes_cutoff(j, cutoff)
            })
            .cloned()
            .collect();
        drop(seq_guard);
        StateSnapshot {
            last_seq,
            runs,
            jobs,
            // Operator-declared capacities live in `AppState`, not the store —
            // composed into the response by `routes::state_handler`.
            runner_pool_capacities: Vec::new(),
            // Stamped by `routes::state_handler` from `AppState::display_ttl`.
            display_ttl_seconds: 0,
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

    /// Force-complete stale non-terminal runs/jobs with conclusion `Stale`,
    /// mirroring `atc-store-pg`'s staleness sweep (see ADR-0013).
    ///
    /// Jobs sweep first so a run's non-terminal-jobs guard reflects jobs
    /// already swept this tick — same ordering rationale as the PG sweep.
    /// Each candidate's staleness is re-checked and, if still stale, applied
    /// in one atomic critical section (`sweep_one_job_if_stale` /
    /// `sweep_one_run_if_stale`) — the same guarantee `atc-store-pg`'s row
    /// lock gives, not merely a narrowed race window. This matters because
    /// `Completed -> Completed` is an *admitted* idempotent replay (by
    /// design, for real webhook redelivery), not a rejected transition: a
    /// naive "recheck, then separately call `apply_job_event`" would let a
    /// real completion landing in that gap get silently overwritten by the
    /// synthetic `Stale` conclusion, with no self-heal (the real webhook
    /// already fired and won't fire again).
    #[tracing::instrument(
        name = "staleness.sweep",
        skip_all,
        fields(jobs.swept = tracing::field::Empty, runs.swept = tracing::field::Empty),
    )]
    pub async fn sweep_stale(&self, threshold: Duration) {
        let now = self.clock.now();

        let candidate_job_ids: Vec<JobId> = {
            let state = self.state.read().await;
            state
                .jobs
                .values()
                .filter(|j| atc_core::state_machine::is_stale_job(j, now, threshold))
                .map(|j| j.id)
                .collect()
        };
        let mut jobs_swept = 0u64;
        for job_id in candidate_job_ids {
            if self.sweep_one_job_if_stale(job_id, now, threshold).await {
                jobs_swept += 1;
            }
        }

        // Runs candidate list is gathered after the jobs pass completes, so
        // the non-terminal-jobs guard reflects jobs already swept above.
        let candidate_run_ids: Vec<RunId> = {
            let state = self.state.read().await;
            state
                .runs
                .values()
                .filter(|r| {
                    let has_non_terminal_jobs =
                        state.run_has_non_terminal_jobs(r.id, r.run_attempt);
                    atc_core::state_machine::is_stale_run(r, has_non_terminal_jobs, now, threshold)
                })
                .map(|r| r.id)
                .collect()
        };
        let mut runs_swept = 0u64;
        for run_id in candidate_run_ids {
            if self.sweep_one_run_if_stale(run_id, now, threshold).await {
                runs_swept += 1;
            }
        }

        let span = tracing::Span::current();
        span.record("jobs.swept", jobs_swept);
        span.record("runs.swept", runs_swept);
        if jobs_swept > 0 || runs_swept > 0 {
            tracing::info!(jobs_swept, runs_swept, "staleness sweep complete");
        }
    }

    /// Re-check `job_id`'s staleness and, if still stale, force-complete it
    /// — all under one `state` write-lock acquisition shared with the
    /// recheck, so no mutation can land in between. Returns `true` if swept.
    ///
    /// A job whose parent run row doesn't exist yet (job-before-run
    /// delivery) is skipped — `JobEventEnvelope` requires org/repo and
    /// there's no run row yet to source them from.
    async fn sweep_one_job_if_stale(
        &self,
        job_id: JobId,
        now: DateTime<Utc>,
        threshold: Duration,
    ) -> bool {
        let mut seq_guard = self.seq.lock().await;
        let mut state = self.state.write().await;

        let Some(existing) = state.jobs.get(&job_id).cloned() else {
            return false;
        };
        if !atc_core::state_machine::is_stale_job(&existing, now, threshold) {
            return false;
        }
        let Some(parent) = state.runs.get(&existing.run_id) else {
            return false;
        };

        let env = JobEventEnvelope {
            job_id: existing.id,
            run_id: existing.run_id,
            org: parent.org.clone(),
            repo: parent.repo.clone(),
            name: existing.name,
            created_at: existing.created_at,
            started_at: existing.started_at,
            completed_at: Some(now),
            run_attempt: existing.run_attempt,
            repo_id: None,
            action: JobEvent::Completed {
                conclusion: JobConclusion::Stale,
                runner: existing.runner,
                labels: existing.labels,
                steps: existing.steps,
            },
        };

        let result = self
            .apply_job_event_locked(&mut seq_guard, &mut state, env.clone())
            .await;
        drop(state);

        match result {
            Ok(allocated) => {
                let _ = self.broadcast_tx.send(CommittedEvent {
                    seq: allocated,
                    event: WebhookEvent::Job(env),
                });
                true
            }
            Err(e) => {
                // Unreachable in practice — the fresh `is_stale_job` check
                // above, taken under the same lock as the write, guarantees
                // the transition is valid. Logged rather than unwrapped in
                // case a future edge case proves otherwise.
                tracing::debug!(job_id = job_id.0, error.message = ?e, "staleness sweep: job apply unexpectedly rejected");
                false
            }
        }
    }

    /// Re-check `run_id`'s staleness (status, age, and the non-terminal-jobs
    /// guard) and, if still stale, force-complete it — all under one
    /// `state` write-lock acquisition shared with the recheck. Returns
    /// `true` if swept.
    async fn sweep_one_run_if_stale(
        &self,
        run_id: RunId,
        now: DateTime<Utc>,
        threshold: Duration,
    ) -> bool {
        let mut seq_guard = self.seq.lock().await;
        let mut state = self.state.write().await;

        let Some(existing) = state.runs.get(&run_id).cloned() else {
            return false;
        };
        let has_non_terminal_jobs = state.run_has_non_terminal_jobs(run_id, existing.run_attempt);
        if !atc_core::state_machine::is_stale_run(&existing, has_non_terminal_jobs, now, threshold)
        {
            return false;
        }

        let env = RunEventEnvelope {
            run_id: existing.id,
            org: existing.org,
            repo: existing.repo,
            workflow_name: existing.workflow_name,
            workflow_path: existing.workflow_path,
            branch: existing.branch,
            head_sha: existing.head_sha,
            commit_message: existing.commit_message,
            trigger_event: existing.event,
            display_title: existing.display_title,
            html_url: existing.html_url,
            created_at: existing.created_at,
            run_started_at: existing.run_started_at,
            updated_at: now,
            completed_at: Some(now),
            run_attempt: existing.run_attempt,
            repo_id: None,
            action: RunEvent::Completed {
                conclusion: RunConclusion::Stale,
            },
        };

        let result = self
            .apply_run_event_locked(&mut seq_guard, &mut state, env.clone())
            .await;
        drop(state);

        match result {
            Ok(allocated) => {
                let _ = self.broadcast_tx.send(CommittedEvent {
                    seq: allocated,
                    event: WebhookEvent::Run(env),
                });
                true
            }
            Err(e) => {
                // Unreachable in practice — see the matching comment in
                // `sweep_one_job_if_stale`.
                tracing::debug!(run_id = run_id.0, error.message = ?e, "staleness sweep: run apply unexpectedly rejected");
                false
            }
        }
    }
}

#[async_trait::async_trait]
impl PersistentStore for InMemoryStore {
    /// Return a consistent snapshot of all state with the current seq cursor.
    ///
    /// Locks seq then read-locks state so snapshot content and cursor describe
    /// the same point in time — concurrent writers cannot interleave.
    ///
    /// `cutoff` filters out completed runs and jobs older than the supplied
    /// instant (display-TTL gate). See the trait doc on
    /// [`PersistentStore::read_snapshot`] for the exact predicate.
    #[tracing::instrument(
        name = "persist.read.snapshot",
        skip_all,
        fields(last_seq = tracing::field::Empty, runs_count = tracing::field::Empty, jobs_count = tracing::field::Empty),
    )]
    async fn read_snapshot(
        &self,
        cutoff: Option<DateTime<Utc>>,
    ) -> Result<StateSnapshot, PersistError> {
        let snap = self.read_snapshot_inner(cutoff).await;
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
        let allocated = self
            .apply_run_event_locked(&mut seq_guard, &mut state, env.clone())
            .await?;
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
        let allocated = self
            .apply_job_event_locked(&mut seq_guard, &mut state, env.clone())
            .await?;
        drop(state);

        tracing::Span::current().record("seq", allocated);
        let _ = self.broadcast_tx.send(CommittedEvent {
            seq: allocated,
            event: WebhookEvent::Job(env),
        });
        Ok(allocated)
    }
}

impl InMemoryStore {
    /// Core of [`PersistentStore::apply_run_event`], factored out so the
    /// staleness sweep (`sweep_one_run_if_stale`) can recheck a candidate's
    /// staleness and apply the resulting envelope inside the SAME lock
    /// acquisition as the recheck — genuinely atomic, not "recheck under one
    /// lock, apply under a second lock a moment later". Both the public
    /// trait method and the sweep call this identical mutation logic; the
    /// only thing that differs between call sites is where the lock is
    /// acquired and whether a staleness recheck happens before this runs.
    ///
    /// Callers own `seq_guard`/`state` and are responsible for dropping
    /// `state` before broadcasting (this fn does not broadcast) — `seq_guard`
    /// stays held through the broadcast so concurrent callers' broadcasts
    /// stay in seq order.
    async fn apply_run_event_locked(
        &self,
        seq_guard: &mut u64,
        state: &mut StateData,
        env: RunEventEnvelope,
    ) -> Result<u64, PersistError> {
        let existing = state.runs.get(&env.run_id).cloned();
        // Reject a stale lower attempt outright — mirrors the PG predicate's
        // `EXCLUDED.run_attempt = runs.run_attempt` gate. A delayed event from a
        // superseded attempt must not reopen or re-conclude the current one.
        if let Some(ref r) = existing
            && env.run_attempt < r.run_attempt
        {
            tracing::warn!(
                run_id = env.run_id.0,
                event_attempt = env.run_attempt,
                stored_attempt = r.run_attempt,
                "rejecting stale lower run attempt"
            );
            return Err(atc_core::PersistError::InvalidTransition);
        }
        // When a new attempt arrives (higher run_attempt), pass None so the
        // state machine constructs a fresh run rather than trying to transition
        // out of the prior attempt's terminal state.
        let is_new_attempt = existing
            .as_ref()
            .map(|r| env.run_attempt > r.run_attempt)
            .unwrap_or(false);
        let run = atc_core::state_machine::apply_run_event(
            if is_new_attempt { None } else { existing },
            env,
        )
        .map_err(|e| {
            tracing::warn!(error.message = %e, "state machine rejected run transition");
            atc_core::PersistError::from(e)
        })?;

        // Transition validated — commit via CoW remove-then-insert.
        state.runs.remove(&run.id);
        state.runs.insert(run.id, run);

        *seq_guard += 1;
        Ok(*seq_guard)
    }

    /// Core of [`PersistentStore::apply_job_event`] — see
    /// [`Self::apply_run_event_locked`]'s doc comment for the shared-helper
    /// rationale.
    async fn apply_job_event_locked(
        &self,
        seq_guard: &mut u64,
        state: &mut StateData,
        env: JobEventEnvelope,
    ) -> Result<u64, PersistError> {
        let job_id = env.job_id;
        let run_id = env.run_id;
        let org = env.org.clone();
        let repo = env.repo.clone();
        let is_new = !state.jobs.contains_key(&job_id);
        let existing = state.jobs.get(&job_id).cloned();

        let job = atc_core::state_machine::apply_job_event(existing, env).map_err(|e| {
            tracing::warn!(error.message = %e, "state machine rejected job transition");
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
        Ok(*seq_guard)
    }
}
