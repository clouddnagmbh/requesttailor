// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand B: reading a JSON Web Token (no signature check) and the notes before a
//! send. Acceptance point 2: header and claims, deadlines, never a value in a note.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rt_core::inspect::{RequestTokenPlace, notes_at, request_tokens};
use rt_core::jwt::{self, IssueKind, JwtError, MAX_TOKEN_LEN};
use rt_core::transport::HttpRequest;
use serde_json::json;

const NOW: i64 = 1_800_000_000;
/// Marker that must never show up in a note.
const CANARY: &str = "RT-CANARY-jwt-claim";

fn token(header: serde_json::Value, claims: serde_json::Value) -> String {
    let part = |v: &serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
    format!(
        "{}.{}.{}",
        part(&header),
        part(&claims),
        URL_SAFE_NO_PAD.encode("sig")
    )
}

fn with_exp(exp: i64) -> String {
    token(
        json!({"alg": "RS256", "typ": "JWT"}),
        json!({"sub": CANARY, "iss": "https://issuer.example", "exp": exp}),
    )
}

fn request(headers: &[(&str, String)], url: &str) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: url.into(),
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), v.clone()))
            .collect(),
        body: None,
        options: Default::default(),
    }
}

#[test]
fn a_valid_token_gives_header_claims_and_signature_length() {
    let jwt = jwt::parse(&with_exp(NOW + 3600)).unwrap();
    assert_eq!(jwt.algorithm(), Some("RS256"));
    assert_eq!(jwt.claims["iss"], "https://issuer.example");
    assert_eq!(jwt.time_claim("exp"), Some(NOW + 3600));
    assert_eq!(jwt.signature_len, 3);
    assert!(jwt.issues(NOW).is_empty());
}

#[test]
fn a_bearer_prefix_and_padding_are_tolerated() {
    let t = with_exp(NOW + 60);
    assert!(jwt::parse(&format!("Bearer {t}")).is_ok());
    assert!(jwt::parse(&format!("  bearer   {t}  ")).is_ok());
}

#[test]
fn an_expired_token_is_reported_with_the_time_and_not_the_value() {
    let jwt = jwt::parse(&with_exp(NOW - 12 * 60)).unwrap();
    let issues = jwt.issues(NOW);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::Expired);
    assert_eq!(issues[0].text, "token expired 12 min ago (exp)");
}

#[test]
fn a_token_that_is_not_valid_yet_and_one_that_expires_soon() {
    let t = token(
        json!({"alg": "HS256"}),
        json!({"nbf": NOW + 300, "exp": NOW + 10}),
    );
    let kinds: Vec<_> = jwt::parse(&t)
        .unwrap()
        .issues(NOW)
        .iter()
        .map(|i| i.kind)
        .collect();
    assert_eq!(kinds, [IssueKind::ExpiresSoon, IssueKind::NotYetValid]);
}

#[test]
fn alg_none_is_flagged_in_any_spelling_and_an_empty_signature_is_fine() {
    for alg in ["none", "None", "NONE"] {
        let t = format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(json!({"alg": alg}).to_string()),
            URL_SAFE_NO_PAD.encode("{}")
        );
        let jwt = jwt::parse(&t).unwrap();
        assert!(jwt.is_unsigned(), "{alg}");
        assert_eq!(jwt.signature_len, 0);
        assert_eq!(jwt.issues(NOW)[0].kind, IssueKind::AlgorithmNone);
    }
}

#[test]
fn broken_tokens_are_refused_without_panicking() {
    for bad in [
        "",
        "abc",
        "a.b",
        "a.b.c.d",
        "..",
        "eyJhbGciOiJub25lIn0..",
        "!!!.!!!.!!!",
        &format!(
            "{}.{}.x",
            URL_SAFE_NO_PAD.encode("not json"),
            URL_SAFE_NO_PAD.encode("{}")
        ),
        &format!(
            "{}.{}.x",
            URL_SAFE_NO_PAD.encode("[1]"),
            URL_SAFE_NO_PAD.encode("{}")
        ),
        &format!(
            "{}.{}.x",
            URL_SAFE_NO_PAD.encode("{}"),
            URL_SAFE_NO_PAD.encode("\"s\"")
        ),
    ] {
        assert!(jwt::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(jwt::parse("a.b").unwrap_err(), JwtError::Malformed);
}

#[test]
fn a_huge_token_is_refused_quickly() {
    let huge = format!("{}.{}.x", "a".repeat(MAX_TOKEN_LEN), "b");
    assert_eq!(jwt::parse(&huge).unwrap_err(), JwtError::TooLarge);
    // Deeply nested JSON is cut off by the parser, not by a stack overflow.
    let deep = format!("{}1{}", "[".repeat(100_000), "]".repeat(100_000));
    let t = format!(
        "{}.{}.",
        URL_SAFE_NO_PAD.encode(&deep),
        URL_SAFE_NO_PAD.encode("{}")
    );
    assert!(t.len() < MAX_TOKEN_LEN * 10);
    assert!(jwt::parse(&t).is_err());
}

#[test]
fn spans_are_rounded_down() {
    assert_eq!(jwt::span(59), "59 s");
    assert_eq!(jwt::span(-125), "2 min");
    assert_eq!(jwt::span(7300), "2 h");
    assert_eq!(jwt::span(3 * 86_400 + 5), "3 d");
}

#[test]
fn the_note_before_sending_names_claim_and_deadline_but_never_the_value() {
    let t = with_exp(NOW - 12 * 60);
    let req = request(
        &[("Authorization", format!("Bearer {t}"))],
        "https://api.example.com/x",
    );
    let notes = notes_at(&req, NOW);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].text,
        "Authorization header: token expired 12 min ago (exp)"
    );
    let json = serde_json::to_string(&notes).unwrap();
    assert!(!json.contains(&t) && !json.contains(CANARY) && !json.contains("eyJ"));
}

#[test]
fn the_query_parameter_is_looked_at_too_and_a_good_token_is_quiet() {
    let old = with_exp(NOW - 5);
    let req = request(
        &[],
        &format!("https://api.example.com/x?a=1&access_token={old}#frag"),
    );
    assert_eq!(request_tokens(&req)[0].0, RequestTokenPlace::Query);
    assert_eq!(notes_at(&req, NOW).len(), 1);
    let fine = with_exp(NOW + 3600);
    let req = request(
        &[("authorization", format!("Bearer {fine}"))],
        "https://x.example/",
    );
    assert!(notes_at(&req, NOW).is_empty());
    // Not a token, not a bearer scheme, a malformed percent escape: nothing, no panic.
    let req = request(
        &[("Authorization", "Basic abc".into())],
        "https://x.example/?access_token=%é",
    );
    assert!(notes_at(&req, NOW).is_empty());
}
