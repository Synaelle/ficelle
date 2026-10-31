// SPDX-License-Identifier: AGPL-3.0-or-later

//! Requests per minute from one address, for the sign-in, registration,
//! refresh and setup endpoints. A token bucket per address in memory: it
//! resets with the server, which is fine for slowing down guessing. Account
//! lockout covers guessing spread over many addresses.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};

use crate::clock::MINUTE;

/// Shared limiter.
#[derive(Clone)]
pub struct RateLimit {
    per_minute: u32,
    buckets: Arc<Mutex<HashMap<IpAddr, Bucket>>>,
}

#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    at: i64,
}

/// Buckets kept before idle ones are dropped, so the map cannot grow forever.
const MAX_BUCKETS: usize = 10_000;

impl RateLimit {
    /// `per_minute` requests a minute, all of them usable at once.
    pub fn new(per_minute: u32) -> Self {
        Self {
            per_minute,
            buckets: Arc::default(),
        }
    }

    /// Takes one request from `ip`'s allowance. `Err` holds the seconds to
    /// wait before the next one is allowed.
    pub fn check(&self, ip: IpAddr, now: i64) -> Result<(), u64> {
        let capacity = f64::from(self.per_minute);
        #[allow(clippy::cast_precision_loss)]
        let per_ms = capacity / MINUTE as f64;
        let mut buckets = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        if buckets.len() > MAX_BUCKETS {
            // a full bucket again after a minute idle, those can go
            buckets.retain(|_, b| now - b.at < MINUTE);
        }
        let bucket = buckets.entry(network(ip)).or_insert(Bucket {
            tokens: capacity,
            at: now,
        });
        #[allow(clippy::cast_precision_loss)]
        let refill = (now - bucket.at).max(0) as f64 * per_ms;
        bucket.tokens = (bucket.tokens + refill).min(capacity);
        bucket.at = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let wait = ((1.0 - bucket.tokens) / per_ms / 1000.0).ceil() as u64;
            Err(wait.max(1))
        }
    }
}

/// The bucket an address counts in: an IPv6 address by its /64, which one
/// home or one server is given whole, so a new address in it is not a new
/// allowance. IPv4 as it is.
fn network(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(std::net::Ipv6Addr::from(
                u128::from(v6) & !((1u128 << 64) - 1),
            )),
        },
        v4 @ IpAddr::V4(_) => v4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ipv6_network_shares_one_allowance() {
        let limit = RateLimit::new(2);
        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        let elsewhere: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert!(limit.check(a, 0).is_ok());
        assert!(limit.check(b, 0).is_ok());
        assert!(limit.check(a, 0).is_err(), "the same /64");
        assert!(limit.check(elsewhere, 0).is_ok());
    }

    #[test]
    fn allows_a_burst_then_refills_over_the_minute() {
        let limit = RateLimit::new(3);
        let ip: IpAddr = [10, 0, 0, 1].into();
        let other: IpAddr = [10, 0, 0, 2].into();
        for _ in 0..3 {
            assert!(limit.check(ip, 0).is_ok());
        }
        let wait = limit.check(ip, 0).unwrap_err();
        assert_eq!(wait, 20, "one request every 20 s at 3 a minute");
        assert!(limit.check(other, 0).is_ok(), "per address");
        assert!(limit.check(ip, 20_000).is_ok());
    }
}
