//! Ed25519 run-pack signatures and the full trust check (`trust` feature,
//! plan Phase 7).
//!
//! The key is a 32-byte seed stored as hex in `signing.key` (mode `0600` on
//! Unix), created on first use. Signatures cover the attestation's
//! `run_pack_hash`, which already commits to every artifact hash.
//!
//! [`check_run_pack`] combines three checks and maps them to `verify-runpack`
//! exit codes:
//!
//! - `0` **ok**: artifacts match and the signature verifies (against the
//!   expected key, when one is given);
//! - `1` **tampered**: an artifact, the signature, the expected key or the
//!   transparency log disagrees with the pack;
//! - `2` **unknown**: the artifacts match but nothing vouches for them (no
//!   signature).

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use re_store::Store;
use re_store::transparency::verify_chain;
use serde_json::{Value, json};

use crate::runpack::{AttestationSigner, verify_run_pack_attestation};

/// The only algorithm this build signs with.
pub const ALGORITHM: &str = "ed25519";

/// An Ed25519 signer.
pub struct Ed25519Signer {
    key: SigningKey,
}

impl std::fmt::Debug for Ed25519Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret.
        write!(f, "Ed25519Signer({})", self.public_key())
    }
}

fn parse_seed(text: &str) -> io::Result<[u8; 32]> {
    let hex = text.trim();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(io::Error::other("signing key must be 64 hex characters"));
    }
    let mut seed = [0; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(io::Error::other)?;
    }
    Ok(seed)
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(text.as_bytes())
}

impl Ed25519Signer {
    /// A signer from a 32-byte seed.
    #[must_use]
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(seed),
        }
    }

    /// Load the key at `path`, creating it (with fresh OS randomness, mode
    /// `0600`) when it does not exist. Returns the signer and whether it was
    /// created.
    ///
    /// # Errors
    /// An unreadable or malformed key file, or no OS randomness.
    pub fn load_or_create(path: &Path) -> io::Result<(Self, bool)> {
        match fs::read_to_string(path) {
            Ok(text) => Ok((Self::from_seed(&parse_seed(&text)?), false)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut seed = [0; 32];
                getrandom::fill(&mut seed).map_err(|e| io::Error::other(e.to_string()))?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let hex = seed.iter().fold(String::with_capacity(64), |mut hex, b| {
                    let _ = write!(hex, "{b:02x}");
                    hex
                });
                write_private(path, &(hex + "\n"))?;
                Ok((Self::from_seed(&seed), true))
            }
            Err(error) => Err(error),
        }
    }
}

impl AttestationSigner for Ed25519Signer {
    fn algorithm(&self) -> &'static str {
        ALGORITHM
    }

    fn public_key(&self) -> String {
        STANDARD.encode(self.key.verifying_key().to_bytes())
    }

    fn sign(&self, message: &[u8]) -> String {
        STANDARD.encode(self.key.sign(message).to_bytes())
    }
}

/// Whether `signature` (base64) by `public_key` (base64) covers `message`.
/// Uses strict verification (no malleable or small-order signatures).
#[must_use]
pub fn verify_signature(public_key: &str, message: &[u8], signature: &str) -> bool {
    let key: Option<[u8; 32]> = STANDARD
        .decode(public_key)
        .ok()
        .and_then(|b| b.try_into().ok());
    let sig: Option<[u8; 64]> = STANDARD
        .decode(signature)
        .ok()
        .and_then(|b| b.try_into().ok());
    let (Some(key), Some(sig)) = (key, sig) else {
        return false;
    };
    VerifyingKey::from_bytes(&key).is_ok_and(|key| {
        key.verify_strict(message, &Signature::from_bytes(&sig))
            .is_ok()
    })
}

/// Artifact hashes, signature and (when `store` is given) transparency-log
/// inclusion for the pack at `dir`. `expected_key` pins the signer.
///
/// Returns the report and the exit code (`0` ok, `1` tampered, `2` unknown).
#[must_use]
pub fn check_run_pack(
    dir: &Path,
    expected_key: Option<&str>,
    store: Option<&Store>,
) -> (Value, u8) {
    let artifacts = verify_run_pack_attestation(dir);
    let attestation: Value = fs::read_to_string(dir.join("attestation.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let field = |key: &str| attestation.get(key).and_then(Value::as_str);
    let hash = field("run_pack_hash").unwrap_or_default();
    let signature = match (
        field("signature"),
        field("public_key"),
        field("signature_algorithm"),
    ) {
        (Some(sig), Some(key), Some(ALGORITHM)) => {
            let valid = verify_signature(key, hash.as_bytes(), sig);
            let key_ok = expected_key.is_none_or(|expected| expected == key);
            json!({"present": true, "valid": valid, "public_key": key, "expected_key_matches": key_ok})
        }
        (Some(_), _, algorithm) => {
            json!({"present": true, "valid": false, "error": format!("unsupported algorithm {algorithm:?}")})
        }
        _ => json!({"present": false}),
    };
    let log = store.map_or(json!({"checked": false}), |store| {
        let run_id = field("run_id").unwrap_or_default();
        match store.inclusion_proof(run_id) {
            Ok(Some(chain)) => {
                let consistent = verify_chain(&chain);
                let entry = chain.last().map(|e| e.run_pack_hash.as_str());
                json!({
                    "checked": true,
                    "included": true,
                    "seq": chain.last().map(|e| e.seq),
                    "chain_valid": consistent.is_ok(),
                    "chain_error": consistent.err(),
                    "hash_matches": entry == Some(hash),
                    "head": chain.last().map(|e| e.node_hash.clone()),
                })
            }
            Ok(None) => json!({"checked": true, "included": false}),
            Err(error) => json!({"checked": false, "error": error.to_string()}),
        }
    });
    let tampered = artifacts["valid"] != true
        || signature["valid"] == false
        || signature["expected_key_matches"] == false
        || log["chain_valid"] == false
        || log["hash_matches"] == false;
    let code = if tampered {
        1
    } else if signature["present"] == true {
        0
    } else {
        2
    };
    let verdict = ["ok", "tampered", "unknown"][usize::from(code)];
    (
        json!({
            "verdict": verdict,
            "valid": artifacts["valid"],
            "mismatched_artifacts": artifacts["mismatched_artifacts"],
            "missing_artifacts": artifacts["missing_artifacts"],
            "run_pack_hash": artifacts.get("run_pack_hash").cloned().unwrap_or(Value::Null),
            "signature": signature,
            "transparency_log": log,
        }),
        code,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_created_private_and_reloaded() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("keys").join("signing.key");
        let (first, created) = Ed25519Signer::load_or_create(&path).expect("create");
        assert!(created);
        let (again, created) = Ed25519Signer::load_or_create(&path).expect("load");
        assert!(!created);
        assert_eq!(first.public_key(), again.public_key());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        assert!(!format!("{first:?}").contains(&fs::read_to_string(&path).unwrap()[..16]));
        fs::write(&path, "not hex").unwrap();
        assert!(Ed25519Signer::load_or_create(&path).is_err());
    }

    #[test]
    fn signatures_verify_only_for_the_signed_message_and_key() {
        let signer = Ed25519Signer::from_seed(&[7; 32]);
        let other = Ed25519Signer::from_seed(&[8; 32]);
        let sig = signer.sign(b"sha256:abc");
        assert!(verify_signature(&signer.public_key(), b"sha256:abc", &sig));
        assert!(!verify_signature(&signer.public_key(), b"sha256:abd", &sig));
        assert!(!verify_signature(&other.public_key(), b"sha256:abc", &sig));
        assert!(!verify_signature("garbage", b"sha256:abc", &sig));
        assert!(!verify_signature(
            &signer.public_key(),
            b"sha256:abc",
            "AAAA"
        ));
    }
}
