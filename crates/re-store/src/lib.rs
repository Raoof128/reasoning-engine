//! `SQLite` persistence for the reasoning engine (port of db.py, sessions.py,
//! memory.py and verifiable/store.py).
//!
//! Databases are interchangeable with the Python implementation: the schema
//! is the same, migrations are additive only, and JSON payloads are written
//! with `re_core::pyjson` so their bytes match what Python writes.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod audit;
mod conn;
mod error;
pub mod memory;
pub mod memory_fts;
pub mod research;
pub mod schema;
pub mod sessions;

pub use conn::Store;
pub use error::StoreError;
