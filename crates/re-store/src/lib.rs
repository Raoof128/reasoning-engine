//! `SQLite` persistence for the reasoning engine (port of db.py, sessions.py,
//! memory.py and verifiable/store.py).
//!
//! Databases are interchangeable with the Python implementation: the schema
//! is the same, migrations are additive only, and JSON payloads are written
//! with `re_core::pyjson` so their bytes match what Python writes.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod conn;
mod error;
pub mod schema;

pub use conn::Store;
pub use error::StoreError;
