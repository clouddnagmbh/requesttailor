// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Rate limits (wave 18, strand D): reading `RateLimit-*`, `X-RateLimit-*` and `Retry-After`,
//! the per-host [`RateTracker`], and the runner's optional throttling (`RunPlan.throttle`).
//!
//! # What is read
//!
//! [`parse`] understands, per field and in this order of precedence:
//!
//! 1. the IETF draft's separate headers `RateLimit-Limit`, `RateLimit-Remaining`,
//!    `RateLimit-Reset`;
//! 2. the combined header, both as a dictionary (`RateLimit: limit=100, remaining=50,
//!    reset=30`) and as the newer item form (`RateLimit: "default";r=50;t=30`);
//! 3. `X-RateLimit-*`, then `X-Rate-Limit-*`.
//!
//! `RateLimit-Policy` (`100;w=60`, `"burst";q=100;w=60`, comma separated) fills
//! [`RateLimits::windows`]. A reset value is a number of seconds, **or** an epoch: values from
//! 10^9 are Unix seconds, from 10^12 Unix milliseconds (the unit is recognised, never
//! guessed); an HTTP date is read too. `Retry-After` is seconds or an HTTP date. Relative
//! values are measured against the response's `Date` header when it has one, else against the
//! local clock; the result is always relative ([`RateLimits::reset_ms`]).
//!
//! [`observe`] is [`parse`] plus the status: `Retry-After` only counts on `429` and `503`
//! (a `201` or `301` carrying it is not about rate limits), and a bare `429` still says "rate
//! limited" (an empty [`RateLimits`]).
//!
//! # The tracker
//!
//! [`RateTracker`] keeps what each host said last and answers "how long until I may send to
//! this host again" ([`RateTracker::wait`]). It is cheap to clone (clones share the state) and
//! takes a clock, so tests run on a fake one. It holds no history and writes no file. The
//! runner uses it for `--respect-limits`; the page follower and the monitors share it through
//! this module only. Nothing here sends or sleeps by itself.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};

use crate::send::ReceivedResponse;

/// What a response said about the limits of its host.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RateLimits {
    /// Requests allowed per window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Requests left in the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<u64>,
    /// Milliseconds until the window resets (relative to the response).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_ms: Option<u64>,
    /// Milliseconds the server asks to wait (`Retry-After`, after `429` or `503`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    /// Windows named by `RateLimit-Policy`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<RateWindow>,
}

/// One quota window, e.g. `100;w=60`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateWindow {
    pub limit: u64,
    /// Window length in seconds.
    pub seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThrottleMode {
    /// The default: the runner never waits for a rate limit.
    #[default]
    Off,
    /// Wait for the reset or `Retry-After` (up to `max_wait_ms`) and retry.
    Respect,
}

/// The runner's behaviour towards rate limits (`rt run --respect-limits [--max-wait 60s]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Throttle {
    #[serde(default)]
    pub mode: ThrottleMode,
    /// Longest single wait; default 60 s, hard maximum [`MAX_WAIT_HARD_MS`].
    #[serde(default = "default_max_wait")]
    pub max_wait_ms: u64,
    /// Retries per request (reads only; writes are never retried automatically).
    #[serde(default = "default_retries")]
    pub retries: u32,
}

pub const DEFAULT_MAX_WAIT_MS: u64 = 60_000;
pub const MAX_WAIT_HARD_MS: u64 = 600_000;
pub const DEFAULT_RETRIES: u32 = 2;

fn default_max_wait() -> u64 {
    DEFAULT_MAX_WAIT_MS
}

fn default_retries() -> u32 {
    DEFAULT_RETRIES
}

impl Default for Throttle {
    fn default() -> Self {
        Self {
            mode: ThrottleMode::Off,
            max_wait_ms: DEFAULT_MAX_WAIT_MS,
            retries: DEFAULT_RETRIES,
        }
    }
}

impl Throttle {
    /// Serde helper: leave the default out of `RunPlan` JSON.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl RateLimits {
    /// Nothing is known (no field set).
    pub fn is_empty(&self) -> bool {
        self.limit.is_none()
            && self.remaining.is_none()
            && self.reset_ms.is_none()
            && self.retry_after_ms.is_none()
            && self.windows.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------
// Clocks

/// Milliseconds on some monotonic scale; only differences matter. [`RateTracker`] reads it
/// when it records and when it is asked, so a test can pass a fake one.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The real clock: milliseconds since the first call in this process (`web_time`, works in
/// the browser too).
pub fn monotonic_clock() -> Clock {
    static START: OnceLock<web_time::Instant> = OnceLock::new();
    let start = *START.get_or_init(web_time::Instant::now);
    Arc::new(move || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX))
}

/// Unix time in milliseconds (for [`crate::runner::RunEvent::Waiting`] and relative dates).
pub fn unix_now_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Parsing

/// Reads the limits a response states; `None` when it states none. Relative dates are
/// measured against the `Date` header, else the local clock. `Retry-After` is returned
/// whatever the status; [`observe`] applies the `429`/`503` rule.
pub fn parse(headers: &[(String, String)]) -> Option<RateLimits> {
    parse_at(headers, unix_now_ms())
}

/// [`parse`] with an explicit "now" (Unix milliseconds) for dates; a `Date` header wins.
pub fn parse_at(headers: &[(String, String)], now_ms: i64) -> Option<RateLimits> {
    let now = header(headers, "date")
        .and_then(http_date_ms)
        .unwrap_or(now_ms);

    let mut limit = None;
    let mut remaining = None;
    let mut reset = None;
    let take = |slot: &mut Option<u64>, value: Option<u64>| {
        if slot.is_none() {
            *slot = value;
        }
    };

    // 1. The IETF draft's separate headers.
    take(
        &mut limit,
        header(headers, "ratelimit-limit").and_then(leading_u64),
    );
    take(
        &mut remaining,
        header(headers, "ratelimit-remaining").and_then(leading_u64),
    );
    take(
        &mut reset,
        header(headers, "ratelimit-reset").and_then(|v| reset_ms(v, now)),
    );

    // 2. The combined header, dictionary or item form.
    if let Some(value) = header(headers, "ratelimit") {
        let combined = combined(value, now);
        take(&mut limit, combined.limit);
        take(&mut remaining, combined.remaining);
        take(&mut reset, combined.reset_ms);
    }

    // 3. Vendor prefixes.
    for prefix in ["x-ratelimit-", "x-rate-limit-"] {
        take(
            &mut limit,
            header(headers, &format!("{prefix}limit")).and_then(leading_u64),
        );
        take(
            &mut remaining,
            header(headers, &format!("{prefix}remaining")).and_then(leading_u64),
        );
        take(
            &mut reset,
            header(headers, &format!("{prefix}reset")).and_then(|v| reset_ms(v, now)),
        );
    }

    let windows = header(headers, "ratelimit-policy")
        .map(policy)
        .unwrap_or_default();
    let retry_after_ms = header(headers, "retry-after").and_then(|v| retry_after_ms(v, now));
    if limit.is_none() {
        limit = windows.first().map(|w| w.limit);
    }
    let limits = RateLimits {
        limit,
        remaining,
        reset_ms: reset,
        retry_after_ms,
        windows,
    };
    (!limits.is_empty()).then_some(limits)
}

/// Called by `send_with` for every response; the result becomes
/// [`crate::send::SendReport::rate_limit`]. [`parse`] plus the status: `Retry-After` counts
/// only on `429` and `503`, and a `429` without any header still reports "limited" (an empty
/// value).
pub fn observe(response: &ReceivedResponse) -> Option<RateLimits> {
    let triggered = matches!(response.status, 429 | 503);
    let mut limits = parse(&response.headers).unwrap_or_default();
    if !triggered {
        limits.retry_after_ms = None;
    }
    if limits.is_empty() && response.status != 429 {
        return None;
    }
    Some(limits)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
}

/// The digits at the start (`"100, 100;w=60"` and `"100;w=60"` give 100).
fn leading_u64(value: &str) -> Option<u64> {
    let value = value.trim().trim_matches('"');
    let end = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    value[..end].parse().ok()
}

/// A plain non-negative decimal number.
fn number(value: &str) -> Option<f64> {
    let value = value.trim().trim_matches('"');
    let digits = value.chars().filter(|c| *c != '.').count();
    let dots = value.len() - digits;
    if digits == 0 || dots > 1 || !value.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    value.parse().ok()
}

/// A reset value as milliseconds from now: seconds, Unix seconds (from 10^9), Unix
/// milliseconds (from 10^12) or an HTTP date.
fn reset_ms(value: &str, now_ms: i64) -> Option<u64> {
    let until = |at_ms: f64| (at_ms - now_ms as f64).max(0.0) as u64;
    match number(value) {
        Some(v) if v >= 1e12 => Some(until(v)),
        Some(v) if v >= 1e9 => Some(until(v * 1000.0)),
        Some(v) => Some((v * 1000.0) as u64),
        None => http_date_ms(value).map(|at| until(at as f64)),
    }
}

/// `Retry-After`: delta seconds or an HTTP date.
fn retry_after_ms(value: &str, now_ms: i64) -> Option<u64> {
    match value.trim().parse::<u64>() {
        Ok(seconds) => Some(seconds.saturating_mul(1000)),
        Err(_) => http_date_ms(value).map(|at| (at - now_ms).max(0) as u64),
    }
}

#[derive(Default)]
struct Combined {
    limit: Option<u64>,
    remaining: Option<u64>,
    reset_ms: Option<u64>,
}

/// `RateLimit: limit=100, remaining=50, reset=30` and `RateLimit: "default";r=50;t=30`.
fn combined(value: &str, now_ms: i64) -> Combined {
    let mut out = Combined::default();
    for part in split_top(value, ',') {
        let part = part.trim();
        let mut pieces = split_top(part, ';').into_iter();
        let first = pieces.next().unwrap_or("").trim();
        let mut params: Vec<(String, String)> = pieces
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
            .collect();
        if let Some((k, v)) = first.split_once('=') {
            params.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
        }
        for (key, v) in params {
            match key.as_str() {
                "limit" | "q" if out.limit.is_none() => out.limit = leading_u64(&v),
                "remaining" | "r" if out.remaining.is_none() => out.remaining = leading_u64(&v),
                "reset" | "t" if out.reset_ms.is_none() => out.reset_ms = reset_ms(&v, now_ms),
                _ => {}
            }
        }
    }
    out
}

/// `RateLimit-Policy`: `100;w=60`, `"burst";q=100;w=60`, several separated by commas.
fn policy(value: &str) -> Vec<RateWindow> {
    split_top(value, ',')
        .into_iter()
        .filter_map(|item| {
            let mut pieces = split_top(item, ';').into_iter();
            let first = pieces.next()?.trim();
            let mut limit = leading_u64(first).filter(|_| number(first).is_some());
            let mut seconds = None;
            for p in pieces {
                let (k, v) = p.split_once('=')?;
                match k.trim().to_ascii_lowercase().as_str() {
                    "q" => limit = leading_u64(v),
                    "w" => seconds = leading_u64(v),
                    _ => {}
                }
            }
            Some(RateWindow {
                limit: limit?,
                seconds: seconds?,
            })
        })
        .collect()
}

/// Splits at `sep` outside double quotes.
fn split_top(value: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in value.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            out.push(&value[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&value[start..]);
    out
}

/// An HTTP date in the preferred form (`Sun, 06 Nov 1994 08:49:37 GMT`) as Unix milliseconds.
fn http_date_ms(value: &str) -> Option<i64> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [_, day, month, year, time, zone] = parts.as_slice() else {
        return None;
    };
    if !zone.eq_ignore_ascii_case("GMT") && !zone.eq_ignore_ascii_case("UTC") {
        return None;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| m.eq_ignore_ascii_case(month))? as i64
        + 1;
    let (day, year): (i64, i64) = (day.parse().ok()?, year.parse().ok()?);
    let mut t = time.split(':');
    let (h, m, s): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    if !(1..=31).contains(&day) || h > 23 || m > 59 || s > 60 || t.next().is_some() {
        return None;
    }
    // Days since 1970-01-01 (civil-from-days, Howard Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + m) * 60_000 + s * 1000)
}

// ---------------------------------------------------------------------------------------------
// Tracker

/// How long to wait and why, as the runner shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wait {
    pub ms: u64,
    /// A sentence for the user, e.g. `rate limit: 0 left, resets in 12 s`.
    pub reason: String,
}

#[derive(Debug, Clone)]
struct Seen {
    limits: RateLimits,
    at: u64,
}

/// Per-host state of the limits seen so far. Clones share the state. The runner, the page
/// follower and the monitors use one tracker per run and never keep a history or a file.
#[derive(Clone)]
pub struct RateTracker {
    hosts: Arc<Mutex<BTreeMap<String, Seen>>>,
    clock: Clock,
}

impl Default for RateTracker {
    fn default() -> Self {
        Self::with_clock(monotonic_clock())
    }
}

impl fmt::Debug for RateTracker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateTracker")
            .field("hosts", &self.lock().keys().collect::<Vec<_>>())
            .finish()
    }
}

impl RateTracker {
    /// A tracker on the real clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// A tracker on `clock` (tests pass a fake one that the fake sleep advances).
    pub fn with_clock(clock: Clock) -> Self {
        Self {
            hosts: Arc::default(),
            clock,
        }
    }

    /// The tracker's clock now.
    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Seen>> {
        self.hosts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What `host` said last (as of the response, not aged).
    pub fn get(&self, host: &str) -> Option<RateLimits> {
        self.lock().get(host).map(|s| s.limits.clone())
    }

    /// Remembers what a response from `host` said, replacing what it said before.
    pub fn record(&self, host: &str, limits: RateLimits) {
        let at = self.now();
        self.lock().insert(host.to_owned(), Seen { limits, at });
    }

    /// [`record`](Self::record) for the host of `url`; false when `url` has none.
    pub fn record_url(&self, url: &str, limits: RateLimits) -> bool {
        match host_of(url) {
            Some(host) => {
                self.record(&host, limits);
                true
            }
            None => false,
        }
    }

    /// Forgets `host`.
    pub fn forget(&self, host: &str) {
        self.lock().remove(host);
    }

    /// How long until `host` may be sent to again: the longer of `Retry-After` and (when
    /// nothing is left) the reset, minus the time that has passed since the response. `None`
    /// when nothing needs waiting for.
    pub fn wait(&self, host: &str) -> Option<Wait> {
        let hosts = self.lock();
        let seen = hosts.get(host)?;
        let elapsed = self.now().saturating_sub(seen.at);
        let l = &seen.limits;
        let reset = (l.remaining == Some(0)).then_some(l.reset_ms).flatten();
        let (total, by_retry_after) = match (l.retry_after_ms, reset) {
            (Some(r), Some(s)) if r >= s => (r, true),
            (Some(_), Some(s)) => (s, false),
            (Some(r), None) => (r, true),
            (None, Some(s)) => (s, false),
            (None, None) => return None,
        };
        let ms = total.saturating_sub(elapsed);
        if ms == 0 {
            return None;
        }
        let reason = if by_retry_after {
            format!("rate limit: the server asks to retry after {}", seconds(ms))
        } else {
            format!("rate limit: 0 left, resets in {}", seconds(ms))
        };
        Some(Wait { ms, reason })
    }
}

/// `host` or `host:port` of an absolute URL, lower case; `None` without a host.
pub fn host_of(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

/// Whole seconds, rounded up: `12 s`.
pub fn seconds(ms: u64) -> String {
    format!("{} s", ms.div_ceil(1000))
}
