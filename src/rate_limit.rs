use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::{Request, Response};

/// Fixed-window rate limiter.
///
/// Each API key gets a counter that resets every `window` duration.
/// The per-key limit is stored in the database (`api_keys.rate_limit`),
/// so callers pass it in when checking.
pub struct RateLimiter {
    window: Duration,
    /// key_id → (window_start, count)
    buckets: Mutex<HashMap<String, (Instant, u64)>>,
}

/// Result of a rate limit check.
/// Stored in request-local state so the response fairing can attach headers.
#[derive(Clone)]
pub struct RateLimitResult {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Configured limit for this key.
    pub limit: u64,
    /// Requests remaining in the current window.
    pub remaining: u64,
    /// Seconds until the current window resets.
    pub reset_secs: u64,
}

/// Rocket fairing that attaches rate limit headers to every response.
/// Reads `RateLimitResult` from request-local state (set by the auth guard).
pub struct RateLimitHeaders;

#[rocket::async_trait]
impl Fairing for RateLimitHeaders {
    fn info(&self) -> Info {
        Info {
            name: "Rate Limit Response Headers",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        if let Some(rl) = request.local_cache(|| Option::<RateLimitResult>::None) {
            response.set_header(Header::new("X-RateLimit-Limit", rl.limit.to_string()));
            response.set_header(Header::new(
                "X-RateLimit-Remaining",
                rl.remaining.to_string(),
            ));
            response.set_header(Header::new("X-RateLimit-Reset", rl.reset_secs.to_string()));
        }
    }
}

impl RateLimiter {
    /// Create a new rate limiter with the given window duration.
    pub fn new(window: Duration) -> Self {
        RateLimiter {
            window,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Check (and consume) one request for `key_id` with the given `limit`.
    ///
    /// Returns a `RateLimitResult` indicating whether the request is allowed
    /// and the current rate limit state for response headers.
    pub fn check(&self, key_id: &str, limit: u64) -> RateLimitResult {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap();

        let entry = buckets
            .entry(key_id.to_string())
            .or_insert_with(|| (now, 0));

        // If the window has elapsed, reset.
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }

        let reset_secs = self
            .window
            .checked_sub(now.duration_since(entry.0))
            .unwrap_or(Duration::ZERO)
            .as_secs();

        if entry.1 >= limit {
            RateLimitResult {
                allowed: false,
                limit,
                remaining: 0,
                reset_secs,
            }
        } else {
            entry.1 += 1;
            RateLimitResult {
                allowed: true,
                limit,
                remaining: limit.saturating_sub(entry.1),
                reset_secs,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_under_limit() {
        let rl = RateLimiter::new(Duration::from_secs(60));
        let r = rl.check("key1", 10);
        assert!(r.allowed);
        assert_eq!(r.remaining, 9);
        assert_eq!(r.limit, 10);
    }

    #[test]
    fn blocks_at_limit() {
        let rl = RateLimiter::new(Duration::from_secs(60));
        for _ in 0..5 {
            rl.check("key1", 5);
        }
        let r = rl.check("key1", 5);
        assert!(!r.allowed);
        assert_eq!(r.remaining, 0);
    }

    #[test]
    fn separate_keys_independent() {
        let rl = RateLimiter::new(Duration::from_secs(60));
        for _ in 0..5 {
            rl.check("key1", 5);
        }
        assert!(!rl.check("key1", 5).allowed);
        assert!(rl.check("key2", 5).allowed);
    }
}
