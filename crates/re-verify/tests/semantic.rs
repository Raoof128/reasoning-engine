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
