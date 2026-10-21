//! Identifier formats used by the Python implementation.

use uuid::Uuid;

/// `str(uuid.uuid4())`: hyphenated, lowercase.
#[must_use]
pub fn new_uuid() -> String {
    random_uuid().hyphenated().to_string()
}

/// A version-4 UUID from the OS, or from the seeded stream while pinned.
fn random_uuid() -> Uuid {
    crate::pin::next_bytes().map_or_else(Uuid::new_v4, |bytes| {
        uuid::Builder::from_random_bytes(bytes).into_uuid()
    })
}

/// 64 bits from the operating system, ignoring any pin: for seeds and other
/// values that must stay random while a recorded run is pinned.
#[must_use]
pub fn os_random_u64() -> u64 {
    let (high, _) = Uuid::new_v4().as_u64_pair();
    high
}

/// `uuid.uuid4().hex[:n]`, with `n` capped at 32.
#[must_use]
pub fn short_hex(n: usize) -> String {
    let mut hex = random_uuid().simple().to_string();
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
        let _serial = crate::pin::serial();
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
