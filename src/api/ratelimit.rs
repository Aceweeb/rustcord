//! Discord HTTP rate limiting.
//!
//! Discord counts requests per *bucket*, and a bucket is counted separately for
//! each major id (channel, guild, webhook), so a send in one channel must not
//! hold back another. The pattern here follows Discord's documented behaviour
//! (and, as a reference implementation, Yapfast's write-up of it): track the
//! route, honour `X-RateLimit-*`, honour global 429s, and never sleep on an
//! absurd wait.
//!
//! Deliberate simplification: buckets are keyed by `route + major id` rather than
//! shared through `X-RateLimit-Bucket`. Two routes that Discord buckets together
//! are counted separately here, which can only make Rustcord *more* conservative.
//!
//! The arithmetic is pure — `now` is passed in as seconds — so the whole module
//! is testable without sleeping or touching the network.

use std::collections::HashMap;

/// A wait longer than this is reported to the user instead of slept on. Discord
/// occasionally answers with waits in the hours after abuse detection.
pub const MAX_WAIT_SECS: f64 = 60.0;

/// What a request costs in Discord's bucket accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Method plus path with snowflake segments templated, e.g.
    /// `GET /channels/:id/messages`. Query strings are dropped: `limit` and
    /// `before` do not change a bucket.
    pub key: String,
    /// The major id the bucket is counted against (`/channels/<id>/…`).
    pub major: Option<String>,
}

impl Route {
    pub fn new(method: &str, path: &str) -> Self {
        let path_only = path.split(['?', '#']).next().unwrap_or(path);
        let segments: Vec<&str> = path_only
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();

        let mut major = None;
        let mut templated = Vec::with_capacity(segments.len());
        for (index, segment) in segments.iter().enumerate() {
            // Discord ids are long, but a test server may use short ones; any
            // all-digit segment is an id, and no route has a literal numeric part.
            let is_snowflake =
                !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit());
            if is_snowflake {
                // The first id under /channels or /guilds is the major parameter.
                if major.is_none() {
                    if let Some(parent) = index.checked_sub(1).and_then(|i| segments.get(i)) {
                        if matches!(*parent, "channels" | "guilds" | "webhooks") {
                            major = Some((*segment).to_string());
                        }
                    }
                }
                templated.push(":id");
            } else {
                templated.push(*segment);
            }
        }

        Self {
            key: format!("{} /{}", method.to_ascii_uppercase(), templated.join("/")),
            major,
        }
    }
}

/// The rate-limit headers Discord may send on any response.
#[derive(Debug, Default, Clone)]
pub struct RateLimitHeaders {
    pub limit: Option<u32>,
    pub remaining: Option<u32>,
    /// `X-RateLimit-Reset-After`, in seconds from now.
    pub reset_after: Option<f64>,
    /// `Retry-After` on a 429.
    pub retry_after: Option<f64>,
    pub global: bool,
}

impl RateLimitHeaders {
    pub fn from_pairs<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Self {
        let mut headers = Self::default();
        for (name, value) in pairs {
            match name.to_ascii_lowercase().as_str() {
                "x-ratelimit-limit" => headers.limit = value.trim().parse().ok(),
                "x-ratelimit-remaining" => headers.remaining = value.trim().parse().ok(),
                "x-ratelimit-reset-after" => headers.reset_after = value.trim().parse().ok(),
                "retry-after" => headers.retry_after = value.trim().parse().ok(),
                "x-ratelimit-global" => headers.global = value.trim().eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
        headers
    }

    pub fn from_headers(headers: &reqwest::header::HeaderMap) -> Self {
        Self::from_pairs(
            headers
                .iter()
                .filter_map(|(name, value)| Some((name.as_str(), value.to_str().ok()?))),
        )
    }

    /// Seconds to wait, preferring `retry_after` over `reset_after`.
    fn wait_hint(&self) -> Option<f64> {
        self.retry_after
            .or(self.reset_after)
            .filter(|wait| *wait >= 0.0)
    }
}

/// `retry_after` from a 429 body. The body wins over the header when Discord
/// sends both, because it carries the global flag alongside it.
pub fn retry_after_from_body(body: &str) -> Option<f64> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("retry_after")
        .and_then(serde_json::Value::as_f64)
        .filter(|wait| *wait >= 0.0)
}

/// Whether a 429 body says the limit is account-wide.
pub fn global_from_body(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("global")
                .and_then(serde_json::Value::as_bool)
                .or_else(|| {
                    value
                        .get("global")
                        .and_then(serde_json::Value::as_str)
                        .map(|flag| flag.eq_ignore_ascii_case("true"))
                })
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    limit: u32,
    remaining: u32,
    /// When the current window resets, in the same seconds-since-start as `now`.
    reset_at: f64,
}

/// Per-route bucket accounting. Cheap to clone nothing; lives behind a mutex in
/// the client because HTTP methods take `&self`.
#[derive(Debug, Default)]
pub struct RateLimiter {
    buckets: HashMap<String, Bucket>,
    /// Set by a global 429; every route waits until it passes.
    global_until: f64,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(route: &Route) -> String {
        match &route.major {
            Some(major) => format!("{}#{major}", route.key),
            None => route.key.clone(),
        }
    }

    /// How long the caller must wait before this route can be used.
    pub fn wait_seconds(&self, route: &Route, now: f64) -> f64 {
        let mut wait = (self.global_until - now).max(0.0);

        if let Some(bucket) = self.buckets.get(&Self::key(route)) {
            // A bucket with nothing left is unusable until its window resets.
            if bucket.remaining == 0 && bucket.limit > 0 {
                wait = wait.max(bucket.reset_at - now);
            }
        }

        wait.max(0.0)
    }

    /// Record what Discord said about a normal response.
    pub fn observe(&mut self, route: &Route, headers: &RateLimitHeaders, now: f64) {
        let Some(limit) = headers.limit else { return };
        let remaining = headers.remaining.unwrap_or(limit);
        let reset_at = now + headers.reset_after.unwrap_or(1.0).max(0.0);

        self.buckets.insert(
            Self::key(route),
            Bucket {
                limit,
                remaining,
                reset_at,
            },
        );
    }

    /// Record a 429 and return how long to wait before retrying.
    ///
    /// `body_retry` is the `retry_after` from the response body, which is more
    /// accurate than the header. A 429 with no timing information falls back to
    /// one second rather than hot-looping.
    pub fn note_429(
        &mut self,
        route: &Route,
        headers: &RateLimitHeaders,
        body_retry: Option<f64>,
        now: f64,
    ) -> f64 {
        let wait = body_retry
            .or_else(|| headers.wait_hint())
            .unwrap_or(1.0)
            .max(0.0);

        if headers.global {
            self.global_until = self.global_until.max(now + wait);
        } else {
            self.buckets.insert(
                Self::key(route),
                Bucket {
                    limit: headers.limit.unwrap_or(1).max(1),
                    remaining: 0,
                    reset_at: now + wait,
                },
            );
        }

        // Clean out windows that have already rolled over.
        self.buckets.retain(|_, bucket| bucket.reset_at > now);
        wait
    }

    pub fn clear(&mut self) {
        self.buckets.clear();
        self.global_until = 0.0;
    }

    pub fn tracked_routes(&self) -> usize {
        self.buckets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> RateLimitHeaders {
        RateLimitHeaders::from_pairs(pairs.iter().copied())
    }

    #[test]
    fn message_history_route_keeps_its_channel_as_major() {
        let route = Route::new(
            "GET",
            "/channels/123456789012345678/messages?limit=100&before=99",
        );
        assert_eq!(route.key, "GET /channels/:id/messages");
        assert_eq!(route.major.as_deref(), Some("123456789012345678"));
    }

    #[test]
    fn routes_without_a_major_are_keyed_alone() {
        let route = Route::new("get", "/users/@me");
        assert_eq!(route.key, "GET /users/@me");
        assert_eq!(route.major, None);

        let send = Route::new("POST", "/channels/42/messages");
        assert_eq!(send.key, "POST /channels/:id/messages");
        assert_eq!(send.major.as_deref(), Some("42"));
    }

    #[test]
    fn different_channels_do_not_share_a_bucket() {
        let mut limiter = RateLimiter::new();
        let one = Route::new("POST", "/channels/111111111/messages");
        let two = Route::new("POST", "/channels/222222222/messages");

        limiter.observe(
            &one,
            &headers(&[
                ("X-RateLimit-Limit", "5"),
                ("X-RateLimit-Remaining", "0"),
                ("X-RateLimit-Reset-After", "2.5"),
            ]),
            10.0,
        );

        assert!(limiter.wait_seconds(&one, 10.0) > 2.0);
        assert_eq!(limiter.wait_seconds(&two, 10.0), 0.0);
    }

    #[test]
    fn a_wait_expires_with_its_window() {
        let mut limiter = RateLimiter::new();
        let route = Route::new("GET", "/channels/1/messages");
        limiter.observe(
            &route,
            &headers(&[
                ("X-RateLimit-Limit", "5"),
                ("X-RateLimit-Remaining", "0"),
                ("X-RateLimit-Reset-After", "1.5"),
            ]),
            100.0,
        );

        assert!((limiter.wait_seconds(&route, 100.0) - 1.5).abs() < 1e-9);
        assert_eq!(limiter.wait_seconds(&route, 101.6), 0.0);
    }

    #[test]
    fn remaining_requests_do_not_wait() {
        let mut limiter = RateLimiter::new();
        let route = Route::new("GET", "/channels/1/messages");
        limiter.observe(
            &route,
            &headers(&[
                ("X-RateLimit-Limit", "5"),
                ("X-RateLimit-Remaining", "3"),
                ("X-RateLimit-Reset-After", "9"),
            ]),
            0.0,
        );
        assert_eq!(limiter.wait_seconds(&route, 0.0), 0.0);
    }

    #[test]
    fn a_global_429_holds_every_route() {
        let mut limiter = RateLimiter::new();
        let route = Route::new("POST", "/channels/1/messages");
        let wait = limiter.note_429(
            &route,
            &headers(&[("X-RateLimit-Global", "true"), ("Retry-After", "3")]),
            None,
            50.0,
        );
        assert!((wait - 3.0).abs() < 1e-9);

        let unrelated = Route::new("GET", "/users/@me");
        assert!((limiter.wait_seconds(&unrelated, 50.0) - 3.0).abs() < 1e-9);
        assert_eq!(limiter.wait_seconds(&unrelated, 53.5), 0.0);
    }

    #[test]
    fn a_local_429_only_holds_its_own_bucket() {
        let mut limiter = RateLimiter::new();
        let rate_limited = Route::new("POST", "/channels/1/messages");
        let other = Route::new("POST", "/channels/2/messages");

        limiter.note_429(&rate_limited, &headers(&[]), Some(2.0), 0.0);

        assert!((limiter.wait_seconds(&rate_limited, 0.0) - 2.0).abs() < 1e-9);
        assert_eq!(limiter.wait_seconds(&other, 0.0), 0.0);
    }

    #[test]
    fn a_429_without_timing_falls_back_to_one_second() {
        let mut limiter = RateLimiter::new();
        let route = Route::new("GET", "/channels/1/messages");
        assert_eq!(limiter.note_429(&route, &headers(&[]), None, 0.0), 1.0);
    }

    #[test]
    fn body_retry_after_beats_the_header() {
        let body = r#"{"message":"You are being rate limited.","retry_after":4.25,"global":false}"#;
        assert_eq!(retry_after_from_body(body), Some(4.25));
        assert!(!global_from_body(body));
        assert!(global_from_body(r#"{"global":true}"#));
        assert_eq!(retry_after_from_body("not json"), None);
    }

    #[test]
    fn malformed_headers_are_ignored_rather_than_fatal() {
        let parsed = headers(&[
            ("X-RateLimit-Limit", "many"),
            ("X-RateLimit-Remaining", "-2"),
            ("X-RateLimit-Global", "TRUE"),
        ]);
        assert_eq!(parsed.limit, None);
        assert_eq!(parsed.remaining, None);
        assert!(parsed.global);
    }

    #[test]
    fn clearing_forgets_every_route() {
        let mut limiter = RateLimiter::new();
        limiter.observe(
            &Route::new("GET", "/channels/1/messages"),
            &headers(&[("X-RateLimit-Limit", "1"), ("X-RateLimit-Remaining", "0")]),
            0.0,
        );
        assert_eq!(limiter.tracked_routes(), 1);
        limiter.clear();
        assert_eq!(limiter.tracked_routes(), 0);
    }
}
