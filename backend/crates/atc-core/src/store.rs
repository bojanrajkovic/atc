//! In-memory state store for domain entities.
//!
//! The [`StateStore`] ingests domain events, maintains current state
//! with secondary indexes, and supports configurable TTL eviction of
//! completed entries.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::clock::Clock;
use crate::job::{InvalidJobTransition, Job};
use crate::run::{InvalidRunTransition, WorkflowRun};
use crate::types::{JobId, RepoKey, RunId};

/// Errors that can occur during state store operations.
#[derive(Debug)]
pub enum StoreError {
    /// A run status transition was invalid.
    InvalidRunTransition(InvalidRunTransition),
    /// A job status transition was invalid.
    InvalidJobTransition(InvalidJobTransition),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRunTransition(e) => write!(f, "{e}"),
            Self::InvalidJobTransition(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRunTransition(e) => Some(e),
            Self::InvalidJobTransition(e) => Some(e),
        }
    }
}

impl From<InvalidRunTransition> for StoreError {
    fn from(e: InvalidRunTransition) -> Self {
        Self::InvalidRunTransition(e)
    }
}

impl From<InvalidJobTransition> for StoreError {
    fn from(e: InvalidJobTransition) -> Self {
        Self::InvalidJobTransition(e)
    }
}

/// In-memory state store for workflow runs and jobs.
///
/// Thread-safe via `tokio::sync::RwLock`. Wrap in `Arc` for sharing
/// across async tasks and Axum handlers.
pub struct StateStore {
    state: RwLock<StateData>,
    clock: Arc<dyn Clock>,
    /// How long to retain completed jobs before eviction.
    completed_ttl: Duration,
}

/// Mutable state behind the `RwLock`.
struct StateData {
    /// Primary map of runs by ID.
    runs: HashMap<RunId, WorkflowRun>,
    /// Primary map of jobs by ID.
    jobs: HashMap<JobId, Job>,
    /// Jobs grouped by parent run.
    jobs_by_run: HashMap<RunId, HashSet<JobId>>,
    /// Jobs grouped by repository.
    jobs_by_repo: HashMap<RepoKey, HashSet<JobId>>,
}

impl StateStore {
    /// Creates a new empty state store.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, completed_ttl: Duration) -> Self {
        Self {
            state: RwLock::new(StateData {
                runs: HashMap::new(),
                jobs: HashMap::new(),
                jobs_by_run: HashMap::new(),
                jobs_by_repo: HashMap::new(),
            }),
            clock,
            completed_ttl,
        }
    }
}
