//! `SciFact` for `bench verify --dataset scifact` (plan Phase 12).
//!
//! The dev split (300 claims with labels) and its corpus are downloaded
//! once into `<data_dir>/scifact/`, checked against a pinned SHA-256, and
//! never committed. `SciFact` (Wadden et al. 2020) releases its claims
//! under CC BY 4.0 and its abstracts, from S2ORC, under ODC-By 1.0.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use re_retrieval::HttpClient;

use crate::Failure;

/// The `SciFact` release tarball.
pub const URL: &str = "https://scifact.s3-us-west-2.amazonaws.com/release/latest/data.tar.gz";

/// SHA-256 of [`URL`] when this was written; a different file is refused.
const SHA256: &str = "sha256:11c621288d41ac144d29b13b0f8503b3820b7d6e8b1f6ff24dff335c196d76be";

/// The tarball is about 3 MB.
const MAX_BYTES: usize = 32 * 1024 * 1024;

/// The files used, by their name in the tarball.
const FILES: [&str; 2] = ["claims_dev.jsonl", "corpus.jsonl"];

/// `(claims, corpus)` text, downloading them first when absent.
///
/// # Errors
/// Missing files while offline, a failed or tampered download, or a
/// tarball without the expected files.
pub async fn load(data_dir: &Path, offline: bool) -> Result<(String, String), Failure> {
    let dir = data_dir.join("scifact");
    let paths: Vec<PathBuf> = FILES.iter().map(|name| dir.join(name)).collect();
    if !paths.iter().all(|p| p.is_file()) {
        if offline {
            return Err(format!(
                "SciFact is not in {} and REASONING_ENGINE_OFFLINE=1 forbids downloading it; \
                 put {} there from {URL}",
                dir.display(),
                FILES.join(" and ")
            )
            .into());
        }
        eprintln!("downloading SciFact from {URL}");
        let tarball = HttpClient::new()?
            .get_document(URL, "application/gzip", MAX_BYTES)
            .await?
            .body;
        extract(&tarball, &dir)?;
    }
    let read = |path: &PathBuf| {
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    Ok((read(&paths[0])?, read(&paths[1])?))
}

/// Check `tarball` against the pinned hash and write [`FILES`] into `dir`.
fn extract(tarball: &[u8], dir: &Path) -> Result<(), Failure> {
    let digest = re_core::hashing::sha256_prefixed(tarball);
    if digest != SHA256 {
        return Err(format!("the SciFact download has {digest}, expected {SHA256}").into());
    }
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    let mut found = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        // Only the two known names are kept, written to paths chosen here,
        // so an entry's own path never decides where anything lands.
        let Some(name) = FILES
            .iter()
            .find(|name| path == Path::new("data").join(name))
        else {
            continue;
        };
        let mut text = String::new();
        entry.read_to_string(&mut text)?;
        found.push((*name, text));
    }
    if found.len() != FILES.len() {
        return Err("the SciFact tarball lacks claims_dev.jsonl or corpus.jsonl".into());
    }
    std::fs::create_dir_all(dir)?;
    for (name, text) in found {
        std::fs::write(dir.join(name), text)?;
    }
    Ok(())
}
