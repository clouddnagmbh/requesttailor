// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand R: the passive security checks (acceptance point "API-Sicherheitsprüfung",
//! passive part). Every rule has a fixture that matches and one that does not; the rules read
//! only the response in hand (no extra request: they are pure functions); each finding carries
//! an OWASP API Security Top 10 (2023) category; a token value, a weak secret or any claim
//! never reaches an observation or a finding (canary). Synthetic data only.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use rt_app::ResponseStore;
use rt_app::inspect::{self, Observation, ResponseView};
use rt_app::security::{self, OwaspCategory, SecurityFinding, SecuritySettings, Severity};
use rt_core::jwt::now_secs;
use rt_core::send::{REPORT_VERSION, ReceivedResponse, ReportBody, SendReport, SentRequest};
use rt_core::transport::{HttpVersion, Timing, TlsInfo};
use serde_json::json;

const CANARY: &str = "RT-CANARY-security-sub";
/// A guessable shared secret; it must never be named in any output.
const WEAK_SECRET: &str = "changeme";
const STRONG_SECRET: &str = "q7Vn3-very-long-and-random-shared-secret-9fK2xLw0";
const DAY: i64 = 86_400;

fn token_signed(alg: &str, secret: &str, claims: serde_json::Value) -> String {
    let part = |v: &serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
    let input = format!(
        "{}.{}",
        part(&json!({"alg": alg, "typ": "JWT"})),
        part(&claims)
    );
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(input.as_bytes());
    format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

/// A token signed with RS256 in name only (the checks never verify).
fn token_rs(claims: serde_json::Value) -> String {
    let part = |v: &serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
    format!(
        "{}.{}.{}",
        part(&json!({"alg": "RS256"})),
        part(&claims),
        URL_SAFE_NO_PAD.encode("signature")
    )
}

fn with_exp(exp: i64) -> serde_json::Value {
    json!({"sub": CANARY, "exp": exp})
}

struct Reply {
    url: &'static str,
    status: u16,
    headers: Vec<(&'static str, String)>,
    request_headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
    tls: Option<TlsInfo>,
}

fn reply() -> Reply {
    Reply {
        url: "https://api.example.com/items",
        status: 200,
        headers: vec![
            ("Content-Type", "application/json".into()),
            ("Strict-Transport-Security", "max-age=31536000".into()),
            ("X-Content-Type-Options", "nosniff".into()),
            ("RateLimit-Limit", "100".into()),
        ],
        request_headers: Vec::new(),
        body: b"{\"ok\":true}".to_vec(),
        tls: None,
    }
}

impl Reply {
    fn header(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_owned()));
        self
    }
    fn without(mut self, name: &str) -> Self {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self
    }
    fn sent(mut self, name: &'static str, value: &str) -> Self {
        self.request_headers.push((name, value.to_owned()));
        self
    }
    fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
    fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
    fn tls(mut self, tls: TlsInfo) -> Self {
        self.tls = Some(tls);
        self
    }
    fn html(self) -> Self {
        self.without("content-type")
            .header("Content-Type", "text/html; charset=utf-8")
    }
    fn token_body(self, token: &str) -> Self {
        self.body(format!("{{\"access_token\":\"{token}\"}}"))
    }

    fn report(&self) -> SendReport {
        let owned = |h: &[(&'static str, String)]| -> Vec<(String, String)> {
            h.iter()
                .map(|(n, v)| ((*n).to_owned(), v.clone()))
                .collect()
        };
        let mut report: SendReport = serde_json::from_value(json!({
            "version": REPORT_VERSION, "request_path": "a.yml", "environment": null,
            "request": null, "response": null, "variables": [], "unresolved": [], "error": null
        }))
        .unwrap();
        report.request = Some(SentRequest {
            method: "GET".into(),
            url: self.url.into(),
            headers: owned(&self.request_headers),
            body_size: 0,
        });
        report.response = Some(ReceivedResponse {
            tls: self.tls.clone(),
            status: self.status,
            version: HttpVersion::Http11,
            headers: owned(&self.headers),
            url: self.url.into(),
            redirects: vec![],
            size: self.body.len() as u64,
            body: ReportBody::Utf8(String::from_utf8_lossy(&self.body).into_owned()),
            truncated: false,
            timing: Timing {
                dns: None,
                connect: None,
                tls: None,
                ttfb: Duration::from_millis(1),
            },
            total: Duration::from_millis(2),
        });
        report
    }

    /// What the apps show: through the store (desktop, web) ...
    fn checks(&self) -> Vec<Observation> {
        let mut store = ResponseStore::new();
        let stored = store.insert(self.report());
        inspect::checks_of(&store, stored.response_id.unwrap(), &[]).unwrap()
    }

    /// ... and what `rt send --checks` shows.
    fn report_checks(&self) -> Vec<Observation> {
        inspect::check_report(&self.report(), &[])
    }

    fn findings(&self) -> Vec<SecurityFinding> {
        security::passive(&self.report(), &SecuritySettings::default())
    }
}

fn hits(reply: &Reply, id: &str) -> bool {
    let store = reply.checks().iter().any(|o| o.id == id);
    let report = reply.report_checks().iter().any(|o| o.id == id);
    assert_eq!(store, report, "{id}: store and report agree");
    store
}

fn found(reply: &Reply, rule: &str) -> Option<SecurityFinding> {
    reply.findings().into_iter().find(|f| f.rule == rule)
}

fn tls(protocol: &str, not_after: i64, key: (&str, u32)) -> TlsInfo {
    TlsInfo {
        protocol: Some(protocol.into()),
        cipher: Some("TLS13_AES_128_GCM_SHA256".into()),
        not_before: Some(now_secs() - 100 * DAY),
        not_after: Some(not_after),
        key_algorithm: Some(key.0.into()),
        key_bits: Some(key.1),
        names: Vec::new(),
    }
}

fn good_tls() -> TlsInfo {
    tls("TLSv1.3", now_secs() + 200 * DAY, ("EC", 256))
}

// ---- the base line ----

#[test]
fn a_clean_response_with_good_tls_has_no_observations_and_no_findings() {
    let r = reply().tls(good_tls());
    assert_eq!(r.checks(), Vec::new());
    assert_eq!(r.findings(), Vec::new());
}

// ---- open CORS ----

#[test]
fn a_wildcard_with_credentials_is_open_cors_in_owasp_api8() {
    let r = reply()
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Credentials", "true");
    assert!(hits(&r, "chk/cors-credentials"));
    let f = found(&r, "chk/cors-credentials").expect("a finding");
    assert_eq!(f.category, OwaspCategory::Api8);
    assert!(
        f.location.contains("access-control-allow-origin")
            || f.location.contains("Access-Control-Allow-Origin")
    );
    // A wildcard without credentials is the normal public API.
    let public = reply().header("Access-Control-Allow-Origin", "*");
    assert!(!hits(&public, "chk/cors-credentials"));
    assert_eq!(found(&public, "chk/cors-credentials"), None);
}

#[test]
fn a_reflected_origin_is_noted_even_without_credentials() {
    let r = reply()
        .sent("Origin", "https://evil.example")
        .header("Access-Control-Allow-Origin", "https://evil.example");
    assert!(hits(&r, "sec/cors-reflected-origin"));
    assert_eq!(
        found(&r, "sec/cors-reflected-origin").unwrap().category,
        OwaspCategory::Api8
    );
    // With credentials the wave 18 rule speaks; the new one stays quiet (one hint, not two).
    let with = r.header("Access-Control-Allow-Credentials", "true");
    assert!(hits(&with, "chk/cors-credentials"));
    assert!(!hits(&with, "sec/cors-reflected-origin"));
    // A fixed allowed origin is not reflected.
    let fixed = reply()
        .sent("Origin", "https://evil.example")
        .header("Access-Control-Allow-Origin", "https://app.example.com");
    assert!(!hits(&fixed, "sec/cors-reflected-origin"));
    // No Origin sent: nothing to reflect.
    assert!(!hits(
        &reply().header("Access-Control-Allow-Origin", "https://app.example.com"),
        "sec/cors-reflected-origin"
    ));
}

// ---- weak tokens ----

#[test]
fn a_token_with_alg_none_is_a_weak_token() {
    let part = |v: serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
    let none = format!(
        "{}.{}.",
        part(json!({"alg": "none"})),
        part(json!({"sub": CANARY}))
    );
    let r = reply()
        .token_body(&none)
        .header("Cache-Control", "no-store");
    assert!(hits(&r, "chk/jwt-alg-none"));
    assert_eq!(
        found(&r, "chk/jwt-alg-none").unwrap().category,
        OwaspCategory::Api2
    );
    let signed = reply()
        .token_body(&token_rs(with_exp(now_secs() + DAY)))
        .header("Cache-Control", "no-store");
    assert!(!hits(&signed, "chk/jwt-alg-none"));
}

#[test]
fn a_signed_token_without_exp_is_noted() {
    let no_exp = token_rs(json!({"sub": CANARY}));
    let r = reply()
        .token_body(&no_exp)
        .header("Cache-Control", "no-store");
    assert!(hits(&r, "sec/jwt-no-exp"));
    let f = found(&r, "sec/jwt-no-exp").unwrap();
    assert_eq!(f.category, OwaspCategory::Api2);
    assert!(f.location.contains("access_token"), "{}", f.location);
    let with = reply()
        .token_body(&token_rs(with_exp(now_secs() + DAY)))
        .header("Cache-Control", "no-store");
    assert!(!hits(&with, "sec/jwt-no-exp"));
    // No token at all.
    assert!(!hits(&reply(), "sec/jwt-no-exp"));
}

#[test]
fn a_token_without_exp_in_a_cookie_or_the_bearer_header_is_noted_too() {
    let no_exp = token_rs(json!({"sub": CANARY}));
    let cookie = reply().header(
        "Set-Cookie",
        &format!("session={no_exp}; Secure; HttpOnly; SameSite=Lax"),
    );
    assert!(hits(&cookie, "sec/jwt-no-exp"));
    let bearer = reply().sent("Authorization", &format!("Bearer {no_exp}"));
    assert!(hits(&bearer, "sec/jwt-no-exp"));
}

#[test]
fn an_expired_token_in_use_is_noted_with_the_outcome() {
    let expired = token_rs(with_exp(now_secs() - 3 * DAY));
    let accepted = reply().sent("Authorization", &format!("Bearer {expired}"));
    assert!(hits(&accepted, "sec/jwt-expired-in-use"));
    let f = found(&accepted, "sec/jwt-expired-in-use").unwrap();
    assert_eq!(f.category, OwaspCategory::Api2);
    assert!(
        f.why.contains("accepted"),
        "the server answered 200: {}",
        f.why
    );
    let refused = reply()
        .status(401)
        .sent("Authorization", &format!("Bearer {expired}"));
    let f = found(&refused, "sec/jwt-expired-in-use").unwrap();
    assert!(!f.why.contains("accepted"), "{}", f.why);
    let fresh = reply().sent(
        "Authorization",
        &format!("Bearer {}", token_rs(with_exp(now_secs() + DAY))),
    );
    assert!(!hits(&fresh, "sec/jwt-expired-in-use"));
    // An expired token that only comes back in a response is not "in use".
    assert!(!hits(
        &reply()
            .token_body(&expired)
            .header("Cache-Control", "no-store"),
        "sec/jwt-expired-in-use"
    ));
    // A masked header (a secret) is not a token and not an observation.
    assert!(!hits(
        &reply().sent("Authorization", "Bearer ••••••"),
        "sec/jwt-expired-in-use"
    ));
}

#[test]
fn a_guessable_hs256_secret_is_a_hint_without_the_value() {
    let weak = token_signed("HS256", WEAK_SECRET, with_exp(now_secs() + DAY));
    let r = reply()
        .token_body(&weak)
        .header("Cache-Control", "no-store");
    assert!(hits(&r, "sec/jwt-weak-hmac"));
    let f = found(&r, "sec/jwt-weak-hmac").unwrap();
    assert_eq!(f.category, OwaspCategory::Api2);
    let text = serde_json::to_string(&(r.checks(), r.findings())).unwrap();
    assert!(
        !text.contains(WEAK_SECRET),
        "the secret is never named: {text}"
    );
    let strong = token_signed("HS256", STRONG_SECRET, with_exp(now_secs() + DAY));
    assert!(!hits(
        &reply()
            .token_body(&strong)
            .header("Cache-Control", "no-store"),
        "sec/jwt-weak-hmac"
    ));
    // An RS256 token is never tested for a shared secret.
    assert!(!hits(
        &reply()
            .token_body(&token_rs(with_exp(now_secs() + DAY)))
            .header("Cache-Control", "no-store"),
        "sec/jwt-weak-hmac"
    ));
}

// ---- security headers ----

#[test]
fn the_missing_headers_map_to_api8() {
    let r = reply()
        .without("strict-transport-security")
        .without("x-content-type-options");
    assert_eq!(
        found(&r, "chk/hsts-missing").unwrap().category,
        OwaspCategory::Api8
    );
    assert_eq!(
        found(&r, "chk/nosniff-missing").unwrap().category,
        OwaspCategory::Api8
    );
    assert_eq!(found(&reply(), "chk/hsts-missing"), None);
    assert_eq!(found(&reply(), "chk/nosniff-missing"), None);
}

#[test]
fn a_response_with_a_token_needs_no_store() {
    let token = token_rs(with_exp(now_secs() + DAY));
    let r = reply().token_body(&token);
    assert!(hits(&r, "sec/cache-control-tokens"));
    assert_eq!(
        found(&r, "sec/cache-control-tokens").unwrap().category,
        OwaspCategory::Api8
    );
    assert!(!hits(
        &reply()
            .token_body(&token)
            .header("Cache-Control", "no-store, max-age=0"),
        "sec/cache-control-tokens"
    ));
    // `public` caching of a token is no better than none.
    assert!(hits(
        &reply()
            .token_body(&token)
            .header("Cache-Control", "public, max-age=600"),
        "sec/cache-control-tokens"
    ));
    // No token, no rule.
    assert!(!hits(&reply(), "sec/cache-control-tokens"));
    // A refusal carries no token worth caching.
    assert!(!hits(
        &reply().status(401).token_body(&token),
        "sec/cache-control-tokens"
    ));
}

// ---- rate limiting ----

#[test]
fn no_rate_limit_headers_is_a_calm_hint() {
    let r = reply().without("ratelimit-limit");
    assert!(hits(&r, "sec/no-rate-limit"));
    let f = found(&r, "sec/no-rate-limit").unwrap();
    assert_eq!(f.category, OwaspCategory::Api4);
    assert_eq!(f.severity, Severity::Low);
    for header in [
        ("RateLimit-Limit", "10"),
        ("X-RateLimit-Remaining", "9"),
        ("RateLimit", "limit=10, remaining=9, reset=5"),
        ("X-Rate-Limit-Limit", "10"),
    ] {
        assert!(
            !hits(
                &reply()
                    .without("ratelimit-limit")
                    .header(header.0, header.1),
                "sec/no-rate-limit"
            ),
            "{header:?}"
        );
    }
    // Pages, refusals and answers without a representation are not API traffic to rate.
    assert!(!hits(&r.html(), "sec/no-rate-limit"));
    assert!(!hits(
        &reply().without("ratelimit-limit").status(500),
        "sec/no-rate-limit"
    ));
    assert!(!hits(
        &reply().without("ratelimit-limit").status(204),
        "sec/no-rate-limit"
    ));
}

// ---- TLS ----

#[test]
fn an_old_protocol_is_noted() {
    for old in ["TLSv1.0", "TLSv1.1", "SSLv3"] {
        let r = reply().tls(tls(old, now_secs() + 200 * DAY, ("EC", 256)));
        assert!(hits(&r, "sec/tls-old-protocol"), "{old}");
        assert_eq!(
            found(&r, "sec/tls-old-protocol").unwrap().category,
            OwaspCategory::Api8
        );
    }
    for ok in ["TLSv1.2", "TLSv1.3"] {
        assert!(
            !hits(
                &reply().tls(tls(ok, now_secs() + 200 * DAY, ("EC", 256))),
                "sec/tls-old-protocol"
            ),
            "{ok}"
        );
    }
}

#[test]
fn an_expired_certificate_and_one_that_expires_soon_are_two_rules() {
    let expired = reply().tls(tls("TLSv1.3", now_secs() - DAY, ("EC", 256)));
    assert!(hits(&expired, "sec/tls-cert-expired"));
    assert!(!hits(&expired, "sec/tls-cert-expiring"));
    let soon = reply().tls(tls("TLSv1.3", now_secs() + 10 * DAY, ("EC", 256)));
    assert!(hits(&soon, "sec/tls-cert-expiring"));
    assert!(!hits(&soon, "sec/tls-cert-expired"));
    let fine = reply().tls(tls("TLSv1.3", now_secs() + 90 * DAY, ("EC", 256)));
    assert!(!hits(&fine, "sec/tls-cert-expired"));
    assert!(!hits(&fine, "sec/tls-cert-expiring"));
    let f = found(&expired, "sec/tls-cert-expired").unwrap();
    assert_eq!(f.category, OwaspCategory::Api8);
    assert!(f.severity >= Severity::High);
}

#[test]
fn a_weak_key_is_noted() {
    for (algorithm, bits) in [("RSA", 1024), ("RSA", 512), ("EC", 192)] {
        let r = reply().tls(tls("TLSv1.3", now_secs() + 200 * DAY, (algorithm, bits)));
        assert!(hits(&r, "sec/tls-weak-key"), "{algorithm} {bits}");
    }
    for (algorithm, bits) in [
        ("RSA", 2048),
        ("RSA", 4096),
        ("EC", 256),
        ("EC", 384),
        ("Ed25519", 256),
    ] {
        let r = reply().tls(tls("TLSv1.3", now_secs() + 200 * DAY, (algorithm, bits)));
        assert!(!hits(&r, "sec/tls-weak-key"), "{algorithm} {bits}");
    }
}

#[test]
fn without_tls_details_there_is_nothing_to_say_about_tls() {
    // The browser's fetch and plain http reveal nothing; that is not a finding.
    let none = reply();
    let all = none.checks();
    assert!(!all.iter().any(|o| o.id.starts_with("sec/tls-")), "{all:?}");
    let rules = security::rules();
    let tls_rules: Vec<_> = rules
        .iter()
        .filter(|r| r.id.starts_with("sec/tls-"))
        .collect();
    assert_eq!(tls_rules.len(), 4);
    assert!(tls_rules.iter().all(|r| r.native_only));
    assert!(
        rules
            .iter()
            .filter(|r| !r.id.starts_with("sec/tls-"))
            .all(|r| !r.native_only)
    );
}

// ---- revealing error pages ----

#[test]
fn a_stack_trace_and_a_server_version_are_api8() {
    let trace = reply()
        .status(500)
        .body("Traceback (most recent call last):\n  File \"app.py\", line 3, in view\n");
    assert!(hits(&trace, "chk/stack-trace"));
    assert_eq!(
        found(&trace, "chk/stack-trace").unwrap().category,
        OwaspCategory::Api8
    );
    let version = reply().header("Server", "nginx/1.18.0");
    assert!(hits(&version, "chk/version-disclosure"));
    assert_eq!(
        found(&version, "chk/version-disclosure").unwrap().category,
        OwaspCategory::Api8
    );
    assert_eq!(
        found(
            &reply().status(500).body("{\"error\":\"failed\"}"),
            "chk/stack-trace"
        ),
        None
    );
}

#[test]
fn a_version_in_the_text_of_an_error_page_is_noted() {
    let page =
        "<h1>Not Found</h1><address>Apache/2.4.41 (Ubuntu) Server at example.com Port 80</address>";
    let r = reply().html().status(404).body(page);
    assert!(hits(&r, "sec/error-page-version"));
    assert_eq!(
        found(&r, "sec/error-page-version").unwrap().category,
        OwaspCategory::Api8
    );
    // The same words on a successful page are not an error page.
    assert!(!hits(&reply().html().body(page), "sec/error-page-version"));
    // An error page without a product and version.
    assert!(!hits(
        &reply().html().status(404).body("<h1>Not Found</h1>"),
        "sec/error-page-version"
    ));
    assert!(!hits(
        &reply().status(404).body("{\"error\":\"nginx is great\"}"),
        "sec/error-page-version"
    ));
    let iis = reply()
        .html()
        .status(500)
        .body("Microsoft-IIS/10.0 Detailed Error");
    assert!(hits(&iis, "sec/error-page-version"));
}

// ---- the rule list, categories, switching off ----

#[test]
fn every_rule_has_an_id_a_title_and_an_owasp_category_and_the_ids_are_stable() {
    let rules = security::rules();
    let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "chk/cors-credentials",
            "sec/cors-reflected-origin",
            "chk/jwt-alg-none",
            "sec/jwt-no-exp",
            "sec/jwt-expired-in-use",
            "sec/jwt-weak-hmac",
            "chk/hsts-missing",
            "chk/nosniff-missing",
            "sec/cache-control-tokens",
            "sec/no-rate-limit",
            "sec/tls-old-protocol",
            "sec/tls-cert-expired",
            "sec/tls-cert-expiring",
            "sec/tls-weak-key",
            "chk/stack-trace",
            "chk/version-disclosure",
            "sec/error-page-version",
        ]
    );
    for r in &rules {
        assert!(!r.title.is_empty(), "{}", r.id);
    }
    // Every rule that is not a wave 18 rule is one of the checks the apps run.
    let run: Vec<&str> = security::checks().iter().map(|c| c.id()).collect();
    for r in rules.iter().filter(|r| r.id.starts_with("sec/")) {
        assert!(run.contains(&r.id.as_str()), "{}", r.id);
    }
    // The ids of the checks are known to the switch-off lists.
    let all: Vec<String> = inspect::all_rules().into_iter().map(|r| r.id).collect();
    for r in &rules {
        assert!(all.contains(&r.id), "{}", r.id);
    }
}

#[test]
fn rules_switch_off_by_id_and_by_the_security_settings() {
    let r = reply().without("ratelimit-limit");
    let mut store = ResponseStore::new();
    let id = store.insert(r.report()).response_id.unwrap();
    let off = inspect::checks_of(&store, id, &["sec/no-rate-limit".to_owned()]).unwrap();
    assert!(!off.iter().any(|o| o.id == "sec/no-rate-limit"));
    let on = inspect::checks_of(&store, id, &[]).unwrap();
    assert!(on.iter().any(|o| o.id == "sec/no-rate-limit"));

    // The device settings: everything off, or single rules.
    let all_off = SecuritySettings {
        enabled: false,
        disabled: Vec::new(),
    };
    assert!(security::passive(&r.report(), &all_off).is_empty());
    let some = SecuritySettings {
        enabled: true,
        disabled: vec!["sec/no-rate-limit".into()],
    };
    assert!(security::passive(&r.report(), &some).is_empty());
    assert!(!security::ignored_by(&all_off).is_empty());
    assert!(security::ignored_by(&SecuritySettings::default()).is_empty());
    assert_eq!(security::ignored_by(&some), ["sec/no-rate-limit"]);
    // Through the stored response, as the desktop and the web shell ask.
    let found =
        security::passive_of(&store, id, &security::ignored_by(&all_off), &all_off).unwrap();
    assert!(found.is_empty());
    let found = security::passive_of(&store, id, &[], &SecuritySettings::default()).unwrap();
    assert!(found.iter().any(|f| f.rule == "sec/no-rate-limit"));
}

#[test]
fn the_settings_file_round_trips_and_a_broken_one_means_the_defaults() {
    let dir = std::env::temp_dir().join(format!("rt-security-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        security::read_settings(Some(&dir)),
        SecuritySettings::default()
    );
    let settings = SecuritySettings {
        enabled: true,
        disabled: vec!["sec/tls-weak-key".into()],
    };
    security::write_settings(Some(&dir), &settings).unwrap();
    assert_eq!(security::read_settings(Some(&dir)), settings);
    std::fs::write(dir.join(security::SETTINGS_FILE), "nonsense").unwrap();
    assert_eq!(
        security::read_settings(Some(&dir)),
        SecuritySettings::default()
    );
    assert!(security::write_settings(None, &settings).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- no value, ever (canary) ----

#[test]
fn no_token_value_secret_or_claim_reaches_any_output() {
    let weak = token_signed("HS256", WEAK_SECRET, json!({"sub": CANARY}));
    let expired = token_rs(with_exp(now_secs() - 3 * DAY));
    let r = reply()
        .without("ratelimit-limit")
        .status(500)
        .sent("Authorization", &format!("Bearer {expired}"))
        .sent("Origin", "https://evil.example")
        .header("Access-Control-Allow-Origin", "https://evil.example")
        .header("Set-Cookie", &format!("session={weak}"))
        .header("Server", "nginx/1.18.0")
        .tls(tls("TLSv1.0", now_secs() - DAY, ("RSA", 1024)))
        .body(format!(
            "Traceback (most recent call last):\n{{\"access_token\":\"{weak}\",\"sub\":\"{CANARY}\"}}\nApache/2.4.41"
        ));
    let observations = [r.checks(), r.report_checks()].concat();
    let findings = r.findings();
    assert!(observations.len() >= 8, "many rules fire: {observations:?}");
    let all = serde_json::to_string(&(&observations, &findings)).unwrap();
    for needle in [CANARY, WEAK_SECRET, &weak, &expired, "eyJ"] {
        assert!(!all.contains(needle), "{needle} appears in {all}");
    }
}

// ---- pure: no extra request, bounded work ----

#[test]
fn the_rules_read_at_most_the_first_mib_and_a_huge_body_costs_the_same() {
    let token = token_signed("HS256", STRONG_SECRET, with_exp(now_secs() + DAY));
    let mut big = format!("{{\"access_token\":\"{token}\",\"pad\":\"").into_bytes();
    big.extend(std::iter::repeat_n(b'x', 4 * 1024 * 1024));
    big.extend(b"\"}");
    let r = reply().body(big);
    let started = std::time::Instant::now();
    let _ = r.report_checks();
    // Generous: the point is that it is not proportional to 4 MiB of parsing and copying.
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_view_can_carry_tls_details_and_the_view_is_all_the_rules_see() {
    let info = tls("TLSv1.0", now_secs() + DAY, ("EC", 256));
    let headers: Vec<(String, String)> = vec![("content-type".into(), "application/json".into())];
    let view =
        ResponseView::new("https://x.example/", 200, &[], &headers, b"{}", 2).with_tls(Some(&info));
    let found = inspect::check_all(&view, &[]);
    assert!(
        found.iter().any(|o| o.id == "sec/tls-old-protocol"),
        "{found:?}"
    );
    let plain = ResponseView::new("https://x.example/", 200, &[], &headers, b"{}", 2);
    assert!(
        !inspect::check_all(&plain, &[])
            .iter()
            .any(|o| o.id.starts_with("sec/tls"))
    );
}
