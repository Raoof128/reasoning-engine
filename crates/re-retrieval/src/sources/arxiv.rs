//! arXiv: preprints across physics, CS, maths and more (port of `arxiv.py`).
//!
//! The Atom feed is untrusted network input. Python parses it with
//! `defusedxml`, which forbids entity declarations; this port goes further
//! and expands nothing but the five predefined entities and character
//! references, so XXE and billion-laughs payloads have nothing to expand.

use quick_xml::NsReader;
use quick_xml::events::{BytesRef, Event};
use quick_xml::name::{Namespace, ResolveResult};
use serde_json::{Map, Value};

use super::HttpSource;
use crate::base::{RecordInput, build_record, coerce_year, py_strip, relevance_score, urlencode};
use crate::client::decode_text;
use crate::error::AdapterError;
use crate::result::RetrievalResult;

/// Production endpoint.
pub const ENDPOINT: &str = "https://export.arxiv.org/api/query";
/// Largest feed accepted; a page of 100 entries is well under 1 MiB.
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const NAME: &str = "arxiv";
const ATOM: &str = "http://www.w3.org/2005/Atom";
const ARXIV: &str = "http://arxiv.org/schemas/atom";

/// Function words that occur in nearly every paper (Python's `_STOPWORDS`).
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "can", "could", "did", "do",
    "does", "for", "from", "has", "have", "how", "if", "in", "into", "is", "it", "its", "no",
    "not", "of", "on", "or", "than", "that", "the", "their", "there", "these", "this", "those",
    "to", "versus", "vs", "was", "were", "what", "when", "where", "which", "who", "whom", "why",
    "will", "with", "would",
];

/// The arXiv `search_query` for a free-text query (Python's `search_query`).
///
/// arXiv binds a field prefix such as `all:` to the first word only and ORs
/// the rest across every field, so `all:{query}` matched "Does", "in" or
/// "2" anywhere in the archive. Each content word gets its own `all:`
/// instead; arXiv ranks papers that match more of them first. Splitting,
/// trimming and the digit test are ASCII-only, as in Python, so both split
/// every query the same way.
#[must_use]
pub fn search_query(query: &str) -> String {
    let terms: Vec<String> = query
        .split([' ', '\t', '\n', '\r', '\x0b', '\x0c'])
        .map(|token| token.trim_matches(|c: char| c.is_ascii_punctuation()))
        .filter(|term| term.chars().count() >= 2 && !term.bytes().all(|b| b.is_ascii_digit()))
        .filter(|term| !STOPWORDS.contains(&term.to_lowercase().as_str()))
        .map(|term| format!("all:{term}"))
        .collect();
    if terms.is_empty() {
        format!("all:{query}")
    } else {
        terms.join(" OR ")
    }
}

/// The arXiv export API search.
#[derive(Debug, Clone)]
pub struct Arxiv {
    endpoint: String,
}

impl Default for Arxiv {
    fn default() -> Self {
        Self::new()
    }
}

impl Arxiv {
    /// The production source.
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoint: ENDPOINT.into(),
        }
    }

    /// Point at a different endpoint (tests).
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

/// The fields Python reads from one `<entry>`, each the `.text` of the first
/// matching direct child (`None` when there is no such child).
#[derive(Debug, Default, PartialEq)]
struct Entry {
    title: Option<String>,
    summary: Option<String>,
    published: Option<String>,
    id: Option<String>,
    doi: Option<String>,
    /// One per `<author>`, the text of its first `<name>` child.
    authors: Vec<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Field {
    Title,
    Summary,
    Published,
    Id,
    Doi,
    AuthorName,
}

/// Text being collected for `field`. `ElementTree`'s `.text` stops at the
/// first child element, so collection ends there too.
struct Capture {
    field: Field,
    depth: usize,
    text: String,
    open: bool,
}

fn malformed(detail: impl std::fmt::Display) -> AdapterError {
    AdapterError::new(
        "malformed_response",
        format!("invalid Atom feed: {detail}"),
        false,
    )
}

/// XML 1.0 `Char`: what expat accepts in content and character references.
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

fn resolve_ref(reference: &BytesRef<'_>) -> Result<char, AdapterError> {
    if let Some(c) = reference.resolve_char_ref().map_err(malformed)? {
        return if is_xml_char(c) {
            Ok(c)
        } else {
            Err(malformed("reference to invalid character"))
        };
    }
    match &**reference {
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "amp" => Ok('&'),
        "apos" => Ok('\''),
        "quot" => Ok('"'),
        name => Err(malformed(format_args!("undefined entity &{name};"))),
    }
}

fn atom_field(ns: &ResolveResult<'_>, local: &str, in_author: bool) -> Option<Field> {
    let is = |want: &str| matches!(ns, ResolveResult::Bound(Namespace(n)) if *n == want);
    match (is(ATOM), is(ARXIV), local, in_author) {
        (true, _, "name", true) => Some(Field::AuthorName),
        (true, _, "title", false) => Some(Field::Title),
        (true, _, "summary", false) => Some(Field::Summary),
        (true, _, "published", false) => Some(Field::Published),
        (true, _, "id", false) => Some(Field::Id),
        (_, true, "doi", false) => Some(Field::Doi),
        _ => None,
    }
}

impl Entry {
    fn slot(&mut self, field: Field) -> &mut Option<String> {
        match field {
            Field::Title => &mut self.title,
            Field::Summary => &mut self.summary,
            Field::Published => &mut self.published,
            Field::Id => &mut self.id,
            Field::Doi => &mut self.doi,
            // An author slot exists from its start tag on.
            Field::AuthorName => self
                .authors
                .last_mut()
                .expect("author opened before its name"),
        }
    }
}

/// Parse the feed into the `<entry>` children of the root, rejecting
/// anything expat would reject plus every entity declaration.
#[allow(clippy::too_many_lines)]
fn parse_feed(xml: &str) -> Result<Vec<Entry>, AdapterError> {
    if let Some(c) = xml.chars().find(|c| !is_xml_char(*c)) {
        return Err(malformed(format_args!(
            "not well-formed (invalid token U+{:04X})",
            c as u32
        )));
    }
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().check_comments = true;

    let mut entries: Vec<Entry> = Vec::new();
    let mut depth = 0usize;
    let mut seen_root = false;
    let mut seen_event = false;
    // Depth of the open `<entry>` and `<author>`, when inside one.
    let mut entry_depth: Option<usize> = None;
    let mut author_depth: Option<usize> = None;
    let mut capture: Option<Capture> = None;

    loop {
        let (ns, event) = reader.read_resolved_event().map_err(malformed)?;
        let first_event = !seen_event;
        seen_event = true;
        match event {
            Event::Start(_) | Event::Empty(_) if depth == 0 && seen_root => {
                return Err(malformed("junk after document element"));
            }
            Event::Start(ref start) | Event::Empty(ref start) => {
                if let ResolveResult::Unknown(prefix) = &ns {
                    return Err(malformed(format_args!("unbound prefix {prefix}")));
                }
                for attribute in start.attributes() {
                    attribute.map_err(malformed)?;
                }
                let empty = matches!(event, Event::Empty(_));
                let name = start.local_name();
                let local = name.as_ref();
                // A child element ends `.text` collection for its parent.
                if let Some(open) = capture.as_mut() {
                    open.open = false;
                }
                let child = depth + 1;
                seen_root = true;
                if let Some(entry) = entry_depth {
                    let parent_is_entry = child == entry + 1;
                    let parent_is_author = author_depth.is_some_and(|a| child == a + 1);
                    let atom = matches!(&ns, ResolveResult::Bound(Namespace(n)) if *n == ATOM);
                    let current = entries.last_mut().expect("entry opened");
                    if parent_is_entry && atom && local == "author" {
                        current.authors.push(None);
                        if !empty {
                            author_depth = Some(child);
                        }
                    } else if (parent_is_entry || parent_is_author)
                        && let Some(field) = atom_field(&ns, local, parent_is_author)
                    {
                        let slot = current.slot(field);
                        if slot.is_none() {
                            *slot = Some(String::new());
                            if !empty {
                                capture = Some(Capture {
                                    field,
                                    depth: child,
                                    text: String::new(),
                                    open: true,
                                });
                            }
                        }
                    }
                } else if child == 2
                    && local == "entry"
                    && matches!(&ns, ResolveResult::Bound(Namespace(n)) if *n == ATOM)
                {
                    entries.push(Entry::default());
                    if !empty {
                        entry_depth = Some(child);
                    }
                }
                if !empty {
                    depth = child;
                }
            }
            Event::End(_) => {
                if capture.as_ref().is_some_and(|c| c.depth == depth) {
                    let done = capture.take().expect("checked above");
                    let current = entries.last_mut().expect("entry opened");
                    *current.slot(done.field) = Some(done.text);
                }
                if author_depth == Some(depth) {
                    author_depth = None;
                }
                if entry_depth == Some(depth) {
                    entry_depth = None;
                }
                depth -= 1;
            }
            Event::Text(ref text) => {
                if depth == 0 {
                    if !text
                        .xml10_content()
                        .chars()
                        .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
                    {
                        return Err(malformed("syntax error: text outside the document element"));
                    }
                } else if let Some(open) = capture.as_mut().filter(|c| c.open) {
                    open.text.push_str(&text.xml10_content());
                }
            }
            Event::CData(ref data) => {
                if depth == 0 {
                    return Err(malformed(
                        "syntax error: CDATA outside the document element",
                    ));
                }
                if let Some(open) = capture.as_mut().filter(|c| c.open) {
                    open.text.push_str(&data.xml10_content());
                }
            }
            Event::GeneralRef(ref reference) => {
                let c = resolve_ref(reference)?;
                if depth == 0 {
                    return Err(malformed(
                        "syntax error: reference outside the document element",
                    ));
                }
                if let Some(open) = capture.as_mut().filter(|c| c.open) {
                    open.text.push(c);
                }
            }
            Event::DocType(ref doctype) => {
                if seen_root || depth > 0 {
                    return Err(malformed("syntax error: misplaced DOCTYPE"));
                }
                if doctype.xml10_content().contains("<!ENTITY") {
                    return Err(malformed(
                        "EntitiesForbidden: entity declarations are not allowed",
                    ));
                }
            }
            Event::Decl(_) if !first_event => {
                return Err(malformed("XML or text declaration not at start of entity"));
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
            Event::Eof => {
                if depth > 0 || !seen_root {
                    return Err(malformed("no element found"));
                }
                return Ok(entries);
            }
        }
    }
}

fn parse(
    run_id: &str,
    query: &str,
    limit: usize,
    entries: Vec<Entry>,
) -> Vec<re_model::records::EvidenceRecord> {
    let field = |value: Option<String>| value.unwrap_or_default();
    entries
        .into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let position = i + 1;
            let landing = field(entry.id);
            let mut metadata = Map::new();
            metadata.insert("arxiv_id".into(), Value::String(landing.clone()));
            build_record(RecordInput {
                source_type: "preprint",
                title: py_strip(&field(entry.title)).to_string(),
                authors: entry
                    .authors
                    .into_iter()
                    .map(|a| py_strip(&field(a)).to_string())
                    .filter(|a| !a.is_empty())
                    .collect(),
                year: coerce_year(&Value::String(field(entry.published))),
                publisher: "arXiv".into(),
                venue: "arXiv".into(),
                doi: entry.doi.map_or(Value::Null, Value::String),
                url: (!landing.is_empty()).then_some(landing),
                score: relevance_score(position, limit, None),
                snippet: py_strip(&field(entry.summary)).to_string(),
                licence_notes: Some("arXiv preprint (not peer-reviewed).".into()),
                metadata,
                ..RecordInput::new(run_id, NAME, query, position)
            })
        })
        .collect()
}

impl HttpSource for Arxiv {
    fn name(&self) -> &'static str {
        NAME
    }

    fn build_url(&self, query: &str, limit: usize) -> String {
        let params = [
            ("search_query", search_query(query)),
            ("start", "0".to_string()),
            ("max_results", limit.to_string()),
            ("sortBy", "relevance".to_string()),
        ];
        format!("{}?{}", self.endpoint, urlencode(&params))
    }

    fn parse_body(&self, run_id: &str, query: &str, limit: usize, body: &[u8]) -> RetrievalResult {
        let parsed = if body.len() > MAX_BODY_BYTES {
            Err(malformed(format_args!(
                "body exceeds {MAX_BODY_BYTES} bytes"
            )))
        } else {
            decode_text(body).and_then(parse_feed)
        };
        match parsed {
            Ok(entries) => RetrievalResult::ok(parse(run_id, query, limit, entries)),
            Err(error) => RetrievalResult::failed(error.to_retrieval_error(NAME)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(entries: &str) -> String {
        format!(
            r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">{entries}</feed>"#
        )
    }

    #[test]
    fn text_stops_at_the_first_child_like_elementtree() {
        let xml = feed("<entry><title>Head <b>bold</b> tail</title></entry>");
        let entries = parse_feed(&xml).expect("valid feed");
        assert_eq!(entries[0].title.as_deref(), Some("Head "));
    }

    #[test]
    fn first_matching_child_wins_and_nested_matches_are_ignored() {
        let xml = feed(
            "<entry><x><title>nested</title></x><title>one</title><title>two</title>\
             <author><name>A</name><name>B</name></author><author/></entry>",
        );
        let entries = parse_feed(&xml).expect("valid feed");
        assert_eq!(entries[0].title.as_deref(), Some("one"));
        assert_eq!(entries[0].authors, vec![Some("A".into()), None]);
    }

    #[test]
    fn foreign_namespaces_do_not_match() {
        let xml = r#"<feed xmlns="urn:other"><entry><title>x</title></entry></feed>"#;
        assert_eq!(parse_feed(xml).expect("valid feed").len(), 0);
    }

    #[test]
    fn hostile_and_broken_documents_are_rejected() {
        let cases = [
            r#"<!DOCTYPE x [<!ENTITY e SYSTEM "file:///etc/passwd">]><feed>&e;</feed>"#,
            "<feed>&nbsp;</feed>",
            "<feed>&#0;</feed>",
            "<feed><a></b></feed>",
            "<feed>",
            "<feed/><feed/>",
            "text<feed/>",
            "<p:feed/>",
            "",
            "<feed>\u{1}</feed>",
        ];
        for xml in cases {
            assert!(parse_feed(xml).is_err(), "accepted {xml:?}");
        }
    }

    #[test]
    fn doctype_without_entities_cdata_and_references_are_fine() {
        let xml = format!(
            "<?xml version=\"1.0\"?><!DOCTYPE feed><!-- c -->{}",
            feed("<entry><title>a &amp; <![CDATA[<b>]]>&#x41;</title></entry>")
        );
        let entries = parse_feed(&xml).expect("valid feed");
        assert_eq!(entries[0].title.as_deref(), Some("a & <b>A"));
    }

    #[test]
    fn search_query_ors_content_words_like_python() {
        let cases = [
            (
                "Does intermittent fasting improve insulin sensitivity in type 2 diabetes?",
                "all:intermittent OR all:fasting OR all:improve OR all:insulin \
                 OR all:sensitivity OR all:type OR all:diabetes",
            ),
            (
                "MCP prompt-injection: a/b & c?",
                "all:MCP OR all:prompt-injection OR all:a/b",
            ),
            ("café نتیجه", "all:café OR all:نتیجه"),
            ("What is it?", "all:What is it?"),
            ("", "all:"),
        ];
        for (query, expected) in cases {
            assert_eq!(search_query(query), expected, "{query:?}");
        }
    }

    #[test]
    fn oversized_bodies_are_rejected() {
        let body = vec![b' '; MAX_BODY_BYTES + 1];
        let result = Arxiv::new().parse_body("run", "q", 5, &body);
        assert_eq!(
            result.error.map(|e| e.error_type),
            Some("malformed_response".into())
        );
    }
}
