//! Open-access full text (`fulltext` feature, plan Phase 9).
//!
//! [`FullTextFetcher::fetch`] resolves a record's full text in order:
//! Europe PMC's JATS XML when the record has a PMCID (no PDF parsing
//! needed), then the open-access PDF URL `OpenAlex` reported. Bodies over
//! [`MAX_BYTES`] are refused. The result is a [`Document`]: the paper's
//! text split into sections, each labelled with one of [`LABELS`].
//!
//! Parsed documents are cached on disk, one JSON file per source URL
//! (named by its SHA-256), unless the client records or replays a tape:
//! a recording must hold every response, and a replay must not depend on
//! the cache.

pub mod europepmc_xml;
pub mod pdf;

use std::path::PathBuf;
use std::sync::LazyLock;

use re_core::hashing::sha256_prefixed;
use re_model::records::QualityHints;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::client::{HttpClient, is_public_url};
use crate::error::AdapterError;
use crate::tape::redact_url;

/// The largest body fetched (20 MB).
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

/// Section labels: everything before the first heading is `front`, and a
/// heading that names none of the others is `other`.
pub const LABELS: [&str; 9] = [
    "front",
    "abstract",
    "introduction",
    "methods",
    "results",
    "discussion",
    "limitations",
    "conclusion",
    "other",
];

/// Heading keywords, most specific first ("Results and discussion" is
/// `discussion`; both count as the body of the paper).
const KEYWORDS: [(&str, &str); 16] = [
    ("abstract", "abstract"),
    ("limitation", "limitations"),
    ("conclusion", "conclusion"),
    ("concluding", "conclusion"),
    ("discussion", "discussion"),
    ("result", "results"),
    ("finding", "results"),
    ("experiment", "results"),
    ("evaluation", "results"),
    ("method", "methods"),
    ("materials", "methods"),
    ("study design", "methods"),
    ("approach", "methods"),
    ("introduction", "introduction"),
    ("background", "introduction"),
    ("motivation", "introduction"),
];

static NUMBERING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:(?:\d+(?:\.\d+)*|[IVX]+|[A-Z])[.)]?\s+)?").expect("valid regex")
});

/// The label for a section heading.
#[must_use]
pub fn label_for(heading: &str) -> &'static str {
    let heading = NUMBERING.replace(heading, "").to_lowercase();
    KEYWORDS
        .iter()
        .find(|(keyword, _)| heading.contains(keyword))
        .map_or("other", |(_, label)| label)
}

/// One labelled section of a paper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// One of [`LABELS`].
    pub label: String,
    /// The heading as printed (empty for `front`).
    pub heading: String,
    /// Paragraph text, whitespace collapsed, paragraphs separated by `\n`.
    pub text: String,
}

impl Section {
    /// A section labelled from its heading.
    #[must_use]
    pub fn new(heading: &str, text: String) -> Self {
        Self {
            label: label_for(heading).into(),
            heading: heading.trim().into(),
            text,
        }
    }
}

/// A paper's full text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Where it came from, with secrets removed.
    pub url: String,
    /// `jats` or `pdf`.
    pub format: String,
    /// The sections, in reading order.
    pub sections: Vec<Section>,
}

/// Collapse runs of whitespace to one space.
#[must_use]
pub fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Fetches and parses full text, with an optional on-disk cache.
#[derive(Debug, Clone)]
pub struct FullTextFetcher {
    client: HttpClient,
    cache_dir: Option<PathBuf>,
    europepmc_base: String,
}

/// Europe PMC's REST API.
pub const EUROPEPMC_BASE: &str = "https://www.ebi.ac.uk/europepmc/webservices/rest";

impl FullTextFetcher {
    /// A fetcher over `client` (give it a tape to record or replay).
    #[must_use]
    pub fn new(client: HttpClient) -> Self {
        Self {
            client,
            cache_dir: None,
            europepmc_base: EUROPEPMC_BASE.into(),
        }
    }

    /// Cache parsed documents in `dir` (ignored while taping).
    #[must_use]
    pub fn with_cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    /// Use another Europe PMC base URL (tests).
    #[must_use]
    pub fn with_europepmc_base(mut self, base: impl Into<String>) -> Self {
        self.europepmc_base = base.into();
        self
    }

    /// Where a record's full text can be fetched, best first.
    #[must_use]
    pub fn candidates(&self, hints: &QualityHints) -> Vec<(String, Format)> {
        let mut found = Vec::new();
        if let Some(pmcid) = hints.pmcid.as_deref().map(str::trim)
            && pmcid.len() > 3
            && pmcid[..3].eq_ignore_ascii_case("PMC")
            && pmcid[3..].bytes().all(|b| b.is_ascii_digit())
        {
            found.push((
                format!("{}/PMC{}/fullTextXML", self.europepmc_base, &pmcid[3..]),
                Format::Jats,
            ));
        }
        if let Some(url) = hints.pdf_url.as_deref()
            && reqwest::Url::parse(url).is_ok_and(|parsed| is_public_url(&parsed))
        {
            found.push((url.to_string(), Format::Pdf));
        }
        found
    }

    /// The first full text that can be fetched and parsed, `None` when the
    /// record has no open-access source.
    ///
    /// # Errors
    /// The last candidate's failure when none succeeds.
    pub async fn fetch(&self, hints: &QualityHints) -> Result<Option<Document>, AdapterError> {
        let mut last = None;
        for (url, format) in self.candidates(hints) {
            match self.fetch_one(&url, format).await {
                Ok(document) => return Ok(Some(document)),
                Err(error) => last = Some(error),
            }
        }
        last.map_or(Ok(None), Err)
    }

    fn cache_path(&self, url: &str) -> Option<PathBuf> {
        if self.client.has_tape() {
            return None;
        }
        let key = sha256_prefixed(redact_url(url).as_bytes());
        self.cache_dir
            .as_ref()
            .map(|dir| dir.join(format!("{}.json", key.trim_start_matches("sha256:"))))
    }

    async fn fetch_one(&self, url: &str, format: Format) -> Result<Document, AdapterError> {
        let cache = self.cache_path(url);
        if let Some(document) = cache
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
        {
            return Ok(document);
        }
        let fetched = self
            .client
            .get_document(url, format.accept(), MAX_BYTES)
            .await?;
        let sections = match format {
            Format::Jats => {
                let xml = String::from_utf8(fetched.body)
                    .map_err(|_| malformed("full-text XML is not UTF-8"))?;
                europepmc_xml::parse(&xml)?
            }
            Format::Pdf => tokio::task::spawn_blocking(move || pdf::parse(&fetched.body))
                .await
                .map_err(malformed)??,
        };
        if sections.iter().all(|s| s.text.is_empty()) {
            return Err(AdapterError::new(
                "empty_result",
                "no text in the full-text document",
                false,
            ));
        }
        let document = Document {
            url: redact_url(url),
            format: format.name().into(),
            sections,
        };
        if let Some(path) = cache
            && let Ok(json) = serde_json::to_string(&document)
        {
            // A cache that cannot be written only costs a refetch.
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path))
                .and_then(|()| std::fs::write(&path, json));
        }
        Ok(document)
    }
}

/// A full-text format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Europe PMC JATS XML.
    Jats,
    /// An open-access PDF.
    Pdf,
}

impl Format {
    /// `jats` or `pdf`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Jats => "jats",
            Self::Pdf => "pdf",
        }
    }

    fn accept(self) -> &'static str {
        match self {
            Self::Jats => "application/xml",
            Self::Pdf => "application/pdf",
        }
    }
}

pub(crate) fn malformed(detail: impl std::fmt::Display) -> AdapterError {
    AdapterError::new(
        "malformed_response",
        format!("unreadable full text: {detail}"),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_map_to_labels() {
        for (heading, label) in [
            ("Abstract", "abstract"),
            ("1. Introduction", "introduction"),
            ("2 Related Work", "other"),
            ("Materials and Methods", "methods"),
            ("III. Experiments", "results"),
            ("4.2 Results", "results"),
            ("Results and Discussion", "discussion"),
            ("Limitations", "limitations"),
            ("6 Conclusion and Future Work", "conclusion"),
            ("Background", "introduction"),
            ("Acknowledgments", "other"),
        ] {
            assert_eq!(label_for(heading), label, "{heading}");
        }
    }

    #[test]
    fn candidates_prefer_europepmc_xml_and_reject_junk() {
        let fetcher = FullTextFetcher::new(HttpClient::new().unwrap());
        let hints = QualityHints {
            pmcid: Some("PMC10710987".into()),
            pdf_url: Some("https://arxiv.org/pdf/2504.00406".into()),
            ..QualityHints::default()
        };
        let found = fetcher.candidates(&hints);
        assert_eq!(
            found,
            [
                (
                    format!("{EUROPEPMC_BASE}/PMC10710987/fullTextXML"),
                    Format::Jats
                ),
                ("https://arxiv.org/pdf/2504.00406".into(), Format::Pdf),
            ]
        );
        let junk = QualityHints {
            pmcid: Some("PMC12/../x".into()),
            pdf_url: Some("http://169.254.169.254/latest/meta-data/".into()),
            ..QualityHints::default()
        };
        assert_eq!(fetcher.candidates(&junk), []);
    }
}
