//! Full text from the committed open-access documents (CC BY 4.0; see
//! `tests/fixtures/fulltext/README.md`), and the fetcher's cache and cap.
#![cfg(feature = "fulltext")]

use std::path::{Path, PathBuf};

use re_model::records::QualityHints;
use re_retrieval::HttpClient;
use re_retrieval::fulltext::{FullTextFetcher, Section, europepmc_xml, pdf};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fulltext")
        .join(name)
}

fn labels(sections: &[Section]) -> Vec<&str> {
    sections.iter().map(|s| s.label.as_str()).collect()
}

fn text_of<'a>(sections: &'a [Section], label: &str) -> &'a str {
    &sections
        .iter()
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("no {label} section"))
        .text
}

#[test]
fn europepmc_articles_split_into_labelled_sections() {
    let xml = std::fs::read_to_string(fixture("PMC10710987.xml")).unwrap();
    let naps = europepmc_xml::parse(&xml).unwrap();
    assert_eq!(
        labels(&naps),
        [
            "abstract",
            "other",
            "introduction",
            "methods",
            "results",
            "discussion",
            "conclusion"
        ]
    );
    assert!(text_of(&naps, "abstract").starts_with("Daytime naps are used by elite athletes"));
    assert!(text_of(&naps, "methods").contains("A total of 23 participants were recruited"));
    assert!(
        !text_of(&naps, "introduction").contains("[]"),
        "citations tidied"
    );

    let xml = std::fs::read_to_string(fixture("PMC10405750.xml")).unwrap();
    let protein = europepmc_xml::parse(&xml).unwrap();
    assert_eq!(
        &labels(&protein)[..6],
        [
            "abstract",
            "introduction",
            "methods",
            "results",
            "discussion",
            "conclusion"
        ]
    );
    assert!(text_of(&protein, "methods").contains("Twenty male trained swimmers"));
}

#[test]
fn an_arxiv_pdf_splits_on_its_headings() {
    let bytes = std::fs::read(fixture("arxiv-2504.00406v2-p1-6.pdf")).unwrap();
    let sections = pdf::parse(&bytes).unwrap();
    assert_eq!(
        labels(&sections),
        ["front", "abstract", "introduction", "other", "results"]
    );
    assert!(text_of(&sections, "front").starts_with("VerifiAgent"));
    assert!(text_of(&sections, "abstract").starts_with("Large language models"));
}

#[tokio::test]
async fn the_fetcher_caches_parsed_documents_and_caps_bodies() {
    let server = MockServer::start().await;
    let xml = std::fs::read_to_string(fixture("PMC10710987.xml")).unwrap();
    Mock::given(method("GET"))
        .and(path("/PMC10710987/fullTextXML"))
        .and(header("accept", "application/xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(xml))
        .expect(1)
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    let fetcher = FullTextFetcher::new(HttpClient::new().unwrap())
        .with_europepmc_base(server.uri())
        .with_cache_dir(cache.path());
    let hints = QualityHints {
        pmcid: Some("PMC10710987".into()),
        ..QualityHints::default()
    };
    let first = fetcher.fetch(&hints).await.unwrap().expect("a document");
    assert_eq!(first.format, "jats");
    assert_eq!(first.sections.len(), 7);
    let second = fetcher.fetch(&hints).await.unwrap().expect("cached");
    assert_eq!(first, second, "the second fetch is served from disk");
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 1);

    assert_eq!(
        fetcher.fetch(&QualityHints::default()).await.unwrap(),
        None,
        "no open-access source"
    );

    Mock::given(method("GET"))
        .and(path("/PMC1/fullTextXML"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b' '; 21 * 1024 * 1024]))
        .mount(&server)
        .await;
    let error = fetcher
        .fetch(&QualityHints {
            pmcid: Some("PMC1".into()),
            ..QualityHints::default()
        })
        .await
        .unwrap_err();
    assert_eq!(error.error_type, "unavailable");
    assert!(
        error.message.contains("larger than 20 MB"),
        "{}",
        error.message
    );
}
