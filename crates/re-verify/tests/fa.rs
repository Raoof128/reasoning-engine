//! Persian and cross-lingual verification (plan Phase 12, `fa` feature).
//! The model tests are ignored by default because `bge-m3` is a 570 MB
//! download; run with `cargo nextest run -p re-verify --features
//! fa,semantic --run-ignored all --test fa`.
#![cfg(feature = "fa")]

use re_model::records::{ClaimRecord, EvidenceRecord};
use re_retrieval::base::{RecordInput, build_record};
use re_verify::LexicalOverlap;
use re_verify::verify::Verifier;

fn claim(text: &str) -> ClaimRecord {
    ClaimRecord::new(
        "claim_0001",
        "run",
        text,
        "empirical",
        "general",
        "medium",
        "low",
        true,
        "test",
    )
    .unwrap()
}

fn evidence(snippets: &[&str]) -> Vec<EvidenceRecord> {
    snippets
        .iter()
        .enumerate()
        .map(|(i, snippet)| {
            build_record(RecordInput {
                title: format!("source {i}"),
                snippet: (*snippet).to_string(),
                ..RecordInput::new("run", "test", "q", i + 1)
            })
        })
        .collect()
}

#[test]
fn the_lexical_tier_matches_persian_spellings() {
    // Arabic yeh and kaf and a missing half-space in the evidence; the
    // standard spelling in the claim.
    let mut claims = vec![claim("خواب روزانه توان بعدازظهر ورزشکاران را بهبود می‌بخشد")];
    let evidence = evidence(&["در این پژوهش خواب روزانه توان بعدازظهر ورزشكاران را بهبود ميبخشد."]);
    let record = LexicalOverlap.verify(&mut claims, &evidence).remove(0);
    assert_eq!(record.support_status, "supported", "{record:?}");
}

#[cfg(feature = "semantic")]
mod models {
    use re_verify::hybrid::{Embedder, cosine};
    use re_verify::models::{BGE_M3, Multilingual, OnnxEmbedder, default_models_dir};

    use super::{claim, evidence};

    /// English abstracts' sentences and their Persian renderings (ours).
    const PAIRS: [(&str, &str); 3] = [
        (
            "Daytime naps improve afternoon power output in rugby players.",
            "خواب روزانه توان خروجی بعدازظهر را در بازیکنان راگبی افزایش می‌دهد.",
        ),
        (
            "Process reward models score every intermediate reasoning step.",
            "مدل‌های پاداش فرایندی به هر گام میانی استدلال امتیاز می‌دهند.",
        ),
        (
            "Vitamin D supplementation did not reduce fracture risk in older adults.",
            "مصرف مکمل ویتامین دی خطر شکستگی را در سالمندان کاهش نداد.",
        ),
    ];

    #[tokio::test]
    #[ignore = "downloads bge-m3"]
    async fn a_persian_query_finds_its_english_source() {
        let m3 = OnnxEmbedder::load_spec(&BGE_M3, &default_models_dir(), false)
            .await
            .expect("model");
        let english: Vec<&str> = PAIRS.iter().map(|p| p.0).collect();
        let persian: Vec<&str> = PAIRS.iter().map(|p| p.1).collect();
        let e = m3.embed(&english).unwrap();
        let p = m3.embed(&persian).unwrap();
        assert_eq!(e[0].len(), 1024);
        for (i, query) in p.iter().enumerate() {
            let scores: Vec<f32> = e.iter().map(|v| cosine(query, v)).collect();
            assert!(scores[i] > 0.6, "pair {i}: {scores:?}");
            let best = scores
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap()
                .0;
            assert_eq!(best, i, "pair {i}: {scores:?}");
        }
    }

    #[tokio::test]
    #[ignore = "downloads bge-small and bge-m3"]
    async fn english_stays_on_bge_small_and_persian_routes_to_m3() {
        let router = Multilingual::load(&default_models_dir(), false)
            .await
            .expect("models");
        assert!(router.id().contains("bge-small-en-v1.5-q@"));
        assert!(router.id().contains("+bge-m3-q@"));
        assert_eq!(router.embed(&[PAIRS[0].0]).unwrap()[0].len(), 384);
        assert_eq!(router.embed(&[PAIRS[0].1]).unwrap()[0].len(), 1024);
        let mixed = router.route(&[PAIRS[0].0, PAIRS[0].1]).unwrap();
        assert_eq!(mixed.embed(&[PAIRS[0].0]).unwrap()[0].len(), 1024);
    }

    #[tokio::test]
    #[ignore = "downloads bge-small, bge-m3 and the NLI model"]
    async fn nli_hands_persian_pairs_to_the_hybrid_tier() {
        let nli = re_verify::load_verifier(Some("nli"), &default_models_dir(), false)
            .await
            .expect("models");
        let mut claims = vec![claim(PAIRS[0].1)];
        let evidence = evidence(&[PAIRS[0].0, PAIRS[2].0]);
        let record = nli.verify(&mut claims, &evidence).remove(0);
        assert_eq!(record.support_status, "supported", "{record:?}");
        assert!(
            record.support_rationale.ends_with(
                "The NLI model reads English only, so this non-English pair was judged by the \
                 hybrid tier."
            ),
            "{}",
            record.support_rationale
        );
    }
}

mod pipeline {
    use std::sync::Arc;

    use re_retrieval::base::{RecordInput, build_record};
    use re_retrieval::sources::BoxFuture;
    use re_retrieval::{AcademicSearchAggregator, RetrievalResult, SourceAdapter};
    use re_store::Store;
    use re_verify::{LexicalOverlap, VerifiableResearchService};

    const CLAIM: &str = "خواب روزانه توان بعدازظهر ورزشکاران راگبی را بهبود می‌بخشد.";

    /// One Persian abstract that supports the claim, in Arabic spelling.
    struct Persian;

    impl SourceAdapter for Persian {
        fn name(&self) -> &'static str {
            "openalex"
        }

        fn search<'a>(
            &'a self,
            run_id: &'a str,
            query: &'a str,
            _: usize,
        ) -> BoxFuture<'a, RetrievalResult> {
            Box::pin(async move {
                RetrievalResult::ok(vec![build_record(RecordInput {
                    title: "خواب و توان".into(),
                    snippet: "خواب روزانه توان بعدازظهر ورزشكاران راگبي را بهبود ميبخشد.".into(),
                    score: 0.9,
                    ..RecordInput::new(run_id, "openalex", query, 1)
                })])
            })
        }
    }

    #[tokio::test]
    async fn a_persian_query_gets_a_persian_report() {
        let dir = tempfile::tempdir().unwrap();
        let service = VerifiableResearchService::new(
            Store::open(dir.path().join("e.db")).unwrap(),
            dir.path().join("runs"),
            Arc::new(AcademicSearchAggregator::new(vec![Arc::new(Persian)])),
            Box::new(LexicalOverlap),
        );
        let result = service
            .run_research_pipeline(
                "خواب روزانه و توان ورزشی",
                CLAIM,
                "standard",
                "general",
                false,
            )
            .await
            .unwrap();
        let pack = std::path::Path::new(result["run_pack"].as_str().unwrap());
        let report = std::fs::read_to_string(pack.join("report.md")).unwrap();
        assert!(
            report.starts_with("<div dir=\"rtl\" lang=\"fa\">\n\n# گزارش پژوهش `run_"),
            "{report}"
        );
        assert!(
            report.contains("- پرسش: خواب روزانه و توان ورزشی\n"),
            "{report}"
        );
        assert!(
            report.contains(&format!("- [پشتیبانی‌شده] {}", CLAIM.trim_end_matches('.'))),
            "{report}"
        );
        assert!(report.contains("</div>\n"), "{report}");
    }
}
