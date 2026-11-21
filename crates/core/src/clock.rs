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
        wall_clock() + SKEW.load(Ordering::Relaxed)
    }
}

/// The system's clock. In a browser, the browser's: there the standard
/// library has none.
#[cfg(not(target_family = "wasm"))]
fn wall_clock() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            Timestamp::try_from(since.as_millis()).unwrap_or(Timestamp::MAX)
        })
}

#[cfg(target_family = "wasm")]
#[expect(
    clippy::cast_possible_truncation,
    reason = "whole milliseconds, far inside the range"
)]
fn wall_clock() -> Timestamp {
    js_sys::Date::now() as Timestamp
}

/// A moment as a calendar date and time of day in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc {
    /// The year, 1970 and so on.
    pub year: i64,
    /// 1 to 12.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
    /// 0 to 23.
    pub hour: u8,
    /// 0 to 59.
    pub minute: u8,
    /// 0 to 59.
    pub second: u8,
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar, after
/// Howard Hinnant's `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date `days` after 1970-01-01, after Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // both in range by construction
    (
        year,
        u8::try_from(month).unwrap_or(1),
        u8::try_from(day).unwrap_or(1),
    )
}

/// A timestamp as its UTC date and time, to the second.
pub fn utc(at: Timestamp) -> Utc {
    let seconds = at.div_euclid(1000);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let of_day = seconds.rem_euclid(86_400);
    // below 86 400 by construction
    let part = |n: i64| u8::try_from(n).unwrap_or(0);
    Utc {
        year,
        month,
        day,
        hour: part(of_day / 3600),
        minute: part(of_day / 60 % 60),
        second: part(of_day % 60),
    }
}

/// A UTC date and time as a timestamp; `None` when it is no real date.
pub fn from_utc(at: Utc) -> Option<Timestamp> {
    let in_month = match at.month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if at.year % 4 == 0 && (at.year % 100 != 0 || at.year % 400 == 0) => 29,
        2 => 28,
        _ => return None,
    };
    if at.day == 0 || at.day > in_month || at.hour > 23 || at.minute > 59 || at.second > 59 {
        return None;
    }
    let days = days_from_civil(at.year, i64::from(at.month), i64::from(at.day));
    let seconds = days * 86_400
        + i64::from(at.hour) * 3600
        + i64::from(at.minute) * 60
        + i64::from(at.second);
    seconds.checked_mul(1000)
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
    let t = utc(at);
    if !(0..=9999).contains(&t.year) {
        return String::from("1970-01-01T00:00:00Z");
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// Reads an HTTP `Date` header, like `Sun, 06 Nov 1994 08:49:37 GMT`.
/// `None` when it does not read.
pub fn from_http_date(text: &str) -> Option<Timestamp> {
    let at = httpdate::parse_http_date(text.trim()).ok()?;
    let since = at.duration_since(std::time::UNIX_EPOCH).ok()?;
    Timestamp::try_from(since.as_millis()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc_to_the_second() {
        assert_eq!(to_rfc3339(1_792_742_400_123), "2026-10-23T08:00:00Z");
    }

    #[test]
    fn utc_dates_go_both_ways() {
        for at in [
            0,
            1_792_742_400_000,
            951_782_400_000,   // 2000-02-29
            4_107_542_399_000, // 2100-02-28T23:59:59
            -86_400_000,
        ] {
            assert_eq!(from_utc(utc(at)), Some(at));
        }
        assert_eq!(
            utc(951_782_400_000),
            Utc {
                year: 2000,
                month: 2,
                day: 29,
                hour: 0,
                minute: 0,
                second: 0
            }
        );
        let day = |year, month, day| Utc {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(from_utc(day(2100, 2, 29)), None);
        assert_eq!(from_utc(day(2026, 4, 31)), None);
        assert_eq!(from_utc(day(2026, 13, 1)), None);
    }

    #[test]
    fn reads_http_dates() {
        assert_eq!(
            from_http_date("Fri, 23 Oct 2026 08:00:00 GMT"),
            Some(1_792_742_400_000)
        );
        assert_eq!(from_http_date(""), None);
        assert_eq!(from_http_date("yesterday"), None);
    }

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(10);
        assert_eq!(clock.now(), 10);
        clock.advance(5);
        assert_eq!(clock.now(), 15);
    }
}
