//! A pinned clock and seeded identifiers, for recorded and replayed runs
//! (plan Phase 7).
//!
//! While a [`Pinned`] guard is alive, [`crate::clock::utc_now`] returns one
//! fixed timestamp and [`crate::ids`] draws from a seeded `SplitMix64`
//! stream instead of the operating system. A recorded run and its replay pin
//! the same clock and seed, so every timestamp and ID in their artifacts
//! matches. Evidence timestamps are read inside concurrent source tasks, so
//! a single fixed time (rather than a recorded sequence) is what makes them
//! order-independent.
//!
//! The pin is process-wide: only one-shot commands (`research --record`,
//! `replay`) set it, never a long-running server.

use std::sync::{Mutex, PoisonError};

struct State {
    clock: String,
    stream: u64,
}

static PIN: Mutex<Option<State>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut Option<State>) -> T) -> T {
    f(&mut PIN.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Unpins the clock and identifiers when dropped.
#[derive(Debug)]
#[must_use = "the pin is released when this guard drops"]
pub struct Pinned(());

impl Drop for Pinned {
    fn drop(&mut self) {
        with_state(|state| *state = None);
    }
}

/// Pin the clock to `clock` (`YYYY-MM-DDTHH:MM:SSZ`) and seed identifiers
/// with `seed` until the returned guard drops.
pub fn pin(clock: &str, seed: u64) -> Pinned {
    with_state(|state| {
        *state = Some(State {
            clock: clock.to_string(),
            stream: seed,
        });
    });
    Pinned(())
}

/// The pinned timestamp, if any.
pub(crate) fn clock() -> Option<String> {
    with_state(|state| state.as_ref().map(|s| s.clock.clone()))
}

/// The next value of the seeded stream, if pinned (`SplitMix64`).
pub(crate) fn next_u64() -> Option<u64> {
    with_state(|state| {
        let s = state.as_mut()?;
        s.stream = s.stream.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s.stream;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Some(z ^ (z >> 31))
    })
}

/// 16 bytes from the seeded stream, if pinned.
pub(crate) fn next_bytes() -> Option<[u8; 16]> {
    let (a, b) = (next_u64()?, next_u64()?);
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&a.to_be_bytes());
    bytes[8..].copy_from_slice(&b.to_be_bytes());
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::utc_now;
    use crate::ids::{new_run_id, new_uuid, short_hex};

    // One test, because the pin is process-wide and tests run in parallel.
    #[test]
    fn pinning_makes_time_and_ids_repeatable_until_dropped() {
        let draw = || (new_run_id(), short_hex(8), new_uuid(), utc_now());
        let first = {
            let _pin = pin("2026-10-19T00:00:00Z", 42);
            draw()
        };
        let second = {
            let _pin = pin("2026-10-19T00:00:00Z", 42);
            draw()
        };
        assert_eq!(first, second);
        assert_eq!(first.3, "2026-10-19T00:00:00Z");
        assert!(first.0.starts_with("run_") && first.0.len() == 16);
        assert_eq!(first.1.len(), 8);
        let uuid = uuid::Uuid::parse_str(&first.2).expect("a valid uuid");
        assert_eq!(uuid.get_version_num(), 4);

        let other = {
            let _pin = pin("2026-10-19T00:00:00Z", 43);
            draw()
        };
        assert_ne!(first.0, other.0, "a different seed gives different ids");
        assert_ne!(utc_now(), "2026-10-19T00:00:00Z", "unpinned after drop");
        assert_ne!(new_run_id(), first.0);
    }
}
