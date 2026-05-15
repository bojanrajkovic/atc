//! PG-side outbox / watermark invariant assertions for integration tests.
//!
//! This module is the canonical home for PG-mode test invariants extracted
//! from the existing integration test files. At extraction time no such
//! helpers existed — every PG test asserted invariants inline via `assert_eq!`
//! on row counts and watermark atomics. As future extraction work identifies
//! repeated invariant-shaped assertions across multiple test files, lift them
//! here behind the same `#[cfg(any(test, feature = "test-support"))]` gate so
//! the test-support feature plays the same role on the PG side as
//! `InMemoryStore::assert_invariants` plays on the in-memory side.
//!
//! Until that lift-and-shift happens, this module exists for the directory
//! shape (AC21) and to anchor the eventual API surface so call sites can
//! migrate without a second relocation.

#![allow(dead_code)]
