// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Evidence and comparison of two answers: JSON structure, ids, and the masked, shortened
//! values an [`super::AccessEvidence`] may carry. Values are never copied from a body except
//! ids that look like identifiers; everything else is shown as a JSON path and a type.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{AccessEvidence, EVIDENCE_LIMIT};

/// Most paths one evidence lists, and most the structure comparison looks at.
const MAX_VALUES: usize = 24;
const MAX_PATHS: usize = 500;
const MAX_DEPTH: usize = 8;
/// An id longer than this is not shown.
const MAX_ID: usize = 64;

pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Replaces every secret value (raw, JSON- or Debug-escaped) in `text` with the mask.
pub fn mask_text(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        let json = serde_json::to_string(secret).unwrap_or_default();
        let json = json.trim_matches('"').to_owned();
        for form in [secret.clone(), json, secret.escape_debug().to_string()] {
            if !form.is_empty() && out.contains(&form) {
                out = out.replace(&form, rt_core::send::MASK);
            }
        }
    }
    out
}

/// `text` cut to at most `limit` bytes on a character boundary.
pub fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// The set of `path:type` strings of a JSON value (arrays collapse to `[]` over their first
/// three elements), the "structure" of an answer.
pub fn structure(value: &Value) -> BTreeSet<String> {
    fn walk(value: &Value, path: &str, depth: usize, out: &mut BTreeSet<String>) {
        if out.len() >= MAX_PATHS {
            return;
        }
        out.insert(format!("{path}:{}", type_name(value)));
        if depth >= MAX_DEPTH {
            return;
        }
        match value {
            Value::Object(map) => {
                for (key, v) in map {
                    walk(v, &format!("{path}.{key}"), depth + 1, out);
                }
            }
            Value::Array(items) => {
                for v in items.iter().take(3) {
                    walk(v, &format!("{path}[]"), depth + 1, out);
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(value, "$", 0, &mut out);
    out
}

/// A scalar as plain text.
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Whether `key` names an identifier (`id`, `orderId`, `order_id`).
fn is_id_key(key: &str) -> bool {
    key == "id" || key.ends_with("Id") || key.ends_with("_id")
}

/// Whether `text` is safe to show as an identifier: digits, a UUID or a short slug, never
/// something that reads like an address or a name.
pub fn looks_like_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_ID
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The object id of an answer: the top-level `id` of an object, or of the first element of an
/// array of objects.
pub fn object_id(value: &Value) -> Option<String> {
    let object = match value {
        Value::Object(_) => value,
        Value::Array(items) => items.first()?,
        _ => return None,
    };
    scalar(object.get("id")?)
}

/// Whether some identifier field near the top of `value` (depth 3) holds `id`.
pub fn holds_id(value: &Value, id: &str) -> bool {
    fn walk(value: &Value, id: &str, depth: usize) -> bool {
        match value {
            Value::Object(map) => map.iter().any(|(k, v)| {
                (is_id_key(k) && scalar(v).as_deref() == Some(id))
                    || (depth < 3 && walk(v, id, depth + 1))
            }),
            Value::Array(items) => items
                .iter()
                .take(20)
                .any(|v| depth < 3 && walk(v, id, depth + 1)),
            _ => false,
        }
    }
    walk(value, id, 0)
}

/// What is shown of one answer: the status, the header names and, for a JSON body, its paths
/// with types (an id-like value is shown, shortened and masked; no other value is).
pub fn evidence(
    profile: &str,
    status: Option<u16>,
    headers: &[(String, String)],
    body: Option<&str>,
    secrets: &[String],
) -> AccessEvidence {
    let mut header_names: Vec<String> = headers
        .iter()
        .map(|(n, _)| n.to_ascii_lowercase())
        .collect();
    header_names.sort();
    header_names.dedup();
    let mut values: Vec<(String, String)> = Vec::new();
    if let Some(text) = body.filter(|t| !t.is_empty()) {
        match serde_json::from_str::<Value>(text) {
            Ok(json) => list(&json, "$", 0, &mut values),
            Err(_) => values.push(("body".into(), format!("text, {} bytes", text.len()))),
        }
    }
    values.truncate(MAX_VALUES);
    let values = values
        .into_iter()
        .map(|(path, value)| {
            (
                cut(&mask_text(&path, secrets), EVIDENCE_LIMIT),
                cut(&mask_text(&value, secrets), EVIDENCE_LIMIT),
            )
        })
        .collect();
    AccessEvidence {
        profile: profile.to_owned(),
        status,
        header_names,
        values,
    }
}

fn list(value: &Value, path: &str, depth: usize, out: &mut Vec<(String, String)>) {
    if out.len() >= MAX_VALUES {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                let here = format!("{path}.{key}");
                match v {
                    Value::Object(_) | Value::Array(_) if depth < 3 => {
                        list(v, &here, depth + 1, out)
                    }
                    _ => {
                        let shown = scalar(v)
                            .filter(|s| is_id_key(key) && looks_like_id(s))
                            .unwrap_or_else(|| type_name(v).to_owned());
                        out.push((here, shown));
                    }
                }
            }
        }
        Value::Array(items) => {
            out.push((format!("{path}[]"), format!("array, {} items", items.len())));
            if let Some(first) = items.first()
                && depth < 3
            {
                list(first, &format!("{path}[0]"), depth + 1, out);
            }
        }
        other => out.push((path.to_owned(), type_name(other).to_owned())),
    }
}
