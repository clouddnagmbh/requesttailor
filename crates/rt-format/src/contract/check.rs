// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection against the spec: every request must hit an operation, send what it
//! declares and nothing else, and its body must fit the operation's request body.

use std::collections::BTreeSet;

use rt_core::model::{Body, RawKind, RequestEntry};
use serde_json::Value;

use super::matching::{self, Match, Seg};
use super::schema;
use super::spec::{Operation, Spec};
use super::{ContractError, ContractFinding, ContractReport, FindingKind};
use crate::{FileSet, read_collection};

/// Headers every client sends; the spec does not need to declare them.
const COMMON_HEADERS: &[&str] = &[
    "accept",
    "accept-charset",
    "accept-encoding",
    "accept-language",
    "authorization",
    "cache-control",
    "connection",
    "content-length",
    "content-type",
    "cookie",
    "host",
    "if-match",
    "if-modified-since",
    "if-none-match",
    "if-unmodified-since",
    "origin",
    "pragma",
    "referer",
    "user-agent",
    "x-requested-with",
];

const MAX_BODY_FINDINGS: usize = 5;

pub(super) fn check(files: &FileSet, spec: &Spec) -> Result<ContractReport, ContractError> {
    let doc = read_collection(files)
        .map_err(|e| ContractError::Unsupported(format!("cannot read the collection: {e}")))?;
    let mut report = ContractReport {
        operations: spec.ops.len() as u32,
        ..ContractReport::default()
    };
    let mut covered: BTreeSet<&str> = BTreeSet::new();
    for entry in matching::requests(doc.model()) {
        let url = matching::parse_url(&entry.spec.url);
        match matching::find(spec, &entry.spec.method, &url) {
            Match::Op(op) => {
                covered.insert(&op.key);
                check_request(spec, op, entry, &url, &mut report.findings);
            }
            Match::OtherMethods(methods) => report.findings.push(ContractFinding {
                request: entry.path.clone(),
                kind: FindingKind::NoOperation,
                operation: None,
                message: format!(
                    "the spec has no {} for this path, only {}",
                    entry.spec.method.to_uppercase(),
                    methods.join(", ")
                ),
            }),
            Match::None => report.findings.push(ContractFinding {
                request: entry.path.clone(),
                kind: FindingKind::NoOperation,
                operation: None,
                message: format!(
                    "no operation of the spec matches {} {}",
                    entry.spec.method.to_uppercase(),
                    entry.spec.url
                ),
            }),
        }
    }
    report.covered = covered.len() as u32;
    report.uncovered = spec
        .ops
        .iter()
        .filter(|o| !covered.contains(o.key.as_str()))
        .map(|o| o.key.clone())
        .collect();
    Ok(report)
}

fn check_request(
    spec: &Spec,
    op: &Operation,
    entry: &RequestEntry,
    url: &matching::ReqUrl,
    out: &mut Vec<ContractFinding>,
) {
    let mut add = |kind, message: String| {
        out.push(ContractFinding {
            request: entry.path.clone(),
            kind,
            operation: Some(op.key.clone()),
            message,
        });
    };

    // Query parameters: the URL is the truth.
    for name in &url.query {
        let known = op
            .params
            .iter()
            .any(|p| p.location == "query" && &p.name == name)
            || spec.key_queries.contains(name);
        if !known {
            add(
                FindingKind::UnknownParameter,
                format!("query parameter `{name}` is not declared by {}", op.key),
            );
        }
    }
    // Headers.
    let headers: Vec<&str> = entry
        .spec
        .headers
        .iter()
        .filter(|h| h.enabled)
        .map(|h| h.name.as_str())
        .collect();
    for name in &headers {
        let lower = name.to_ascii_lowercase();
        let known = COMMON_HEADERS.contains(&lower.as_str())
            || op
                .params
                .iter()
                .any(|p| p.location == "header" && p.name.eq_ignore_ascii_case(name))
            || spec
                .key_headers
                .iter()
                .any(|k| k.eq_ignore_ascii_case(name));
        if !known && !name.contains("{{") {
            add(
                FindingKind::UnknownParameter,
                format!("header `{name}` is not declared by {}", op.key),
            );
        }
    }
    // Required parameters.
    for p in op.params.iter().filter(|p| p.required) {
        let present = match p.location.as_str() {
            "query" => url.query.contains(&p.name),
            "header" => headers.iter().any(|h| h.eq_ignore_ascii_case(&p.name)),
            _ => true,
        };
        if !present {
            add(
                FindingKind::MissingParameter,
                format!("required {} parameter `{}` is not sent", p.location, p.name),
            );
        }
    }
    // Path parameter names: `:name` against the template's `{name}`.
    let k = url.segs.len().saturating_sub(op.segs.len());
    for (spec_seg, req_seg) in op.segs.iter().zip(&url.segs[k..]) {
        if let (Seg::Colon(name), Some(want)) = (
            req_seg,
            spec_seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')),
        ) && name != want
        {
            add(
                FindingKind::UnknownParameter,
                format!(
                    "path parameter `{name}` is not declared; {} has `{want}`",
                    op.key
                ),
            );
        }
    }
    // Body.
    for message in body_problems(spec, op, &entry.spec.body, &entry.spec.headers) {
        add(FindingKind::BodyMismatch, message);
    }
}

fn mime_of(body: &Body, headers: &[rt_core::model::KeyValue]) -> Option<String> {
    let explicit = || {
        headers
            .iter()
            .find(|h| {
                h.enabled && h.name.eq_ignore_ascii_case("content-type") && !h.value.contains("{{")
            })
            .map(|h| {
                h.value
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            })
    };
    Some(match body {
        Body::None => return None,
        Body::Raw { kind, .. } => match explicit() {
            Some(m) if !m.is_empty() => m,
            _ => match kind {
                RawKind::Json => "application/json",
                RawKind::Text => "text/plain",
                RawKind::Xml => "application/xml",
                RawKind::Sparql => "application/sparql-query",
            }
            .into(),
        },
        Body::FormUrlEncoded { .. } => "application/x-www-form-urlencoded".into(),
        Body::Multipart { .. } => "multipart/form-data".into(),
        Body::File { content_type, .. } => content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".into())
            .to_ascii_lowercase(),
        Body::GraphQl { .. } => "application/json".into(),
        #[allow(unreachable_patterns)]
        _ => return None,
    })
}

fn media_fits(pattern: &str, mime: &str) -> bool {
    let pattern = pattern
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if pattern == "*/*" || pattern == mime {
        return true;
    }
    if let Some(t) = pattern.strip_suffix("/*") {
        return mime.split('/').next() == Some(t);
    }
    pattern.ends_with("json") && mime.ends_with("json")
}

fn body_problems(
    spec: &Spec,
    op: &Operation,
    body: &Body,
    headers: &[rt_core::model::KeyValue],
) -> Vec<String> {
    let Some(mime) = mime_of(body, headers) else {
        return match &op.body {
            Some(b) if b.required => vec![format!("{} requires a request body", op.key)],
            _ => Vec::new(),
        };
    };
    let Some(spec_body) = &op.body else {
        return vec![format!("{} takes no request body", op.key)];
    };
    if spec_body.content.is_empty() {
        return Vec::new();
    }
    let Some((_, schema_value)) = spec_body.content.iter().find(|(m, _)| media_fits(m, &mime))
    else {
        let accepted: Vec<&str> = spec_body.content.iter().map(|(m, _)| m.as_str()).collect();
        return vec![format!(
            "the body is {mime}, {} accepts {}",
            op.key,
            accepted.join(", ")
        )];
    };
    if schema_value.is_null() {
        return Vec::new();
    }
    match body {
        Body::Raw { data, .. } if mime.ends_with("json") => {
            match serde_json::from_str::<Value>(data) {
                Ok(instance) => {
                    schema::validate(&spec.root, schema_value, &instance, MAX_BODY_FINDINGS)
                        .into_iter()
                        .map(|m| format!("body {m}"))
                        .collect()
                }
                // Not JSON yet: templates outside strings. Nothing to judge.
                Err(_) => Vec::new(),
            }
        }
        Body::FormUrlEncoded { fields } => {
            let names: Vec<&str> = fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| f.name.as_str())
                .collect();
            missing_fields(spec, schema_value, &names)
        }
        Body::Multipart { parts } => {
            let names: Vec<&str> = parts
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.name.as_str())
                .collect();
            missing_fields(spec, schema_value, &names)
        }
        _ => Vec::new(),
    }
}

fn missing_fields(spec: &Spec, schema_value: &Value, names: &[&str]) -> Vec<String> {
    let eff = schema::effective(&spec.root, schema_value);
    eff.required
        .iter()
        .filter(|r| !names.contains(&r.as_str()))
        .map(|r| format!("body: required field `{r}` is missing"))
        .collect()
}
