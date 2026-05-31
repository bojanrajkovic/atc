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
pub mod state_machine;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod types;

pub use clock::{Clock, SystemClock};
#[cfg(any(test, feature = "test-support"))]
pub use clock::{TestClock, fixed_test_timestamp};
pub use event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
pub use job::{InvalidJobTransition, Job, JobConclusion, JobStatus, RunnerInfo, Step, StepStatus};
pub use persist::PersistError;
pub use run::{InvalidRunTransition, RunConclusion, RunStatus, WorkflowRun};
pub use state_machine::StateMachineError;
pub use types::{
    JobId, LabelSet, RepoKey, RunId, RunnerPoolCapacity, RunnerPoolStats, RunnerPoolTotal,
};
