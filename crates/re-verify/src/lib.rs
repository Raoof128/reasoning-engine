//! Claim extraction, verification, the quality gate and attested run packs
//! (port of `verifiable/claims.py`, `quality.py`, `runpack.py` and
//! `service.py`).
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod claims;
pub mod error;
pub mod gate;
pub mod runpack;
pub mod service;
pub mod verify;

pub use error::VerifyError;
pub use service::VerifiableResearchService;
pub use verify::{LexicalOverlap, Verifier, verifier_from_name};
