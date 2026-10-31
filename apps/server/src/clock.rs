// SPDX-License-Identifier: AGPL-3.0-or-later

//! Wall clock in unix milliseconds. Tests move it forward to reach token
//! expiry and lock ends without waiting.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Shared clock.
#[derive(Clone, Default)]
pub struct Clock {
    skew: Arc<AtomicI64>,
}

/// One minute in milliseconds.
pub const MINUTE: i64 = 60_000;
/// One hour in milliseconds.
pub const HOUR: i64 = 60 * MINUTE;
/// One day in milliseconds.
pub const DAY: i64 = 24 * 60 * MINUTE;

impl Clock {
    /// Now, in unix milliseconds.
    pub fn now(&self) -> i64 {
        let real = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        real + self.skew.load(Ordering::Relaxed)
    }

    /// Moves time forward, for tests.
    #[cfg(test)]
    pub fn advance(&self, ms: i64) {
        self.skew.fetch_add(ms, Ordering::Relaxed);
    }
}
