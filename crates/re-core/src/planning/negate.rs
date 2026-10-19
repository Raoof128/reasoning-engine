//! Contradiction queries for the adversarial angle (plan Phase 10).
//!
//! [`negated_queries`] turns a claim or query into searches for evidence
//! against it, from fixed templates: `<X> does not <verb> <Y>`,
//! `no effect of <X> on <Y>`, `<X> replication failure` and
//! `<X> limitations`. The verb is the first one in [`VERBS`] that appears
//! as a whole word; without one, `<X>` is the whole text. Deterministic,
//! so the same claim always yields the same searches.

/// Verbs a claim may turn on, as (third person, base form).
pub const VERBS: &[(&str, &str)] = &[
    ("improves", "improve"),
    ("reduces", "reduce"),
    ("increases", "increase"),
    ("decreases", "decrease"),
    ("causes", "cause"),
    ("prevents", "prevent"),
    ("outperforms", "outperform"),
    ("predicts", "predict"),
    ("enhances", "enhance"),
    ("impairs", "impair"),
    ("affects", "affect"),
    ("lowers", "lower"),
    ("raises", "raise"),
    ("boosts", "boost"),
    ("mitigates", "mitigate"),
    ("harms", "harm"),
    ("supports", "support"),
    ("generalises", "generalise"),
    ("generalizes", "generalize"),
    ("detects", "detect"),
    ("protects", "protect"),
    ("accelerates", "accelerate"),
];

/// Words that end a subject without being part of it.
const SKIP_TAIL: &[&str] = &["also", "often", "significantly", "consistently", "strongly"];

fn clean(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', '!', '?', ';', ':', ','])
        .to_string()
}

/// `(subject, base verb, plural subject, object)` when a known verb splits
/// the text.
fn split(text: &str) -> Option<(String, &'static str, bool, String)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (i, word) in words.iter().enumerate().skip(1) {
        let lower = word.to_lowercase();
        let found = VERBS.iter().find_map(|(third, base)| {
            if lower == *third {
                Some((*base, false))
            } else if lower == *base {
                Some((*base, true))
            } else {
                None
            }
        });
        if let Some((base, plural)) = found {
            let mut subject: Vec<&str> = words[..i].to_vec();
            while subject
                .last()
                .is_some_and(|w| SKIP_TAIL.contains(&w.to_lowercase().as_str()))
            {
                subject.pop();
            }
            let object = words[i + 1..].join(" ");
            if subject.is_empty() || object.is_empty() {
                return None;
            }
            return Some((subject.join(" "), base, plural, object));
        }
    }
    None
}

/// Searches for evidence against `claim`, most specific first.
#[must_use]
pub fn negated_queries(claim: &str) -> Vec<String> {
    let claim = clean(claim);
    if claim.is_empty() {
        return Vec::new();
    }
    match split(&claim) {
        Some((subject, verb, plural, object)) => {
            let auxiliary = if plural { "do" } else { "does" };
            vec![
                format!("{subject} {auxiliary} not {verb} {object}"),
                format!("no effect of {subject} on {object}"),
                format!("{subject} replication failure"),
                format!("{subject} limitations"),
            ]
        }
        None => vec![
            format!("evidence against {claim}"),
            format!("{claim} replication failure"),
            format!("{claim} limitations"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_with_a_known_verb_split_into_four_queries() {
        assert_eq!(
            negated_queries("Daytime naps improve afternoon peak power in athletes."),
            [
                "Daytime naps do not improve afternoon peak power in athletes",
                "no effect of Daytime naps on afternoon peak power in athletes",
                "Daytime naps replication failure",
                "Daytime naps limitations",
            ]
        );
        assert_eq!(
            negated_queries("Retrieval augmentation significantly reduces hallucination"),
            [
                "Retrieval augmentation does not reduce hallucination",
                "no effect of Retrieval augmentation on hallucination",
                "Retrieval augmentation replication failure",
                "Retrieval augmentation limitations",
            ]
        );
    }

    #[test]
    fn other_text_gets_generic_queries() {
        assert_eq!(
            negated_queries("  process reward models for code  "),
            [
                "evidence against process reward models for code",
                "process reward models for code replication failure",
                "process reward models for code limitations",
            ]
        );
        assert_eq!(negated_queries(" . "), Vec::<String>::new());
        assert_eq!(
            negated_queries("Improves everything")[0],
            "evidence against Improves everything",
            "a verb needs a subject"
        );
    }
}
