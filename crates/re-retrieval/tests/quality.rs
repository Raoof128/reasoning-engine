//! Quality hints and classification on real recorded provider responses.

use std::path::PathBuf;

use re_retrieval::quality::classify;
use re_retrieval::sources::{SourceConfig, free_source};

fn parse(source: &str) -> Vec<re_model::records::EvidenceRecord> {
    let body = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/fixtures/http/{source}.body")),
    )
    .expect("fixture");
    free_source(source, &SourceConfig::default())
        .expect("source")
        .parse_body("run", "reward model", 3, &body)
        .evidence
}

#[test]
fn openalex_records_carry_type_venue_retraction_and_oa_pdf() {
    let records = parse("openalex");
    let first = &records[0];
    assert_eq!(first.hints.publication_types, ["conference-paper"]);
    assert_eq!(first.hints.venue_type.as_deref(), Some("conference"));
    assert_eq!(first.hints.retracted, Some(false));
    assert_eq!(first.hints.open_access, Some(true));
    assert_eq!(classify(&first.hints), Some("peer_reviewed_article"));
    assert!(
        !first.to_dict().to_string().contains("conference-paper"),
        "hints never reach the Python-compatible record"
    );
}

#[test]
fn europepmc_records_carry_publication_types() {
    let records = parse("europepmc");
    let first = &records[0];
    assert_eq!(first.hints.publication_types, ["Preprint"]);
    assert_eq!(first.hints.retracted, Some(false));
    assert_eq!(first.hints.open_access, Some(false));
    assert_eq!(classify(&first.hints), Some("preprint"));
}
