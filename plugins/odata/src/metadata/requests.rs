// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Requests from a model ([`requests_for`]): what the entity browser adds to a collection and
//! what the `edmx` importer writes.
//!
//! Per entity set a folder named after the set:
//! - `List <set>`: `GET {{baseUrl}}<set>?$top=20` (V2 also `$format=json`);
//! - `Read <set>`: `GET` by key, with sample key literals by type (`'1'`, `1`, a zero GUID,
//!   `true`, ...; V2 typed literals such as `guid'...'`, `datetime'...'`);
//! - `Create <set>` (if insertable): `POST` with a JSON body of the creatable properties
//!   (not computed, not `sap:creatable="false"`, no draft administration fields);
//! - `Update <set>` (if updatable): `PATCH` with the updatable properties. V2 too: SAP Gateway
//!   and CAP accept `PATCH` like `MERGE`, and `PATCH` is what every HTTP stack sends. On a
//!   draft-enabled set the key addresses the draft (`IsActiveEntity=false`), as RAP and CAP
//!   only change drafts;
//! - `Delete <set>` (if deletable).
//!
//! Per operation one request: functions with `GET` (V4 parameters inline, V2 in the query),
//! actions with `POST` (V4 parameters as JSON body, V2 in the query). Bound operations go to
//! the folder of the first entity set of their type, unbound ones to the collection root.
//!
//! V2 requests ask for JSON (`$format=json` on reads, `Accept: application/json` on writes).
//! URLs start with `{{baseUrl}}`, the service root with its trailing `/`.

use rt_plugin_sdk::api::NewRequest;
use rt_plugin_sdk::model::{Body, KeyValue, Param, ParamKind, RawKind, RequestSpec};

use crate::edm::{EntitySet, EntityType, Model, Operation, OperationKind, Property, Version};

/// Draft administration properties of RAP and CAP draft entities; never in bodies.
const DRAFT_FIELDS: [&str; 3] = ["IsActiveEntity", "HasActiveEntity", "HasDraftEntity"];

pub fn requests_for(model: &Model) -> Vec<NewRequest> {
    let mut out: Vec<NewRequest> = Vec::new();
    for set in &model.entity_sets {
        let Some(ty) = model.entity_type(&set.entity_type) else {
            continue;
        };
        let folder = set.name.clone();
        let mut add = |name: String, spec: RequestSpec| push(&mut out, &folder, name, spec);
        add(format!("List {}", set.name), list(model, set));
        if ty.key.is_empty() {
            continue;
        }
        add(
            format!("Read {}", set.name),
            by_key(model, set, ty, "GET", true),
        );
        if set.insertable {
            let mut spec = request(model, "POST", format!("{{{{baseUrl}}}}{}", set.name));
            spec.body = json(body(model, ty, Purpose::Create));
            add(format!("Create {}", set.name), spec);
        }
        if set.updatable {
            let mut spec = by_key(model, set, ty, "PATCH", !set.draft);
            spec.body = json(body(model, ty, Purpose::Update));
            add(format!("Update {}", set.name), spec);
        }
        if set.deletable {
            add(
                format!("Delete {}", set.name),
                by_key(model, set, ty, "DELETE", true),
            );
        }
    }
    for op in &model.operations {
        if let Some((folder, spec)) = operation(model, op) {
            push(&mut out, &folder, op.name.clone(), spec);
        }
    }
    out
}

/// Adds a request with the next `seq` of its folder; a name taken in the folder gets ` (2)`.
fn push(out: &mut Vec<NewRequest>, folder: &str, name: String, mut spec: RequestSpec) {
    let in_folder: Vec<&NewRequest> = out.iter().filter(|r| r.folder == folder).collect();
    let mut unique = name.clone();
    let mut n = 1;
    while in_folder
        .iter()
        .any(|r| r.name.eq_ignore_ascii_case(&unique))
    {
        n += 1;
        unique = format!("{name} ({n})");
    }
    spec.name = unique.clone();
    spec.seq = Some((in_folder.len() + 1) as f64);
    out.push(NewRequest {
        folder: folder.to_owned(),
        name: unique,
        spec,
    });
}

/// A request without body; `params` mirror the URL's query.
fn request(model: &Model, method: &str, url: String) -> RequestSpec {
    let params = url
        .split_once('?')
        .map(|(_, q)| {
            q.split('&')
                .filter(|p| !p.is_empty())
                .map(|p| {
                    let (name, value) = p.split_once('=').unwrap_or((p, ""));
                    Param {
                        name: name.to_owned(),
                        value: value.to_owned(),
                        kind: ParamKind::Query,
                        enabled: true,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let mut headers = Vec::new();
    if model.version == Version::V2 && method != "GET" {
        headers.push(KeyValue::new("Accept", "application/json"));
    }
    RequestSpec {
        method: method.to_owned(),
        url,
        params,
        headers,
        ..RequestSpec::default()
    }
}

/// `url` with `$format=json` for V2 reads.
fn v2_json(model: &Model, method: &str, url: String) -> String {
    if model.version == Version::V2 && method == "GET" {
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}$format=json")
    } else {
        url
    }
}

fn list(model: &Model, set: &EntitySet) -> RequestSpec {
    let url = v2_json(model, "GET", format!("{{{{baseUrl}}}}{}?$top=20", set.name));
    request(model, "GET", url)
}

/// A request to one entity. `active` is the value of `IsActiveEntity` in draft keys.
fn by_key(
    model: &Model,
    set: &EntitySet,
    ty: &EntityType,
    method: &str,
    active: bool,
) -> RequestSpec {
    let path = format!("{}{}", set.name, key_predicate(model, ty, active));
    let url = v2_json(model, method, format!("{{{{baseUrl}}}}{path}"));
    request(model, method, url)
}

/// `('1')` for one key property, `(A='1',B=2)` for several.
pub fn key_predicate(model: &Model, ty: &EntityType, active: bool) -> String {
    let literal = |name: &str| {
        let ty_name = ty
            .properties
            .iter()
            .find(|p| p.name == name)
            .map_or("Edm.String", |p| p.r#type.as_str());
        if name == "IsActiveEntity" && ty_name == "Edm.Boolean" {
            active.to_string()
        } else {
            sample_literal(model.version, ty_name)
        }
    };
    match ty.key.as_slice() {
        [one] => format!("({})", literal(one)),
        keys => format!(
            "({})",
            keys.iter()
                .map(|k| format!("{k}={}", literal(k)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

/// A sample URL literal of an EDM primitive type.
pub fn sample_literal(version: Version, ty: &str) -> String {
    let v2 = version == Version::V2;
    let s = match ty {
        "Edm.Guid" if v2 => "guid'00000000-0000-0000-0000-000000000000'",
        "Edm.Guid" => "00000000-0000-0000-0000-000000000000",
        "Edm.Int16" | "Edm.Int32" | "Edm.Byte" | "Edm.SByte" => "1",
        "Edm.Int64" if v2 => "1L",
        "Edm.Int64" => "1",
        "Edm.Decimal" if v2 => "1M",
        "Edm.Double" if v2 => "1d",
        "Edm.Single" if v2 => "1f",
        "Edm.Decimal" | "Edm.Double" | "Edm.Single" => "1",
        "Edm.Boolean" => "true",
        "Edm.DateTime" => "datetime'2026-01-01T00:00:00'",
        "Edm.DateTimeOffset" if v2 => "datetimeoffset'2026-01-01T00:00:00Z'",
        "Edm.DateTimeOffset" => "2026-01-01T00:00:00Z",
        "Edm.Date" => "2026-01-01",
        "Edm.Time" => "time'PT00H00M00S'",
        "Edm.TimeOfDay" => "00:00:00",
        "Edm.Duration" => "duration'PT0S'",
        _ => "'1'",
    };
    s.to_owned()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Create,
    Update,
}

/// Body properties of `ty` for `purpose`, with sample values.
fn body(model: &Model, ty: &EntityType, purpose: Purpose) -> Json {
    let draft = DRAFT_FIELDS
        .iter()
        .all(|f| ty.properties.iter().any(|p| p.name == *f));
    let fields = ty
        .properties
        .iter()
        .filter(|p| !(draft && DRAFT_FIELDS.contains(&p.name.as_str())))
        .filter(|p| !flag(p, "computed", "true"))
        .filter(|p| match purpose {
            Purpose::Create => !flag(p, "creatable", "false"),
            Purpose::Update => {
                !ty.key.contains(&p.name)
                    && !flag(p, "updatable", "false")
                    && !flag(p, "immutable", "true")
            }
        })
        .map(|p| (p.name.clone(), sample_value(model, &p.r#type, 0)))
        .collect();
    Json::Object(fields)
}

fn flag(p: &Property, name: &str, value: &str) -> bool {
    p.annotations.iter().any(|(n, v)| n == name && v == value)
}

/// A sample JSON value of a type: primitive, complex (nested object) or collection (`[]`).
fn sample_value(model: &Model, ty: &str, depth: usize) -> Json {
    let v2 = model.version == Version::V2;
    let text = |s: &str| Json::String(s.to_owned());
    match ty {
        t if t.starts_with("Collection(") => Json::Array,
        "Edm.String" | "Edm.Binary" => text(""),
        "Edm.Boolean" => Json::Raw("false"),
        "Edm.Int16" | "Edm.Int32" | "Edm.Byte" | "Edm.SByte" => Json::Raw("0"),
        // V2 JSON writes 64-bit and decimal numbers as strings.
        "Edm.Int64" | "Edm.Decimal" | "Edm.Double" | "Edm.Single" if v2 => text("0"),
        "Edm.Int64" | "Edm.Decimal" | "Edm.Double" | "Edm.Single" => Json::Raw("0"),
        "Edm.Guid" => text("00000000-0000-0000-0000-000000000000"),
        "Edm.DateTime" => text("/Date(1767225600000)/"),
        "Edm.DateTimeOffset" if v2 => text("/Date(1767225600000+0000)/"),
        "Edm.DateTimeOffset" => text("2026-01-01T00:00:00Z"),
        "Edm.Date" => text("2026-01-01"),
        "Edm.Time" => text("PT00H00M00S"),
        "Edm.TimeOfDay" => text("00:00:00"),
        "Edm.Duration" => text("PT0S"),
        t => match model.complex_types.iter().find(|c| c.name == t) {
            Some(c) if depth < 4 => Json::Object(
                c.properties
                    .iter()
                    .map(|p| (p.name.clone(), sample_value(model, &p.r#type, depth + 1)))
                    .collect(),
            ),
            Some(_) => Json::Raw("{}"),
            // Enumerations and type definitions: a string is the usual JSON form.
            None => text(""),
        },
    }
}

/// One request for an operation, and its folder.
fn operation(model: &Model, op: &Operation) -> Option<(String, RequestSpec)> {
    let v4 = model.version == Version::V4;
    let (folder, path) = match &op.bound_to {
        None => (String::new(), op.name.clone()),
        Some(bound) => {
            let (ty_name, collection) = match bound
                .strip_prefix("Collection(")
                .and_then(|r| r.strip_suffix(')'))
            {
                Some(inner) => (inner, true),
                None => (bound.as_str(), false),
            };
            let set = model
                .entity_sets
                .iter()
                .find(|s| s.entity_type == ty_name)?;
            let ty = model.entity_type(ty_name)?;
            let target = if collection || ty.key.is_empty() {
                set.name.clone()
            } else {
                format!("{}{}", set.name, key_predicate(model, ty, true))
            };
            (
                set.name.clone(),
                format!("{target}/{}.{}", model.namespace, op.name),
            )
        }
    };
    let spec = match (op.kind, v4) {
        (OperationKind::Function, true) => {
            let args = op
                .parameters
                .iter()
                .map(|p| format!("{}={}", p.name, sample_literal(model.version, &p.r#type)))
                .collect::<Vec<_>>()
                .join(",");
            request(model, "GET", format!("{{{{baseUrl}}}}{path}({args})"))
        }
        (OperationKind::Action, true) => {
            let mut spec = request(model, "POST", format!("{{{{baseUrl}}}}{path}"));
            spec.body = json(Json::Object(
                op.parameters
                    .iter()
                    .map(|p| (p.name.clone(), sample_value(model, &p.r#type, 0)))
                    .collect(),
            ));
            spec
        }
        (kind, false) => {
            let method = if kind == OperationKind::Function {
                "GET"
            } else {
                "POST"
            };
            let query = op
                .parameters
                .iter()
                .map(|p| format!("{}={}", p.name, sample_literal(model.version, &p.r#type)))
                .collect::<Vec<_>>()
                .join("&");
            let mut url = format!("{{{{baseUrl}}}}{path}");
            if !query.is_empty() {
                url = format!("{url}?{query}");
            }
            request(model, method, v2_json(model, method, url))
        }
    };
    Some((folder, spec))
}

fn json(value: Json) -> Body {
    Body::Raw {
        kind: RawKind::Json,
        data: value.pretty(),
    }
}

/// A JSON value that keeps the order of object members (`serde_json` sorts them without its
/// `preserve_order` feature, and property order is what users expect).
#[derive(Debug, Clone)]
enum Json {
    Raw(&'static str),
    String(String),
    /// Always empty: sample collections.
    Array,
    Object(Vec<(String, Json)>),
}

impl Json {
    fn pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Json::Raw(r) => out.push_str(r),
            Json::String(s) => out.push_str(&quote(s)),
            Json::Array => out.push_str("[]"),
            Json::Object(members) if members.is_empty() => out.push_str("{}"),
            Json::Object(members) => {
                out.push_str("{\n");
                for (i, (name, value)) in members.iter().enumerate() {
                    out.push_str(&"  ".repeat(indent + 1));
                    out.push_str(&quote(name));
                    out.push_str(": ");
                    value.write(out, indent + 1);
                    if i + 1 < members.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&"  ".repeat(indent));
                out.push('}');
            }
        }
    }
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}
