// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand R2: the free passive `rt security audit` and the distribution hook
//! (`Distribution.security_audit`). A local server answers like
//! `testdata/insight/vulnerable-api.mjs` (open CORS, a bearer token without `exp`, no
//! security headers) and like its `/good` path. Every format, the secret canary, exit codes,
//! guard refusal, the production banner, writing requests, and a fake distribution extension.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rt_cli::security_cmd::{AuditContext, SecurityAuditExtension};
use rt_core::guard::{GuardHandle, GuardVerdict, Purpose, SendCheck, SendGuard};

/// A secret that is a JWT with `alg: none` and no `exp`, so the rules fire on it.
const TOKEN: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiJSVC1DQU5BUlktdG9rZW4tOTkxIn0.";
const CANARY: &str = "RT-CANARY-token-991";
const QUERY_SECRET: &str = "RT-CANARY-query-17";

struct Server {
    port: u16,
    hits: Arc<AtomicUsize>,
    methods: Arc<Mutex<Vec<String>>>,
}

fn server() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let methods = Arc::new(Mutex::new(Vec::new()));
    let (h, m) = (hits.clone(), methods.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let line = head.lines().next().unwrap_or("").to_owned();
            let mut parts = line.split(' ');
            let method = parts.next().unwrap_or("").to_owned();
            let path = parts
                .next()
                .unwrap_or("")
                .split('?')
                .next()
                .unwrap_or("")
                .to_owned();
            h.fetch_add(1, Ordering::SeqCst);
            m.lock().unwrap().push(format!("{method} {path}"));
            let (status, headers, body) = match path.as_str() {
                "/cors" => (
                    200,
                    "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Credentials: true\r\n",
                    format!("{{\"access_token\":\"{TOKEN}\"}}"),
                ),
                "/good" => (
                    200,
                    "Strict-Transport-Security: max-age=31536000\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nX-RateLimit-Limit: 100\r\nX-RateLimit-Remaining: 99\r\n",
                    "{\"ok\":true}".to_owned(),
                ),
                _ => (200, "", "{\"ok\":true}".to_owned()),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Server {
        port,
        hits,
        methods,
    }
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-secaudit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn request(root: &Path, file: &str, seq: u32, method: &str, path: &str) {
    write(
        &root.join(file),
        &format!(
            "info:\n  name: {file}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {method}\n  url: \"{{{{base}}}}{path}?key={{{{token}}}}&other={QUERY_SECRET}\"\n  headers:\n    - name: Authorization\n      value: Bearer {{{{token}}}}\n"
        ),
    );
}

/// A collection with `cors.yml`, `good.yml` and (when `post`) `create.yml` (POST).
fn collection(root: &Path, port: u16, post: bool) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Audit\n",
    );
    for env in ["dev", "PRD"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
            ),
        );
    }
    request(root, "cors.yml", 1, "GET", "/cors");
    request(root, "good.yml", 2, "GET", "/good");
    if post {
        request(root, "create.yml", 3, "POST", "/create");
    }
}

struct Out {
    code: i32,
    out: String,
    err: String,
}

fn rt_with(args: &[&str], distribution: &rt_cli::Distribution<'_>) -> Out {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rt_cli::run_distribution(
        args.iter().copied().map(String::from),
        &mut out,
        &mut err,
        distribution,
    );
    Out {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

fn audit(root: &Path, extra: &[&str]) -> Out {
    audit_with(root, extra, &rt_cli::Distribution::default())
}

fn audit_with(root: &Path, extra: &[&str], distribution: &rt_cli::Distribution<'_>) -> Out {
    let token = format!("token={TOKEN}");
    let mut args = vec![
        "security",
        "audit",
        root.to_str().unwrap(),
        "-e",
        "dev",
        "--var",
        &token,
        "--no-plugins",
        "--no-scripts",
        "--no-prompt",
    ];
    args.extend_from_slice(extra);
    rt_with(&args, distribution)
}

fn clean(texts: &[&str]) {
    for t in texts {
        for needle in [TOKEN, CANARY, QUERY_SECRET] {
            assert!(!t.contains(needle), "{needle} leaked into:\n{t}");
        }
    }
}

#[test]
fn text_report_lists_findings_and_never_fails_without_fail_on() {
    let srv = server();
    let dir = tmp("text");
    collection(&dir, srv.port, false);
    let o = audit(&dir, &[]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(
        o.out.contains("RequestTailor security audit: Audit"),
        "{}",
        o.out
    );
    assert!(o.out.contains("not proof"), "{}", o.out);
    assert!(o.out.contains("chk/cors-credentials"), "{}", o.out);
    assert!(o.out.contains("cors.yml"), "{}", o.out);
    // Each request goes out exactly once.
    assert_eq!(srv.hits.load(Ordering::SeqCst), 2);
    clean(&[&o.out, &o.err]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_format_renders_the_findings_without_a_secret() {
    let srv = server();
    let dir = tmp("formats");
    collection(&dir, srv.port, false);
    let mut all = Vec::new();
    for format in ["text", "markdown", "html", "json", "sarif"] {
        let o = audit(&dir, &["--format", format]);
        assert_eq!(o.code, 0, "{format}: {}", o.err);
        assert!(
            o.out.contains("chk/cors-credentials"),
            "{format}: {}",
            o.out
        );
        assert!(o.out.contains("not proof"), "{format}: {}", o.out);
        clean(&[&o.out, &o.err]);
        all.push((format, o.out));
    }
    let get = |f: &str| all.iter().find(|(n, _)| *n == f).unwrap().1.clone();

    assert!(get("markdown").starts_with("# Security audit: Audit"));
    assert!(get("markdown").contains("## API"));

    let html = get("html");
    assert!(html.starts_with("<!doctype html>"));
    for external in ["<script", "href=", "src=", "<link", "@import", "url("] {
        assert!(!html.contains(external), "html references {external}");
    }

    let json: serde_json::Value = serde_json::from_str(&get("json")).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["summary"]["requests"], 2);
    assert!(json["findings"].as_array().unwrap().iter().any(|f| {
        f["rule"] == "chk/cors-credentials" && f["request"] == "cors.yml" && f["severity"] != ""
    }));
    // The query is not part of a reported URL.
    assert!(
        json["requests"][0]["url"]
            .as_str()
            .unwrap()
            .ends_with("/cors")
    );

    let sarif: serde_json::Value = serde_json::from_str(&get("sarif")).unwrap();
    assert_eq!(sarif["version"], "2.1.0");
    assert!(sarif["$schema"].as_str().unwrap().contains("sarif-2.1.0"));
    let run = &sarif["runs"][0];
    assert_eq!(run["tool"]["driver"]["name"], "rt security audit");
    let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
    let results = run["results"].as_array().unwrap();
    assert!(!results.is_empty());
    for r in rules {
        let tags = r["properties"]["tags"].as_array().unwrap();
        assert!(
            tags.iter()
                .any(|t| t.as_str().unwrap().starts_with("OWASP-API")),
            "{r}"
        );
        assert!(r["shortDescription"]["text"].is_string());
    }
    for r in results {
        let index = r["ruleIndex"].as_u64().unwrap() as usize;
        assert_eq!(rules[index]["id"], r["ruleId"]);
        assert!(matches!(
            r["level"].as_str().unwrap(),
            "note" | "warning" | "error"
        ));
        assert!(
            r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
                .as_str()
                .unwrap()
                .ends_with(".yml")
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn out_writes_the_report_to_a_file() {
    let srv = server();
    let dir = tmp("out");
    collection(&dir, srv.port, false);
    let file = dir.join("reports/audit.sarif");
    let o = audit(
        &dir,
        &["--format", "sarif", "--out", file.to_str().unwrap()],
    );
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.is_empty(), "{}", o.out);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
    clean(&[&text]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fail_on_gives_exit_9_only_when_a_finding_reaches_the_level() {
    let srv = server();
    let dir = tmp("exit");
    collection(&dir, srv.port, false);
    assert_eq!(audit(&dir, &["--fail-on", "info"]).code, rt_cli::EXIT_AUDIT);
    assert_eq!(audit(&dir, &["--fail-on", "info"]).code, 9);
    assert_eq!(audit(&dir, &["--fail-on", "critical"]).code, 0);
    let bad = audit(&dir, &["--fail-on", "urgent"]);
    assert_eq!(bad.code, 2);
    assert!(bad.err.contains("unknown severity"), "{}", bad.err);
    let bad = audit(&dir, &["--format", "pdf"]);
    assert_eq!(bad.code, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_clean_collection_has_no_exit_9() {
    let srv = server();
    let dir = tmp("clean");
    collection(&dir, srv.port, false);
    std::fs::remove_file(dir.join("cors.yml")).unwrap();
    let o = audit(&dir, &["--fail-on", "low"]);
    assert_eq!(o.code, 0, "{}{}", o.out, o.err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn active_probes_are_not_part_of_the_free_build() {
    let srv = server();
    let dir = tmp("authorized");
    collection(&dir, srv.port, false);
    let o = audit(&dir, &["--i-am-authorized"]);
    assert_eq!(o.code, 2);
    assert!(o.err.contains("distribution"), "{}", o.err);
    assert_eq!(srv.hits.load(Ordering::SeqCst), 0, "nothing was sent");
    let o = audit(&dir, &["--frobnicate"]);
    assert_eq!(o.code, 2);
    assert!(o.err.contains("unknown flag"), "{}", o.err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writing_requests_are_left_out_unless_allowed() {
    let srv = server();
    let dir = tmp("writes");
    collection(&dir, srv.port, true);
    let o = audit(&dir, &[]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("left out: create.yml"), "{}", o.out);
    assert!(
        !srv.methods
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.starts_with("POST"))
    );
    let o = audit(&dir, &["--allow-writes"]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(
        srv.methods
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == "POST /create")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Refuses every request to `/cors`.
struct RefuseCors(Mutex<Vec<(Purpose, String)>>);

impl SendGuard for RefuseCors {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        assert!(!check.url.contains(TOKEN), "the guard saw a secret");
        self.0
            .lock()
            .unwrap()
            .push((check.purpose, check.request_path.clone()));
        if check.url.contains("/cors") {
            GuardVerdict::Refuse {
                rule: "test.no-cors".into(),
                reason: "not this one".into(),
            }
        } else {
            GuardVerdict::Allow
        }
    }
}

#[test]
fn a_refused_request_is_not_sent_and_exits_5() {
    let srv = server();
    let dir = tmp("guard");
    collection(&dir, srv.port, false);
    let guard = Arc::new(RefuseCors(Mutex::default()));
    let o = audit_with(
        &dir,
        &["--fail-on", "info"],
        &rt_cli::Distribution {
            guard: GuardHandle::new(guard.clone()),
            ..Default::default()
        },
    );
    assert_eq!(o.code, rt_cli::EXIT_BLOCKED, "{}{}", o.out, o.err);
    assert_eq!(rt_cli::EXIT_BLOCKED, 5);
    assert!(o.out.contains("refused by the send guard"), "{}", o.out);
    assert_eq!(srv.hits.load(Ordering::SeqCst), 1, "only /good went out");
    // The free passive run is a run of the collection: purpose main.
    assert!(
        guard
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|(p, _)| *p == Purpose::Main)
    );
    clean(&[&o.out, &o.err]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn production_gets_a_banner() {
    let srv = server();
    let dir = tmp("prd");
    collection(&dir, srv.port, false);
    let token = format!("token={TOKEN}");
    let o = rt_with(
        &[
            "security",
            "audit",
            dir.to_str().unwrap(),
            "-e",
            "PRD",
            "--var",
            &token,
            "--no-plugins",
            "--no-scripts",
        ],
        &rt_cli::Distribution::default(),
    );
    assert!(o.err.contains("PRODUCTION: running"), "{}", o.err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_and_registration() {
    let o = rt_with(&["security", "audit", "--help"], &Default::default());
    assert_eq!(o.code, 0);
    assert!(o.out.contains("rt security audit"), "{}", o.out);
    assert!(o.out.contains("--fail-on"), "{}", o.out);
    let o = rt_with(&["--help"], &Default::default());
    assert!(o.out.contains("--format text|markdown|html|json|sarif"));
}

// ---------------------------------------------------------------------------------------
// The distribution hook

/// A fake distribution: adds an "active" finding to the passive ones, probes once through the
/// session, and reads its own flags.
struct Fake {
    seen: Mutex<Option<Seen>>,
}

#[derive(Debug)]
struct Seen {
    rest: Vec<String>,
    passive_rules: Vec<String>,
    probe_status: Option<u16>,
    probe_masked: String,
    requests: Vec<String>,
}

impl SecurityAuditExtension for Fake {
    fn run(&self, ctx: &AuditContext<'_>, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
        let authorized = ctx.args.rest.iter().any(|a| a == "--i-am-authorized");
        if !authorized {
            let _ = writeln!(stderr, "rt: pass --i-am-authorized");
            return 2;
        }
        // One probe through the core: the spec as written, a changed header.
        let path = "good.yml";
        let mut spec = ctx.session.request(path).unwrap();
        spec.headers.push(rt_core::model::KeyValue {
            name: "X-Probe".into(),
            value: "1".into(),
            enabled: true,
        });
        let probe = ctx.session.send(path, Some(spec));
        let mut report = ctx.passive.clone();
        report.requests[0]
            .findings
            .push(rt_app::security::SecurityFinding {
                rule: "active/mass-assignment".into(),
                category: rt_app::security::OwaspCategory::Api3,
                severity: rt_app::security::Severity::High,
                location: "request body".into(),
                why: format!("probe answered {}", ctx.session.mask(TOKEN)),
                fix: "Whitelist fields.".into(),
            });
        *self.seen.lock().unwrap() = Some(Seen {
            rest: ctx
                .args
                .rest
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            passive_rules: ctx
                .passive
                .findings()
                .map(|(_, f)| f.rule.clone())
                .collect(),
            probe_status: probe.response.as_ref().map(|r| r.status),
            probe_masked: ctx.session.mask(&format!("x {TOKEN} y")),
            requests: ctx.session.requests().to_vec(),
        });
        if rt_cli::security_cmd::write_report(ctx.args, &report, stdout, stderr).is_err() {
            return 2;
        }
        report.exit_code(ctx.args.fail_on)
    }
}

#[test]
fn a_distribution_handles_the_command_and_merges_its_findings() {
    let srv = server();
    let dir = tmp("hook");
    collection(&dir, srv.port, false);
    let fake = Fake {
        seen: Mutex::new(None),
    };
    let guard = Arc::new(RefuseCors(Mutex::default()));
    let o = audit_with(
        &dir,
        &["--format", "json", "--fail-on", "high", "--i-am-authorized"],
        &rt_cli::Distribution {
            guard: GuardHandle::new(guard.clone()),
            security_audit: Some(&fake),
            ..Default::default()
        },
    );
    // cors.yml is refused by the guard in the passive run, but the probe of good.yml went
    // through; the distribution's finding is high, so the hook's own exit code applies.
    let seen = fake.seen.lock().unwrap().take().expect("hook ran");
    assert_eq!(seen.rest, ["--i-am-authorized"]);
    assert_eq!(seen.probe_status, Some(200));
    assert_eq!(seen.probe_masked, "x *** y");
    assert_eq!(seen.requests, ["cors.yml", "good.yml"]);
    // The probe was asked as an audit probe, the passive run as a main request.
    let asked = guard.0.lock().unwrap().clone();
    assert!(
        asked.contains(&(Purpose::Audit, "good.yml".into())),
        "{asked:?}"
    );
    assert!(asked.contains(&(Purpose::Main, "good.yml".into())));
    let json: serde_json::Value = serde_json::from_str(&o.out).unwrap();
    let rules: Vec<&str> = json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap())
        .collect();
    assert!(rules.contains(&"active/mass-assignment"), "{rules:?}");
    clean(&[&o.out, &o.err]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_hook_sees_passive_findings_and_decides_the_exit_code() {
    let srv = server();
    let dir = tmp("hook2");
    collection(&dir, srv.port, false);
    let fake = Fake {
        seen: Mutex::new(None),
    };
    let o = audit_with(
        &dir,
        &["--format", "json", "--fail-on", "high", "--i-am-authorized"],
        &rt_cli::Distribution {
            security_audit: Some(&fake),
            ..Default::default()
        },
    );
    let seen = fake.seen.lock().unwrap().take().expect("hook ran");
    assert!(
        seen.passive_rules
            .iter()
            .any(|r| r == "chk/cors-credentials"),
        "{:?}",
        seen.passive_rules
    );
    assert_eq!(o.code, rt_cli::EXIT_AUDIT, "{}", o.err);
    // Without its flag the distribution refuses itself: the core does not interfere.
    let o = audit_with(
        &dir,
        &[],
        &rt_cli::Distribution {
            security_audit: Some(&fake),
            ..Default::default()
        },
    );
    assert_eq!(o.code, 2);
    assert!(o.err.contains("--i-am-authorized"), "{}", o.err);
    let _ = std::fs::remove_dir_all(&dir);
}
