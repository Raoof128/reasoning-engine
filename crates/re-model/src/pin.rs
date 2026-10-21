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
//! `replay`) set it, never a long-running server. Pins never overlap: a
//! second [`pin`] waits until the first guard drops, so two recorded runs
//! in one process (parallel tests, an embedding library) each get their own
//! seed instead of resetting and then unpinning each other mid-run. Nesting
//! a pin on the thread that holds one therefore deadlocks; do not.

use std::sync::{Condvar, Mutex, PoisonError};

struct State {
    clock: String,
    stream: u64,
}

static PIN: Mutex<Option<State>> = Mutex::new(None);
/// Signalled when a pin is released, waking the next [`pin`] in line.
static RELEASED: Condvar = Condvar::new();

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
        RELEASED.notify_all();
    }
}

/// Pin the clock to `clock` (`YYYY-MM-DDTHH:MM:SSZ`) and seed identifiers
/// with `seed` until the returned guard drops. Blocks while another pin is
/// held.
pub fn pin(clock: &str, seed: u64) -> Pinned {
    let mut state = PIN.lock().unwrap_or_else(PoisonError::into_inner);
    while state.is_some() {
        state = RELEASED.wait(state).unwrap_or_else(PoisonError::into_inner);
    }
    *state = Some(State {
        clock: clock.to_string(),
        stream: seed,
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

/// Serialises this crate's unit tests that read the clock or draw IDs.
/// They cannot tell a pin held by a parallel test from no pin at all, so
/// any such test running beside a pinned one would consume its stream.
#[cfg(test)]
pub(crate) fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
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

    const CLOCK: &str = "2026-10-19T00:00:00Z";

    #[test]
    fn concurrent_pins_take_turns_instead_of_clobbering_each_other() {
        let _serial = serial();
        let draws = || {
            (0..50)
                .map(|_| {
                    std::thread::yield_now();
                    new_uuid()
                })
                .collect::<Vec<_>>()
        };
        let expected = {
            let _pin = pin(CLOCK, 7);
            draws()
        };
        let threads: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(move || {
                    let _pin = pin(CLOCK, 7);
                    draws()
                })
            })
            .collect();
        for thread in threads {
            assert_eq!(thread.join().expect("no panic"), expected);
        }
    }

    #[test]
    fn pinning_makes_time_and_ids_repeatable_until_dropped() {
        let _serial = serial();
        let draw = || (new_run_id(), short_hex(8), new_uuid(), utc_now());
        let first = {
            let _pin = pin(CLOCK, 42);
            draw()
        };
        let second = {
            let _pin = pin(CLOCK, 42);
            draw()
        };
        assert_eq!(first, second);
        assert_eq!(first.3, CLOCK);
        assert!(first.0.starts_with("run_") && first.0.len() == 16);
        assert_eq!(first.1.len(), 8);
        let uuid = uuid::Uuid::parse_str(&first.2).expect("a valid uuid");
        assert_eq!(uuid.get_version_num(), 4);

        let other = {
            let _pin = pin(CLOCK, 43);
            draw()
        };
        assert_ne!(first.0, other.0, "a different seed gives different ids");
        assert_ne!(utc_now(), CLOCK, "unpinned after drop");
        assert_ne!(new_run_id(), first.0);
    }
}
