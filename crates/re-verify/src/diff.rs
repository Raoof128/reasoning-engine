//! Drift between two run packs of the same research (`learn` feature, plan
//! Phase 11): which claims changed verdict, which sources gained or lost a
//! risk flag (a retraction, say), which appeared or disappeared, and what
//! the retraction cascade added.
//!
//! Claims are matched by text and records by DOI, URL or title, so the
//! comparison survives the new run's fresh identifiers.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// One claim whose verdict moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaimChange {
    /// The claim.
    pub claim: String,
    /// Verdict before.
    pub before: String,
    /// Verdict now.
    pub after: String,
}

/// One source whose risk flags moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlagChange {
    /// Its title.
    pub title: String,
    /// Flags it gained.
    pub added: Vec<String>,
    /// Flags it lost.
    pub removed: Vec<String>,
}

/// Everything that changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Drift {
    /// Claims whose verdict changed.
    pub claims: Vec<ClaimChange>,
    /// Sources whose risk flags changed.
    pub flags: Vec<FlagChange>,
    /// Titles of sources found now but not before.
    pub new_sources: Vec<String>,
    /// Titles of sources found before but not now.
    pub lost_sources: Vec<String>,
    /// Claims newly in the retraction cascade (`graph` builds).
    pub cascade: Vec<String>,
}

impl Drift {
    /// Whether anything changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
            && self.flags.is_empty()
            && self.new_sources.is_empty()
            && self.lost_sources.is_empty()
            && self.cascade.is_empty()
    }

    /// The Markdown report.
    #[must_use]
    pub fn markdown(&self, run_id: &str, new_run_id: &str, date: &str) -> String {
        let mut out = format!("# Drift for {run_id}\n\nRe-verified on {date} as {new_run_id}.\n");
        if self.is_empty() {
            out.push_str("\nNothing changed.\n");
            return out;
        }
        if !self.claims.is_empty() {
            out.push_str("\n## Verdicts\n\n");
            for c in &self.claims {
                let _ = writeln!(out, "- {}: {} -> {}", c.claim, c.before, c.after);
            }
        }
        if !self.flags.is_empty() {
            out.push_str("\n## Source flags\n\n");
            for f in &self.flags {
                let mut parts = Vec::new();
                if !f.added.is_empty() {
                    parts.push(format!("now {}", f.added.join(", ")));
                }
                if !f.removed.is_empty() {
                    parts.push(format!("no longer {}", f.removed.join(", ")));
                }
                let _ = writeln!(out, "- {}: {}", f.title, parts.join("; "));
            }
        }
        if !self.cascade.is_empty() {
            out.push_str("\n## Retraction cascade\n\n");
            for claim in &self.cascade {
                let _ = writeln!(out, "- {claim} now rests on retracted work");
            }
        }
        for (heading, list) in [
            ("New sources", &self.new_sources),
            ("Sources no longer found", &self.lost_sources),
        ] {
            if !list.is_empty() {
                let _ = writeln!(out, "\n## {heading}\n");
                for title in list {
                    let _ = writeln!(out, "- {title}");
                }
            }
        }
        out
    }
}

fn read(dir: &Path, name: &str) -> Value {
    std::fs::read_to_string(dir.join(name))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Array(Vec::new()))
}

fn items(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Claim text to verdict.
fn verdicts(dir: &Path) -> BTreeMap<String, String> {
    items(&read(dir, "claims.json"))
        .iter()
        .map(|c| (text(c, "text").to_string(), text(c, "status").to_string()))
        .collect()
}

fn source_key(record: &Value) -> String {
    let doi = text(record, "doi");
    if !doi.is_empty() {
        return format!("doi:{}", doi.to_lowercase());
    }
    let url = text(record, "url");
    if !url.is_empty() {
        return format!("url:{url}");
    }
    format!("title:{}", text(record, "title").to_lowercase())
}

/// Source key to (title, flags).
fn sources(dir: &Path) -> BTreeMap<String, (String, Vec<String>)> {
    items(&read(dir, "evidence_ledger.json"))
        .iter()
        .map(|e| {
            let flags = e
                .get("risk_flags")
                .and_then(Value::as_array)
                .map(|f| {
                    f.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            (source_key(e), (text(e, "title").to_string(), flags))
        })
        .collect()
}

fn cascade(dir: &Path) -> Vec<String> {
    items(&read(dir, "cascade.json"))
        .iter()
        .map(|entry| text(entry, "claim").to_string())
        .collect()
}

/// What changed from the pack at `before` to the pack at `after`.
#[must_use]
pub fn diff(before: &Path, after: &Path) -> Drift {
    let (old, new) = (verdicts(before), verdicts(after));
    let claims = old
        .iter()
        .filter_map(|(claim, was)| {
            let now = new.get(claim)?;
            (now != was).then(|| ClaimChange {
                claim: claim.clone(),
                before: was.clone(),
                after: now.clone(),
            })
        })
        .collect();
    let (old, new) = (sources(before), sources(after));
    let mut flags = Vec::new();
    for (key, (title, was)) in &old {
        if let Some((_, now)) = new.get(key) {
            let added: Vec<String> = now.iter().filter(|f| !was.contains(f)).cloned().collect();
            let removed: Vec<String> = was.iter().filter(|f| !now.contains(f)).cloned().collect();
            if !added.is_empty() || !removed.is_empty() {
                flags.push(FlagChange {
                    title: title.clone(),
                    added,
                    removed,
                });
            }
        }
    }
    let titles = |from: &BTreeMap<String, (String, Vec<String>)>,
                  missing_in: &BTreeMap<String, (String, Vec<String>)>| {
        from.iter()
            .filter(|(key, _)| !missing_in.contains_key(*key))
            .map(|(_, (title, _))| title.clone())
            .collect()
    };
    let was_cascading = cascade(before);
    Drift {
        claims,
        flags,
        new_sources: titles(&new, &old),
        lost_sources: titles(&old, &new),
        cascade: cascade(after)
            .into_iter()
            .filter(|claim| !was_cascading.contains(claim))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pack(dir: &Path, claims: &Value, evidence: &Value, cascade: Option<&Value>) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("claims.json"), claims.to_string()).unwrap();
        std::fs::write(dir.join("evidence_ledger.json"), evidence.to_string()).unwrap();
        if let Some(cascade) = cascade {
            std::fs::write(dir.join("cascade.json"), cascade.to_string()).unwrap();
        }
    }

    #[test]
    fn verdicts_flags_sources_and_cascades_are_compared() {
        let root = tempfile::tempdir().unwrap();
        let (before, after) = (root.path().join("a"), root.path().join("b"));
        pack(
            &before,
            &json!([{"text": "Naps help", "status": "supported"}, {"text": "Sleep matters", "status": "supported"}]),
            &json!([
                {"title": "Nap trial", "doi": "10.1/X", "risk_flags": []},
                {"title": "Old review", "url": "https://e.org/r", "risk_flags": ["preprint"]}
            ]),
            None,
        );
        pack(
            &after,
            &json!([{"text": "Naps help", "status": "partially_supported"}, {"text": "Sleep matters", "status": "supported"}]),
            &json!([
                {"title": "Nap trial", "doi": "10.1/x", "risk_flags": ["retracted"]},
                {"title": "New study", "doi": "10.2/y", "risk_flags": []}
            ]),
            Some(&json!([{"claim": "Naps help"}])),
        );
        let drift = diff(&before, &after);
        assert_eq!(
            drift.claims,
            [ClaimChange {
                claim: "Naps help".into(),
                before: "supported".into(),
                after: "partially_supported".into()
            }]
        );
        assert_eq!(
            drift.flags,
            [FlagChange {
                title: "Nap trial".into(),
                added: vec!["retracted".into()],
                removed: Vec::new()
            }]
        );
        assert_eq!(drift.new_sources, ["New study"]);
        assert_eq!(drift.lost_sources, ["Old review"]);
        assert_eq!(drift.cascade, ["Naps help"]);
        let report = drift.markdown("run_a", "run_b", "2026-10-19");
        assert!(report.contains("- Naps help: supported -> partially_supported"));
        assert!(report.contains("- Nap trial: now retracted"));
        assert!(report.contains("- Naps help now rests on retracted work"));

        assert!(diff(&before, &before).is_empty());
        assert!(
            diff(&before, &before)
                .markdown("run_a", "run_c", "d")
                .ends_with("Nothing changed.\n")
        );
    }
}
