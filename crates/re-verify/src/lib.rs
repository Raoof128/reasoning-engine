//! Claim extraction, verification, the quality gate and attested run packs
//! (port of `verifiable/claims.py`, `quality.py`, `runpack.py` and
//! `service.py`).
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod bench;
#[cfg(feature = "graph")]
pub mod calibration;
#[cfg(feature = "fulltext")]
pub mod chunks;
#[cfg(feature = "trust")]
pub mod citations;
pub mod claims;
#[cfg(feature = "graph")]
pub mod counter;
pub mod error;
pub mod gate;
pub mod hybrid;
pub mod ledger;
#[cfg(feature = "semantic")]
pub mod models;
pub mod nli;
#[cfg(feature = "trust")]
pub mod numeric;
pub mod replay;
pub mod runpack;
pub mod service;
#[cfg(feature = "trust")]
pub mod signing;
pub mod verify;

pub use error::VerifyError;
pub use service::VerifiableResearchService;
pub use verify::{LexicalOverlap, Verifier, load_verifier, verifier_from_name};
