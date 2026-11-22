// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Conformance harness, shared by the native runner (`crates/rt-cli/tests/conformance.rs`) and
//! the browser runner (`crates/rt-wasm/tests/conformance.rs`) via `#[path]`. The comparison
//! logic and the list of allowed browser differences live here and nowhere else.
//!
//! # Case file (`cases.json`)
//!
//! `collections` maps a key to `{ path, environment?, vars? }`: `path` is relative to
//! `testdata/`, `environment` and `vars` (runtime variables, `--var`) apply to every case of
//! that collection. Each case has `name`, `collection`, `request` (path relative to the
//! collection root), optional `environment` (overrides), `vars` (appended), `body_limit`,
//! `browser` (a reason to skip the case in the browser; only for documented fetch
//! limitations) and `expect`:
//!
//! | Field | Checks |
//! | --- | --- |
//! | `error` | `null`/absent: no error. Else `not-found`, `prepare`, `auth`, `plugin` or `transport/<kind>` |
//! | `status` | response status |
//! | `unresolved` | exactly these names, order ignored |
//! | `no_request`, `no_response` | `request` / `response` is `null` |
//! | `sent` | `method`, `url`, `headers` (subset), `headers_absent`, `body_size`, `not_contains` (string absent from the whole `request` JSON) |
//! | `echo` | the test server's `/echo` JSON: `method`, `path`, `query`, `headers` (subset), `headers_absent`, `body`, `body_contains`, `body_len` |
//! | `response` | `url`, `version`, `encoding`, `truncated`, `size`, `redirects` (count), `headers` (subset), `body_json` (equal), `body_pattern` (n bytes of `/bytes/n`) |
//! | `variables` | `[name, layer]` pairs that must be in `variables` |
//! | `error_contains` | the error's `message` contains this (e.g. for `script` errors) |
//! | `script` | wave 7, compared exactly: `tests` (`[name, passed]`), `logs` (`[level, text]`), `var_changes` (`[name, value]`), `flow` (`null`: none) |
//!
//! Header names compare case-insensitively; a header value ending in `*` is a prefix match.
//! `{base}` (`http://127.0.0.1:{port}`) and `{port}` are replaced in `vars` and URLs.
//!
//! The harness checks the report as JSON (`serde_json::Value`), so the CLI test can check
//! `rt send` output with the same code.

#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

pub const CASES: &str = include_str!("cases.json");

/// `rt_testserver::DEFAULT_PORT`. The browser cannot discover a random port, and the WASM
/// tests cannot depend on the (native) test server crate; the native tests check they agree.
pub const DEFAULT_BROWSER_PORT: u16 = 47811;

/// The port the browser tests expect the test server on: `RT_CONFORMANCE_PORT` at build time
/// (set by `scripts/conformance-wasm.sh`, so parallel checkouts can use their own port), else
/// [`DEFAULT_BROWSER_PORT`].
pub const BROWSER_PORT: u16 = match option_env!("RT_CONFORMANCE_PORT") {
    Some(p) => parse_port(p),
    None => DEFAULT_BROWSER_PORT,
};

const fn parse_port(s: &str) -> u16 {
    let b = s.as_bytes();
    assert!(
        !b.is_empty() && b.len() <= 5,
        "RT_CONFORMANCE_PORT: not a port"
    );
    let mut n: u32 = 0;
    let mut i = 0;
    while i < b.len() {
        assert!(b[i].is_ascii_digit(), "RT_CONFORMANCE_PORT: not a port");
        n = n * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    assert!(n > 0 && n <= 65535, "RT_CONFORMANCE_PORT: not a port");
    n as u16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Native,
    Browser,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    #[serde(rename = "$comment")]
    _comment: Option<String>,
    pub collections: BTreeMap<String, CollectionDef>,
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionDef {
    pub path: String,
    pub environment: Option<String>,
    #[serde(default)]
    pub vars: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub collection: String,
    pub request: String,
    pub environment: Option<String>,
    #[serde(default)]
    pub vars: Vec<(String, String)>,
    pub body_limit: Option<usize>,
    /// Skip in the browser, with the reason.
    pub browser: Option<String>,
    pub expect: Expect,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    pub error: Option<String>,
    pub status: Option<u16>,
    pub unresolved: Option<Vec<String>>,
    #[serde(default)]
    pub no_request: bool,
    #[serde(default)]
    pub no_response: bool,
    pub sent: Option<SentExpect>,
    pub echo: Option<EchoExpect>,
    pub response: Option<ResponseExpect>,
    #[serde(default)]
    pub variables: Vec<(String, String)>,
    /// Wave 7, strand C: `report.assertions` and `report.var_changes`, entry by entry; each
    /// expected entry lists the fields it checks (`{base}`/`{port}` substituted in strings).
    pub assertions: Option<Vec<Value>>,
    pub var_changes: Option<Vec<Value>>,
    pub error_contains: Option<String>,
    pub script: Option<ScriptExpect>,
}

/// Wave 7: what scripts produced. Each given field is compared exactly.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptExpect {
    pub tests: Option<Vec<(String, bool)>>,
    pub logs: Option<Vec<(String, String)>>,
    pub var_changes: Option<Vec<(String, Option<String>)>>,
    /// `Some(Value::Null)`: no flow in the report.
    #[serde(default, deserialize_with = "some_value")]
    pub flow: Option<Value>,
}

/// Keeps an explicit `null` as `Some(Value::Null)`.
fn some_value<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SentExpect {
    pub method: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub headers_absent: Vec<String>,
    pub body_size: Option<u64>,
    pub not_contains: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EchoExpect {
    pub method: Option<String>,
    pub path: Option<String>,
    pub query: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub headers_absent: Vec<String>,
    pub body: Option<String>,
    #[serde(default)]
    pub body_contains: Vec<String>,
    pub body_len: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseExpect {
    pub url: Option<String>,
    pub version: Option<String>,
    pub encoding: Option<String>,
    pub truncated: Option<bool>,
    pub size: Option<u64>,
    pub redirects: Option<usize>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub body_json: Option<Value>,
    pub body_pattern: Option<usize>,
}

/// What to send for one case on one target.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Collection root relative to `testdata/`.
    pub collection_path: String,
    pub request: String,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    pub body_limit: Option<usize>,
}

pub fn suite() -> Suite {
    serde_json::from_str(CASES).expect("testdata/conformance/cases.json is valid")
}

impl Suite {
    pub fn plan(&self, case: &Case, port: u16) -> Plan {
        let def = self
            .collections
            .get(&case.collection)
            .unwrap_or_else(|| panic!("case {:?}: unknown collection", case.name));
        Plan {
            collection_path: def.path.clone(),
            request: case.request.clone(),
            environment: case.environment.clone().or_else(|| def.environment.clone()),
            vars: def
                .vars
                .iter()
                .chain(&case.vars)
                .map(|(k, v)| (k.clone(), subst(v, port)))
                .collect(),
            body_limit: case.body_limit,
        }
    }
}

pub fn base(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn subst(s: &str, port: u16) -> String {
    s.replace("{base}", &base(port))
        .replace("{port}", &port.to_string())
}

// ---------------------------------------------------------------------------------------------
// Browser differences. Everything not listed here must be identical on both targets.

/// Request headers a page may not set (Fetch spec, "forbidden request-header"); the browser
/// drops or replaces them, so expectations on them are not checked in the browser.
pub const FORBIDDEN_HEADERS: &[&str] = &[
    "accept-charset",
    "accept-encoding",
    "access-control-request-headers",
    "access-control-request-method",
    "connection",
    "content-length",
    "cookie",
    "cookie2",
    "date",
    "dnt",
    "expect",
    "host",
    "keep-alive",
    "origin",
    "referer",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "via",
];

/// Applies the documented browser differences to the expectation and the actual error kind:
/// - `response.redirects`: fetch follows redirects without reporting them (list is empty).
/// - `response.version`: fetch does not expose the HTTP version (`unknown`).
/// - timing: dns/connect/tls are `null` in the browser; timing is not compared anyway.
/// - forbidden request headers (above) are not checked on the echo.
/// - transport errors: fetch reports every network failure as the same `TypeError`, so
///   `connect`, `dns`, `tls`, `blocked` and `other` are one kind, `network`.
fn normalize(target: Target, expect: &mut Expect, error: Option<String>) -> Option<String> {
    if target == Target::Native {
        return error;
    }
    if let Some(r) = &mut expect.response {
        r.redirects = None;
        r.version = None;
    }
    if let Some(e) = &mut expect.echo {
        let allowed = |n: &str| !FORBIDDEN_HEADERS.contains(&n.to_ascii_lowercase().as_str());
        e.headers.retain(|(n, _)| allowed(n));
        e.headers_absent.retain(|n| allowed(n));
    }
    let network = |kind: String| match kind.as_str() {
        "transport/connect" | "transport/dns" | "transport/tls" | "transport/blocked"
        | "transport/other" => "transport/network".to_owned(),
        _ => kind,
    };
    expect.error = expect.error.take().map(network);
    error.map(network)
}

// ---------------------------------------------------------------------------------------------
// Comparison

/// Checks one report (as JSON) against the case. Returns every mismatch.
pub fn check(case: &Case, target: Target, port: u16, report: &Value) -> Vec<String> {
    let mut expect = case.expect.clone();
    let mut f = Vec::new();
    let error = error_kind(&report["error"]);
    let error = normalize(target, &mut expect, error);

    if report["version"] != 1 {
        f.push(format!("report version {}", report["version"]));
    }
    if report["request_path"] != case.request.as_str() {
        f.push(format!("request_path {}", report["request_path"]));
    }
    if error != expect.error {
        f.push(format!(
            "error: expected {:?}, got {:?} ({})",
            expect.error, error, report["error"]
        ));
    }
    if let Some(want) = &expect.unresolved {
        let mut want = want.clone();
        let mut got: Vec<String> = strings(&report["unresolved"]);
        want.sort();
        got.sort();
        if want != got {
            f.push(format!("unresolved: expected {want:?}, got {got:?}"));
        }
    }
    if let Some(want) = &expect.error_contains {
        let message = report["error"]["message"].to_string();
        if !message.contains(want.as_str()) {
            f.push(format!("error: {message} does not contain {want:?}"));
        }
    }
    if let Some(script) = &expect.script {
        check_script(script, report, &mut f);
    }
    if expect.no_request && !report["request"].is_null() {
        f.push("request: expected null".into());
    }
    if expect.no_response && !report["response"].is_null() {
        f.push("response: expected null".into());
    }
    for (name, layer) in &expect.variables {
        let found = report["variables"].as_array().is_some_and(|vars| {
            vars.iter()
                .any(|v| v["name"] == name.as_str() && v["source"]["layer"] == layer.as_str())
        });
        if !found {
            f.push(format!(
                "variables: no {name} from {layer} in {}",
                report["variables"]
            ));
        }
    }
    if let Some(sent) = &expect.sent {
        check_sent(sent, port, &report["request"], &mut f);
    }
    if let Some(want) = &expect.assertions {
        check_entries("assertions", want, port, &report["assertions"], &mut f);
    }
    if let Some(want) = &expect.var_changes {
        check_entries("var_changes", want, port, &report["var_changes"], &mut f);
    }

    let response = &report["response"];
    let needs_response =
        expect.status.is_some() || expect.echo.is_some() || expect.response.is_some();
    if needs_response && response.is_null() {
        f.push("response: missing".into());
        return f;
    }
    if let Some(status) = expect.status
        && response["status"] != status
    {
        f.push(format!(
            "status: expected {status}, got {}",
            response["status"]
        ));
    }
    if !response.is_null() {
        let ttfb = response["timing"]["ttfb"].as_f64().unwrap_or(-1.0);
        let total = response["total"].as_f64().unwrap_or(-1.0);
        if !(0.0..=total).contains(&ttfb) {
            f.push(format!("timing: ttfb {ttfb} ms, total {total} ms"));
        }
    }
    let body = body_bytes(&response["body"]);
    if let Some(r) = &expect.response {
        check_response(r, port, response, body.as_deref(), &mut f);
    }
    if let Some(e) = &expect.echo {
        match body
            .as_deref()
            .and_then(|b| serde_json::from_slice::<Value>(b).ok())
        {
            Some(echo) => check_echo(e, &echo, &mut f),
            None => f.push(format!(
                "echo: response body is not JSON: {}",
                response["body"]
            )),
        }
    }
    f
}

/// Wave 7: a list in the report (omitted when empty) against the expected entries, each a
/// subset of its fields.
fn check_entries(what: &str, want: &[Value], port: u16, got: &Value, f: &mut Vec<String>) {
    let empty = Vec::new();
    let got = got.as_array().unwrap_or(&empty);
    if got.len() != want.len() {
        f.push(format!(
            "{what}: expected {} entries, got {}: {got:?}",
            want.len(),
            got.len()
        ));
        return;
    }
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        let Some(fields) = w.as_object() else {
            f.push(format!("{what}[{i}]: the expected entry must be an object"));
            continue;
        };
        for (key, value) in fields {
            let value = match value {
                Value::String(s) => Value::String(subst(s, port)),
                other => other.clone(),
            };
            if g[key] != value {
                f.push(format!(
                    "{what}[{i}].{key}: expected {value}, got {} in {g}",
                    g[key]
                ));
            }
        }
    }
}

fn check_script(want: &ScriptExpect, report: &Value, f: &mut Vec<String>) {
    let list = |key: &str| report[key].as_array().cloned().unwrap_or_default();
    if let Some(want) = &want.tests {
        let got: Vec<(String, bool)> = list("tests")
            .iter()
            .map(|t| {
                let name = t["name"].as_str().unwrap_or_default().to_owned();
                (name, t["passed"].as_bool().unwrap_or(false))
            })
            .collect();
        if &got != want {
            f.push(format!("script tests: expected {want:?}, got {got:?}"));
        }
    }
    if let Some(want) = &want.logs {
        let got: Vec<(String, String)> = list("logs")
            .iter()
            .map(|l| {
                let level = l["level"].as_str().unwrap_or_default().to_owned();
                (level, l["text"].as_str().unwrap_or_default().to_owned())
            })
            .collect();
        if &got != want {
            f.push(format!("script logs: expected {want:?}, got {got:?}"));
        }
    }
    if let Some(want) = &want.var_changes {
        let got: Vec<(String, Option<String>)> = list("var_changes")
            .iter()
            .map(|c| {
                let name = c["name"].as_str().unwrap_or_default().to_owned();
                (name, c["value"].as_str().map(str::to_owned))
            })
            .collect();
        if &got != want {
            f.push(format!(
                "script var_changes: expected {want:?}, got {got:?}"
            ));
        }
    }
    if let Some(want) = &want.flow
        && &report["flow"] != want
    {
        f.push(format!(
            "script flow: expected {want}, got {}",
            report["flow"]
        ));
    }
}

fn error_kind(error: &Value) -> Option<String> {
    let kind = error["kind"].as_str()?;
    Some(if kind == "transport" {
        format!(
            "transport/{}",
            error["message"]["kind"].as_str().unwrap_or("?")
        )
    } else {
        kind.to_owned()
    })
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// `[[name, value], ...]` as pairs.
fn pairs(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| Some((p[0].as_str()?.to_owned(), p[1].as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn header_matches(want: &str, got: &str) -> bool {
    match want.strip_suffix('*') {
        Some(prefix) => got.starts_with(prefix),
        None => got == want,
    }
}

fn check_headers(
    what: &str,
    want: &[(String, String)],
    absent: &[String],
    got: &[(String, String)],
    f: &mut Vec<String>,
) {
    for (name, value) in want {
        let ok = got
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case(name) && header_matches(value, v));
        if !ok {
            f.push(format!("{what}: no header {name}: {value} in {got:?}"));
        }
    }
    for name in absent {
        if got.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
            f.push(format!("{what}: header {name} must be absent in {got:?}"));
        }
    }
}

fn check_sent(want: &SentExpect, port: u16, sent: &Value, f: &mut Vec<String>) {
    if sent.is_null() {
        f.push("request: missing".into());
        return;
    }
    if let Some(m) = &want.method
        && sent["method"] != m.as_str()
    {
        f.push(format!(
            "request.method: expected {m}, got {}",
            sent["method"]
        ));
    }
    if let Some(u) = &want.url {
        let u = subst(u, port);
        if sent["url"] != u.as_str() {
            f.push(format!("request.url: expected {u}, got {}", sent["url"]));
        }
    }
    if let Some(n) = want.body_size
        && sent["body_size"] != n
    {
        f.push(format!(
            "request.body_size: expected {n}, got {}",
            sent["body_size"]
        ));
    }
    if let Some(s) = &want.not_contains
        && sent.to_string().contains(s.as_str())
    {
        f.push(format!("request: must not contain {s:?}: {sent}"));
    }
    check_headers(
        "request",
        &want.headers,
        &want.headers_absent,
        &pairs(&sent["headers"]),
        f,
    );
}

fn check_response(
    want: &ResponseExpect,
    port: u16,
    r: &Value,
    body: Option<&[u8]>,
    f: &mut Vec<String>,
) {
    if let Some(u) = &want.url {
        let u = subst(u, port);
        if r["url"] != u.as_str() {
            f.push(format!("response.url: expected {u}, got {}", r["url"]));
        }
    }
    if let Some(v) = &want.version
        && r["version"] != v.as_str()
    {
        f.push(format!(
            "response.version: expected {v}, got {}",
            r["version"]
        ));
    }
    if let Some(e) = &want.encoding
        && r["body"]["encoding"] != e.as_str()
    {
        f.push(format!(
            "response.body.encoding: expected {e}, got {}",
            r["body"]["encoding"]
        ));
    }
    if let Some(t) = want.truncated
        && r["truncated"] != t
    {
        f.push(format!(
            "response.truncated: expected {t}, got {}",
            r["truncated"]
        ));
    }
    if let Some(s) = want.size
        && r["size"] != s
    {
        f.push(format!("response.size: expected {s}, got {}", r["size"]));
    }
    if let Some(n) = want.redirects {
        let got = r["redirects"].as_array().map_or(usize::MAX, Vec::len);
        if got != n {
            f.push(format!(
                "response.redirects: expected {n}, got {}",
                r["redirects"]
            ));
        }
    }
    check_headers("response", &want.headers, &[], &pairs(&r["headers"]), f);
    if let Some(j) = &want.body_json {
        match body.and_then(|b| serde_json::from_slice::<Value>(b).ok()) {
            Some(got) if &got == j => {}
            _ => f.push(format!(
                "response.body: expected JSON {j}, got {}",
                r["body"]
            )),
        }
    }
    if let Some(n) = want.body_pattern {
        let ok = body.is_some_and(|b| {
            b.len() == n && b.iter().enumerate().all(|(i, &x)| x as usize == i % 251)
        });
        if !ok {
            f.push(format!(
                "response.body: expected the {n}-byte /bytes pattern, got {} bytes",
                body.map_or(0, <[u8]>::len)
            ));
        }
    }
}

fn check_echo(want: &EchoExpect, echo: &Value, f: &mut Vec<String>) {
    for (field, value) in [
        ("method", &want.method),
        ("path", &want.path),
        ("query", &want.query),
        ("body", &want.body),
    ] {
        if let Some(v) = value
            && echo[field] != v.as_str()
        {
            f.push(format!("echo.{field}: expected {v:?}, got {}", echo[field]));
        }
    }
    if let Some(n) = want.body_len
        && echo["body_len"] != n
    {
        f.push(format!(
            "echo.body_len: expected {n}, got {}",
            echo["body_len"]
        ));
    }
    let body = echo["body"].as_str().unwrap_or("");
    for part in &want.body_contains {
        if !body.contains(part.as_str()) {
            f.push(format!("echo.body: must contain {part:?}, got {body:?}"));
        }
    }
    check_headers(
        "echo",
        &want.headers,
        &want.headers_absent,
        &pairs(&echo["headers"]),
        f,
    );
}

/// The response body bytes from the report's `{encoding, data}`.
fn body_bytes(body: &Value) -> Option<Vec<u8>> {
    let data = body["data"].as_str()?;
    match body["encoding"].as_str()? {
        "utf8" => Some(data.as_bytes().to_vec()),
        "base64" => base64_decode(data),
        _ => None,
    }
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.trim_end_matches('=').bytes() {
        acc = (acc << 6) | ALPHABET.iter().position(|&a| a == c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Runs the suite's bookkeeping: prints one line per case and panics with all failures.
pub struct Outcome {
    failures: Vec<(String, Vec<String>)>,
    passed: usize,
    skipped: usize,
}

impl Outcome {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
            passed: 0,
            skipped: 0,
        }
    }

    pub fn skip(&mut self, case: &Case, reason: &str) -> String {
        self.skipped += 1;
        format!("skip  {} ({reason})", case.name)
    }

    pub fn record(&mut self, case: &Case, failures: Vec<String>, report: &str) -> String {
        if failures.is_empty() {
            self.passed += 1;
            format!("ok    {}", case.name)
        } else {
            let mut lines = failures;
            if !report.is_empty() {
                lines.push(format!("report: {report}"));
            }
            self.failures.push((case.name.clone(), lines));
            format!("FAIL  {}", case.name)
        }
    }

    pub fn finish(self) {
        let summary = format!(
            "conformance: {} passed, {} failed, {} skipped",
            self.passed,
            self.failures.len(),
            self.skipped
        );
        if self.failures.is_empty() {
            return;
        }
        let mut msg = summary;
        for (name, lines) in &self.failures {
            msg.push_str(&format!("\n\n{name}:"));
            for l in lines {
                msg.push_str(&format!("\n  - {l}"));
            }
        }
        panic!("{msg}");
    }
}

/// The harness itself: the case file parses, every collection exists and has a request file
/// for every case (except `not-found` cases).
pub fn validate(suite: &Suite, exists: impl Fn(&str) -> bool) -> Vec<String> {
    let mut problems = Vec::new();
    for (key, def) in &suite.collections {
        if !exists(&format!("{}/opencollection.yml", def.path)) {
            problems.push(format!(
                "collection {key}: no opencollection.yml in {}",
                def.path
            ));
        }
    }
    for case in &suite.cases {
        let Some(def) = suite.collections.get(&case.collection) else {
            problems.push(format!(
                "{}: unknown collection {}",
                case.name, case.collection
            ));
            continue;
        };
        let file = format!("{}/{}", def.path, case.request);
        if case.expect.error.as_deref() != Some("not-found") && !exists(&file) {
            problems.push(format!("{}: no file {file}", case.name));
        }
    }
    problems
}
