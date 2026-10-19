//! Pure, I/O-free algorithms for the reasoning engine.
//!
//! Every function here is a port of the Python implementation under
//! `src/reasoning_engine/` and must reproduce `tests/golden/golden_vectors.json`
//! exactly. See `docs/rust-rewrite-plan-2026.md` section 11 for the parity
//! hazards that shaped these ports.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod claims;
pub mod difficulty;
pub mod directional;
pub mod dora;
pub mod gate;
pub mod hashing;
#[rustfmt::skip]
mod html_entities;
pub mod html_unescape;
pub mod planning;
pub mod profiles;
pub mod pyjson;
pub mod pystr;
pub mod rank;
pub mod sanitize;
pub mod types;
pub mod ucb;
pub mod validation;
