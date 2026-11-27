//! Draft-to-claim extraction (port of `claims.extract_claims`).

use re_core::claims::{ClaimType, extract_draft_claims};
use re_model::records::{ClaimRecord, ModelError};

fn type_name(kind: ClaimType) -> &'static str {
    match kind {
        ClaimType::Quantitative => "quantitative",
        ClaimType::Causal => "causal",
        ClaimType::Normative => "normative",
        ClaimType::Hypothesis => "hypothesis",
        ClaimType::Empirical => "empirical",
    }
}

/// Section headings under which a draft lists what it cites. Nothing below
/// one of these is a claim.
const REFERENCE_HEADINGS: [&str; 6] = [
    "sources",
    "references",
    "bibliography",
    "works cited",
    "citations",
    "further reading",
];

fn heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|&c| c == '#').count();
    let rest = trimmed.get(level..)?;
    ((1..=6).contains(&level) && rest.starts_with(' ')).then(|| (level, rest.trim()))
}

fn list_item(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return Some(rest);
        }
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    let rest = trimmed.get(digits..)?;
    if (1..=3).contains(&digits) && (rest.starts_with(". ") || rest.starts_with(") ")) {
        return rest.get(2..);
    }
    None
}

/// Whether `text` is written in Markdown: a heading, a table row, a code
/// fence or a list item starts one of its lines. Plain prose is left exactly
/// as it is, so drafts that were never Markdown extract as before.
fn looks_like_markdown(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        heading(line).is_some()
            || trimmed.starts_with('|')
            || trimmed.starts_with("```")
            || list_item(line).is_some()
    })
}

/// `[label](target)` to `label`, and emphasis and code marks removed.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            let target = &after[close + 2..];
            target
                .find(')')
                .map(|end| (&after[..close], &target[end + 1..]))
        });
        match link {
            Some((label, tail)) if !label.contains('\n') => {
                out.push_str(&rest[..open]);
                out.push_str(label);
                rest = tail;
            }
            _ => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "").replace("__", "").replace('`', "")
}

/// The prose of a Markdown draft, one statement per sentence.
///
/// A report's headings, table rows, code blocks and reference list are not
/// claims, but a sentence splitter that only looks for full stops turns each
/// of them into one, and every one then needs a citation it cannot have.
/// Headings, tables, rules and fenced code are dropped, everything under a
/// references heading is dropped, list items become sentences of their own,
/// and a paragraph that ends without a full stop gets one so it does not run
/// into the next.
fn markdown_prose(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut paragraph = String::new();
    let mut in_fence = false;
    // The level of the references heading we are under, if any.
    let mut skipping: Option<usize> = None;
    let flush = |paragraph: &mut String, out: &mut Vec<String>| {
        let text = inline(paragraph.trim());
        paragraph.clear();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if text.ends_with(['.', '!', '?']) {
            out.push(text.to_string());
        } else {
            out.push(format!("{}.", text.trim_end_matches([':', ';', ','])));
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut paragraph, &mut out);
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some((level, title)) = heading(line) {
            flush(&mut paragraph, &mut out);
            let title = title.trim_end_matches('#').trim().to_lowercase();
            if REFERENCE_HEADINGS.contains(&title.as_str()) {
                skipping = Some(level);
            } else if skipping.is_some_and(|under| level <= under) {
                skipping = None;
            }
            continue;
        }
        if skipping.is_some() {
            continue;
        }
        let rule = trimmed.len() >= 3
            && trimmed
                .chars()
                .all(|c| matches!(c, '-' | '*' | '_' | '=' | ' '));
        if trimmed.is_empty() || trimmed.starts_with('|') || rule {
            flush(&mut paragraph, &mut out);
            continue;
        }
        if let Some(item) = list_item(line) {
            flush(&mut paragraph, &mut out);
            paragraph.push_str(item.trim());
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed.trim_start_matches('>').trim());
    }
    flush(&mut paragraph, &mut out);
    out.join(" ")
}

/// One `needs_more_evidence` claim per sentence of `text`. A Markdown draft
/// is read as prose first: headings, tables, code and the reference list
/// are dropped, and list items become sentences.
///
/// # Errors
/// Never in practice: every extracted type is a valid claim type.
pub fn extract_claims(
    run_id: &str,
    text: &str,
    domain: &str,
) -> Result<Vec<ClaimRecord>, ModelError> {
    let prose;
    let text = if looks_like_markdown(text) {
        prose = markdown_prose(text);
        prose.as_str()
    } else {
        text
    };
    extract_draft_claims(text, domain)
        .into_iter()
        .map(|draft| {
            ClaimRecord::new(
                &draft.claim_id,
                run_id,
                &draft.text,
                type_name(draft.claim_type),
                domain,
                &draft.importance,
                &draft.risk_level,
                draft.requires_citation,
                "draft",
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(draft: &str) -> Vec<String> {
        extract_claims("run_1", draft, "general")
            .expect("valid claims")
            .into_iter()
            .map(|c| c.text)
            .collect()
    }

    #[test]
    fn plain_prose_is_untouched() {
        assert_eq!(
            texts("Mock evidence for MCP defences. Teams should audit tools!  It works?"),
            [
                "Mock evidence for MCP defences",
                "Teams should audit tools",
                "It works"
            ]
        );
    }

    #[test]
    fn headings_tables_code_and_references_are_not_claims() {
        let draft = "# Report\n\n## Key findings\n\n### 1. What is installed\n\n\
                     The skill holds 15 files. It has **no tests**.\n\n\
                     | Probe | Result |\n| --- | --- |\n| a | CLEAN, score 0.033 |\n\n\
                     ```bash\ncargo test. It passes.\n```\n\n\
                     See [the paper](https://doi.org/10.1/x) for `details`.\n\n\
                     ## Sources\n\n1. El Attar et al. Findings of AACL 2026. https://doi.org/10.1/y\n\n\
                     ### Notes\n\nStill under sources.\n\n## Open questions\n\nDoes it scale?\n";
        assert_eq!(
            texts(draft),
            [
                "The skill holds 15 files",
                "It has no tests",
                "See the paper for details",
                "Does it scale",
            ]
        );
    }

    #[test]
    fn list_items_and_unterminated_paragraphs_are_their_own_sentences() {
        let draft = "Findings:\n\n- Scores rose by 12%\n- Recall fell\n\nA closing line\nwrapped over two lines";
        assert_eq!(
            texts(draft),
            [
                "Findings",
                "Scores rose by 12%",
                "Recall fell",
                "A closing line wrapped over two lines",
            ]
        );
    }
}
