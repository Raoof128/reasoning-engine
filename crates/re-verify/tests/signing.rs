//! Signed run packs, the transparency log, and `verify-runpack` verdicts.
#![cfg(feature = "trust")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use re_retrieval::{AcademicSearchAggregator, adapters_from};
use re_store::Store;
use re_verify::signing::{Ed25519Signer, check_run_pack};
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::Value;

fn service(root: &Path, signer: Option<Ed25519Signer>) -> VerifiableResearchService {
    let adapters =
        adapters_from(|var| (var == "REASONING_ENGINE_OFFLINE").then(|| "1".into())).unwrap();
    let service = VerifiableResearchService::new(
        Store::open(root.join("engine.db")).unwrap(),
        root.join("runs"),
        Arc::new(AcademicSearchAggregator::new(adapters)),
        Box::new(LexicalOverlap),
    );
    match signer {
        Some(signer) => service.with_signer(Arc::new(signer)),
        None => service,
    }
}

async fn export(service: &VerifiableResearchService) -> PathBuf {
    let result = service
        .run_research_pipeline(
            "Scholar Gateway exposes semantic search",
            "Scholar Gateway exposes semantic search.",
            "standard",
            "auto",
            false,
        )
        .await
        .unwrap();
    PathBuf::from(result["run_pack"].as_str().unwrap())
}

fn attestation(pack: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(pack.join("attestation.json")).unwrap()).unwrap()
}

#[tokio::test]
async fn signed_packs_verify_and_every_kind_of_tampering_is_caught() {
    let root = tempfile::tempdir().unwrap();
    let signer = Ed25519Signer::from_seed(&[3; 32]);
    let key = re_verify::runpack::AttestationSigner::public_key(&signer);
    let service = service(root.path(), Some(signer));
    let pack = export(&service).await;
    let store = service.store();

    let att = attestation(&pack);
    assert_eq!(att["signature_algorithm"], "ed25519");
    assert_eq!(att["public_key"], key.as_str());

    let (report, code) = check_run_pack(&pack, Some(&key), Some(store));
    assert_eq!(
        (code, report["verdict"].as_str()),
        (0, Some("ok")),
        "{report}"
    );
    assert_eq!(report["transparency_log"]["included"], true);
    assert_eq!(report["transparency_log"]["hash_matches"], true);

    // Pinning a different signer key is a failure even though the
    // signature itself is valid.
    let other =
        re_verify::runpack::AttestationSigner::public_key(&Ed25519Signer::from_seed(&[4; 32]));
    assert_eq!(check_run_pack(&pack, Some(&other), Some(store)).1, 1);

    // An edited artifact.
    let report_md = pack.join("report.md");
    let original = std::fs::read_to_string(&report_md).unwrap();
    std::fs::write(
        &report_md,
        original.replace("Quality gate", "Quality gate (edited)"),
    )
    .unwrap();
    let (report, code) = check_run_pack(&pack, None, Some(store));
    assert_eq!(code, 1, "{report}");
    assert!(
        report["mismatched_artifacts"]
            .to_string()
            .contains("report.md")
    );
    std::fs::write(&report_md, original).unwrap();

    // A forged signature.
    let mut forged = att.clone();
    forged["signature"] =
        Value::String(forged["signature"].as_str().unwrap().replacen('A', "B", 1));
    if forged["signature"] == att["signature"] {
        forged["signature"] =
            Value::String(format!("{}A", &att["signature"].as_str().unwrap()[..86]));
    }
    std::fs::write(
        pack.join("attestation.json"),
        serde_json::to_string_pretty(&forged).unwrap(),
    )
    .unwrap();
    let (report, code) = check_run_pack(&pack, None, Some(store));
    assert_eq!(code, 1);
    assert_eq!(report["signature"]["valid"], false);
    std::fs::write(
        pack.join("attestation.json"),
        serde_json::to_string_pretty(&att).unwrap(),
    )
    .unwrap();
    assert_eq!(check_run_pack(&pack, None, Some(store)).1, 0, "restored");

    // An attacker who re-signs with their own key passes the signature
    // check alone, but not the pinned key or the transparency log.
    let mut resigned = att.clone();
    let attacker = Ed25519Signer::from_seed(&[9; 32]);
    let fake_hash = "sha256:0000";
    resigned["run_pack_hash"] = fake_hash.into();
    resigned["public_key"] = re_verify::runpack::AttestationSigner::public_key(&attacker).into();
    resigned["signature"] =
        re_verify::runpack::AttestationSigner::sign(&attacker, fake_hash.as_bytes()).into();
    std::fs::write(
        pack.join("attestation.json"),
        serde_json::to_string_pretty(&resigned).unwrap(),
    )
    .unwrap();
    let (report, code) = check_run_pack(&pack, None, Some(store));
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["transparency_log"]["hash_matches"], false);
}

#[tokio::test]
async fn unsigned_packs_are_unknown_not_ok() {
    let root = tempfile::tempdir().unwrap();
    let service = service(root.path(), None);
    let pack = export(&service).await;
    let att = attestation(&pack);
    assert_eq!(
        att["signature"],
        Value::Null,
        "unsigned packs keep Python's shape"
    );
    assert!(att.get("public_key").is_none());
    let (report, code) = check_run_pack(&pack, None, Some(service.store()));
    assert_eq!(
        (code, report["verdict"].as_str()),
        (2, Some("unknown")),
        "{report}"
    );
    assert_eq!(report["transparency_log"]["included"], false);
    assert!(
        !service
            .store()
            .table_names()
            .unwrap()
            .contains(&"transparency_log".to_string()),
        "unsigned exports never create the log table"
    );
}
