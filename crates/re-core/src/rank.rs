//! Lexical ranking for the hybrid verifier: Okapi BM25 over a small set of
//! snippets and reciprocal-rank fusion (RRF) of several rankings.
//!
//! The verifier ranks at most a few dozen snippets per claim, so a search
//! engine would be all overhead; these are the textbook formulas.

/// BM25 term-frequency saturation.
pub const BM25_K1: f64 = 1.2;
/// BM25 length normalisation.
pub const BM25_B: f64 = 0.75;
/// RRF constant from Cormack, Clarke & Buettcher (2009).
pub const RRF_K: f64 = 60.0;

/// Lowercased runs of letters and digits.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// BM25 score of every document for `query`, in document order. IDF uses
/// Lucene's `ln(1 + (N - n + 0.5) / (n + 0.5))`, which stays positive when a
/// term appears in most documents.
#[must_use]
pub fn bm25_scores(query: &str, documents: &[&str]) -> Vec<f64> {
    let docs: Vec<Vec<String>> = documents.iter().map(|d| tokenize(d)).collect();
    if docs.is_empty() {
        return Vec::new();
    }
    let n = docs.len() as f64;
    let avg_len = docs.iter().map(Vec::len).sum::<usize>() as f64 / n;
    let mut terms = tokenize(query);
    terms.sort_unstable();
    terms.dedup();
    let idf: Vec<f64> = terms
        .iter()
        .map(|t| {
            let containing = docs.iter().filter(|d| d.contains(t)).count() as f64;
            (1.0 + (n - containing + 0.5) / (containing + 0.5)).ln()
        })
        .collect();
    docs.iter()
        .map(|doc| {
            let len_norm = if avg_len > 0.0 {
                doc.len() as f64 / avg_len
            } else {
                0.0
            };
            terms
                .iter()
                .zip(&idf)
                .map(|(term, idf)| {
                    let tf = doc.iter().filter(|t| *t == term).count() as f64;
                    idf * tf * (BM25_K1 + 1.0) / (tf + BM25_K1 * (1.0 - BM25_B + BM25_B * len_norm))
                })
                .sum()
        })
        .collect()
}

/// Indices ordered by descending score; ties keep document order.
#[must_use]
pub fn ranking(scores: &[f64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    order
}

/// Fuse rankings: each document scores `sum(1 / (k + rank))` over the
/// rankings it appears in (rank from 1). Returns `(index, score)` by
/// descending score; ties keep the lower index first.
#[must_use]
pub fn reciprocal_rank_fusion(rankings: &[Vec<usize>], k: f64) -> Vec<(usize, f64)> {
    let size = rankings.iter().flatten().max().map_or(0, |m| m + 1);
    let mut fused = vec![0.0; size];
    let mut seen = vec![false; size];
    for ranking in rankings {
        for (position, &index) in ranking.iter().enumerate() {
            fused[index] += 1.0 / (k + position as f64 + 1.0);
            seen[index] = true;
        }
    }
    let mut out: Vec<(usize, f64)> = fused
        .into_iter()
        .enumerate()
        .filter(|(i, _)| seen[*i])
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenises_unicode_words() {
        assert_eq!(
            tokenize("PRMs, ORM-based: café 12%"),
            ["prms", "orm", "based", "café", "12"]
        );
    }

    #[test]
    fn bm25_prefers_matching_and_shorter_documents() {
        let docs = [
            "process reward models score every step",
            "outcome reward models score the final answer only, which is cheaper to label at scale",
            "gardening tomatoes in spring",
        ];
        let scores = bm25_scores("process reward step", &docs);
        assert_eq!(ranking(&scores), [0, 1, 2]);
        assert!(scores[2].abs() < f64::EPSILON);
        assert!(scores.iter().all(|s| *s >= 0.0));
        assert_eq!(bm25_scores("anything", &[]), Vec::<f64>::new());
    }

    #[test]
    fn bm25_matches_a_hand_computation() {
        // One query term in one of two equal-length docs:
        // idf = ln(1 + 1.5 / 1.5) = ln 2; tf = 1; len_norm = 1
        // score = ln2 * 1 * 2.2 / (1 + 1.2) = ln 2.
        let scores = bm25_scores("alpha", &["alpha beta", "gamma delta"]);
        assert!((scores[0] - std::f64::consts::LN_2).abs() < 1e-12);
        assert!(scores[1].abs() < f64::EPSILON);
    }

    #[test]
    fn rrf_rewards_agreement_and_breaks_ties_by_index() {
        let fused = reciprocal_rank_fusion(&[vec![2, 0, 1], vec![0, 2, 1]], RRF_K);
        let order: Vec<usize> = fused.iter().map(|(i, _)| *i).collect();
        assert_eq!(order, [0, 2, 1], "0 and 2 tie; 0 has the lower index");
        assert!((fused[2].1 - 2.0 / 63.0).abs() < 1e-12);
        assert_eq!(reciprocal_rank_fusion(&[], RRF_K), Vec::new());
    }
}
