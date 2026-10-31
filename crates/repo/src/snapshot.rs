// SPDX-License-Identifier: AGPL-3.0-or-later

//! Snapshots and which ones to keep.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

/// What a run backed up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Its name in the repository, not stored inside it.
    #[serde(skip)]
    pub id: String,
    /// Unix milliseconds.
    pub time: i64,
    /// The device or server that made it.
    pub host: String,
    /// `vault` (an app's notes) or `server` (an instance).
    pub kind: String,
    /// The app or server version that made it.
    pub app_version: String,
    /// What it holds, by name.
    pub items: Vec<Item>,
    /// Counts to show: notes, folders, files...
    #[serde(default)]
    pub counts: BTreeMap<String, u64>,
}

impl Snapshot {
    /// Its plain size, what a restore writes.
    pub fn size(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }

    /// An item by name.
    pub fn item(&self, name: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.name == name)
    }
}

/// One thing a snapshot holds: its content is its objects in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// `notes/<id>`, `files/<sha256>`...
    pub name: String,
    /// Plain size.
    pub size: u64,
    /// Object ids.
    pub objects: Vec<String>,
}

/// Which snapshots to keep: the newest `last`, then the newest of each of
/// the latest `daily` days, `weekly` weeks... The newest is always kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Policy {
    /// The newest ones, whatever their dates.
    pub last: u32,
    /// Days.
    pub daily: u32,
    /// Weeks, Monday to Sunday.
    pub weekly: u32,
    /// Months.
    pub monthly: u32,
    /// Years.
    pub yearly: u32,
}

impl Default for Policy {
    /// The last 10, then 7 daily, 4 weekly, 12 monthly: a backup made by
    /// hand is not thinned out at once by the one of the same day.
    fn default() -> Self {
        Self {
            last: 10,
            daily: 7,
            weekly: 4,
            monthly: 12,
            yearly: 0,
        }
    }
}

/// Year, month and day of a day count from 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil(days: i64) -> (i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month)
}

/// The snapshots `policy` keeps among `(id, time)`, times in Unix
/// milliseconds, days counted in the time zone `offset_seconds` from UTC.
pub fn keep(snapshots: &[(String, i64)], policy: Policy, offset_seconds: i64) -> HashSet<String> {
    let mut newest_first: Vec<&(String, i64)> = snapshots.iter().collect();
    newest_first.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    let mut kept = HashSet::new();
    if let Some(first) = newest_first.first() {
        kept.insert(first.0.clone());
    }
    let day = |time: i64| (time.div_euclid(1000) + offset_seconds).div_euclid(86_400);
    let buckets: [(u32, &dyn Fn(i64) -> i64); 5] = [
        (policy.last, &|_| 0),
        (policy.daily, &|t| day(t)),
        // 1970-01-01 was a Thursday: three days on, weeks start on Monday
        (policy.weekly, &|t| (day(t) + 3).div_euclid(7)),
        (policy.monthly, &|t| {
            let (y, m) = civil(day(t));
            y * 12 + m
        }),
        (policy.yearly, &|t| civil(day(t)).0),
    ];
    for (i, (count, bucket)) in buckets.iter().enumerate() {
        let mut left = *count;
        let mut last: Option<i64> = None;
        for (id, time) in &newest_first {
            if left == 0 {
                break;
            }
            // "last" keeps each one, the others one per period
            let b = bucket(*time);
            if i == 0 || last != Some(b) {
                kept.insert(id.clone());
                left -= 1;
                last = Some(b);
            }
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    fn at(days: i64, hour: i64) -> i64 {
        days * DAY + hour * 3_600_000
    }

    #[test]
    fn the_calendar_is_right() {
        assert_eq!(civil(0), (1970, 1));
        assert_eq!(civil(31), (1970, 2));
        // 2000-02-29, 2026-10-28
        assert_eq!(civil(11_016), (2000, 2));
        assert_eq!(civil(20_754), (2026, 10));
        assert_eq!(civil(-1), (1969, 12));
    }

    #[test]
    fn keeps_one_per_period_newest_of_each() {
        // four a day for 400 days, from 2025-01-01
        let start = 20_089;
        let mut all = Vec::new();
        for d in 0..400 {
            for h in [1, 7, 13, 19] {
                all.push((format!("{d}-{h}"), at(start + d, h)));
            }
        }
        let policy = Policy {
            last: 0,
            ..Policy::default()
        };
        let kept = keep(&all, policy, 0);
        // 7 days, then the last of 4 weeks and 12 months, some shared
        assert!(kept.contains("399-19"), "the newest");
        for d in 393..400 {
            assert!(kept.contains(&format!("{d}-19")), "day {d}");
        }
        assert!(!kept.contains("399-13"), "one per day");
        assert!(kept.len() <= 7 + 4 + 12);
        assert!(kept.len() >= 12, "{}", kept.len());
        // the oldest kept: 12 months back from 2026-02-04, the last
        // snapshot of March 2025 (day 89 from 2025-01-01)
        let oldest = kept
            .iter()
            .map(|id| id.split('-').next().unwrap().parse::<i64>().unwrap())
            .min()
            .unwrap();
        assert_eq!(oldest, 89);
        assert!(kept.contains("89-19"));
    }

    #[test]
    fn days_follow_the_time_zone() {
        // 23:30 UTC and 00:30 UTC the next day: one day in Paris (UTC+1 in
        // winter), two in UTC
        let a = ("a".to_owned(), at(20_089, 23) + 1_800_000);
        let b = ("b".to_owned(), at(20_090, 0) + 1_800_000);
        let policy = Policy {
            last: 0,
            daily: 5,
            weekly: 0,
            monthly: 0,
            ..Policy::default()
        };
        assert_eq!(keep(&[a.clone(), b.clone()], policy, 0).len(), 2);
        assert_eq!(keep(&[a, b], policy, 3600).len(), 1);
    }

    #[test]
    fn an_empty_policy_keeps_the_newest_and_last_keeps_n() {
        let all: Vec<_> = (0..10).map(|i| (format!("{i}"), at(i, 0))).collect();
        let none = Policy {
            last: 0,
            daily: 0,
            weekly: 0,
            monthly: 0,
            yearly: 0,
        };
        assert_eq!(keep(&all, none, 0), HashSet::from(["9".to_owned()]));
        let three = Policy { last: 3, ..none };
        assert_eq!(keep(&all, three, 0).len(), 3);
        assert!(keep(&[], Policy::default(), 0).is_empty());
    }
}
