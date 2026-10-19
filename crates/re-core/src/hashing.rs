//! Content hashes used by the provenance ledger and run packs.

use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::pyjson::{DumpOptions, dumps};

/// `sha256:<lowercase hex>` of `bytes`.
#[must_use]
pub fn sha256_prefixed(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(7 + 64);
    out.push_str("sha256:");
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Port of Python's `stable_json_hash`: SHA-256 over
/// `json.dumps(value, sort_keys=True, separators=(",", ":"))`, which keeps
/// Python's default `ensure_ascii=True`.
#[must_use]
pub fn stable_json_hash(value: &Value) -> String {
    sha256_prefixed(dumps(value, &DumpOptions::compact_sorted()).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digest() {
        assert_eq!(
            sha256_prefixed(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
