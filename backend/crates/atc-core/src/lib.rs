#![deny(missing_docs)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Core domain types and business logic for ATC.

pub mod clock;
pub mod job;
pub mod run;
pub mod types;

pub use clock::{Clock, SystemClock};
#[cfg(any(test, feature = "test-support"))]
pub use clock::TestClock;
pub use job::{Job, JobConclusion, JobStatus, RunnerInfo, Step, StepStatus};
pub use run::{RunConclusion, RunStatus, WorkflowRun};
pub use types::{JobId, LabelSet, RepoKey, RunId};
