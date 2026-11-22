// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! AWS Signature V4 against the published test vectors in `testdata/sigv4` (origin and
//! licence in `testdata/sigv4/SOURCES.md`).

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use bytes::Bytes;
use rt_core::auth::sigv4::{Credentials, SignOptions, sign_request};
use rt_core::transport::{HttpBody, HttpRequest};

fn suite() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/sigv4/v4")
}

fn read(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok()
}

/// `request.txt` → request. `Host` becomes the URL's authority (it is signed from there);
/// header continuation lines stay in the value, as on the wire.
fn parse_request(text: &str) -> HttpRequest {
    let (head, body) = match text.split_once("\n\n") {
        Some((head, body)) => (head, body),
        None => (text.trim_end_matches('\n'), ""),
    };
    let mut lines = head.lines();
    let request_line = lines.next().unwrap();
    let method = request_line.split(' ').next().unwrap().to_owned();
    let target = request_line
        .strip_prefix(&format!("{method} "))
        .unwrap()
        .strip_suffix(" HTTP/1.1")
        .unwrap_or_else(|| request_line.strip_prefix(&format!("{method} ")).unwrap());
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.starts_with([' ', '\t']) {
            let last = headers.last_mut().unwrap();
            last.1.push('\n');
            last.1.push_str(line);
            continue;
        }
        let (name, value) = line.split_once(':').unwrap();
        headers.push((name.to_owned(), value.to_owned()));
    }
    let host = headers
        .iter()
        .position(|(n, _)| n.eq_ignore_ascii_case("host"))
        .map(|i| headers.remove(i).1)
        .unwrap();
    HttpRequest {
        method,
        url: format!("https://{host}{target}"),
        headers,
        body: (!body.is_empty()).then(|| HttpBody {
            content_type: None,
            bytes: Bytes::copy_from_slice(body.as_bytes()),
        }),
        options: Default::default(),
    }
}

struct Context {
    credentials: Credentials,
    options: SignOptions,
    now: std::time::SystemTime,
}

/// `2015-08-30T12:36:00Z` → time.
fn parse_timestamp(s: &str) -> std::time::SystemTime {
    let n = |r: std::ops::Range<usize>| s[r].parse::<i64>().unwrap();
    let (y, m, d) = (n(0..4), n(5..7), n(8..10));
    let (hh, mm, ss) = (n(11..13), n(14..16), n(17..19));
    // Days from civil (Howard Hinnant), proleptic Gregorian.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hh * 3600 + mm * 60 + ss;
    UNIX_EPOCH + Duration::from_secs(secs as u64)
}

fn context(dir: &Path, name: &str) -> Context {
    let Some(json) = read(dir, "context.json") else {
        // The two `aws-sigv4` additions: the crate's own test parameters.
        return Context {
            credentials: Credentials {
                access_key_id: "ANOTREAL".into(),
                secret_access_key: "notrealrnrELgWzOk3IfjzDKtFBhDby".into(),
                session_token: None,
                region: "test-region".into(),
                service: "testservicename".into(),
            },
            options: SignOptions::for_service("testservicename"),
            now: parse_timestamp("2021-05-11T15:40:45Z"),
        };
    };
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let c = &v["credentials"];
    let s = |v: &serde_json::Value| v.as_str().unwrap().to_owned();
    let single = matches!(
        name,
        "get-space-normalized" | "get-space-unnormalized" | "get-utf8"
    );
    Context {
        credentials: Credentials {
            access_key_id: s(&c["access_key_id"]),
            secret_access_key: s(&c["secret_access_key"]),
            session_token: c["token"].as_str().map(str::to_owned),
            region: s(&v["region"]),
            service: s(&v["service"]),
        },
        options: SignOptions {
            normalize_path: v["normalize"].as_bool().unwrap(),
            double_encode_path: !single,
            content_sha256_header: v["sign_body"].as_bool().unwrap(),
            sign_session_token: !v["omit_session_token"].as_bool().unwrap_or(false),
        },
        now: parse_timestamp(v["timestamp"].as_str().unwrap()),
    }
}

/// The `Authorization` line of `header-signed-request.txt`.
fn expected_authorization(signed: &str) -> String {
    signed
        .lines()
        .find_map(|l| l.strip_prefix("Authorization:"))
        .unwrap()
        .trim()
        .to_owned()
}

#[test]
fn every_vector_signs_as_published() {
    let mut names: Vec<String> = std::fs::read_dir(suite())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names.len(), 40, "{names:?}");
    let mut failures = Vec::new();
    for name in &names {
        let dir = suite().join(name);
        let ctx = context(&dir, name);
        let mut request = parse_request(&read(&dir, "request.txt").unwrap());
        let signed = match sign_request(&mut request, &ctx.credentials, &ctx.options, ctx.now) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let expected = read(&dir, "header-canonical-request.txt").unwrap();
        if signed.canonical_request != expected {
            failures.push(format!(
                "{name}: canonical request\n--- want\n{expected}\n--- got\n{}",
                signed.canonical_request
            ));
            continue;
        }
        if let Some(sts) = read(&dir, "header-string-to-sign.txt")
            && read(&dir, "context.json").is_some()
            && signed.string_to_sign != sts
        {
            failures.push(format!(
                "{name}: string to sign\n--- want\n{sts}\n--- got\n{}",
                signed.string_to_sign
            ));
            continue;
        }
        if let Some(sig) = read(&dir, "header-signature.txt") {
            if signed.signature != sig.trim() {
                failures.push(format!("{name}: signature {} != {}", signed.signature, sig));
                continue;
            }
            let want = expected_authorization(&read(&dir, "header-signed-request.txt").unwrap());
            let got = request
                .headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("authorization"))
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            if got != want {
                failures.push(format!("{name}: Authorization\n want {want}\n got  {got}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn signed_request_carries_date_and_session_token() {
    let dir = suite().join("get-vanilla-with-session-token");
    let ctx = context(&dir, "get-vanilla-with-session-token");
    let mut request = parse_request(&read(&dir, "request.txt").unwrap());
    sign_request(&mut request, &ctx.credentials, &ctx.options, ctx.now).unwrap();
    let header = |name: &str| {
        request
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(header("x-amz-date"), Some("20150830T123600Z"));
    assert_eq!(
        header("x-amz-security-token"),
        ctx.credentials.session_token.as_deref()
    );
    assert!(header("x-amz-content-sha256").is_none());
}
