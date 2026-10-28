//! Rate limiting for Explicit Lookups: an in-memory token bucket per
//! client, so the service can't be used to walk the address space.
//!
//! A client is an IPv4 address or an IPv6 /64 prefix (one allocation, so
//! rotating addresses inside it doesn't escape the limit). Each bucket
//! holds up to `burst` tokens and refills at `per_second`; every Explicit
//! Lookup spends one. Clients in the allow-list are never limited. A
//! bucket that has refilled to full is no different from a new one, so
//! idle buckets are dropped from time to time and memory stays bounded by
//! the clients seen in the last refill period.
//!
//! Time is tokio's [`Instant`], so tests drive it with the paused clock.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;
use std::time::Duration;

use ipnet::IpNet;
use tokio::time::Instant;

use crate::config::RateLimitConfig;

pub struct RateLimiter {
    burst: f64,
    per_second: f64,
    allow: Vec<IpNet>,
    /// Buckets that would be full again this long after their last use
    /// are dropped (at most once per this period).
    idle_after: Duration,
    state: Mutex<State>,
}

struct State {
    buckets: HashMap<IpAddr, Bucket>,
    last_sweep: Instant,
}

struct Bucket {
    tokens: f64,
    updated: Instant,
}

impl RateLimiter {
    pub fn new(config: &RateLimitConfig) -> Self {
        let burst = f64::from(config.burst);
        Self {
            burst,
            per_second: config.per_second,
            allow: config.allow.clone(),
            idle_after: Duration::from_secs_f64(burst / config.per_second)
                .max(Duration::from_secs(1)),
            state: Mutex::new(State {
                buckets: HashMap::new(),
                last_sweep: Instant::now(),
            }),
        }
    }

    /// Spends one of `client`'s tokens. When the bucket is empty, returns
    /// how long until the next token (whole seconds, at least one, for
    /// `Retry-After`).
    pub fn check(&self, client: IpAddr) -> Result<(), Duration> {
        let client = client.to_canonical();
        if self.allow.iter().any(|net| net.contains(&client)) {
            return Ok(());
        }
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        if now.duration_since(state.last_sweep) >= self.idle_after {
            let idle_after = self.idle_after;
            state
                .buckets
                .retain(|_, b| now.duration_since(b.updated) < idle_after);
            state.last_sweep = now;
        }
        let bucket = state.buckets.entry(key(client)).or_insert(Bucket {
            tokens: self.burst,
            updated: now,
        });
        let elapsed = now.duration_since(bucket.updated).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.per_second).min(self.burst);
        bucket.updated = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let wait = ((1.0 - bucket.tokens) / self.per_second).ceil().max(1.0);
            Err(Duration::from_secs_f64(wait))
        }
    }

    #[cfg(test)]
    fn buckets(&self) -> usize {
        self.state.lock().unwrap().buckets.len()
    }
}

/// The bucket a client shares: its IPv4 address, or its IPv6 /64.
fn key(client: IpAddr) -> IpAddr {
    match client {
        IpAddr::V4(_) => client,
        IpAddr::V6(v6) => {
            let prefix = u128::from(v6) & !((1u128 << 64) - 1);
            IpAddr::V6(Ipv6Addr::from(prefix))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(burst: u32, per_second: f64) -> RateLimiter {
        RateLimiter::new(&RateLimitConfig {
            burst,
            per_second,
            allow: vec![],
        })
    }

    #[test]
    fn ipv6_clients_share_their_64() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(key(a), "2001:db8:1:2::".parse::<IpAddr>().unwrap());
        assert_eq!(key(a), key(b));
        assert_ne!(key(a), key(c));
    }

    #[tokio::test(start_paused = true)]
    async fn retry_after_rounds_up_to_whole_seconds() {
        let l = limiter(1, 0.4);
        let ip = "8.8.8.8".parse().unwrap();
        assert_eq!(l.check(ip), Ok(()));
        assert_eq!(l.check(ip), Err(Duration::from_secs(3)));
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(l.check(ip), Err(Duration::from_secs(1)));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(l.check(ip), Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn idle_buckets_expire() {
        let l = limiter(10, 1.0);
        for i in 0..100u8 {
            l.check(IpAddr::from([10, 0, 0, i])).unwrap();
        }
        assert_eq!(l.buckets(), 100);
        // Ten seconds later they would all be full again: dropped on the
        // next check, which adds its own.
        tokio::time::advance(Duration::from_secs(10)).await;
        l.check("8.8.8.8".parse().unwrap()).unwrap();
        assert_eq!(l.buckets(), 1);
    }
}
