//! Europe PMC full-text JATS XML to labelled sections.
//!
//! The abstract becomes one `abstract` section; each top-level `<sec>` of
//! the body becomes a section labelled from its `<title>`, holding the text
//! of every paragraph inside it (nested subsections included). Citations,
//! tables, figures, formulas and the back matter (references,
//! acknowledgements) are skipped. Entities are never expanded beyond the
//! five XML ones and character references, so a hostile DTD cannot inflate
//! the document.

use std::sync::LazyLock;

use quick_xml::Reader;
use quick_xml::events::{BytesRef, Event};
use regex::Regex;

use super::{Section, collapse, malformed};
use crate::error::AdapterError;

/// Elements whose text never reaches a section.
const SKIPPED: [&str; 10] = [
    "table-wrap",
    "fig",
    "disp-formula",
    "inline-formula",
    "math",
    "supplementary-material",
    "ref-list",
    "fn-group",
    "object-id",
    "label",
];

/// What a dropped citation leaves behind: `[]`, `[, ]`, `( ; )`.
static EMPTY_BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(?:\[[\s,;–-]*\]|\([\s,;–-]*\))").expect("valid regex"));

/// Collapse whitespace and tidy citation remnants.
fn tidy(text: &str) -> String {
    let text = collapse(text);
    let text = EMPTY_BRACKETS.replace_all(&text, "");
    text.replace(" .", ".").replace(" ,", ",")
}

fn resolve(reference: &BytesRef<'_>) -> char {
    if let Ok(Some(c)) = reference.resolve_char_ref() {
        return c;
    }
    match &**reference {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "apos" => '\'',
        "quot" => '"',
        // Undeclared or DTD entities: a space keeps words apart.
        _ => ' ',
    }
}

/// Where the parser is.
#[derive(Default)]
struct State {
    /// Open element names, outermost first.
    stack: Vec<String>,
    /// Depth of a skipped element being ignored, if any.
    skip: Option<usize>,
    /// Depth of the paragraph being collected, if any.
    paragraph: Option<usize>,
    /// Depth of the title being collected, if any.
    title: Option<usize>,
    text: String,
    sections: Vec<Section>,
}

impl State {
    fn inside(&self, name: &str) -> bool {
        self.stack.iter().any(|n| n == name)
    }

    /// The body's top-level `<sec>` depth, if inside one.
    fn top_sec(&self) -> Option<usize> {
        let body = self.stack.iter().position(|n| n == "body")?;
        (self.stack.get(body + 1).map(String::as_str) == Some("sec")).then_some(body + 2)
    }

    fn current(&mut self, heading: &str) -> &mut Section {
        if self.sections.last().is_none_or(|s| s.heading != heading) {
            self.sections.push(Section::new(heading, String::new()));
        }
        self.sections.last_mut().expect("pushed above")
    }

    fn open(&mut self, name: &str, citation: bool) {
        self.stack.push(name.to_string());
        let depth = self.stack.len();
        if self.skip.is_some() {
            return;
        }
        if SKIPPED.contains(&name) || name == "back" || citation {
            self.skip = Some(depth);
        } else if name == "p" && self.paragraph.is_none() && self.title.is_none() {
            self.paragraph = Some(depth);
            self.text.clear();
        } else if name == "title" && self.paragraph.is_none() && self.top_sec() == Some(depth - 1) {
            self.title = Some(depth);
            self.text.clear();
        } else if name == "sec" && self.top_sec() == Some(depth) {
            // A new top-level section; its title arrives next.
            self.sections.push(Section::new("", String::new()));
            self.sections.last_mut().expect("pushed").label = "other".into();
        }
    }

    fn close(&mut self) {
        let depth = self.stack.len();
        if self.skip == Some(depth) {
            self.skip = None;
        } else if self.title == Some(depth) {
            self.title = None;
            let heading = collapse(&self.text);
            if let Some(section) = self.sections.last_mut() {
                *section = Section::new(&heading, std::mem::take(&mut section.text));
            }
        } else if self.paragraph == Some(depth) {
            self.paragraph = None;
            let text = tidy(&self.text);
            if !text.is_empty() {
                let section = if self.inside("abstract") && !self.inside("body") {
                    let section = self.current("Abstract");
                    section.label = "abstract".into();
                    Some(section)
                } else if self.top_sec().is_some() {
                    self.sections.last_mut()
                } else if self.inside("body") {
                    Some(self.current(""))
                } else {
                    None
                };
                if let Some(section) = section {
                    if !section.text.is_empty() {
                        section.text.push('\n');
                    }
                    section.text.push_str(&text);
                }
            }
        }
        self.stack.pop();
    }

    fn push_text(&mut self, text: &str) {
        if self.skip.is_none() && (self.paragraph.is_some() || self.title.is_some()) {
            self.text.push_str(text);
        }
    }
}

/// Parse JATS XML into labelled sections.
///
/// # Errors
/// `malformed_response` for XML that is not well formed.
pub fn parse(xml: &str) -> Result<Vec<Section>, AdapterError> {
    let mut reader = Reader::from_str(xml);
    let mut state = State::default();
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Start(start) => {
                let local = start.local_name();
                let name: &str = local.as_ref();
                let name = name.to_string();
                let citation = name == "xref"
                    && start.attributes().flatten().any(|a| {
                        let key: &str = a.key.as_ref();
                        key == "ref-type" && &*a.value == "bibr"
                    });
                state.open(&name, citation);
            }
            Event::End(_) => {
                if state.stack.is_empty() {
                    return Err(malformed("unbalanced end tag"));
                }
                state.close();
            }
            Event::Empty(empty) => {
                // `<break/>` and friends separate words.
                let local = empty.local_name();
                let name: &str = local.as_ref();
                if name != "xref" {
                    state.push_text(" ");
                }
            }
            Event::Text(text) => state.push_text(&text.xml10_content()),
            Event::CData(data) => state.push_text(&data.xml10_content()),
            Event::GeneralRef(reference) => {
                state.push_text(resolve(&reference).encode_utf8(&mut [0; 4]));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !state.stack.is_empty() {
        return Err(malformed("unclosed elements"));
    }
    Ok(state
        .sections
        .into_iter()
        .filter(|s| !s.text.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?>
<!DOCTYPE article [<!ENTITY lol "lol">]>
<article><front><article-meta>
  <abstract><sec><title>Background</title><p>Naps help &amp; heal.</p></sec>
  <p>Second <italic>abstract</italic> paragraph.</p></abstract>
</article-meta></front>
<body>
  <p>Loose body text &lol; here.</p>
  <sec><title>1. Introduction</title><p>Sleep matters <xref ref-type="bibr">[1]</xref>.</p></sec>
  <sec><title>Methods</title>
    <sec><title>Participants</title><p>Fifteen athletes&#x2009;took part.</p></sec>
    <table-wrap><p>Table text</p></table-wrap>
    <p>Power was measured<xref ref-type="fig">Figure 1</xref>.</p>
  </sec>
  <sec><title>Results</title><p>Peak power rose by 157.6 W.</p></sec>
</body>
<back><ref-list><ref><p>A reference.</p></ref></ref-list></back>
</article>"#;

    #[test]
    fn sections_are_labelled_and_noise_is_skipped() {
        let sections = parse(XML).unwrap();
        let summary: Vec<(&str, &str, &str)> = sections
            .iter()
            .map(|s| (s.label.as_str(), s.heading.as_str(), s.text.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "abstract",
                    "Abstract",
                    "Naps help & heal.\nSecond abstract paragraph."
                ),
                ("other", "", "Loose body text here."),
                ("introduction", "1. Introduction", "Sleep matters."),
                (
                    "methods",
                    "Methods",
                    "Fifteen athletes took part.\nPower was measuredFigure 1."
                ),
                ("results", "Results", "Peak power rose by 157.6 W."),
            ]
        );
    }

    #[test]
    fn broken_xml_is_malformed() {
        let error = parse("<article><body><p>open</body>").unwrap_err();
        assert_eq!(error.error_type, "malformed_response");
    }
}
