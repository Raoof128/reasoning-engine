//! The real ONNX models. Ignored by default because they download about
//! 350 MB on first run; run with `cargo nextest run -p re-verify
//! --features semantic --run-ignored all`.
#![cfg(feature = "semantic")]

use re_verify::hybrid::{Embedder, cosine};
use re_verify::models::{OnnxEmbedder, OnnxNli, default_models_dir};
use re_verify::nli::NliModel;

#[tokio::test]
#[ignore = "downloads the BGE model"]
async fn embeddings_are_unit_length_and_meaningful() {
    let embedder = OnnxEmbedder::load(&default_models_dir(), false)
        .await
        .expect("model");
    let v = embedder
        .embed(&[
            "Process reward models score every reasoning step.",
            "PRMs give feedback on each intermediate step of a solution.",
            "Tomatoes grow best in full sun.",
        ])
        .expect("embed");
    assert_eq!(v[0].len(), 384);
    assert!((cosine(&v[0], &v[0]) - 1.0).abs() < 1e-4);
    let related = cosine(&v[0], &v[1]);
    let unrelated = cosine(&v[0], &v[2]);
    assert!(
        related > unrelated + 0.15,
        "related {related} vs unrelated {unrelated}"
    );
}

#[tokio::test]
#[ignore = "downloads the NLI model"]
async fn nli_separates_entailment_contradiction_and_neutral() {
    let nli = OnnxNli::load(&default_models_dir(), false)
        .await
        .expect("model");
    let scores = nli
        .classify(&[
            (
                "A man is eating pizza at a restaurant.",
                "A man is eating food.",
            ),
            (
                "A man is eating pizza at a restaurant.",
                "Nobody is eating anything.",
            ),
            (
                "A man is eating pizza at a restaurant.",
                "The man is a doctor.",
            ),
        ])
        .expect("classify");
    for s in &scores {
        assert!((s.contradiction + s.entailment + s.neutral - 1.0).abs() < 1e-4);
    }
    assert!(scores[0].entailment > 0.7, "{:?}", scores[0]);
    assert!(scores[1].contradiction > 0.7, "{:?}", scores[1]);
    assert!(
        scores[2].neutral > scores[2].entailment && scores[2].neutral > scores[2].contradiction,
        "{:?}",
        scores[2]
    );
}

#[tokio::test]
async fn offline_never_downloads() {
    let empty = tempfile::tempdir().expect("tmp");
    let error = OnnxNli::load(empty.path(), true)
        .await
        .err()
        .expect("refused");
    assert!(
        error.to_string().contains("REASONING_ENGINE_OFFLINE=1"),
        "{error}"
    );
    assert_eq!(
        std::fs::read_dir(empty.path()).expect("dir").count(),
        0,
        "nothing written"
    );
}

fn labelled_set() -> re_verify::bench::LabelledSet {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/verify/labelled_pairs.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("set")).expect("valid set")
}

/// The Phase 5 exit criterion: NLI finds contradictions lexical cannot.
#[tokio::test]
#[ignore = "downloads both models; prints the benchmark tables"]
async fn nli_beats_lexical_on_contradiction_recall() {
    use re_verify::bench::evaluate;
    use re_verify::{LexicalOverlap, load_verifier};
    let set = labelled_set();
    let dir = default_models_dir();
    let lexical = evaluate(&LexicalOverlap, &set.pairs);
    let hybrid = evaluate(
        load_verifier(Some("hybrid"), &dir, false)
            .await
            .expect("hybrid")
            .as_ref(),
        &set.pairs,
    );
    let nli = evaluate(
        load_verifier(Some("nli"), &dir, false)
            .await
            .expect("nli")
            .as_ref(),
        &set.pairs,
    );
    for report in [&lexical, &hybrid, &nli] {
        println!("{}", report.table());
    }
    assert!(nli.recall("contradicted") > lexical.recall("contradicted"));
    assert!(nli.recall("contradicted") >= 0.5, "{}", nli.table());
}

/// Calibration aid for the hybrid thresholds: claim/evidence cosine by gold
/// label. Run with `--no-capture`.
#[tokio::test]
#[ignore = "downloads the BGE model; prints cosine quantiles"]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "quantile indices of a short, non-empty list"
)]
async fn hybrid_cosine_by_label() {
    let embedder = OnnxEmbedder::load(&default_models_dir(), false)
        .await
        .expect("model");
    let set = labelled_set();
    let scored: Vec<(f32, bool)> = set
        .pairs
        .iter()
        .map(|p| {
            let v = embedder.embed(&[&p.claim, &p.evidence]).expect("embed");
            (cosine(&v[0], &v[1]), p.label == "supported")
        })
        .collect();
    // Supported-vs-rest F1 for each candidate threshold.
    let best = (60..=95)
        .map(|t| {
            let threshold = t as f32 / 100.0;
            let tp = scored.iter().filter(|(c, s)| *s && *c >= threshold).count() as f32;
            let predicted = scored.iter().filter(|(c, _)| *c >= threshold).count() as f32;
            let actual = scored.iter().filter(|(_, s)| *s).count() as f32;
            let (p, r) = (tp / predicted.max(1.0), tp / actual.max(1.0));
            (
                threshold,
                if p + r > 0.0 {
                    2.0 * p * r / (p + r)
                } else {
                    0.0
                },
            )
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("thresholds");
    println!("best supported threshold {:.2} (F1 {:.3})", best.0, best.1);
    for label in re_verify::bench::LABELS {
        let mut cosines: Vec<f32> = set
            .pairs
            .iter()
            .filter(|p| p.label == label)
            .map(|p| {
                let v = embedder.embed(&[&p.claim, &p.evidence]).expect("embed");
                cosine(&v[0], &v[1])
            })
            .collect();
        cosines.sort_by(f32::total_cmp);
        let q = |f: f64| cosines[((cosines.len() - 1) as f64 * f).round() as usize];
        println!(
            "{label:>12}: min {:.3} p25 {:.3} median {:.3} p75 {:.3} max {:.3}",
            q(0.0),
            q(0.25),
            q(0.5),
            q(0.75),
            q(1.0)
        );
    }
}
