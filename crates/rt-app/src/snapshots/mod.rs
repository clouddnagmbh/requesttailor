// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests (wave 17, strand B): freeze a response, compare later responses with it,
//! ignore volatile paths by hand. This module holds the types and the file format, the
//! interface the strands share; the comparison and the shells' commands come with strand B.
//!
//! # Files are the truth
//!
//! A snapshot is a companion file next to the request, `<request name>.snap.json`, plain JSON
//! with fixed keys in a stable order (a diff needs no tool). The request file changes by
//! exactly one assertion in `runtime.assertions`: `expression: res`, `operator:
//! matchesSnapshot` ([`MATCHES_SNAPSHOT`]), `value`: the companion file relative to the
//! request file. Older builds report the unknown operator as a **failure**, never as a silent
//! pass. The reader of the collection only reads `*.yml`; companion files go through
//! `Backend.companions` (staged, written with `save`).
//!
//! ```json
//! { "version": 1, "status": 200, "headers": { "content-type": "application/json" },
//!   "body": { "…": "…" }, "ignore": ["$.items[*].createdAt"] }
//! ```
//!
//! # Rules the strand implements
//!
//! - Headers: only [`HEADER_ALLOWLIST`] (`etag` only on request); never `date`, `set-cookie`
//!   or anything else.
//! - Body: JSON as is; other text as a JSON string up to
//!   [`crate::compare::SNAPSHOT_BODY_LIMIT`]; beyond that [`SnapshotBody::Digest`].
//! - `ignore` takes paths of the expression language of `rt_core::assert`
//!   (`res.body.items[0].id`, `$`-style as in the example) with `[*]` for every array
//!   element; a path without a wildcard must match what `assert` resolves exactly.
//! - Secrets: every secret value of the environment becomes `***` before freezing; the
//!   comparison masks the fresh response the same way. A snapshot that
//!   `crate::secret_scan` flags is not written (`AppError::SecretsFound`).
//! - Freezing is a command of the app and `rt snapshot update`, never part of a send or `rt
//!   run`; a missing file fails the assertion with `no snapshot yet; run `rt snapshot
//!   update``.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use rt_core::assert::MATCHES_SNAPSHOT;

/// The version of the companion file format.
pub const SNAPSHOT_VERSION: u32 = 1;

/// The response headers a snapshot keeps (lowercase). `etag` only when the user asks for it.
pub const HEADER_ALLOWLIST: &[&str] = &["content-type", "content-language", "cache-control"];

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
    /// More than [`crate::compare::SNAPSHOT_BODY_LIMIT`] bytes of a body that is not JSON:
    /// hash and length instead of the text.
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

/// What comparing a response with the snapshot of a request found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotState {
    /// Equal after the ignored paths.
    Matches,
    Differs,
    /// The assertion names a file that does not exist.
    Missing,
    /// The request has no snapshot assertion and no `.snap.json` beside it.
    None,
}

/// The result of `SnapshotApi.check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCheck {
    pub request_path: String,
    /// The companion file, relative to the collection; empty for [`SnapshotState::None`].
    pub snapshot: String,
    pub state: SnapshotState,
    /// Empty unless [`SnapshotState::Differs`]; ignored paths are not listed.
    pub differences: Vec<SnapshotDiff>,
}

/// The result of freezing (or accepting a new response): what is staged, unsaved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFreeze {
    pub request_path: String,
    /// The companion file, relative to the collection.
    pub snapshot: String,
    /// The request got its assertion with this freeze (`false`: it already had one).
    pub assertion_added: bool,
    pub content: Snapshot,
}

/// What the user chose when freezing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FreezeOptions {
    /// Keep the `etag` header (off by default).
    #[serde(default)]
    pub keep_etag: bool,
    /// The `value` of the new assertion; `None`: [`default_value`]. Ignored when the request
    /// already has a snapshot assertion.
    #[serde(default)]
    pub file: Option<String>,
}

/// A request with a snapshot, for `list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub request_path: String,
    /// The companion file, relative to the collection.
    pub snapshot: String,
    /// The file exists (staged or saved).
    pub exists: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_names() {
        assert_eq!(default_value("users/get.yml"), "get.snap.json");
        assert_eq!(default_value("get.yml"), "get.snap.json");
        assert_eq!(
            companion_path("users/get.yml", "get.snap.json"),
            "users/get.snap.json"
        );
        assert_eq!(companion_path("get.yml", "get.snap.json"), "get.snap.json");
    }

    #[test]
    fn the_file_format_has_the_documented_keys() {
        let snap = Snapshot {
            version: SNAPSHOT_VERSION,
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: SnapshotBody::Value(serde_json::json!({ "a": 1 })),
            ignore: vec!["$.a".into()],
        };
        let text = serde_json::to_string(&snap).unwrap();
        assert_eq!(
            text,
            r#"{"version":1,"status":200,"headers":{"content-type":"application/json"},"body":{"a":1},"ignore":["$.a"]}"#
        );
        assert_eq!(serde_json::from_str::<Snapshot>(&text).unwrap(), snap);
        let big = SnapshotBody::Digest {
            digest: "ab".into(),
            length: 9,
        };
        let text = serde_json::to_string(&big).unwrap();
        assert_eq!(text, r#"{"$digest":"ab","$length":9}"#);
        assert_eq!(serde_json::from_str::<SnapshotBody>(&text).unwrap(), big);
    }
}
