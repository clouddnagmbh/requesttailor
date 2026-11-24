// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The engine of snapshot tests (wave 17, strand B): freeze a response, compare a later one
//! with it. Pure functions over bytes, so the same code runs in the app, the browser, `rt run`
//! and `rt mcp`. The file format and the commands live in `rt_app::snapshots`, which
//! re-exports the types.
//!
//! # Ignore paths
//!
//! A path of `ignore` addresses a place of the response: `$.items[*].createdAt` or
//! `res.body.items[0].id` (the body), `res.headers.content-type`, `res.status`. `[*]` stands
//! for every element of an array. A path ignores everything below it. A path without a
//! wildcard names exactly what `rt_core::assert` resolves. A path that cannot be read matches
//! nothing and is refused by [`check_ignore`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The assertion operator of snapshot tests (re-exported by `rt_core::assert`).
pub use crate::assert::MATCHES_SNAPSHOT;

/// The version of the companion file format.
pub const SNAPSHOT_VERSION: u32 = 1;

/// The response headers a snapshot keeps (lowercase). `etag` only when the user asks for it.
pub const HEADER_ALLOWLIST: &[&str] = &["content-type", "content-language", "cache-control"];

/// Bytes of a text body a snapshot keeps (the same as `rt_app::compare::SNAPSHOT_BODY_LIMIT`,
/// a test there keeps them equal).
pub const BODY_LIMIT: usize = 256 * 1024;

/// A frozen response, the content of a `.snap.json` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub status: u16,
    /// Allowlisted headers, lowercase names, sorted.
    pub headers: BTreeMap<String, String>,
    pub body: SnapshotBody,
    /// Paths the comparison skips, in the order the user added them.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// The body of a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SnapshotBody {
    /// More than [`BODY_LIMIT`] bytes of a body that is not JSON: hash and length instead of
    /// the text.
    Digest {
        #[serde(rename = "$digest")]
        digest: String,
        #[serde(rename = "$length")]
        length: u64,
    },
    /// A JSON body as is, or any other text body as a JSON string.
    Value(Value),
}

/// One way the response differs from the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDiff {
    /// Where, as a path of the expression language (`status`, `headers.content-type`,
    /// `body.items[0].id`).
    pub path: String,
    pub kind: SnapshotDiffKind,
    /// What the snapshot has (`None`: nothing there). JSON, secrets masked.
    pub expected: Option<Value>,
    /// What the response has (`None`: nothing there). JSON, secrets masked.
    pub actual: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotDiffKind {
    /// Same place, another value.
    Changed,
    /// The response has something the snapshot does not.
    Added,
    /// The snapshot has something the response lacks.
    Removed,
    /// Another kind of value (number against text, object against array, ...).
    Type,
}

/// A response as the engine reads it.
#[derive(Debug, Clone, Copy)]
pub struct ResponseView<'a> {
    pub status: u16,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

/// What a freeze keeps beyond the defaults.
#[derive(Debug, Clone, Default)]
pub struct Freeze {
    /// Keep the `etag` header.
    pub keep_etag: bool,
    /// Paths to ignore.
    pub ignore: Vec<String>,
}

/// Freezes `response`: allowlisted headers, the body as JSON or text, every value of
/// `secrets` replaced by `***` first.
pub fn freeze(response: &ResponseView<'_>, secrets: &[String], options: &Freeze) -> Snapshot {
    Snapshot {
        version: SNAPSHOT_VERSION,
        status: response.status,
        headers: kept_headers(response.headers, options.keep_etag, secrets),
        body: body_of(response.body, secrets),
        ignore: options.ignore.clone(),
    }
}

/// Reads the text of a `.snap.json` file.
pub fn parse(text: &str) -> Result<Snapshot, String> {
    let snapshot: Snapshot = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if snapshot.version != SNAPSHOT_VERSION {
        return Err(format!(
            "snapshot version {} is not supported (this build reads version {SNAPSHOT_VERSION})",
            snapshot.version
        ));
    }
    Ok(snapshot)
}

/// The text of a `.snap.json` file: pretty JSON, fixed key order, a final newline.
pub fn to_text(snapshot: &Snapshot) -> String {
    let mut text = serde_json::to_string_pretty(snapshot).expect("a snapshot is JSON");
    text.push('\n');
    text
}

/// Compares `response` with `snapshot`, skipping the snapshot's ignored paths. Empty: equal.
/// Secrets of `secrets` are masked in the response first, as when freezing.
pub fn compare(
    snapshot: &Snapshot,
    response: &ResponseView<'_>,
    secrets: &[String],
) -> Vec<SnapshotDiff> {
    let keep_etag = snapshot.headers.contains_key("etag");
    let fresh = Snapshot {
        version: snapshot.version,
        status: response.status,
        headers: kept_headers(response.headers, keep_etag, secrets),
        body: body_of(response.body, secrets),
        ignore: Vec::new(),
    };
    let ignore: Vec<Vec<Seg>> = snapshot
        .ignore
        .iter()
        .filter_map(|p| parse_ignore(p).ok())
        .collect();
    let mut out = Vec::new();
    let mut at = |path: &[Seg], kind, expected: Option<&Value>, actual: Option<&Value>| {
        if ignore.iter().any(|pattern| covers(pattern, path)) {
            return;
        }
        out.push(SnapshotDiff {
            path: render(path),
            kind,
            expected: expected.cloned(),
            actual: actual.cloned(),
        });
    };
    let root = |name: &str| vec![Seg::Key(name.to_owned())];
    if snapshot.status != fresh.status {
        at(
            &root("status"),
            SnapshotDiffKind::Changed,
            Some(&Value::from(snapshot.status)),
            Some(&Value::from(fresh.status)),
        );
    }
    let names: std::collections::BTreeSet<&String> = snapshot
        .headers
        .keys()
        .chain(fresh.headers.keys())
        .collect();
    for name in names {
        let mut path = root("headers");
        path.push(Seg::Key(name.clone()));
        match (snapshot.headers.get(name), fresh.headers.get(name)) {
            (Some(a), Some(b)) if a != b => at(
                &path,
                SnapshotDiffKind::Changed,
                Some(&Value::from(a.as_str())),
                Some(&Value::from(b.as_str())),
            ),
            (Some(a), None) => at(
                &path,
                SnapshotDiffKind::Removed,
                Some(&Value::from(a.as_str())),
                None,
            ),
            (None, Some(b)) => at(
                &path,
                SnapshotDiffKind::Added,
                None,
                Some(&Value::from(b.as_str())),
            ),
            _ => {}
        }
    }
    let body_path = root("body");
    match (&snapshot.body, &fresh.body) {
        (SnapshotBody::Value(a), SnapshotBody::Value(b)) => {
            walk(&body_path, a, b, &mut at);
        }
        (a, b) if a != b => {
            let json = |b: &SnapshotBody| serde_json::to_value(b).ok();
            at(
                &body_path,
                SnapshotDiffKind::Changed,
                json(a).as_ref(),
                json(b).as_ref(),
            );
        }
        _ => {}
    }
    out
}

/// Whether `path` is a path [`compare`] can use.
pub fn check_ignore(path: &str) -> Result<(), String> {
    parse_ignore(path).map(|_| ())
}

fn kept_headers(
    headers: &[(String, String)],
    keep_etag: bool,
    secrets: &[String],
) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        if HEADER_ALLOWLIST.contains(&name.as_str()) || (keep_etag && name == "etag") {
            out.entry(name).or_default().push(value);
        }
    }
    out.into_iter()
        .map(|(name, values)| (name, crate::send::mask(&values.join(", "), secrets)))
        .collect()
}

fn body_of(bytes: &[u8], secrets: &[String]) -> SnapshotBody {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return digest(bytes);
    };
    let text = crate::send::mask(text, secrets);
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        return SnapshotBody::Value(value);
    }
    if text.len() > BODY_LIMIT {
        return digest(text.as_bytes());
    }
    SnapshotBody::Value(Value::String(text))
}

fn digest(bytes: &[u8]) -> SnapshotBody {
    let hash = Sha256::digest(bytes);
    SnapshotBody::Digest {
        digest: hash.iter().map(|b| format!("{b:02x}")).collect(),
        length: bytes.len() as u64,
    }
}

/// The `value` of the assertion for a new snapshot: `users/get.yml` gives `get.snap.json`
/// (the file beside the request).
pub fn default_value(request_path: &str) -> String {
    let file = request_path.rsplit('/').next().unwrap_or(request_path);
    let stem = file.strip_suffix(".yml").unwrap_or(file);
    format!("{stem}.snap.json")
}

/// The companion file of the request at `request_path` for an assertion `value`, relative to
/// the collection root (`value` is relative to the request file).
pub fn companion_path(request_path: &str, value: &str) -> String {
    match request_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{value}"),
        None => value.to_owned(),
    }
}

// ---- paths ----

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Key(String),
    Index(usize),
    Any,
}

fn render(path: &[Seg]) -> String {
    let mut out = String::new();
    for (i, seg) in path.iter().enumerate() {
        match seg {
            Seg::Key(k) if k.chars().all(|c| c.is_alphanumeric() || "_-$".contains(c)) => {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(k);
            }
            Seg::Key(k) => out.push_str(&format!("[{}]", Value::from(k.as_str()))),
            Seg::Index(n) => out.push_str(&format!("[{n}]")),
            Seg::Any => out.push_str("[*]"),
        }
    }
    out
}

/// A pattern covers a path if it is the path or a parent of it.
fn covers(pattern: &[Seg], path: &[Seg]) -> bool {
    pattern.len() <= path.len()
        && pattern.iter().zip(path).all(|(p, s)| match (p, s) {
            (Seg::Any, Seg::Index(_)) => true,
            (a, b) => a == b,
        })
}

fn parse_ignore(text: &str) -> Result<Vec<Seg>, String> {
    let text = text.trim();
    let (head, rest): (&str, &str) = if let Some(rest) = text.strip_prefix('$') {
        ("body", rest)
    } else {
        let text = text.strip_prefix("res.").unwrap_or(text);
        let end = text.find(['.', '[']).unwrap_or(text.len());
        let (head, rest) = text.split_at(end);
        if !matches!(head, "body" | "headers" | "status") {
            return Err(format!(
                "`{text}`: a path starts with `$`, `res.body`, `res.headers` or `res.status`"
            ));
        }
        (head, rest)
    };
    let mut path = vec![Seg::Key(head.to_owned())];
    let mut rest = rest;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let key = after[..end].trim();
            if key.is_empty() {
                return Err("an empty name after `.`".into());
            }
            path.push(Seg::Key(key.to_owned()));
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('[') {
            let after = after.trim_start();
            if let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let inner = &after[1..];
                let close = inner
                    .find(quote)
                    .ok_or_else(|| format!("a missing closing {quote}"))?;
                path.push(Seg::Key(inner[..close].to_owned()));
                rest = inner[close + 1..]
                    .trim_start()
                    .strip_prefix(']')
                    .ok_or("a missing `]`")?;
            } else {
                let close = after.find(']').ok_or("a missing `]`")?;
                let inside = after[..close].trim();
                path.push(if inside == "*" {
                    Seg::Any
                } else {
                    Seg::Index(
                        inside
                            .parse()
                            .map_err(|_| format!("`[{inside}]` is not an index, `*` or a name"))?,
                    )
                });
                rest = &after[close + 1..];
            }
        } else {
            return Err(format!("unexpected `{rest}`"));
        }
    }
    Ok(path)
}

// ---- JSON diff ----

type Sink<'a> = dyn FnMut(&[Seg], SnapshotDiffKind, Option<&Value>, Option<&Value>) + 'a;

fn walk(path: &[Seg], expected: &Value, actual: &Value, sink: &mut Sink<'_>) {
    use SnapshotDiffKind::*;
    let child = |seg: Seg| {
        let mut p = path.to_vec();
        p.push(seg);
        p
    };
    match (expected, actual) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for key in keys {
                let p = child(Seg::Key(key.clone()));
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => walk(&p, x, y, sink),
                    (Some(x), None) => sink(&p, Removed, Some(x), None),
                    (None, Some(y)) => sink(&p, Added, None, Some(y)),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for i in 0..a.len().max(b.len()) {
                let p = child(Seg::Index(i));
                match (a.get(i), b.get(i)) {
                    (Some(x), Some(y)) => walk(&p, x, y, sink),
                    (Some(x), None) => sink(&p, Removed, Some(x), None),
                    (None, Some(y)) => sink(&p, Added, None, Some(y)),
                    (None, None) => {}
                }
            }
        }
        (a, b) if a == b => {}
        (a, b) if kind_of(a) != kind_of(b) => sink(path, Type, Some(a), Some(b)),
        (a, b) => sink(path, Changed, Some(a), Some(b)),
    }
}

fn kind_of(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

// ---- the assertion ----

/// The message of a failing snapshot assertion for the differences found.
pub fn summary(differences: &[SnapshotDiff]) -> String {
    let shown: Vec<String> = differences
        .iter()
        .take(5)
        .map(|d| {
            let show =
                |v: &Option<Value>| v.as_ref().map_or("nothing".to_owned(), |v| v.to_string());
            format!("{} ({} -> {})", d.path, show(&d.expected), show(&d.actual))
        })
        .collect();
    let more = differences.len().saturating_sub(5);
    let mut text = format!(
        "the response differs from the snapshot at {} place(s): {}",
        differences.len(),
        shown.join("; ")
    );
    if more > 0 {
        text.push_str(&format!("; and {more} more"));
    }
    text
}
