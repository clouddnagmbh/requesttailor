// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OpenAPI 3 (JSON) as the SAP Business Accelerator Hub delivers it → requests.
//!
//! - One request per path and method; name from `summary`, else `operationId`, else
//!   `METHOD path`. Folder per first tag (folder docs from the top-level `tags`); operations
//!   without tags at the root. Paths are sorted, methods in the order GET, PUT, POST, DELETE,
//!   OPTIONS, HEAD, PATCH, TRACE (JSON objects carry no reliable order).
//! - URL `{{baseUrl}}` + path, `{key}` → `:key` (the core substitutes these, also inside
//!   OData key segments such as `A_Product(':Product')`), path parameters with their example
//!   or default.
//! - Required query parameters are in the URL; optional ones (`$top`, `$filter`, …) are
//!   disabled parameters, so nothing invalid such as an empty `$top=` is sent. Header
//!   parameters become headers, disabled when optional.
//! - Body: the media type's `example`, else the first of `examples`, else a skeleton
//!   generated from the schema (reported). Bodies that are not JSON, XML or text and have no
//!   example (`multipart/mixed` of `$batch`) are reported and left out.
//! - Responses, schemas, callbacks, other `x-sap-*` extensions: documentation, reported once.
//!
//! Local `$ref`s (`#/components/…`) are resolved with a bounded depth; others are reported.

use rt_plugin_sdk::model::{Body, KeyValue, Param, ParamKind, RawKind, RequestSpec};
use rt_plugin_sdk::{Loss, LossEffect, LossKind};
use serde_json::{Map, Value};

use super::files::{FolderDoc, RequestFile};

const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];
const MAX_DEPTH: usize = 8;
const MAX_REF_DEPTH: usize = 32;

/// What [`requests`] built.
#[derive(Debug, Default)]
pub struct Converted {
    pub requests: Vec<RequestFile>,
    pub folder_docs: Vec<FolderDoc>,
    pub losses: Vec<Loss>,
}

pub fn requests(root: &Value) -> Converted {
    let mut out = Converted::default();
    let tag_order: Vec<String> = root["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .collect();
    for tag in root["tags"].as_array().into_iter().flatten() {
        if let (Some(name), Some(docs)) = (tag["name"].as_str(), tag["description"].as_str()) {
            out.folder_docs.push(FolderDoc {
                folder: vec![name.to_owned()],
                docs: docs.to_owned(),
            });
        }
    }

    let empty = Map::new();
    let paths = root["paths"].as_object().unwrap_or(&empty);
    let mut keys: Vec<&String> = paths.keys().collect();
    keys.sort();
    let mut built: Vec<(usize, RequestFile)> = Vec::new();
    let mut generated = 0u32;
    let mut operation_security = false;
    let mut documentation = false;
    for path in keys {
        let item = resolve(root, &paths[path.as_str()], 0);
        let shared = item["parameters"].as_array().cloned().unwrap_or_default();
        for method in METHODS {
            let Some(op) = item.get(method).filter(|o| o.is_object()) else {
                continue;
            };
            let name = op["summary"]
                .as_str()
                .or(op["operationId"].as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{} {path}", method.to_ascii_uppercase()));
            let folder: Vec<String> = op["tags"]
                .as_array()
                .and_then(|t| t.first())
                .and_then(Value::as_str)
                .map(|t| vec![t.to_owned()])
                .unwrap_or_default();
            let location = match folder.first() {
                Some(f) => format!("{f} / {name}"),
                None => name.clone(),
            };
            let mut spec = RequestSpec {
                name: name.clone(),
                method: method.to_ascii_uppercase(),
                ..RequestSpec::default()
            };
            let mut query = Vec::new();
            for param in merged_parameters(root, &shared, op["parameters"].as_array()) {
                let Some(pname) = param["name"].as_str() else {
                    continue;
                };
                let required = param["required"].as_bool().unwrap_or(false);
                let value = param_value(root, &param, required);
                match param["in"].as_str().unwrap_or_default() {
                    "path" => spec.params.push(Param {
                        name: pname.to_owned(),
                        value,
                        kind: ParamKind::Path,
                        enabled: true,
                    }),
                    "query" => {
                        if required {
                            query.push(format!("{pname}={}", value.replace(' ', "%20")));
                        }
                        spec.params.push(Param {
                            name: pname.to_owned(),
                            value,
                            kind: ParamKind::Query,
                            enabled: required,
                        });
                    }
                    "header" => spec.headers.push(KeyValue {
                        name: pname.to_owned(),
                        value,
                        enabled: required,
                    }),
                    other => out.losses.push(loss(
                        &location,
                        LossKind::Other,
                        LossEffect::Dropped,
                        format!("{other} parameter `{pname}` not imported"),
                    )),
                }
            }
            // Query params first, then path params in URL order (as the built-in importer).
            spec.params.sort_by_key(|p| p.kind == ParamKind::Path);
            spec.url = format!("{{{{baseUrl}}}}{}", path_to_url(path));
            if !query.is_empty() {
                spec.url.push('?');
                spec.url.push_str(&query.join("&"));
            }

            if let Some(body) = op.get("requestBody") {
                let body = resolve(root, body, 0);
                match request_body(root, &body) {
                    BodyOut::Body {
                        content_type,
                        body,
                        generated: g,
                    } => {
                        spec.headers
                            .insert(0, KeyValue::new("Content-Type", content_type));
                        spec.body = body;
                        if g {
                            generated += 1;
                        }
                    }
                    BodyOut::Lost(detail) => out.losses.push(loss(
                        &location,
                        LossKind::Body,
                        LossEffect::Dropped,
                        detail,
                    )),
                    BodyOut::None => {}
                }
            }
            operation_security |= op.get("security").is_some();
            documentation |= op.get("callbacks").is_some() || op.get("responses").is_some();
            let docs = op["description"].as_str().map(str::to_owned);
            let rank = folder.first().map_or(usize::MAX, |f| {
                tag_order
                    .iter()
                    .position(|t| t == f)
                    .unwrap_or(tag_order.len())
            });
            built.push((rank, RequestFile { folder, spec, docs }));
        }
    }
    // Folders in the order of the top-level `tags`, then the others, the root last.
    built.sort_by_key(|(rank, _)| *rank);
    out.requests = built.into_iter().map(|(_, r)| r).collect();

    if generated > 0 {
        out.losses.push(loss(
            "collection",
            LossKind::Body,
            LossEffect::Approximated,
            format!(
                "{generated} request body skeleton(s) generated from the schema (no example in the specification)"
            ),
        ));
    }
    if operation_security {
        out.losses.push(loss(
            "collection",
            LossKind::Auth,
            LossEffect::Approximated,
            "per-operation security not imported; every request uses the collection's auth"
                .to_owned(),
        ));
    }
    if documentation || root.get("components").is_some() {
        out.losses.push(loss(
            "collection",
            LossKind::Documentation,
            LossEffect::Dropped,
            "responses, schemas and callbacks are API documentation and not imported".to_owned(),
        ));
    }
    out
}

pub fn loss(location: &str, kind: LossKind, effect: LossEffect, detail: String) -> Loss {
    Loss {
        location: location.to_owned(),
        kind,
        effect,
        detail,
    }
}

/// Path-level parameters overridden by the operation's (same `name` and `in`).
fn merged_parameters(root: &Value, shared: &[Value], own: Option<&Vec<Value>>) -> Vec<Value> {
    let mut out: Vec<Value> = shared.iter().map(|p| resolve(root, p, 0)).collect();
    for p in own.into_iter().flatten() {
        let p = resolve(root, p, 0);
        out.retain(|q| !(q["name"] == p["name"] && q["in"] == p["in"]));
        out.push(p);
    }
    out
}

/// The value a parameter gets: its example or default; for required ones else the first
/// enum value; else empty.
fn param_value(root: &Value, param: &Value, required: bool) -> String {
    let schema = resolve(root, &param["schema"], 0);
    let candidates = [
        &param["example"],
        &schema["example"],
        &schema["default"],
        &schema["items"]["default"],
    ];
    if let Some(v) = candidates.into_iter().find(|v| !v.is_null()) {
        return text(v);
    }
    if required && let Some(first) = schema["enum"].as_array().and_then(|e| e.first()) {
        return text(first);
    }
    String::new()
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(text).collect::<Vec<_>>().join(","),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

enum BodyOut {
    None,
    Body {
        content_type: String,
        body: Body,
        generated: bool,
    },
    Lost(String),
}

fn request_body(root: &Value, body: &Value) -> BodyOut {
    let Some(content) = body["content"].as_object().filter(|c| !c.is_empty()) else {
        return BodyOut::None;
    };
    let pick = content
        .iter()
        .find(|(t, _)| is_json(t))
        .or_else(|| content.iter().next());
    let Some((content_type, media)) = pick else {
        return BodyOut::None;
    };
    let base = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let example = if !media["example"].is_null() {
        Some(media["example"].clone())
    } else {
        media["examples"]
            .as_object()
            .and_then(|e| e.values().next())
            .map(|e| resolve(root, e, 0)["value"].clone())
            .filter(|v| !v.is_null())
    };
    if is_json(&base) {
        let (value, generated) = match example {
            Some(v) => (v, false),
            None => (sample(root, &media["schema"], 0), true),
        };
        let data = serde_json::to_string_pretty(&value).unwrap_or_default();
        return BodyOut::Body {
            content_type: content_type.clone(),
            body: Body::Raw {
                kind: RawKind::Json,
                data,
            },
            generated,
        };
    }
    let kind = if base.ends_with("xml") {
        RawKind::Xml
    } else if base.starts_with("text/") {
        RawKind::Text
    } else {
        return BodyOut::Lost(format!(
            "request body of type {content_type} not imported (write it by hand; for $batch the plugin's Batch tab)"
        ));
    };
    match example {
        Some(Value::String(data)) => BodyOut::Body {
            content_type: content_type.clone(),
            body: Body::Raw { kind, data },
            generated: false,
        },
        _ => BodyOut::Lost(format!(
            "request body of type {content_type} has no example and was not imported"
        )),
    }
}

fn is_json(content_type: &str) -> bool {
    let t = content_type.split(';').next().unwrap_or_default().trim();
    t.eq_ignore_ascii_case("application/json") || t.to_ascii_lowercase().ends_with("+json")
}

/// A JSON value for `schema`: its example or default, else built from its type.
pub fn sample(root: &Value, schema: &Value, depth: usize) -> Value {
    if depth > MAX_DEPTH {
        return Value::Null;
    }
    let schema = resolve(root, schema, 0);
    for key in ["example", "default"] {
        if !schema[key].is_null() {
            return schema[key].clone();
        }
    }
    if let Some(all) = schema["allOf"].as_array() {
        let mut merged = Map::new();
        for part in all {
            if let Value::Object(m) = sample(root, part, depth + 1) {
                merged.extend(m);
            }
        }
        return Value::Object(merged);
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(first) = schema[key].as_array().and_then(|a| a.first()) {
            return sample(root, first, depth + 1);
        }
    }
    if let Some(first) = schema["enum"].as_array().and_then(|e| e.first()) {
        return first.clone();
    }
    let kind = schema["type"]
        .as_str()
        .unwrap_or(if schema.get("properties").is_some() {
            "object"
        } else {
            ""
        });
    match kind {
        "object" => {
            let mut out = Map::new();
            for (name, prop) in schema["properties"].as_object().into_iter().flatten() {
                if resolve(root, prop, 0)["readOnly"].as_bool() == Some(true) {
                    continue;
                }
                out.insert(name.clone(), sample(root, prop, depth + 1));
            }
            Value::Object(out)
        }
        "array" => {
            let item = sample(root, &schema["items"], depth + 1);
            Value::Array(if item.is_null() {
                Vec::new()
            } else {
                vec![item]
            })
        }
        "integer" | "number" => Value::from(0),
        "boolean" => Value::Bool(false),
        "string" => Value::String(
            match schema["format"].as_str() {
                Some("date") => "2026-01-01",
                Some("date-time") => "2026-01-01T00:00:00Z",
                Some("decimal") => "0",
                _ => "string",
            }
            .to_owned(),
        ),
        _ => Value::Null,
    }
}

/// Follows local `$ref`s (`#/a/b`); a missing or foreign target gives `null`.
pub fn resolve(root: &Value, value: &Value, depth: usize) -> Value {
    match value["$ref"].as_str() {
        None => value.clone(),
        Some(_) if depth >= MAX_REF_DEPTH => Value::Null,
        Some(r) => match r.strip_prefix('#') {
            Some(pointer) => match root.pointer(pointer) {
                Some(target) => resolve(root, target, depth + 1),
                None => Value::Null,
            },
            None => Value::Null,
        },
    }
}

/// Every `$ref` that does not point into this document.
pub fn foreign_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(m) => {
            if let Some(r) = m.get("$ref").and_then(Value::as_str)
                && !r.starts_with('#')
                && !out.iter().any(|o| o == r)
            {
                out.push(r.to_owned());
            }
            m.values().for_each(|v| foreign_refs(v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| foreign_refs(v, out)),
        _ => {}
    }
}

/// `/A_Product('{Product}')` → `/A_Product(':Product')`.
pub fn path_to_url(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if end > 0 => {
                out.push(':');
                out.push_str(&after[..end]);
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths() {
        assert_eq!(
            path_to_url("/A_P('{Product}')/to_D"),
            "/A_P(':Product')/to_D"
        );
        assert_eq!(path_to_url("/S/{id}"), "/S/:id");
        assert_eq!(path_to_url("/x{}"), "/x{}");
    }

    #[test]
    fn samples() {
        let root = json!({"components": {"schemas": {"A": {"type": "object", "properties": {
            "s": {"type": "string"}, "n": {"type": "integer"}, "b": {"$ref": "#/components/schemas/B"},
            "r": {"type": "string", "readOnly": true}, "l": {"type": "array", "items": {"type": "boolean"}}
        }}, "B": {"type": "string", "format": "date"}}}});
        let v = sample(&root, &json!({"$ref": "#/components/schemas/A"}), 0);
        assert_eq!(
            v,
            json!({"s": "string", "n": 0, "b": "2026-01-01", "l": [false]})
        );
        // Cycles end at the depth bound.
        let cyclic = json!({"components": {"schemas": {"C": {"type": "object", "properties": {"c": {"$ref": "#/components/schemas/C"}}}}}});
        let _ = sample(&cyclic, &json!({"$ref": "#/components/schemas/C"}), 0);
        assert_eq!(
            resolve(&root, &json!({"$ref": "other.json#/x"}), 0),
            Value::Null
        );
    }
}
