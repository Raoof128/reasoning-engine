//! Academic source adapters and the search aggregator (port of
//! `verifiable/sources/*.py` and `verifiable/aggregator.py`).
//!
//! Every adapter is split into a pure `build_url` and a pure `parse_body`,
//! so parity with Python is proven on recorded responses without a network;
//! `search` only adds the HTTP fetch in between.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod aggregate;
pub mod base;
pub mod client;
pub mod error;
pub mod result;
pub mod sources;

pub use aggregate::{AcademicSearchAggregator, adapters_from, default_adapters};
pub use client::HttpClient;
pub use error::AdapterError;
pub use result::RetrievalResult;
pub use sources::SourceAdapter;
