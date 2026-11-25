// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The passive security rules of wave 19, strand R, as [`ResponseCheck`]s: they run in the
//! same pass as the checks of wave 18 and show in the same chip and tab. Each rule is a pure
//! function over one [`ResponseView`] (headers, the first MiB of the body, the TLS details the
//! native transport knows): it sends nothing and keeps nothing. What a token says is never
//! repeated: a finding names the rule, the place (a header or a JSON key) and one sentence.

use memchr::memmem;
use rt_core::inspect::{Observation, Severity};
use rt_core::jwt::{self, IssueKind};

use super::tokens::{self, Found};
use crate::inspect::{ResponseCheck, ResponseView};

/// The rules that are new in this wave; the ones of wave 18 that the security audit lists too
/// (`chk/cors-credentials`, `chk/jwt-alg-none`, `chk/hsts-missing`, `chk/nosniff-missing`,
/// `chk/stack-trace`, `chk/version-disclosure`) stay in `rt_app::inspect` and are not run twice.
pub fn checks() -> &'static [&'static dyn ResponseCheck] {
    &[
        &CorsReflectedOrigin,
        &JwtNoExp,
        &JwtExpiredInUse,
        &JwtWeakHmac,
        &CacheControlTokens,
        &NoRateLimit,
        &TlsOldProtocol,
        &TlsCertExpired,
        &TlsCertExpiring,
        &TlsWeakKey,
        &ErrorPageVersion,
    ]
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

/// Tokens of the request and of the response, in that order.
fn all_tokens<'a>(v: &ResponseView<'a>) -> Vec<Found<'a>> {
    let mut found = tokens::in_request(v);
    found.extend(tokens::in_response(v));
    found
}

// ---- CORS ----

struct CorsReflectedOrigin;
impl ResponseCheck for CorsReflectedOrigin {
    fn id(&self) -> &'static str {
        "sec/cors-reflected-origin"
    }
    fn title(&self) -> &'static str {
        "CORS repeats the request's Origin"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        let Some(allowed) = v.header("access-control-allow-origin").map(str::trim) else {
            return;
        };
        let Some(origin) = v.request_header("origin").map(str::trim) else {
            return;
        };
        let credentials = v
            .header("access-control-allow-credentials")
            .is_some_and(|c| c.trim().eq_ignore_ascii_case("true"));
        // With credentials `chk/cors-credentials` already says it; one hint is enough.
        if credentials || origin.is_empty() || allowed == "*" || allowed != origin {
            return;
        }
        out.push(observation(
            self.id(),
            self.severity(),
            "header Access-Control-Allow-Origin",
            "The allowed origin repeats whatever Origin the request sent, so any site may be allowed to read this answer.",
            "Compare the Origin with an allow-list before answering instead of echoing it.",
        ));
    }
}

// ---- tokens ----

struct JwtNoExp;
impl ResponseCheck for JwtNoExp {
    fn id(&self) -> &'static str {
        "sec/jwt-no-exp"
    }
    fn title(&self) -> &'static str {
        "Token without an expiry"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        for found in all_tokens(v) {
            // `alg: none` has its own rule.
            if found.jwt.is_unsigned() || found.jwt.time_claim("exp").is_some() {
                continue;
            }
            out.push(observation(
                self.id(),
                self.severity(),
                found.place,
                "A token without an exp claim never expires, so a leaked one stays valid for good.",
                "Issue tokens with a short exp and refresh them.",
            ));
        }
    }
}

struct JwtExpiredInUse;
impl ResponseCheck for JwtExpiredInUse {
    fn id(&self) -> &'static str {
        "sec/jwt-expired-in-use"
    }
    fn title(&self) -> &'static str {
        "Expired token sent with the request"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        let now = jwt::now_secs();
        for found in tokens::in_request(v) {
            let expired = found
                .jwt
                .issues(now)
                .iter()
                .any(|i| i.kind == IssueKind::Expired);
            if !expired {
                continue;
            }
            let accepted = v.status < 400;
            out.push(observation(
                self.id(),
                if accepted {
                    Severity::Warn
                } else {
                    Severity::Info
                },
                found.place,
                if accepted {
                    "The request carried an expired token and the server accepted it, so it does not check exp."
                } else {
                    "The request carried an expired token, which the server refused as it should."
                },
                if accepted {
                    "Make the server reject tokens whose exp has passed."
                } else {
                    "Get a fresh token before sending."
                },
            ));
        }
    }
}

/// Shared secrets that show up in tutorials and default configurations; a token signed with
/// one of them can be forged by anybody. Never printed.
const WEAK_SECRETS: &[&str] = &[
    "secret",
    "password",
    "changeme",
    "changeit",
    "123456",
    "12345678",
    "1234",
    "admin",
    "jwt",
    "jwtsecret",
    "jwt_secret",
    "jwt-secret",
    "your-256-bit-secret",
    "your-secret",
    "your_secret_key",
    "secretkey",
    "secret_key",
    "mysecret",
    "my-secret",
    "supersecret",
    "super-secret",
    "test",
    "testing",
    "key",
    "default",
    "letmein",
    "qwerty",
    "token",
    "hmac",
    "hs256",
    "s3cr3t",
    "abc123",
    "password123",
    "insecure",
    "dev",
    "development",
    "example",
    "private",
    "signingkey",
    "shhhhh",
];

/// At most this many tokens are tried per response.
const MAX_HMAC_TOKENS: usize = 4;

struct JwtWeakHmac;
impl ResponseCheck for JwtWeakHmac {
    fn id(&self) -> &'static str {
        "sec/jwt-weak-hmac"
    }
    fn title(&self) -> &'static str {
        "Token signed with a guessable secret"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        let hs256 = all_tokens(v)
            .into_iter()
            .filter(|f| f.jwt.algorithm() == Some("HS256"))
            .take(MAX_HMAC_TOKENS);
        for found in hs256 {
            if WEAK_SECRETS
                .iter()
                .any(|secret| jwt::hs256_secret_matches(found.text, secret))
            {
                out.push(observation(
                    self.id(),
                    self.severity(),
                    found.place,
                    "The token's signature matches a common, guessable secret, so anybody can forge tokens for this API.",
                    "Use a long random secret (at least 256 bits) or an asymmetric algorithm, and rotate the old one.",
                ));
            }
        }
    }
}

// ---- caching ----

struct CacheControlTokens;
impl ResponseCheck for CacheControlTokens {
    fn id(&self) -> &'static str {
        "sec/cache-control-tokens"
    }
    fn title(&self) -> &'static str {
        "Token in a response that may be cached"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        if !(200..300).contains(&v.status) {
            return;
        }
        let no_store = v.headers_named("cache-control").any(|h| {
            h.split(',')
                .any(|d| d.trim().eq_ignore_ascii_case("no-store"))
        });
        if no_store {
            return;
        }
        if let Some(found) = tokens::in_response(v).into_iter().next() {
            out.push(observation(
                self.id(),
                self.severity(),
                found.place,
                "A response with a token should say Cache-Control: no-store, or a shared cache may keep the token.",
                "Send Cache-Control: no-store (and Pragma: no-cache) with every response that carries a token.",
            ));
        }
    }
}

// ---- rate limiting ----

struct NoRateLimit;
impl ResponseCheck for NoRateLimit {
    fn id(&self) -> &'static str {
        "sec/no-rate-limit"
    }
    fn title(&self) -> &'static str {
        "No sign of rate limiting"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        if !(200..300).contains(&v.status) || !v.has_representation() || v.is_html() {
            return;
        }
        // The same reading the rate tracker (`rt_core::ratelimit`) records for the host:
        // `RateLimit-*`, the combined header, `X-RateLimit-*` and `Retry-After`.
        if rt_core::ratelimit::parse(v.headers).is_none() {
            out.push(observation(
                self.id(),
                self.severity(),
                "response headers",
                "The response states no rate limit (no RateLimit or X-RateLimit headers), so clients cannot tell whether the API throttles them, and it may not.",
                "Limit the request rate per client and send RateLimit-Limit, RateLimit-Remaining and Retry-After.",
            ));
        }
    }
}

// ---- TLS ----

/// A certificate with less than this left counts as expiring.
const EXPIRING_SECS: i64 = 14 * 86_400;

struct TlsOldProtocol;
impl ResponseCheck for TlsOldProtocol {
    fn id(&self) -> &'static str {
        "sec/tls-old-protocol"
    }
    fn title(&self) -> &'static str {
        "Outdated TLS protocol"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        let Some(protocol) = v.tls.and_then(|t| t.protocol.as_deref()) else {
            return;
        };
        let normal = protocol.replace('_', ".");
        let old = normal.starts_with("SSL")
            || normal.eq_ignore_ascii_case("TLSv1.0")
            || normal.eq_ignore_ascii_case("TLSv1.1")
            || normal.eq_ignore_ascii_case("TLSv1");
        if old {
            out.push(observation(
                self.id(),
                self.severity(),
                format!("TLS connection ({normal})"),
                "The connection uses a TLS version that is deprecated and has known attacks.",
                "Allow TLS 1.2 and 1.3 only.",
            ));
        }
    }
}

struct TlsCertExpired;
impl ResponseCheck for TlsCertExpired {
    fn id(&self) -> &'static str {
        "sec/tls-cert-expired"
    }
    fn title(&self) -> &'static str {
        "Certificate expired"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        let Some(not_after) = v.tls.and_then(|t| t.not_after) else {
            return;
        };
        let now = jwt::now_secs();
        if not_after <= now {
            out.push(observation(
                self.id(),
                self.severity(),
                "TLS certificate",
                &format!(
                    "The server's certificate expired {} ago.",
                    jwt::span(now - not_after)
                ),
                "Renew the certificate and automate renewal.",
            ));
        }
    }
}

struct TlsCertExpiring;
impl ResponseCheck for TlsCertExpiring {
    fn id(&self) -> &'static str {
        "sec/tls-cert-expiring"
    }
    fn title(&self) -> &'static str {
        "Certificate expires soon"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        let Some(not_after) = v.tls.and_then(|t| t.not_after) else {
            return;
        };
        let left = not_after - jwt::now_secs();
        if left > 0 && left < EXPIRING_SECS {
            out.push(observation(
                self.id(),
                self.severity(),
                "TLS certificate",
                &format!("The server's certificate expires in {}.", jwt::span(left)),
                "Renew the certificate before it runs out and automate renewal.",
            ));
        }
    }
}

struct TlsWeakKey;
impl ResponseCheck for TlsWeakKey {
    fn id(&self) -> &'static str {
        "sec/tls-weak-key"
    }
    fn title(&self) -> &'static str {
        "Weak certificate key"
    }
    fn severity(&self) -> Severity {
        Severity::Warn
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        let Some(tls) = v.tls else {
            return;
        };
        let (Some(algorithm), Some(bits)) = (tls.key_algorithm.as_deref(), tls.key_bits) else {
            return;
        };
        let weak = match algorithm {
            "RSA" => bits < 2048,
            "EC" => bits < 224,
            _ => false,
        };
        if weak {
            out.push(observation(
                self.id(),
                self.severity(),
                format!("TLS certificate ({algorithm} {bits} bit)"),
                "The certificate's key is too short to resist an attacker with modest resources.",
                "Issue a certificate with an RSA key of at least 2048 bits, or an EC key of 256 bits.",
            ));
        }
    }
}

// ---- revealing error pages ----

/// How much of an error page is read for a product and version.
const ERROR_PAGE_LIMIT: usize = 64 * 1024;

const PRODUCTS: &[&str] = &[
    "Apache/",
    "nginx/",
    "Microsoft-IIS/",
    "Tomcat/",
    "Jetty/",
    "lighttpd/",
    "OpenSSL/",
    "PHP/",
    "Phusion Passenger ",
];

struct ErrorPageVersion;
impl ResponseCheck for ErrorPageVersion {
    fn id(&self) -> &'static str {
        "sec/error-page-version"
    }
    fn title(&self) -> &'static str {
        "Software version on an error page"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn run(&self, v: &ResponseView<'_>, out: &mut Vec<Observation>) {
        if v.status < 400 {
            return;
        }
        let body = &v.body[..v.body.len().min(ERROR_PAGE_LIMIT)];
        let revealed = PRODUCTS.iter().any(|product| {
            memmem::find_iter(body, product.as_bytes())
                .any(|at| body.get(at + product.len()).is_some_and(u8::is_ascii_digit))
        });
        if revealed {
            out.push(observation(
                self.id(),
                self.severity(),
                "body",
                "The error page names the server software and its version, which tells an attacker which known flaws to try.",
                "Use a generic error page and turn off the version banner (e.g. server_tokens off, ServerTokens Prod).",
            ));
        }
    }
}
