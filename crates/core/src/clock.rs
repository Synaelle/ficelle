// SPDX-License-Identifier: AGPL-3.0-or-later

//! Time source. Timestamps are milliseconds since the Unix epoch, UTC.
//!
//! The store reads time through [`Clock`] so tests can move time by hand,
//! for example to check that versions are grouped or that old trash is purged.

use std::sync::atomic::{AtomicI64, Ordering};

/// Milliseconds since 1970-01-01 UTC.
pub type Timestamp = i64;

/// One minute in [`Timestamp`] units.
pub const MINUTE: Timestamp = 60_000;
/// One day in [`Timestamp`] units.
pub const DAY: Timestamp = 24 * 60 * MINUTE;

/// Gives the current time.
pub trait Clock: Send + Sync {
    /// Current time.
    fn now(&self) -> Timestamp;
}

/// The real wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

/// Added to the wall clock, see [`set_skew`].
static SKEW: AtomicI64 = AtomicI64::new(0);

/// Makes this process's wall clock run ahead (or behind) by `ms`, as a
/// device with a wrong clock would. For tests of the real apps only.
pub fn set_skew(ms: Timestamp) {
    SKEW.store(ms, Ordering::Relaxed);
}

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        jiff::Timestamp::now().as_millisecond() + SKEW.load(Ordering::Relaxed)
    }
}

/// A clock that only moves when told to, for tests and seed data.
#[derive(Debug)]
pub struct ManualClock(AtomicI64);

impl ManualClock {
    /// Starts at `start`.
    pub fn new(start: Timestamp) -> Self {
        Self(AtomicI64::new(start))
    }

    /// Moves the clock forward.
    pub fn advance(&self, by: Timestamp) {
        self.0.fetch_add(by, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        self.0.load(Ordering::SeqCst)
    }
}

/// Formats a timestamp as RFC 3339 in UTC, for example `2026-10-23T08:00:00Z`.
pub fn to_rfc3339(at: Timestamp) -> String {
    // whole seconds are enough for people and other apps reading the export
    jiff::Timestamp::from_second(at.div_euclid(1000))
        .map_or_else(|_| String::from("1970-01-01T00:00:00Z"), |t| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc_to_the_second() {
        assert_eq!(to_rfc3339(1_792_742_400_123), "2026-10-23T08:00:00Z");
    }

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(10);
        assert_eq!(clock.now(), 10);
        clock.advance(5);
        assert_eq!(clock.now(), 15);
    }
}
