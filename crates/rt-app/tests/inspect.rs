// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand B: the JWT inspector and the passive response checks (acceptance points 2
//! and 3). Every rule has a fixture that matches and one that does not; the checks read at most
//! 1 MiB of a body; ignoring works per device and per collection; no token value reaches a
//! note, an observation, a report or a file. Synthetic data only.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rt_app::inspect::{
    self, BODY_LIMIT, ClaimKind, JwtWarningKind, Observation, Severity, TokenLocation,
};
use rt_app::{ResponseStore, SendInput, Workspace};
use rt_core::send::{REPORT_VERSION, ReceivedResponse, ReportBody, SendReport, SentRequest};
use rt_core::transport::{HttpVersion, Timing};
use rt_format::{COLLECTION_FILE, FileSet};
use serde_json::json;

const CANARY: &str = "RT-CANARY-inspect-sub";

fn token(header: serde_json::Value, claims: serde_json::Value) -> String {
    let part = |v: &serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
    format!(
        "{}.{}.{}",
        part(&header),
        part(&claims),
        URL_SAFE_NO_PAD.encode("signature")
    )
}

fn unsigned_token() -> String {
    format!(
        "{}.{}.",
        URL_SAFE_NO_PAD.encode(json!({"alg": "none"}).to_string()),
        URL_SAFE_NO_PAD.encode(json!({"sub": CANARY}).to_string())
    )
}

/// What one response looks like to the checks.
struct Reply {
    url: &'static str,
    status: u16,
    headers: Vec<(&'static str, String)>,
    request_headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

fn reply() -> Reply {
    Reply {
        url: "https://api.example.com/items",
        status: 200,
        headers: vec![
            ("Content-Type", "application/json".into()),
            ("Strict-Transport-Security", "max-age=31536000".into()),
            ("X-Content-Type-Options", "nosniff".into()),
        ],
        request_headers: Vec::new(),
        body: b"{\"ok\":true}".to_vec(),
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
    fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
    fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
    fn url(mut self, url: &'static str) -> Self {
        self.url = url;
        self
    }
    fn html(self) -> Self {
        self.without("content-type")
            .header("Content-Type", "text/html; charset=utf-8")
    }

    fn report(&self) -> SendReport {
        let owned = |h: &[(&'static str, String)]| -> Vec<(String, String)> {
            h.iter()
                .map(|(n, v)| ((*n).to_owned(), v.clone()))
                .collect()
        };
        let body = match String::from_utf8(self.body.clone()) {
            Ok(s) => ReportBody::Utf8(s),
            Err(e) => {
                ReportBody::Base64(base64::engine::general_purpose::STANDARD.encode(e.into_bytes()))
            }
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
            tls: None,
            status: self.status,
            version: HttpVersion::Http11,
            headers: owned(&self.headers),
            url: self.url.into(),
            redirects: vec![],
            size: self.body.len() as u64,
            body,
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

    /// Through the store, as the apps see a response.
    fn checks(&self) -> Vec<Observation> {
        self.checks_ignoring(&[])
    }

    fn checks_ignoring(&self, ignore: &[String]) -> Vec<Observation> {
        let mut store = ResponseStore::new();
        let stored = store.insert(self.report());
        inspect::checks_of(&store, stored.response_id.unwrap(), ignore).unwrap()
    }
}

fn ids(observations: &[Observation]) -> Vec<&str> {
    observations.iter().map(|o| o.id.as_str()).collect()
}

fn hits(reply: &Reply, id: &str) -> bool {
    let through_store = reply.checks().iter().any(|o| o.id == id);
    let through_report = inspect::check_report(&reply.report(), &[])
        .iter()
        .any(|o| o.id == id);
    assert_eq!(
        through_store, through_report,
        "{id}: store and report agree"
    );
    through_store
}

#[test]
fn a_clean_json_response_has_no_observations() {
    assert_eq!(ids(&reply().checks()), Vec::<&str>::new());
}

// ---- the nine rules ----

#[test]
fn hsts_is_missing_only_over_https() {
    assert!(hits(
        &reply().without("strict-transport-security"),
        "chk/hsts-missing"
    ));
    assert!(!hits(&reply(), "chk/hsts-missing"));
    assert!(!hits(
        &reply()
            .without("strict-transport-security")
            .url("http://127.0.0.1/items"),
        "chk/hsts-missing"
    ));
    assert!(!hits(
        &reply().without("strict-transport-security").status(304),
        "chk/hsts-missing"
    ));
}

#[test]
fn nosniff_must_be_nosniff() {
    assert!(hits(
        &reply().without("x-content-type-options"),
        "chk/nosniff-missing"
    ));
    assert!(hits(
        &reply()
            .without("x-content-type-options")
            .header("X-Content-Type-Options", "other"),
        "chk/nosniff-missing"
    ));
    assert!(!hits(&reply(), "chk/nosniff-missing"));
}

#[test]
fn csp_is_asked_of_html_only() {
    assert!(hits(&reply().html(), "chk/csp-missing"));
    assert!(!hits(
        &reply()
            .html()
            .header("Content-Security-Policy", "default-src 'self'"),
        "chk/csp-missing"
    ));
    assert!(!hits(&reply(), "chk/csp-missing"));
}

#[test]
fn frame_protection_is_x_frame_options_or_frame_ancestors() {
    assert!(hits(&reply().html(), "chk/frame-protection-missing"));
    assert!(!hits(
        &reply().html().header("X-Frame-Options", "DENY"),
        "chk/frame-protection-missing"
    ));
    assert!(!hits(
        &reply().html().header(
            "Content-Security-Policy",
            "default-src 'self'; frame-ancestors 'none'"
        ),
        "chk/frame-protection-missing"
    ));
    assert!(hits(
        &reply()
            .html()
            .header("Content-Security-Policy", "default-src 'self'"),
        "chk/frame-protection-missing"
    ));
    assert!(!hits(&reply(), "chk/frame-protection-missing"));
}

#[test]
fn cookies_need_secure_httponly_and_samesite() {
    let bad = reply().header("Set-Cookie", "sid=abc; Path=/");
    let found = bad.checks();
    let cookie = found.iter().find(|o| o.id == "chk/cookie-flags").unwrap();
    assert_eq!(cookie.severity, Severity::Warn);
    assert_eq!(cookie.location, "header Set-Cookie (sid)");
    assert!(cookie.why.contains("Secure, HttpOnly, SameSite"));
    assert!(
        !cookie.why.contains("abc"),
        "the cookie value is never quoted"
    );

    let good = reply().header(
        "Set-Cookie",
        "sid=abc; Secure; HttpOnly; SameSite=Lax; Path=/",
    );
    assert!(!hits(&good, "chk/cookie-flags"));
    // Flags are case-insensitive; a partly set cookie names only what is missing.
    let partly = reply().header("Set-Cookie", "a=1; secure; HTTPONLY");
    let found = partly.checks();
    let cookie = found.iter().find(|o| o.id == "chk/cookie-flags").unwrap();
    assert!(cookie.why.contains("lacks SameSite,"), "{}", cookie.why);
    // One observation per cookie.
    let two = reply()
        .header("Set-Cookie", "a=1")
        .header("Set-Cookie", "b=2");
    assert_eq!(
        two.checks()
            .iter()
            .filter(|o| o.id == "chk/cookie-flags")
            .count(),
        2
    );
    // Over plain http a cookie cannot be Secure: only the other two are asked for.
    let plain = reply()
        .url("http://127.0.0.1/")
        .header("Set-Cookie", "a=1; HttpOnly; SameSite=Lax");
    assert!(!hits(&plain, "chk/cookie-flags"));
}

#[test]
fn cors_star_with_credentials_and_a_reflected_origin_with_credentials() {
    let star = reply()
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Credentials", "true");
    assert!(hits(&star, "chk/cors-credentials"));
    assert!(!hits(
        &reply().header("Access-Control-Allow-Origin", "*"),
        "chk/cors-credentials"
    ));
    assert!(!hits(
        &reply()
            .header("Access-Control-Allow-Origin", "https://app.example.com")
            .header("Access-Control-Allow-Credentials", "true"),
        "chk/cors-credentials"
    ));
    let mut reflected = reply()
        .header("Access-Control-Allow-Origin", "https://evil.example")
        .header("Access-Control-Allow-Credentials", "true");
    reflected
        .request_headers
        .push(("Origin", "https://evil.example".into()));
    assert!(hits(&reflected, "chk/cors-credentials"));
    assert!(hits(
        &reply()
            .header("Access-Control-Allow-Origin", "null")
            .header("Access-Control-Allow-Credentials", "true"),
        "chk/cors-credentials"
    ));
}

#[test]
fn versions_in_server_headers_are_info() {
    let found = reply().header("Server", "nginx/1.18.0").checks();
    let o = found
        .iter()
        .find(|o| o.id == "chk/version-disclosure")
        .unwrap();
    assert_eq!(o.severity, Severity::Info);
    assert_eq!(o.location, "header Server");
    assert!(hits(
        &reply().header("X-Powered-By", "PHP/8.1.2"),
        "chk/version-disclosure"
    ));
    assert!(hits(
        &reply().header("X-AspNet-Version", "4.0.30319"),
        "chk/version-disclosure"
    ));
    assert!(!hits(
        &reply().header("Server", "nginx"),
        "chk/version-disclosure"
    ));
    assert!(!hits(
        &reply().header("X-Powered-By", "Express"),
        "chk/version-disclosure"
    ));
}

#[test]
fn stack_traces_in_error_responses_for_five_languages() {
    let traces: [(&str, &str); 5] = [
        (
            "Java",
            "Exception in thread main\n\tat com.acme.Foo.bar(Foo.java:42)\n",
        ),
        (
            ".NET",
            "Oops\n   at Acme.Foo.Bar() in C:\\src\\Foo.cs:line 12\n",
        ),
        (
            "Python",
            "Traceback (most recent call last):\n  File \"app.py\", line 3, in <module>\n",
        ),
        (
            "Node",
            "Error: x\n    at Object.<anonymous> (/srv/app/index.js:10:5)\n",
        ),
        ("PHP", "Stack trace:\n#0 /var/www/index.php(3): run()\n"),
    ];
    for (language, body) in traces {
        let found = reply().status(500).body(body).checks();
        let o = found
            .iter()
            .find(|o| o.id == "chk/stack-trace")
            .unwrap_or_else(|| panic!("{language}"));
        assert!(o.location.contains(language), "{}", o.location);
        assert_eq!(o.severity, Severity::Warn);
        assert!(
            !o.why.contains("Foo.java") && !o.location.contains("/srv"),
            "no excerpt of the body"
        );
        // The same text in a success answer is not an error page.
        assert!(!hits(&reply().body(body), "chk/stack-trace"), "{language}");
    }
    assert!(!hits(
        &reply().status(500).body("{\"error\":\"internal\"}"),
        "chk/stack-trace"
    ));
    assert!(!hits(
        &reply().status(404).body("at the end of the day"),
        "chk/stack-trace"
    ));
}

#[test]
fn a_token_with_alg_none_in_body_header_or_cookie() {
    let t = unsigned_token();
    assert!(hits(
        &reply().body(format!("{{\"access_token\":\"{t}\"}}")),
        "chk/jwt-alg-none"
    ));
    assert!(hits(&reply().header("X-Token", &t), "chk/jwt-alg-none"));
    let found = reply()
        .header(
            "Set-Cookie",
            &format!("session={t}; Secure; HttpOnly; SameSite=Lax"),
        )
        .checks();
    let o = found.iter().find(|o| o.id == "chk/jwt-alg-none").unwrap();
    assert_eq!(o.location, "header Set-Cookie (session)");
    let signed = token(json!({"alg": "RS256"}), json!({"sub": CANARY}));
    assert!(!hits(
        &reply().body(format!("{{\"access_token\":\"{signed}\"}}")),
        "chk/jwt-alg-none"
    ));
    // A look-alike inside a longer base64 run is not a token.
    assert!(!hits(&reply().body(format!("xx{t}")), "chk/jwt-alg-none"));
}

#[test]
fn every_rule_has_a_fixture_above_and_ids_are_stable() {
    let ids: Vec<String> = inspect::rules().into_iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        [
            "chk/hsts-missing",
            "chk/nosniff-missing",
            "chk/csp-missing",
            "chk/frame-protection-missing",
            "chk/cookie-flags",
            "chk/cors-credentials",
            "chk/version-disclosure",
            "chk/stack-trace",
            "chk/jwt-alg-none",
        ]
    );
    for r in inspect::rules() {
        assert!(!r.title.is_empty());
    }
}

#[test]
fn an_observation_has_a_place_a_why_and_a_fix() {
    let all = reply()
        .html()
        .without("strict-transport-security")
        .without("x-content-type-options")
        .checks();
    assert_eq!(
        ids(&all),
        [
            "chk/hsts-missing",
            "chk/nosniff-missing",
            "chk/csp-missing",
            "chk/frame-protection-missing"
        ]
    );
    for o in &all {
        assert!(!o.location.is_empty() && o.why.ends_with('.') && o.fix.ends_with('.'));
        assert!(o.why.matches(". ").count() == 0, "one sentence: {}", o.why);
    }
}

// ---- limits ----

#[test]
fn only_the_first_mebibyte_of_the_body_is_read() {
    let trace = "\n\tat com.acme.Foo.bar(Foo.java:42)\n";
    let mut early = vec![b' '; 1000];
    early.extend_from_slice(trace.as_bytes());
    early.resize(3 * BODY_LIMIT, b' ');
    assert!(hits(&reply().status(500).body(early), "chk/stack-trace"));
    let mut late = vec![b' '; BODY_LIMIT + 10];
    late.extend_from_slice(trace.as_bytes());
    assert!(!hits(&reply().status(500).body(late), "chk/stack-trace"));
}

#[test]
fn a_big_body_costs_what_one_mebibyte_costs() {
    let line =
        b"{\"id\": 1234567, \"name\": \"some value that is not a trace\", \"eyes\": \"eyJx\"}\n";
    let fill = |n: usize| -> Vec<u8> { line.iter().copied().cycle().take(n).collect() };
    let time = |body: Vec<u8>| {
        let reply = reply().status(500).body(body);
        let mut store = ResponseStore::new();
        let id = store.insert(reply.report()).response_id.unwrap();
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let start = web_time::Instant::now();
            let _ = inspect::checks_of(&store, id, &[]).unwrap();
            best = best.min(start.elapsed());
        }
        best
    };
    let one = time(fill(BODY_LIMIT));
    let big = time(fill(40 * BODY_LIMIT));
    // The same work plus a copy of the first MiB; allow generous noise in a debug build.
    assert!(
        big < one * 4 + Duration::from_millis(50),
        "1 MiB {one:?}, 40 MiB {big:?}"
    );
}

#[test]
fn a_response_without_a_body_or_without_a_response_has_no_observations() {
    let mut store = ResponseStore::new();
    let mut report = reply().report();
    report.response = None;
    let stored = store.insert(report);
    assert!(stored.response_id.is_none());
    assert!(inspect::check_report(&reply().report(), &[]).is_empty());
    let mut none = reply().report();
    none.response = None;
    assert!(inspect::check_report(&none, &[]).is_empty());
    let empty = reply().body(Vec::new());
    assert!(empty.checks().is_empty());
}

#[test]
fn binary_bodies_are_checked_on_their_headers() {
    let r = reply()
        .without("strict-transport-security")
        .body(vec![0xff, 0xfe, 0x00, 0x01]);
    assert!(hits(&r, "chk/hsts-missing"));
}

// ---- ignoring ----

#[test]
fn a_rule_that_is_off_is_not_run() {
    let r = reply()
        .without("strict-transport-security")
        .without("x-content-type-options");
    let off = vec!["chk/hsts-missing".to_owned()];
    assert_eq!(ids(&r.checks_ignoring(&off)), ["chk/nosniff-missing"]);
    assert_eq!(
        ids(&inspect::check_report(&r.report(), &off)),
        ["chk/nosniff-missing"]
    );
}

fn workspace() -> Workspace {
    let mut set = FileSet::default();
    set.files.insert(
        COLLECTION_FILE.to_owned(),
        b"opencollection: \"1.0.0\"\ninfo:\n  name: shop\n".to_vec(),
    );
    set.files.insert(
        "a.yml".to_owned(),
        b"info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"https://api.example.com/a?access_token={{tok}}\"\n  auth: inherit\n"
            .to_vec(),
    );
    Workspace::open(set).unwrap()
}

#[test]
fn a_collection_switches_a_rule_off_in_opencollection_yml() {
    let mut ws = workspace();
    assert!(inspect::collection_ignored(&ws).is_empty());
    inspect::ignore_in_collection(&mut ws, "chk/hsts-missing").unwrap();
    inspect::ignore_in_collection(&mut ws, "chk/hsts-missing").unwrap();
    assert_eq!(inspect::collection_ignored(&ws), ["chk/hsts-missing"]);
    assert_eq!(ws.dirty(), [COLLECTION_FILE]);
    let saved = ws.save().unwrap();
    let text = saved[0].text.clone();
    assert!(
        text.contains("checks:\n      ignore:\n        - chk/hsts-missing"),
        "{text}"
    );
    // Unknown ids are refused, not written.
    assert!(inspect::ignore_in_collection(&mut ws, "chk/nope").is_err());
    assert_eq!(
        inspect::all_ignored(&ws, &["chk/stack-trace".into()]),
        ["chk/hsts-missing", "chk/stack-trace"]
    );
}

// ---- tokens ----

#[test]
fn jwt_gives_header_claims_with_kinds_and_warnings() {
    let now = 1_800_000_000;
    let t = token(
        json!({"alg": "RS256", "typ": "JWT"}),
        json!({"zzz": 1, "exp": now - 720, "iss": "https://idp.example", "sub": CANARY, "scope": "read write", "nbf": now + 60}),
    );
    let info = inspect::jwt_at(&t, now).unwrap();
    assert_eq!(info.algorithm.as_deref(), Some("RS256"));
    assert!(!info.signature_verified);
    assert_eq!(info.expires_at, Some(now - 720));
    let names: Vec<_> = info.claims.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["iss", "sub", "nbf", "exp", "scope", "zzz"]);
    let by = |n: &str| info.claims.iter().find(|c| c.name == n).unwrap();
    assert_eq!(by("exp").kind, ClaimKind::Time);
    assert_eq!(by("scope").kind, ClaimKind::List);
    assert_eq!(by("iss").kind, ClaimKind::Text);
    assert!(by("iss").meaning.is_some() && by("zzz").meaning.is_none());
    let kinds: Vec<_> = info.warnings.iter().map(|w| w.kind).collect();
    assert_eq!(
        kinds,
        [JwtWarningKind::Expired, JwtWarningKind::NotYetValid]
    );
    // A broken token is an error with a reason, not a panic.
    assert!(inspect::jwt("not a token").is_err());
}

#[test]
fn tokens_of_a_resolved_request_from_header_auth_block_and_query() {
    let t = token(
        json!({"alg": "HS256"}),
        json!({"sub": CANARY, "exp": 4_000_000_000_i64}),
    );
    let input = SendInput {
        environment: None,
        vars: vec![("tok".into(), t.clone())],
        secrets: Vec::new(),
    };
    let mut ws = workspace();
    // The query parameter of the fixture request.
    let found = inspect::request_tokens(&ws, "a.yml", input.clone()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].location, TokenLocation::Query);
    assert!(found[0].token.warnings.is_empty());

    let mut spec = ws.request("a.yml").unwrap();
    spec.auth = rt_core::model::Auth::Bearer {
        token: "{{tok}}".into(),
    };
    spec.url = "https://api.example.com/a".into();
    ws.update_request("a.yml", spec.clone()).unwrap();
    let found = inspect::request_tokens(&ws, "a.yml", input.clone()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].location, TokenLocation::Auth);

    spec.auth = rt_core::model::Auth::None;
    spec.headers.push(rt_core::model::KeyValue::new(
        "Authorization",
        "Bearer {{tok}}",
    ));
    ws.update_request("a.yml", spec).unwrap();
    let found = inspect::request_tokens(&ws, "a.yml", input).unwrap();
    assert_eq!(found[0].location, TokenLocation::Authorization);

    // No token, nothing found; an unknown request is an error.
    let none = inspect::request_tokens(&ws, "a.yml", SendInput::default()).unwrap();
    assert!(none.is_empty());
    assert!(inspect::request_tokens(&ws, "missing.yml", SendInput::default()).is_err());
}

#[test]
fn tokens_of_a_response_from_body_fields_and_set_cookie() {
    let id = token(json!({"alg": "RS256"}), json!({"sub": CANARY}));
    let access = token(json!({"alg": "RS256"}), json!({"scp": ["a"]}));
    let body = json!({"data": {"id_token": id, "other": id}, "access_token": access, "token": "not a token"});
    let cookie = token(json!({"alg": "HS256"}), json!({"sub": CANARY}));
    let r = reply().body(body.to_string()).header(
        "Set-Cookie",
        &format!("jwt={cookie}; Secure; HttpOnly; SameSite=Strict"),
    );
    let mut store = ResponseStore::new();
    let stored = store.insert(r.report());
    let found = inspect::response_tokens(&store, stored.response_id.unwrap()).unwrap();
    let places: Vec<_> = found.iter().map(|f| f.location.clone()).collect();
    assert_eq!(
        places,
        [
            TokenLocation::Cookie { name: "jwt".into() },
            TokenLocation::Body {
                path: "$.access_token".into()
            },
            TokenLocation::Body {
                path: "$.data.id_token".into()
            },
        ]
    );
}

// ---- canary ----

#[test]
fn no_token_value_reaches_an_observation_a_note_a_report_or_a_file() {
    let now = 1_800_000_000;
    let expired = token(
        json!({"alg": "none"}),
        json!({"sub": CANARY, "exp": now - 5}),
    );
    // Observations of a response full of the token.
    let r = reply()
        .status(500)
        .body(format!(
            "{{\"access_token\":\"{expired}\"}}\nat com.acme.X.y(X.java:1)"
        ))
        .header("X-Token", &expired)
        .header("Set-Cookie", &format!("s={expired}"));
    let report = r.report();
    let observations = inspect::check_report(&report, &[]);
    assert!(!observations.is_empty());
    let all = serde_json::to_string(&observations).unwrap();
    assert!(!all.contains(CANARY) && !all.contains("eyJ") && !all.contains(&expired));
    // The note before sending, through the core.
    let request = rt_core::transport::HttpRequest {
        method: "GET".into(),
        url: "https://api.example.com/".into(),
        headers: vec![("Authorization".into(), format!("Bearer {expired}"))],
        body: None,
        options: Default::default(),
    };
    let notes = rt_core::inspect::notes_at(&request, now);
    assert_eq!(notes.len(), 1);
    assert!(!serde_json::to_string(&notes).unwrap().contains(CANARY));
    // Looking at a request's tokens writes nothing.
    let ws = workspace();
    let _ = inspect::request_tokens(
        &ws,
        "a.yml",
        SendInput {
            environment: None,
            vars: vec![("tok".into(), expired.clone())],
            secrets: vec![],
        },
    )
    .unwrap();
    assert!(ws.dirty().is_empty());
}

// ---- the budget `response_checks` ----

/// A response of about `size` bytes: JSON lines with the odd token-looking string, an error
/// status so the stack-trace rule reads every line, and headers that trigger several rules.
fn heavy(size: usize) -> Reply {
    let line = format!(
        "{{\"id\": 1234567, \"name\": \"item with a longer name\", \"ref\": \"{}\", \"tags\": [\"a\", \"b\"]}}\n",
        unsigned_token().replace('.', "-")
    );
    let mut body = Vec::with_capacity(size + line.len());
    while body.len() < size {
        body.extend_from_slice(line.as_bytes());
    }
    body.truncate(size);
    reply()
        .status(500)
        .body(body)
        .without("strict-transport-security")
        .header("Server", "nginx/1.18.0")
        .header("Set-Cookie", "sid=abc; Path=/")
        .header("Access-Control-Allow-Origin", "*")
}

fn median_ms(reply: &Reply, runs: usize) -> f64 {
    let mut store = ResponseStore::new();
    let id = store.insert(reply.report()).response_id.unwrap();
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = web_time::Instant::now();
            let found = inspect::checks_of(&store, id, &[]).unwrap();
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            assert!(!found.is_empty());
            ms
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[runs / 2]
}

/// `scripts/perf/checks.mjs` runs this in a release build with `RT_PERF_OUT=<dir>` and reads
/// `<dir>/checks.json`. Limits: 2 ms for 50 KB, 20 ms for 1 MiB (the budget), and a 100 MB
/// body costs no more than a 1 MiB one.
#[test]
fn response_checks_are_measured() {
    const RUNS: usize = 9;
    let small = median_ms(&heavy(50 * 1024), RUNS);
    let one = median_ms(&heavy(BODY_LIMIT), RUNS);
    let huge = median_ms(&heavy(100 * 1024 * 1024), 3);
    println!("response_checks: 50 KB {small:.3} ms, 1 MiB {one:.3} ms, 100 MB {huge:.3} ms");
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({ "response_checks": one, "small": small, "huge": huge });
        std::fs::write(
            std::path::Path::new(&dir).join("checks.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
        // Only a release build is held to the limits.
        assert!(small <= 2.0, "50 KB: {small} ms");
        assert!(one <= 20.0, "1 MiB: {one} ms");
        assert!(
            huge <= one * 1.5 + 2.0,
            "100 MB {huge} ms vs 1 MiB {one} ms"
        );
    }
}
