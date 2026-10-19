//! Open-access PDFs to labelled sections, with `pdf-extract`.
//!
//! PDF has no structure to read, so sections come from heading lines: a
//! short line that is only a known heading, optionally numbered
//! (`1 Introduction`, `IV. RESULTS`, `Abstract`). Text before the first
//! heading is `front` (title, authors). Everything from `References` or
//! `Bibliography` on is dropped. Words hyphenated across lines are joined.
//!
//! `pdf-extract` panics on some malformed files; a panic is caught and
//! reported as `malformed_response`.

use std::sync::LazyLock;

use regex::Regex;

use super::{Section, collapse, malformed};
use crate::error::AdapterError;

static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)^
        (?:(?:\d+(?:\.\d+)*|[IVX]+)\.?\s+)?
        (abstract|introduction|background|related\ work|preliminaries
        |methods?|methodology|materials\ and\ methods|approach|experimental\ setup
        |experiments?|results?|evaluation|results\ and\ discussion|discussion
        |limitations?|conclusions?|conclusion\ and\ future\ work|concluding\ remarks
        |references|bibliography|acknowledge?ments?|appendix)
        \s*[.:]?$",
    )
    .expect("valid regex")
});

/// An inline `Abstract.` or `Abstract—` opening a paragraph.
static INLINE_ABSTRACT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^abstract\s*[.:—–-]\s*(\S.*)$").expect("valid regex"));

/// Extract and section a PDF.
///
/// # Errors
/// `malformed_response` when the PDF cannot be read.
pub fn parse(bytes: &[u8]) -> Result<Vec<Section>, AdapterError> {
    let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes))
        .map_err(|_| malformed("the PDF parser failed"))?
        .map_err(malformed)?;
    Ok(sections(&text))
}

/// Split extracted text into labelled sections.
#[must_use]
pub fn sections(text: &str) -> Vec<Section> {
    let mut sections = vec![Section::new("", String::new())];
    sections[0].label = "front".into();
    let mut buffer = String::new();
    let flush = |sections: &mut Vec<Section>, buffer: &mut String| {
        let text = collapse(buffer);
        buffer.clear();
        if let Some(last) = sections.last_mut() {
            last.text = text;
        }
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.len() <= 60 && HEADING.is_match(line) {
            flush(&mut sections, &mut buffer);
            let lower = line.to_lowercase();
            if lower.contains("references") || lower.contains("bibliography") {
                break;
            }
            sections.push(Section::new(line, String::new()));
            continue;
        }
        if let Some(rest) = INLINE_ABSTRACT.captures(line).and_then(|c| c.get(1))
            && sections.len() == 1
        {
            flush(&mut sections, &mut buffer);
            sections.push(Section::new("Abstract", String::new()));
            buffer.push_str(rest.as_str());
            buffer.push(' ');
            continue;
        }
        // Join `exam-` + `ple` across a line break.
        if let Some(stem) = line.strip_suffix('-')
            && stem.chars().last().is_some_and(char::is_alphabetic)
        {
            buffer.push_str(stem);
        } else {
            buffer.push_str(line);
            buffer.push(' ');
        }
    }
    if !buffer.is_empty() {
        flush(&mut sections, &mut buffer);
    }
    sections.retain(|s| !s.text.is_empty());
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_split_and_references_end_the_text() {
        let text = "VerifiAgent: a unified agent\nJane Doe\n\nAbstract\nWe verify reason-\ning steps.\n\
            1 Introduction\nLLMs make mistakes.\n2.1 A subsection heading\nStill intro.\n\
            4 Experiments\nAccuracy rose to 91%.\nReferences\n[1] Someone. 2024.";
        let summary: Vec<(String, String)> = sections(text)
            .into_iter()
            .map(|s| (s.label, s.text))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "front".into(),
                    "VerifiAgent: a unified agent Jane Doe".into()
                ),
                ("abstract".into(), "We verify reasoning steps.".into()),
                (
                    "introduction".into(),
                    "LLMs make mistakes. 2.1 A subsection heading Still intro.".into()
                ),
                ("results".into(), "Accuracy rose to 91%.".into()),
            ]
        );
    }

    #[test]
    fn inline_abstracts_open_a_section() {
        let found = sections("Title\nAbstract—Short text here.\nI. INTRODUCTION\nBody.");
        assert_eq!(found[1].label, "abstract");
        assert_eq!(found[1].text, "Short text here.");
        assert_eq!(found[2].label, "introduction");
    }

    #[test]
    fn garbage_is_malformed_not_a_panic() {
        let error = parse(b"%PDF-1.4 not really a pdf").unwrap_err();
        assert_eq!(error.error_type, "malformed_response");
    }
}
