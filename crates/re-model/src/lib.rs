//! Record types, clock and identifier helpers (port of verifiable/models.py).
//!
//! This crate exists so `re-store`, `re-retrieval` and `re-verify` can share
//! records without a dependency cycle. Unlike `re-core` it is allowed to read
//! the clock and the random number generator.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod clock;
pub mod ids;
pub mod records;

pub use records::ModelError;
