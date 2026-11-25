// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --checks` (wave 18, strand B) on the built binary: one line per observation on
//! stderr and in the report, nothing without the flag, rules the collection switched off stay
//! quiet, the exit code never changes, nothing is sent but the one request, and a token value
//! never appears in a line or a note.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

const CANARY: &str = "RT-CANARY-checks-cli";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn collection(tag: &str, extra_collection: &str, request_extra: &str, port: u16) -> Dir {
    let dir = std::env::temp_dir().join(format!("rt-checks-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        format!("opencollection: 1.0.0\ninfo:\n  name: shop\n{extra_collection}"),
    )
    .unwrap();
    std::fs::write(
        dir.join("get.yml"),
        format!(
            "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://127.0.0.1:{port}/x\"\n  auth: inherit\n{request_extra}"
        ),
    )
    .unwrap();
    Dir(dir)
}

/// A server that answers every request with `reply` and counts them.
fn serve(reply: String) -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf);
            seen.fetch_add(1, Ordering::SeqCst);
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (port, count)
}

fn http(status: &str, headers: &[&str], body: &str) -> String {
    let mut out = format!("HTTP/1.1 {status}\r\n");
    for h in headers {
        out.push_str(h);
        out.push_str("\r\n");
    }
    format!(
        "{out}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn send(dir: &Dir, flags: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("send")
        .arg(dir.0.join("get.yml"))
        .args(flags)
        .env("RT_DATA_DIR", std::env::temp_dir().join("rt-checks-data"))
        .output()
        .expect("run rt");
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn token(alg: &str, exp: i64) -> String {
    let part = |v: Value| URL_SAFE_NO_PAD.encode(v.to_string());
    format!(
        "{}.{}.{}",
        part(json!({"alg": alg})),
        part(json!({"sub": CANARY, "exp": exp})),
        URL_SAFE_NO_PAD.encode("sig")
    )
}

fn plain_reply() -> String {
    http(
        "200 OK",
        &[
            "Content-Type: application/json",
            "RateLimit-Limit: 100",
            "Server: nginx/1.18.0",
            "Set-Cookie: sid=abc; Path=/",
            "Access-Control-Allow-Origin: *",
            "Access-Control-Allow-Credentials: true",
        ],
        "{\"ok\":true}",
    )
}

#[test]
fn checks_print_one_line_per_observation_and_fill_the_report() {
    let (port, count) = serve(plain_reply());
    let dir = collection("lines", "", "", port);
    let out = send(&dir, &["--checks"]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let lines: Vec<&str> = out
        .stderr
        .lines()
        .filter(|l| l.starts_with("check "))
        .collect();
    for id in [
        "chk/nosniff-missing",
        "chk/cookie-flags",
        "chk/cors-credentials",
        "chk/version-disclosure",
    ] {
        assert!(lines.iter().any(|l| l.contains(id)), "{id} in {lines:?}");
    }
    // Plain http: no HSTS line. Each line says severity, place, why and fix.
    assert!(!lines.iter().any(|l| l.contains("chk/hsts-missing")));
    assert!(
        lines.iter().all(|l| l.contains(" Fix: ")
            && (l.starts_with("check info ") || l.starts_with("check warn ")))
    );
    let report: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    let in_report: Vec<&str> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap())
        .collect();
    assert_eq!(in_report.len(), lines.len());
    assert!(in_report.contains(&"chk/cookie-flags"));
    assert!(
        report["checks"][0]["severity"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    // Nothing went out but the request itself.
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn without_the_flag_the_report_has_no_checks_and_stderr_is_quiet() {
    let (port, _) = serve(plain_reply());
    let dir = collection("quiet", "", "", port);
    let out = send(&dir, &[]);
    assert_eq!(out.code, 0);
    let report: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert!(report.get("checks").is_none(), "{}", out.stdout);
    assert!(!out.stderr.contains("check "), "{}", out.stderr);
}

#[test]
fn a_rule_the_collection_switched_off_stays_quiet() {
    let (port, _) = serve(plain_reply());
    let ignore = "extensions:\n  requesttailor:\n    checks:\n      ignore:\n        - chk/cookie-flags\n        - chk/version-disclosure\n";
    let dir = collection("ignore", ignore, "", port);
    let out = send(&dir, &["--checks"]);
    assert!(
        !out.stderr.contains("chk/cookie-flags") && !out.stderr.contains("chk/version-disclosure"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("chk/cors-credentials"),
        "{}",
        out.stderr
    );
}

#[test]
fn observations_never_change_the_exit_code_and_never_quote_a_token() {
    let t = token("none", 4_000_000_000);
    let body = format!(
        "Traceback (most recent call last):\n  File \"app.py\", line 1, in <module>\n{{\"access_token\":\"{t}\"}}"
    );
    let (port, _) = serve(http(
        "500 Internal Server Error",
        &["Content-Type: text/plain"],
        &body,
    ));
    let dir = collection("exit", "", "", port);
    let without = send(&dir, &[]);
    let with = send(&dir, &["--checks"]);
    assert_eq!(with.code, without.code);
    assert!(
        with.stderr.contains("chk/stack-trace") && with.stderr.contains("chk/jwt-alg-none"),
        "{}",
        with.stderr
    );
    for line in with.stderr.lines().filter(|l| l.starts_with("check ")) {
        assert!(
            !line.contains(CANARY) && !line.contains(&t) && !line.contains("eyJ"),
            "{line}"
        );
    }
    let report: Value = serde_json::from_str(with.stdout.trim()).unwrap();
    let checks = serde_json::to_string(&report["checks"]).unwrap();
    assert!(!checks.contains(CANARY) && !checks.contains("eyJ"));
}

#[test]
fn an_expired_token_is_a_note_and_the_send_goes_ahead() {
    let t = token("RS256", 1_000_000_000);
    let (port, count) = serve(plain_reply());
    let extra = format!("  headers:\n    - name: Authorization\n      value: \"Bearer {t}\"\n");
    let dir = collection("expired", "", &extra, port);
    let out = send(&dir, &[]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(count.load(Ordering::SeqCst), 1, "nothing is blocked");
    let report: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    let notes = report["notes"].as_array().unwrap();
    let note = notes
        .iter()
        .find(|n| n["text"].as_str().is_some_and(|t| t.contains("expired")))
        .unwrap_or_else(|| panic!("{notes:?}"));
    assert_eq!(note["level"], "warning");
    assert!(note["text"].as_str().unwrap().contains("ago (exp)"));
    // The value is in the request the user wrote, but not in the note.
    assert!(!note["text"].as_str().unwrap().contains(CANARY));
    assert!(!note["text"].as_str().unwrap().contains(&t));
    assert!(!out.stderr.contains(&t));
}

// ---- wave 19, strand R: the passive security rules in `rt send --checks` ----

fn rule_ids(report: &Value) -> Vec<String> {
    report["checks"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|o| o["id"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_security_rules_show_in_the_checks_and_never_quote_a_token_or_a_secret() {
    let weak = {
        use hmac::{Hmac, Mac};
        let part = |v: Value| URL_SAFE_NO_PAD.encode(v.to_string());
        let input = format!(
            "{}.{}",
            part(json!({"alg": "HS256"})),
            part(json!({"sub": CANARY}))
        );
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(b"changeme").unwrap();
        mac.update(input.as_bytes());
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        )
    };
    let reply = http(
        "200 OK",
        &["Content-Type: application/json"],
        &format!("{{\"access_token\":\"{weak}\"}}"),
    );
    let (port, count) = serve(reply);
    let dir = collection("sec", "", "", port);
    let out = send(&dir, &["--checks"]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(count.load(Ordering::SeqCst), 1, "no extra request");
    let report: Value = serde_json::from_str(&out.stdout).unwrap();
    let ids = rule_ids(&report);
    for id in [
        "sec/jwt-weak-hmac",
        "sec/jwt-no-exp",
        "sec/cache-control-tokens",
        "sec/no-rate-limit",
    ] {
        assert!(ids.iter().any(|i| i == id), "{id} in {ids:?}");
        assert!(out.stderr.contains(id), "{id} on stderr: {}", out.stderr);
    }
    for needle in [CANARY, "changeme", weak.as_str()] {
        assert!(!out.stderr.contains(needle), "{needle} on stderr");
        assert!(
            !report["checks"].to_string().contains(needle),
            "{needle} in the checks"
        );
    }
}

#[test]
fn a_rule_the_collection_switched_off_stays_quiet_for_the_security_rules_too() {
    let (port, _) = serve(http("200 OK", &["Content-Type: application/json"], "{}"));
    let dir = collection(
        "sec-off",
        "extensions:\n  requesttailor:\n    checks:\n      ignore:\n        - sec/no-rate-limit\n",
        "",
        port,
    );
    let out = send(&dir, &["--checks"]);
    let report: Value = serde_json::from_str(&out.stdout).unwrap();
    assert!(
        !rule_ids(&report).iter().any(|i| i == "sec/no-rate-limit"),
        "{}",
        out.stderr
    );
}
