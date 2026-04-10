#![deny(missing_docs)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Core domain types and business logic for ATC.

pub mod run;
pub mod types;

pub use run::{RunConclusion, RunStatus, WorkflowRun};
pub use types::{JobId, LabelSet, RepoKey, RunId};
