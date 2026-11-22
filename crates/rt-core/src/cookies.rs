// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Cookies (wave 5, strand D): the RFC 6265 rules the shells' `CookieJar`s share, plus the
//! steps `send_with` calls.
//!
//! - [`parse_set_cookie`]: one `Set-Cookie` header (RFC 6265 5.2) into a [`Cookie`], with the
//!   storage model of 5.3: `Domain` must domain-match the request host, a missing `Domain`
//!   makes a host-only cookie, a missing or relative `Path` takes the default path,
//!   `Max-Age` wins over `Expires`. `SameSite` is only stored (there are no cross-site
//!   requests here).
//! - [`CookieStore`]: the cookies of one environment; [`CookieStore::header`] picks the
//!   cookies for a URL (5.4: host, path, `Secure` only over `https` or to a loopback host,
//!   not expired), longest path first.
//! - [`CookieJars`]: the stores of all environments of a collection, in the JSON the shells
//!   persist; [`SharedJars`] is an in-memory `CookieJar` over it.
//!
//! Limits, on purpose: there is no public suffix list, so `Domain` is only refused when it
//! has no dot (`Domain=com`) or does not domain-match the host. `Set-Cookie` of redirect
//! responses that the transport follows itself is not seen (the native transport follows
//! redirects internally, the browser hides them); only the final response's cookies are
//! stored. A request with "follow redirects" off sees every hop.
//!
//! Cookies are session secrets: they are stored by the shells outside collection files and
//! masked in the report ([`mask_cookie_header`], [`mask_set_cookie`]).

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::services::{CookieJar, LocalBoxFuture};
use crate::transport::ResponseHead;

/// At most this many cookies per store; the oldest go first.
pub const MAX_COOKIES: usize = 3000;

/// One stored cookie (RFC 6265 5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// Lowercase, without a leading dot.
    pub domain: String,
    /// Sent to `domain` only, not to its subdomains (no `Domain` attribute).
    pub host_only: bool,
    pub path: String,
    /// Unix milliseconds; `None` for a session cookie.
    pub expires: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    /// As the server wrote it (`Strict`, `Lax`, `None`); stored, not enforced.
    pub same_site: Option<String>,
    /// Unix milliseconds of the first store; orders cookies with equal paths.
    pub created: i64,
}

impl Cookie {
    fn expired(&self, now: i64) -> bool {
        self.expires.is_some_and(|e| e <= now)
    }

    fn same_slot(&self, other: &Cookie) -> bool {
        self.name == other.name && self.domain == other.domain && self.path == other.path
    }
}

/// The parts of a request URL the cookie rules need.
struct Target {
    host: String,
    is_ip: bool,
    path: String,
    /// `https`, or a loopback host (treated as secure, as browsers do).
    secure: bool,
}

impl Target {
    fn parse(url: &str) -> Option<Self> {
        let url = url::Url::parse(url).ok()?;
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        let (host, is_ip, loopback) = match url.host()? {
            url::Host::Domain(d) => {
                let d = d.to_ascii_lowercase();
                let loopback = d == "localhost" || d.ends_with(".localhost");
                (d, false, loopback)
            }
            url::Host::Ipv4(ip) => (ip.to_string(), true, ip.is_loopback()),
            url::Host::Ipv6(ip) => (ip.to_string(), true, ip.is_loopback()),
        };
        Some(Self {
            host,
            is_ip,
            path: url.path().to_owned(),
            secure: url.scheme() == "https" || loopback,
        })
    }
}

/// RFC 6265 5.1.3.
fn domain_match(host: &str, is_ip: bool, domain: &str) -> bool {
    host == domain
        || (!is_ip
            && host.len() > domain.len()
            && host.ends_with(domain)
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

/// RFC 6265 5.1.4.
fn default_path(path: &str) -> String {
    if !path.starts_with('/') {
        return "/".to_owned();
    }
    match path.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(i) => path[..i].to_owned(),
    }
}

/// RFC 6265 5.1.4.
fn path_match(request_path: &str, cookie_path: &str) -> bool {
    request_path == cookie_path
        || (request_path.starts_with(cookie_path)
            && (cookie_path.ends_with('/')
                || request_path.as_bytes().get(cookie_path.len()) == Some(&b'/')))
}

/// Parses one `Set-Cookie` value received from `url` at `now` (Unix ms). `None` if the
/// header is invalid or the cookie may not be set for that URL.
pub fn parse_set_cookie(header: &str, url: &str, now: i64) -> Option<Cookie> {
    let target = Target::parse(url)?;
    let mut parts = header.split(';');
    let (name, value) = parts.next()?.split_once('=')?;
    let (name, value) = (name.trim(), value.trim());
    if name.is_empty() {
        return None;
    }
    let mut expires_attr = None;
    let mut max_age = None;
    let mut domain = None;
    let mut path = None;
    let mut secure = false;
    let mut http_only = false;
    let mut same_site = None;
    for attr in parts {
        let (key, val) = attr.split_once('=').unwrap_or((attr, ""));
        let (key, val) = (key.trim(), val.trim());
        match key.to_ascii_lowercase().as_str() {
            "expires" => {
                if let Some(t) = parse_cookie_date(val) {
                    expires_attr = Some(t);
                }
            }
            "max-age" => {
                let digits = val.strip_prefix('-').unwrap_or(val);
                if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    let secs = val.parse::<i64>().unwrap_or(if val.starts_with('-') {
                        i64::MIN / 2
                    } else {
                        i64::MAX / 2000
                    });
                    max_age = Some(if secs <= 0 {
                        i64::MIN
                    } else {
                        now.saturating_add(secs.saturating_mul(1000))
                    });
                }
            }
            "domain" => {
                let d = val.trim_start_matches('.').to_ascii_lowercase();
                if !d.is_empty() {
                    domain = Some(d);
                }
            }
            "path" => {
                path = Some(if val.starts_with('/') {
                    val.to_owned()
                } else {
                    default_path(&target.path)
                });
            }
            "secure" => secure = true,
            "httponly" => http_only = true,
            "samesite" if !val.is_empty() => same_site = Some(val.to_owned()),
            _ => {}
        }
    }
    let (domain, host_only) = match domain {
        Some(d) if d == target.host => (d, true),
        Some(d) => {
            // Without a public suffix list: refuse single labels (`com`) and anything the
            // host does not belong to.
            if !d.contains('.') || !domain_match(&target.host, target.is_ip, &d) {
                return None;
            }
            (d, false)
        }
        None => (target.host.clone(), true),
    };
    Some(Cookie {
        name: name.to_owned(),
        value: value.to_owned(),
        domain,
        host_only,
        path: path.unwrap_or_else(|| default_path(&target.path)),
        expires: max_age.or(expires_attr),
        secure,
        http_only,
        same_site,
        created: now,
    })
}

/// RFC 6265 5.1.1 (the lenient cookie-date algorithm) → Unix milliseconds.
fn parse_cookie_date(s: &str) -> Option<i64> {
    let is_delimiter = |c: char| matches!(c, '\t' | ' '..='/' | ';'..='@' | '['..='`' | '{'..='~');
    let mut time = None;
    let mut day = None;
    let mut month = None;
    let mut year = None;
    for token in s.split(is_delimiter).filter(|t| !t.is_empty()) {
        let leading_digits = |t: &str, min: usize, max: usize| -> Option<u32> {
            let n = t.bytes().take_while(u8::is_ascii_digit).count();
            (min..=max).contains(&n).then(|| t[..n].parse().ok())?
        };
        if time.is_none() {
            let fields: Vec<&str> = token.split(':').collect();
            if fields.len() == 3
                && let (Some(h), Some(m), Some(sec)) = (
                    leading_digits(fields[0], 1, 2),
                    leading_digits(fields[1], 1, 2),
                    leading_digits(fields[2], 1, 2),
                )
            {
                time = Some((h, m, sec));
                continue;
            }
        }
        if day.is_none()
            && let Some(d) = leading_digits(token, 1, 2)
        {
            day = Some(d);
            continue;
        }
        if month.is_none() && token.len() >= 3 {
            const MONTHS: [&str; 12] = [
                "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
            ];
            let prefix = token[..3].to_ascii_lowercase();
            if let Some(i) = MONTHS.iter().position(|m| *m == prefix) {
                month = Some(i as u32 + 1);
                continue;
            }
        }
        if year.is_none()
            && let Some(y) = leading_digits(token, 2, 4)
        {
            year = Some(match y {
                70..=99 => y + 1900,
                0..=69 => y + 2000,
                y => y,
            });
        }
    }
    let ((h, m, sec), d, mo, y) = (time?, day?, month?, year?);
    if !(1..=31).contains(&d) || y < 1601 || h > 23 || m > 59 || sec > 59 {
        return None;
    }
    let days = days_from_civil(i64::from(y), mo, d);
    Some((days * 86_400 + i64::from(h * 3600 + m * 60 + sec)) * 1000)
}

/// Proleptic Gregorian date → days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The current time in Unix milliseconds.
pub fn now_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// The cookies of one environment.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CookieStore {
    cookies: Vec<Cookie>,
}

impl CookieStore {
    /// Every stored cookie, expired ones included until the next change.
    pub fn cookies(&self) -> &[Cookie] {
        &self.cookies
    }

    /// Stores the `Set-Cookie` headers of a response to `url`. An expired cookie removes
    /// the stored one with the same name, domain and path.
    pub fn store(&mut self, url: &str, set_cookie: &[String], now: i64) {
        for header in set_cookie {
            let Some(mut cookie) = parse_set_cookie(header, url, now) else {
                continue;
            };
            if let Some(i) = self.cookies.iter().position(|c| c.same_slot(&cookie)) {
                cookie.created = self.cookies[i].created;
                self.cookies.remove(i);
            }
            if !cookie.expired(now) {
                self.cookies.push(cookie);
            }
        }
        self.cookies.retain(|c| !c.expired(now));
        if self.cookies.len() > MAX_COOKIES {
            self.cookies.sort_by_key(|c| c.created);
            let extra = self.cookies.len() - MAX_COOKIES;
            self.cookies.drain(..extra);
        }
    }

    /// The `Cookie` header value for a request to `url`, or `None` if no cookie applies.
    pub fn header(&self, url: &str, now: i64) -> Option<String> {
        let target = Target::parse(url)?;
        let mut matching: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| !c.expired(now))
            .filter(|c| {
                if c.host_only {
                    target.host == c.domain
                } else {
                    domain_match(&target.host, target.is_ip, &c.domain)
                }
            })
            .filter(|c| path_match(&target.path, &c.path))
            .filter(|c| !c.secure || target.secure)
            .collect();
        if matching.is_empty() {
            return None;
        }
        matching.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then(a.created.cmp(&b.created))
        });
        Some(
            matching
                .iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    /// Removes one cookie; `false` if there was none.
    pub fn remove(&mut self, domain: &str, path: &str, name: &str) -> bool {
        let before = self.cookies.len();
        self.cookies
            .retain(|c| !(c.domain == domain && c.path == path && c.name == name));
        self.cookies.len() != before
    }

    pub fn clear(&mut self) {
        self.cookies.clear();
    }

    /// Drops expired cookies.
    pub fn purge(&mut self, now: i64) {
        self.cookies.retain(|c| !c.expired(now));
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }
}

/// The cookie stores of one collection, per environment (`""`: no environment). The JSON
/// is what the shells persist: `{"version": 1, "scopes": {"Local": [cookie, ...]}}`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CookieJars {
    pub version: u32,
    pub scopes: BTreeMap<String, CookieStore>,
}

/// Version of the [`CookieJars`] JSON.
pub const JARS_VERSION: u32 = 1;

impl CookieJars {
    pub fn new() -> Self {
        Self {
            version: JARS_VERSION,
            scopes: BTreeMap::new(),
        }
    }

    /// Reads persisted jars; unreadable or newer data gives empty jars (cookies are
    /// session state, losing them is not an error).
    pub fn from_json(bytes: &[u8]) -> Self {
        match serde_json::from_slice::<Self>(bytes) {
            Ok(jars) if jars.version == JARS_VERSION => jars,
            _ => Self::new(),
        }
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn scope(&self, scope: &str) -> Option<&CookieStore> {
        self.scopes.get(scope)
    }

    pub fn store(&mut self, scope: &str, url: &str, set_cookie: &[String], now: i64) {
        let store = self.scopes.entry(scope.to_owned()).or_default();
        store.store(url, set_cookie, now);
        if store.is_empty() {
            self.scopes.remove(scope);
        }
    }

    pub fn header(&self, scope: &str, url: &str, now: i64) -> Option<String> {
        self.scopes.get(scope)?.header(url, now)
    }

    pub fn remove(&mut self, scope: &str, domain: &str, path: &str, name: &str) -> bool {
        let Some(store) = self.scopes.get_mut(scope) else {
            return false;
        };
        let removed = store.remove(domain, path, name);
        if store.is_empty() {
            self.scopes.remove(scope);
        }
        removed
    }

    pub fn clear(&mut self, scope: &str) {
        self.scopes.remove(scope);
    }
}

/// An in-memory [`CookieJar`] over [`CookieJars`], with the system clock. Shells that
/// persist the jars wrap it and save after [`CookieJar::store`].
#[derive(Debug, Default)]
pub struct SharedJars {
    jars: Mutex<CookieJars>,
}

impl SharedJars {
    pub fn new(jars: CookieJars) -> Self {
        Self {
            jars: Mutex::new(jars),
        }
    }

    /// Runs `f` on the jars (a poisoned lock is recovered: the data is plain values).
    pub fn with<R>(&self, f: impl FnOnce(&mut CookieJars) -> R) -> R {
        let mut jars = self.jars.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut jars)
    }

    pub fn snapshot(&self) -> CookieJars {
        self.with(|j| j.clone())
    }
}

impl CookieJar for SharedJars {
    fn cookie_header<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
    ) -> LocalBoxFuture<'a, Option<String>> {
        let header = self.with(|j| j.header(scope, url, now_ms()));
        Box::pin(std::future::ready(header))
    }

    fn store<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
        set_cookie: &'a [String],
    ) -> LocalBoxFuture<'a, ()> {
        self.with(|j| j.store(scope, url, set_cookie, now_ms()));
        Box::pin(std::future::ready(()))
    }
}

/// Adds the jar's cookies to the request. A `Cookie` header the user wrote wins for the
/// names it sets; the jar's other cookies are appended to it.
pub fn add_cookie_header(headers: &mut Vec<(String, String)>, cookie: &str) {
    let Some((_, existing)) = headers
        .iter_mut()
        .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
    else {
        headers.push(("Cookie".to_owned(), cookie.to_owned()));
        return;
    };
    let names: Vec<String> = existing
        .split(';')
        .filter_map(|p| Some(p.split_once('=')?.0.trim().to_owned()))
        .collect();
    let extra: Vec<&str> = cookie
        .split(';')
        .map(str::trim)
        .filter(|p| {
            p.split_once('=')
                .is_some_and(|(n, _)| !names.iter().any(|m| m == n.trim()))
        })
        .collect();
    if extra.is_empty() {
        return;
    }
    let mut merged = existing.trim_end().trim_end_matches(';').to_owned();
    for pair in extra {
        if !merged.is_empty() {
            merged.push_str("; ");
        }
        merged.push_str(pair);
    }
    *existing = merged;
}

/// The `Set-Cookie` header values of a response.
pub fn set_cookie_values(head: &ResponseHead) -> Vec<String> {
    head.headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
        .map(|(_, value)| value.clone())
        .collect()
}

/// A `Cookie` header value for the report: `a=***; b=***`.
pub fn mask_cookie_header(value: &str) -> String {
    value
        .split(';')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) => format!("{}={}", name.trim(), crate::send::MASK),
            None => pair.trim().to_owned(),
        })
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

/// A `Set-Cookie` value for the report: the value masked, the attributes as received.
pub fn mask_set_cookie(value: &str) -> String {
    match value.split_once(';') {
        Some((pair, attributes)) => format!("{};{attributes}", mask_cookie_header(pair)),
        None => mask_cookie_header(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    fn parse(header: &str, url: &str) -> Option<Cookie> {
        parse_set_cookie(header, url, NOW)
    }

    #[test]
    fn name_value_and_defaults() {
        let c = parse("sid=abc; HttpOnly", "https://api.example.com/v1/login").unwrap();
        assert_eq!((c.name.as_str(), c.value.as_str()), ("sid", "abc"));
        assert_eq!(c.domain, "api.example.com");
        assert!(c.host_only && c.http_only && !c.secure);
        assert_eq!(c.path, "/v1");
        assert_eq!(c.expires, None);
        assert_eq!(c.created, NOW);
        assert!(parse("novalue", "https://x.example/").is_none());
        assert!(parse("=v", "https://x.example/").is_none());
        assert_eq!(parse("a=", "https://x.example/").unwrap().value, "");
        assert_eq!(
            parse(" a = \"q\" ", "https://x.example/").unwrap().value,
            "\"q\""
        );
    }

    #[test]
    fn domain_attribute() {
        let c = parse("a=1; Domain=.Example.com", "https://api.example.com/").unwrap();
        assert_eq!(c.domain, "example.com");
        assert!(!c.host_only);
        let c = parse("a=1; Domain=api.example.com", "https://api.example.com/").unwrap();
        assert!(c.host_only, "domain equal to the host");
        assert!(parse("a=1; Domain=other.com", "https://api.example.com/").is_none());
        assert!(parse("a=1; Domain=com", "https://example.com/").is_none());
        assert!(parse("a=1; Domain=ample.com", "https://example.com/").is_none());
        assert!(parse("a=1; Domain=0.1", "http://127.0.0.1/").is_none());
        let c = parse("a=1; Domain=", "https://example.com/").unwrap();
        assert!(c.host_only, "empty Domain is ignored");
    }

    #[test]
    fn path_attribute() {
        let c = parse("a=1; Path=/api", "https://x.example/other/page").unwrap();
        assert_eq!(c.path, "/api");
        let c = parse("a=1; Path=relative", "https://x.example/docs/page").unwrap();
        assert_eq!(c.path, "/docs");
        assert_eq!(default_path("/"), "/");
        assert_eq!(default_path("/a"), "/");
        assert_eq!(default_path("/a/b/"), "/a/b");
        assert!(path_match("/a/b", "/a"));
        assert!(path_match("/a/", "/a/"));
        assert!(path_match("/a/b", "/a/"));
        assert!(!path_match("/ab", "/a"));
        assert!(path_match("/a", "/a"));
    }

    #[test]
    fn expiry() {
        let c = parse("a=1; Max-Age=60", "https://x.example/").unwrap();
        assert_eq!(c.expires, Some(NOW + 60_000));
        let c = parse(
            "a=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT; Max-Age=60",
            "https://x.example/",
        )
        .unwrap();
        assert_eq!(c.expires, Some(NOW + 60_000), "Max-Age wins");
        let c = parse(
            "a=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
            "https://x.example/",
        )
        .unwrap();
        assert_eq!(c.expires, Some(1_445_412_480_000));
        let c = parse("a=1; Max-Age=0", "https://x.example/").unwrap();
        assert!(c.expired(NOW));
        let c = parse("a=1; Max-Age=-5", "https://x.example/").unwrap();
        assert!(c.expired(NOW));
        let c = parse("a=1; Max-Age=abc", "https://x.example/").unwrap();
        assert_eq!(c.expires, None, "invalid Max-Age is ignored");
        let c = parse("a=1; Max-Age=99999999999999999999", "https://x.example/").unwrap();
        assert!(c.expires.unwrap() > NOW);
    }

    #[test]
    fn cookie_dates() {
        let rfc1123 = parse_cookie_date("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
        assert_eq!(rfc1123, 784_111_777_000);
        assert_eq!(
            parse_cookie_date("Sunday, 06-Nov-94 08:49:37 GMT"),
            Some(rfc1123)
        );
        assert_eq!(parse_cookie_date("Sun Nov  6 08:49:37 1994"), Some(rfc1123));
        assert_eq!(
            parse_cookie_date("06 Nov 2069 00:00:00"),
            parse_cookie_date("6 nov 69 0:0:0")
        );
        assert!(parse_cookie_date("Sun, 32 Nov 1994 08:49:37 GMT").is_none());
        assert!(parse_cookie_date("Sun, 06 Nov 1994 25:49:37 GMT").is_none());
        assert!(parse_cookie_date("garbage").is_none());
        assert_eq!(parse_cookie_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
    }

    #[test]
    fn flags_and_same_site() {
        let c = parse("a=1; Secure; SameSite=Lax; HttpOnly", "https://x.example/").unwrap();
        assert!(c.secure && c.http_only);
        assert_eq!(c.same_site.as_deref(), Some("Lax"));
        let c = parse("a=1; secure; samesite", "https://x.example/").unwrap();
        assert!(c.secure);
        assert_eq!(c.same_site, None);
    }

    fn store(entries: &[(&str, &str)]) -> CookieStore {
        let mut s = CookieStore::default();
        for (url, header) in entries {
            s.store(url, &[(*header).to_owned()], NOW);
        }
        s
    }

    #[test]
    fn matching_by_host_path_and_scheme() {
        let s = store(&[
            ("https://api.example.com/", "host=1"),
            ("https://api.example.com/", "wide=2; Domain=example.com"),
            ("https://api.example.com/", "deep=3; Path=/v1/users"),
            ("https://api.example.com/", "sec=4; Secure"),
        ]);
        assert_eq!(
            s.header("https://api.example.com/v1/users/7", NOW)
                .as_deref(),
            Some("deep=3; host=1; wide=2; sec=4")
        );
        assert_eq!(
            s.header("http://api.example.com/", NOW).as_deref(),
            Some("host=1; wide=2"),
            "Secure only over https"
        );
        assert_eq!(
            s.header("https://www.example.com/", NOW).as_deref(),
            Some("wide=2"),
            "host-only cookies stay on their host"
        );
        assert_eq!(s.header("https://example.org/", NOW), None);
        assert_eq!(s.header("not a url", NOW), None);
    }

    #[test]
    fn loopback_counts_as_secure() {
        let s = store(&[("http://localhost:8080/", "a=1; Secure")]);
        assert_eq!(
            s.header("http://localhost:8080/x", NOW).as_deref(),
            Some("a=1")
        );
        let s = store(&[("http://127.0.0.1:8080/", "a=1; Secure")]);
        assert_eq!(s.header("http://127.0.0.1:9/", NOW).as_deref(), Some("a=1"));
    }

    #[test]
    fn replacing_and_deleting() {
        let mut s = store(&[("https://x.example/", "a=1"), ("https://x.example/", "b=2")]);
        s.store("https://x.example/", &["a=3".to_owned()], NOW + 10);
        assert_eq!(s.cookies().len(), 2);
        let a = s.cookies().iter().find(|c| c.name == "a").unwrap();
        assert_eq!(
            (a.value.as_str(), a.created),
            ("3", NOW),
            "keeps the creation time"
        );
        s.store("https://x.example/", &["b=; Max-Age=0".to_owned()], NOW);
        assert_eq!(s.header("https://x.example/", NOW).as_deref(), Some("a=3"));
        let mut s = store(&[("https://x.example/", "a=1; Max-Age=1")]);
        assert_eq!(s.header("https://x.example/", NOW + 2000), None, "expired");
        s.purge(NOW + 2000);
        assert!(s.is_empty());
        let mut s = store(&[("https://x.example/", "a=1")]);
        assert!(!s.remove("x.example", "/", "b"));
        assert!(s.remove("x.example", "/", "a"));
        assert!(s.is_empty());
    }

    #[test]
    fn store_is_capped() {
        let mut s = CookieStore::default();
        let headers: Vec<String> = (0..MAX_COOKIES + 5).map(|i| format!("c{i}=v")).collect();
        for (i, h) in headers.iter().enumerate() {
            s.store(
                "https://x.example/",
                std::slice::from_ref(h),
                NOW + i as i64,
            );
        }
        assert_eq!(s.cookies().len(), MAX_COOKIES);
        assert!(s.cookies().iter().all(|c| c.name != "c0"), "oldest dropped");
    }

    #[test]
    fn jars_per_scope_and_json() {
        let mut jars = CookieJars::new();
        jars.store("Local", "https://x.example/", &["a=1".into()], NOW);
        jars.store("Prod", "https://x.example/", &["a=2".into()], NOW);
        assert_eq!(
            jars.header("Local", "https://x.example/", NOW).as_deref(),
            Some("a=1")
        );
        assert_eq!(
            jars.header("Prod", "https://x.example/", NOW).as_deref(),
            Some("a=2")
        );
        assert_eq!(jars.header("", "https://x.example/", NOW), None);
        let back = CookieJars::from_json(&jars.to_json());
        assert_eq!(back, jars);
        let json: serde_json::Value = serde_json::from_slice(&jars.to_json()).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["scopes"]["Local"][0]["name"], "a");
        assert_eq!(CookieJars::from_json(b"not json"), CookieJars::new());
        assert_eq!(
            CookieJars::from_json(br#"{"version": 99, "scopes": {}}"#),
            CookieJars::new()
        );
        assert!(jars.remove("Local", "x.example", "/", "a"));
        assert!(jars.scope("Local").is_none());
        jars.clear("Prod");
        assert!(jars.scopes.is_empty());
    }

    #[test]
    fn user_cookie_header_wins_per_name() {
        let mut headers = vec![("cookie".to_owned(), "a=mine".to_owned())];
        add_cookie_header(&mut headers, "a=jar; b=jar");
        assert_eq!(headers, [("cookie".to_owned(), "a=mine; b=jar".to_owned())]);
        let mut headers = vec![("Cookie".to_owned(), "a=mine;".to_owned())];
        add_cookie_header(&mut headers, "a=jar");
        assert_eq!(headers[0].1, "a=mine;");
        let mut headers = Vec::new();
        add_cookie_header(&mut headers, "a=1");
        assert_eq!(headers, [("Cookie".to_owned(), "a=1".to_owned())]);
    }

    #[test]
    fn masking_for_the_report() {
        assert_eq!(mask_cookie_header("a=1; b=two=2"), "a=***; b=***");
        assert_eq!(
            mask_set_cookie("sid=abc; Path=/; HttpOnly"),
            "sid=***; Path=/; HttpOnly"
        );
        assert_eq!(mask_set_cookie("sid=abc"), "sid=***");
    }

    #[test]
    fn shared_jars_implement_the_service() {
        let jars = SharedJars::default();
        let set_cookie = ["a=1".to_owned()];
        let fut = CookieJar::store(&jars, "Local", "https://x.example/", &set_cookie);
        drop(fut);
        // `store` applies before the future is polled; the future only reports completion.
        let header = jars.with(|j| j.header("Local", "https://x.example/", now_ms()));
        assert_eq!(header.as_deref(), Some("a=1"));
    }
}
