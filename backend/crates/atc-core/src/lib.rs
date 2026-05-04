#![deny(missing_docs)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Core domain types and business logic for ATC.

pub mod clock;
pub mod event;
pub mod job;
pub mod persist;
pub mod run;
pub mod store;
pub mod types;

#[cfg(any(test, feature = "test-support"))]
pub use clock::TestClock;
pub use clock::{Clock, SystemClock};
pub use event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
pub use job::{InvalidJobTransition, Job, JobConclusion, JobStatus, RunnerInfo, Step, StepStatus};
pub use persist::{PersistError, PersistentStore};
pub use run::{InvalidRunTransition, RunConclusion, RunStatus, WorkflowRun};
pub use store::{QueryResult, RunnerPoolStats, StateStore, StoreError};
pub use types::{JobId, LabelSet, RepoKey, RunId};
