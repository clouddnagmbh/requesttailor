// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The passive response checks (wave 18, strand B): rules with stable ids `chk/…` that look at a
//! response the user already got and say what it shows. They send nothing, write nothing and
//! read only the headers and the first [`BODY_LIMIT`] bytes of the body (a 100 MB response costs
//! the same as a 1 MiB one). An [`Observation`] is not a vulnerability: id, severity, where,
//! one sentence why, one sentence how to fix. Nothing in an observation is a secret: it names a
//! header or a cookie, never a token or a value from the body.
//!
//! # The mechanism
//!
//! A rule is a [`ResponseCheck`]: an id and a function over a [`ResponseView`] that pushes
//! observations. [`builtin`] lists the nine rules of this wave; [`run_checks`] runs any list of
//! rules over a view and drops what the `ignore` predicate switches off (by rule id, before the
//! rule runs). Other parts that check responses passively (the security audit of wave 19) bring
//! their own `ResponseCheck`s and call [`run_checks`] with their list, so ignoring, ordering and
//! the body limit stay the same everywhere. The rules are plain Rust and cost no allocation
//! beyond the observations; keep a new rule linear in the view.

use memchr::{memchr, memmem};
use rt_core::inspect::{Observation, Severity};
use rt_core::jwt;
use rt_core::transport::TlsInfo;

/// How much of the body the checks read.
pub const BODY_LIMIT: usize = 1024 * 1024;

/// What a rule sees of one response. `body` is at most the first [`BODY_LIMIT`] bytes
/// ([`ResponseView::new`] cuts it); `body_complete` says whether that is all of it.
#[derive(Debug, Clone, Copy)]
pub struct ResponseView<'a> {
    /// The final URL of the response.
    pub url: &'a str,
    pub status: u16,
    /// The request's headers as sent (secrets masked), for rules that compare, e.g. `Origin`.
    pub request_headers: &'a [(String, String)],
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
    pub body_complete: bool,
    /// What the native transport learned about the TLS connection (wave 19, strand R); `None`
    /// in the browser, for plain http and when the caller does not know.
    pub tls: Option<&'a TlsInfo>,
}

impl<'a> ResponseView<'a> {
    /// A view over `body_prefix`, cut to [`BODY_LIMIT`]; `total` is the full body size.
    pub fn new(
        url: &'a str,
        status: u16,
        request_headers: &'a [(String, String)],
        headers: &'a [(String, String)],
        body_prefix: &'a [u8],
        total: u64,
    ) -> Self {
        let body = &body_prefix[..body_prefix.len().min(BODY_LIMIT)];
        ResponseView {
            url,
            status,
            request_headers,
            headers,
            body,
            body_complete: body.len() as u64 >= total,
            tls: None,
        }
    }

    /// The same view with the TLS details of the connection (wave 19, strand R).
    pub fn with_tls(mut self, tls: Option<&'a TlsInfo>) -> Self {
        self.tls = tls;
        self
    }

    /// The first response header called `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&'a str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Every response header called `name`.
    pub fn headers_named<'n>(&self, name: &'n str) -> impl Iterator<Item = &'a str> + use<'a, 'n> {
        let headers = self.headers;
        headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn request_header(&self, name: &str) -> Option<&'a str> {
        self.request_headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn is_https(&self) -> bool {
        self.url
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
    }

    pub fn is_html(&self) -> bool {
        self.header("content-type").is_some_and(|ct| {
            let ct = ct.to_ascii_lowercase();
            ct.contains("text/html") || ct.contains("application/xhtml")
        })
    }

    /// Statuses that carry no representation; header rules skip them.
    pub(crate) fn has_representation(&self) -> bool {
        !matches!(self.status, 100..=199 | 204 | 304)
    }
}

/// One passive check. Implementations are unit structs; they hold no state.
pub trait ResponseCheck: Sync {
    /// The stable id of the rule, `chk/…`. Every observation of the rule carries it.
    fn id(&self) -> &'static str;
    /// A short name for settings lists.
    fn title(&self) -> &'static str;
    /// The most severe observation the rule makes.
    fn severity(&self) -> Severity;
    /// Pushes what `view` shows.
    fn run(&self, view: &ResponseView<'_>, out: &mut Vec<Observation>);
}

/// Runs `checks` over `view` in order, skipping the rules `ignored` names.
pub fn run_checks(
    view: &ResponseView<'_>,
    checks: &[&dyn ResponseCheck],
    ignored: &dyn Fn(&str) -> bool,
) -> Vec<Observation> {
    let mut out = Vec::new();
    for check in checks {
        if !ignored(check.id()) {
            check.run(view, &mut out);
        }
    }
    out
}

/// The nine rules of this wave, in the order their observations are listed.
pub fn builtin() -> &'static [&'static dyn ResponseCheck] {
    &[
        &HstsMissing,
        &NosniffMissing,
        &CspMissing,
        &FrameProtectionMissing,
        &CookieFlags,
        &CorsCredentials,
        &VersionDisclosure,
        &StackTrace,
        &JwtAlgNone,
    ]
}

/// All built-in rules over `view`, minus the ids in `ignore`.
pub fn check_response(view: &ResponseView<'_>, ignore: &[String]) -> Vec<Observation> {
    run_checks(view, builtin(), &|id| ignore.iter().any(|i| i == id))
}

fn observation(
    id: &str,
    severity: Severity,
    location: impl Into<String>,
    why: &str,
    fix: &str,
) -> Observation {
    Observation {
        id: id.to_owned(),
        severity,
        location: location.into(),
        why: why.to_owned(),
        fix: fix.to_owned(),
    }
}

// ---- header rules ----

struct HstsMissing;
impl ResponseCheck for HstsMissing {
    fn id(&self) -> &'static str {
        "chk/hsts-missing"
    }
    fn title(&self) -> &'static str {
        "Strict-Transport-Security is missing"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        if v.is_https() && v.has_representation() && v.header("strict-transport-security").is_none()
        {
            out.push(observation(
                self.id(),
                self.severity(),
                "response headers",
                "Without Strict-Transport-Security a browser may still try plain http for this host.",
                "Send Strict-Transport-Security: max-age=31536000; includeSubDomains over https.",
            ));
        }
    }
}

struct NosniffMissing;
impl ResponseCheck for NosniffMissing {
    fn id(&self) -> &'static str {
        "chk/nosniff-missing"
    }
    fn title(&self) -> &'static str {
        "X-Content-Type-Options is missing"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        if !v.has_representation() {
            return;
        }
        let ok = v
            .headers_named("x-content-type-options")
            .any(|h| h.trim().eq_ignore_ascii_case("nosniff"));
        if !ok {
            out.push(observation(
                self.id(),
                self.severity(),
                "response headers",
                "A browser may guess a content type other than the declared one and run the body as script.",
                "Send X-Content-Type-Options: nosniff.",
            ));
        }
    }
}

struct CspMissing;
impl ResponseCheck for CspMissing {
    fn id(&self) -> &'static str {
        "chk/csp-missing"
    }
    fn title(&self) -> &'static str {
        "Content-Security-Policy is missing (HTML)"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        if v.is_html() && v.has_representation() && v.header("content-security-policy").is_none() {
            out.push(observation(
                self.id(),
                self.severity(),
                "response headers",
                "An HTML page without a Content-Security-Policy gives injected script free rein.",
                "Send a Content-Security-Policy that limits script sources, starting with default-src 'self'.",
            ));
        }
    }
}

struct FrameProtectionMissing;
impl ResponseCheck for FrameProtectionMissing {
    fn id(&self) -> &'static str {
        "chk/frame-protection-missing"
    }
    fn title(&self) -> &'static str {
        "No protection against framing (HTML)"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        if !(v.is_html() && v.has_representation()) {
            return;
        }
        let csp_frames = v
            .headers_named("content-security-policy")
            .any(|h| h.to_ascii_lowercase().contains("frame-ancestors"));
        if v.header("x-frame-options").is_none() && !csp_frames {
            out.push(observation(
                self.id(),
                self.severity(),
                "response headers",
                "Another site can show this page in a frame and trick people into clicking it.",
                "Send X-Frame-Options: DENY or a Content-Security-Policy with frame-ancestors 'none'.",
            ));
        }
    }
}

// ---- cookies ----

struct CookieFlags;
impl ResponseCheck for CookieFlags {
    fn id(&self) -> &'static str {
        "chk/cookie-flags"
    }
    fn title(&self) -> &'static str {
        "Cookie without Secure, HttpOnly or SameSite"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        for cookie in v.headers_named("set-cookie") {
            let mut parts = cookie.split(';');
            let name = parts
                .next()
                .unwrap_or("")
                .split('=')
                .next()
                .unwrap_or("")
                .trim();
            let (mut secure, mut http_only, mut same_site) = (false, false, false);
            for attr in parts {
                let attr = attr.trim();
                let key = attr.split('=').next().unwrap_or("").trim();
                if key.eq_ignore_ascii_case("secure") {
                    secure = true;
                } else if key.eq_ignore_ascii_case("httponly") {
                    http_only = true;
                } else if key.eq_ignore_ascii_case("samesite") {
                    same_site = true;
                }
            }
            let mut missing = Vec::new();
            // A cookie over plain http cannot be Secure; that is the transport's problem.
            if !secure && v.is_https() {
                missing.push("Secure");
            }
            if !http_only {
                missing.push("HttpOnly");
            }
            if !same_site {
                missing.push("SameSite");
            }
            if !missing.is_empty() {
                out.push(observation(
                    self.id(),
                    self.severity(),
                    format!("header Set-Cookie ({name})"),
                    &format!(
                        "The cookie lacks {}, so it may leak over http, be read by script or travel with cross-site requests.",
                        missing.join(", ")
                    ),
                    "Add the missing attributes, e.g. Secure; HttpOnly; SameSite=Lax.",
                ));
            }
        }
    }
}

// ---- CORS ----

struct CorsCredentials;
impl ResponseCheck for CorsCredentials {
    fn id(&self) -> &'static str {
        "chk/cors-credentials"
    }
    fn title(&self) -> &'static str {
        "CORS allows any origin with credentials"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        let credentials = v
            .header("access-control-allow-credentials")
            .is_some_and(|c| c.trim().eq_ignore_ascii_case("true"));
        let Some(origin) = v.header("access-control-allow-origin").map(str::trim) else {
            return;
        };
        if !credentials {
            return;
        }
        let reflected = v
            .request_header("origin")
            .is_some_and(|o| !o.trim().is_empty() && o.trim() == origin);
        let (why, fix) = if origin == "*" {
            (
                "Access-Control-Allow-Origin * together with Allow-Credentials true: any site may be allowed to read answers that depend on the user's cookies.",
                "Answer with an explicit list of trusted origins, or drop Access-Control-Allow-Credentials.",
            )
        } else if reflected || origin.eq_ignore_ascii_case("null") {
            (
                "The allowed origin repeats the request's Origin (or is null) together with Allow-Credentials true: any site may be allowed to read answers that depend on the user's cookies.",
                "Compare the Origin with an allow-list before answering, and never allow null with credentials.",
            )
        } else {
            return;
        };
        out.push(observation(
            self.id(),
            self.severity(),
            "header Access-Control-Allow-Origin",
            why,
            fix,
        ));
    }
}

// ---- version disclosure ----

struct VersionDisclosure;
impl ResponseCheck for VersionDisclosure {
    fn id(&self) -> &'static str {
        "chk/version-disclosure"
    }
    fn title(&self) -> &'static str {
        "Server version in a header"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        for name in ["server", "x-powered-by"] {
            for value in v.headers_named(name) {
                if has_version(value) {
                    out.push(observation(
                        self.id(),
                        self.severity(),
                        format!("header {}", canonical(name)),
                        "The header names the software and its version, which tells an attacker which known flaws to try.",
                        "Remove the version, or the header, in the server or proxy configuration.",
                    ));
                }
            }
        }
        for name in ["x-aspnet-version", "x-aspnetmvc-version"] {
            if v.header(name).is_some() {
                out.push(observation(
                    self.id(),
                    self.severity(),
                    format!("header {}", canonical(name)),
                    "The header names the framework version, which tells an attacker which known flaws to try.",
                    "Remove the header in the framework or proxy configuration.",
                ));
            }
        }
    }
}

fn canonical(name: &str) -> &'static str {
    match name {
        "server" => "Server",
        "x-powered-by" => "X-Powered-By",
        "x-aspnet-version" => "X-AspNet-Version",
        _ => "X-AspNetMvc-Version",
    }
}

/// A digit, a dot and another digit somewhere: `nginx/1.18.0`, `PHP/8.1`.
fn has_version(value: &str) -> bool {
    let b = value.as_bytes();
    b.windows(3)
        .any(|w| w[0].is_ascii_digit() && w[1] == b'.' && w[2].is_ascii_digit())
}

// ---- stack traces ----

struct StackTrace;
impl ResponseCheck for StackTrace {
    fn id(&self) -> &'static str {
        "chk/stack-trace"
    }
    fn title(&self) -> &'static str {
        "Stack trace in an error response"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        if v.status < 400 {
            return;
        }
        if let Some((language, line)) = find_stack_trace(v.body) {
            out.push(observation(
                self.id(),
                self.severity(),
                format!("body, line {line} ({language})"),
                "The error answer shows internal file names and call frames, which helps an attacker map the application.",
                "Return a short error with an id and keep the trace in the server log.",
            ));
        }
    }
}

/// The first line that is part of a trace: language and 1-based line number. General patterns
/// for Java, .NET, Python, Node and PHP only.
fn find_stack_trace(body: &[u8]) -> Option<(&'static str, usize)> {
    // Cheap exit: every pattern has one of these bytes sequences.
    let mut start = 0;
    let mut number = 1;
    while start < body.len() {
        let end = memchr(b'\n', &body[start..]).map_or(body.len(), |p| start + p);
        let line = trim(&body[start..end]);
        if let Some(language) = classify(line) {
            return Some((language, number));
        }
        start = end + 1;
        number += 1;
    }
    None
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if first.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if last.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn has(line: &[u8], needle: &str) -> bool {
    memmem::find(line, needle.as_bytes()).is_some()
}

fn classify(line: &[u8]) -> Option<&'static str> {
    if line.len() < 8 || line.len() > 2000 {
        return None;
    }
    if line.starts_with(b"at ") {
        if has(line, ".java:") || has(line, "(Native Method)") || has(line, "(Unknown Source)") {
            return Some("Java");
        }
        if has(line, ".cs:line ") || (has(line, " in ") && has(line, ":line ")) {
            return Some(".NET");
        }
        if has(line, "node:internal")
            || has(line, ".js:")
            || has(line, ".mjs:")
            || has(line, ".ts:")
        {
            return Some("Node");
        }
    }
    if line.starts_with(b"Caused by: ") && has(line, "Exception") {
        return Some("Java");
    }
    if line.starts_with(b"Exception in thread ") {
        return Some("Java");
    }
    if line.starts_with(b"Traceback (most recent call last)") {
        return Some("Python");
    }
    if line.starts_with(b"File \"") && has(line, "\", line ") && has(line, ", in ") {
        return Some("Python");
    }
    if line == b"Stack trace:"
        || line.starts_with(b"PHP Fatal error")
        || line.starts_with(b"PHP Warning")
    {
        return Some("PHP");
    }
    if line.starts_with(b"#0 ") && has(line, ".php(") {
        return Some("PHP");
    }
    if (line.starts_with(b"Fatal error: ") || line.starts_with(b"Warning: "))
        && has(line, ".php on line ")
    {
        return Some("PHP");
    }
    if line.starts_with(b"Server Error in '") && has(line, "Application") {
        return Some(".NET");
    }
    None
}

// ---- tokens without a signature ----

struct JwtAlgNone;
impl ResponseCheck for JwtAlgNone {
    fn id(&self) -> &'static str {
        "chk/jwt-alg-none"
    }
    fn title(&self) -> &'static str {
        "Token with alg none"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        for (name, value) in v.headers {
            if has_unsigned_token(value.as_bytes()) {
                let place = if name.eq_ignore_ascii_case("set-cookie") {
                    let cookie = value.split('=').next().unwrap_or("").trim();
                    format!("header Set-Cookie ({cookie})")
                } else {
                    format!("header {name}")
                };
                out.push(unsigned(self, place));
            }
        }
        if has_unsigned_token(v.body) {
            out.push(unsigned(self, "body".to_owned()));
        }
    }
}

fn unsigned(rule: &JwtAlgNone, place: String) -> Observation {
    observation(
        rule.id(),
        rule.severity(),
        place,
        "A token whose header says alg none carries no signature, so anyone can forge its claims.",
        "Issue signed tokens and make the verifier reject alg none.",
    )
}

/// Looks for a token-shaped string (`eyJ…` at a boundary, three parts) whose header says
/// `alg: none`. At most [`MAX_CANDIDATES`] candidates, each at most [`MAX_CANDIDATE_LEN`].
fn has_unsigned_token(haystack: &[u8]) -> bool {
    const MAX_CANDIDATES: usize = 64;
    const MAX_CANDIDATE_LEN: usize = 8192;
    let mut seen = 0;
    for at in memmem::find_iter(haystack, b"eyJ") {
        if at > 0 && is_token_byte(haystack[at - 1]) {
            continue;
        }
        seen += 1;
        if seen > MAX_CANDIDATES {
            break;
        }
        let rest = &haystack[at..];
        let len = rest
            .iter()
            .take(MAX_CANDIDATE_LEN)
            .position(|&b| !is_token_byte(b))
            .unwrap_or(rest.len().min(MAX_CANDIDATE_LEN));
        let Ok(candidate) = std::str::from_utf8(&rest[..len]) else {
            continue;
        };
        if candidate.matches('.').count() == 2
            && jwt::parse(candidate).is_ok_and(|t| t.is_unsigned())
        {
            return true;
        }
    }
    false
}

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_a_digit_dot_digit() {
        assert!(has_version("nginx/1.18.0"));
        assert!(has_version("PHP/8.1"));
        assert!(!has_version("nginx"));
        assert!(!has_version("Express"));
    }

    #[test]
    fn trace_lines_are_classified_by_language() {
        assert_eq!(classify(b"at com.acme.Foo.bar(Foo.java:42)"), Some("Java"));
        assert_eq!(
            classify(b"at Foo.Bar() in C:\\src\\Foo.cs:line 12"),
            Some(".NET")
        );
        assert_eq!(
            classify(b"Traceback (most recent call last):"),
            Some("Python")
        );
        assert_eq!(
            classify(b"at Object.<anonymous> (/app/index.js:10:5)"),
            Some("Node")
        );
        assert_eq!(classify(b"Stack trace:"), Some("PHP"));
        assert_eq!(classify(b"at the end of the day we ship"), None);
    }
}
