// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic diffs for the app (wave 12): [`rt_format::diff`] plus what only the app knows.
//! Every shell (desktop, web through rt-wasm, `rt diff`) goes through [`diff`], so masking
//! happens in one place: besides secret variables (masked by `rt-format`), values the secret
//! scanner flags (`secret_scan`) never appear in clear.
//!
//! Owner: wave 12 interface; implementation strand A.

use serde::{Deserialize, Serialize};

pub use rt_format::diff::{
    ChangeKind, ChangeTarget, ChangedFile, CollectionDiff, DiffValue, FieldChange, FieldPath,
    FileStatus, ModelChange,
};

use crate::AppError;

/// What the Git view diffs (`DiffApi`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DiffScope {
    /// The saved files against `HEAD`.
    Working,
    /// A commit against its first parent (the empty tree for a root commit).
    Commit { rev: String },
    /// `from` → `to`, e.g. two branches.
    Between { from: String, to: String },
}

/// The semantic diff of `old` → `new`, masked: besides the secret variables `rt-format` hides,
/// a value the secret scanner flags (a token typed into a header, a password in the auth, a
/// key in a body) is replaced by [`DiffValue::Secret`]. The field path stays, so the diff
/// still says *that* it changed.
pub fn diff(
    old: &rt_format::FileSet,
    new: &rt_format::FileSet,
) -> Result<CollectionDiff, AppError> {
    let mut diff = rt_format::diff::diff(old, new)?;
    mask(&mut diff);
    Ok(diff)
}

/// Masks every value of `diff` the scanner flags. Applied by [`diff`]; public for diffs
/// that come from elsewhere (a merge conflict view).
pub fn mask(diff: &mut CollectionDiff) {
    for change in &mut diff.changes {
        for field in &mut change.fields {
            let path = field.path.clone();
            for value in [&mut field.old, &mut field.new].into_iter().flatten() {
                if flagged(&path, value) {
                    *value = DiffValue::Secret;
                }
            }
        }
    }
}

/// The text the scanner is shown for one value: as it would stand in the file, with the
/// entry's name next to it (`name: Authorization` / `value: …`) where the field is a list
/// entry.
fn probe(path: &[String], text: &str) -> String {
    let path = if path.first().map(String::as_str) == Some("defaults") {
        &path[1..]
    } else {
        path
    };
    let entry = match path.first().map(String::as_str) {
        Some("headers" | "vars" | "variables") => path.get(1),
        Some("params") => path.get(2),
        _ => None,
    };
    let (key, value_key) = match entry {
        Some(name) => (Some(name.split('#').next().unwrap_or(name)), "value"),
        None => (None, path.last().map_or("value", String::as_str)),
    };
    let mut out = String::new();
    if let Some(name) = key {
        out.push_str(&format!("name: {name}\n"));
    }
    if text.contains('\n') {
        out.push_str(&format!("{value_key}: |\n"));
        for line in text.lines() {
            out.push_str(&format!("  {line}\n"));
        }
    } else {
        out.push_str(&format!("{value_key}: {text}\n"));
    }
    out
}

fn flagged(path: &[String], value: &DiffValue) -> bool {
    let text = match value {
        DiffValue::Text { text } => text.clone(),
        DiffValue::Json { value } => serde_json::to_string_pretty(value).unwrap_or_default(),
        DiffValue::Secret => return false,
    };
    if text.is_empty() {
        return false;
    }
    !crate::secret_scan::scan(&[("diff", &probe(path, &text))]).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_format::FileSet;

    fn set(request: &str) -> FileSet {
        let mut s = FileSet::default();
        s.files.insert(
            "opencollection.yml".into(),
            b"opencollection: 1.0.0\n\ninfo:\n  name: demo\n".to_vec(),
        );
        s.files.insert("a.yml".into(), request.as_bytes().to_vec());
        s
    }

    #[test]
    fn credentials_typed_into_a_request_are_masked() {
        let req = |h: &str| {
            format!(
                "info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://x\n  headers:\n    - name: Authorization\n      value: {h}\n    - name: X-Note\n      value: {h}-note\n  auth: inherit\n"
            )
        };
        let d = diff(
            &set(&req("Bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789")),
            &set(&req("Bearer {{token}}")),
        )
        .unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("ghp_"), "{json}");
        let auth = d.changes[0]
            .fields
            .iter()
            .find(|f| f.path == ["headers", "Authorization"])
            .unwrap();
        assert_eq!(auth.old, Some(DiffValue::Secret));
        // The new value is only a placeholder: it stays readable.
        assert!(matches!(auth.new, Some(DiffValue::Text { .. })));
    }
}
