//! GitHub webhook payload types for deserialization.
//!
//! These types model the subset of GitHub's webhook JSON that ATC needs.
//! They are `pub(crate)` — consumers of `atc-github` never see them;
//! they only see domain events from `atc-core`.
