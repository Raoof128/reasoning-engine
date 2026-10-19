//! Identifier formats used by the Python implementation.

use uuid::Uuid;

/// `str(uuid.uuid4())`: hyphenated, lowercase.
#[must_use]
pub fn new_uuid() -> String {
    Uuid::new_v4().hyphenated().to_string()
}

/// `uuid.uuid4().hex[:n]`, with `n` capped at 32.
#[must_use]
pub fn short_hex(n: usize) -> String {
    let mut hex = Uuid::new_v4().simple().to_string();
    hex.truncate(n.min(32));
    hex
}

/// `run_` plus 12 hex characters, as `ResearchRun.create` produces.
#[must_use]
pub fn new_run_id() -> String {
    format!("run_{}", short_hex(12))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_match_python() {
        let uuid = new_uuid();
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.matches('-').count(), 4);
        let run = new_run_id();
        assert!(run.starts_with("run_") && run.len() == 16);
        assert!(
            run[4..]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_ne!(new_run_id(), new_run_id());
    }
}
