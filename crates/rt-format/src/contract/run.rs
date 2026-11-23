// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The run wizard ("Ready to send?") and the response check: a request against the
//! operation it matches, before sending (what is missing or invalid) and after (where the
//! response differs from the declared responses). Validation is strict (`strict`).

use rt_core::model::{Body, ParamKind, Protocol, RequestSpec};
use serde_json::Value;

use super::check::{media_fits, mime_of};
use super::matching::{self, Match, ReqUrl};
use super::spec::{Media, Operation, Param, RunBody, Spec};
use super::strict::{self, Style, Violation};
use super::{
    DifferencePlace, InputField, InputPlace, InputProblem, MAX_CHECKED_BODY, ProblemKind,
    RequestFacts, ResponseCheck, ResponseDifference, ResponseFacts, SendRequirements, ServerOption,
    VarFact, VarState,
};

/// Details of one input problem.
const MAX_DETAILS: usize = 5;
/// Differences of one response.
const MAX_DIFFERENCES: usize = 20;
/// Headers the wizard does not ask for: the client and the auth set them.
const SKIPPED_HEADERS: &[&str] = &["accept", "content-type", "authorization"];

fn operation_of<'a>(spec: &'a Spec, request: &RequestSpec) -> Option<(&'a Operation, ReqUrl)> {
    if !matches!(request.protocol, Protocol::Http)
        || matches!(request.method.to_ascii_uppercase().as_str(), "GRPC" | "WS")
    {
        return None;
    }
    let url = matching::parse_url(&request.url);
    match matching::find(spec, &request.method, &url) {
        Match::Op(op) => Some((op, url)),
        _ => None,
    }
}

pub(super) fn requirements(spec: &Spec, facts: &RequestFacts<'_>) -> Option<SendRequirements> {
    let request = facts.request;
    let (op, url) = operation_of(spec, request)?;
    let mut fields = Vec::new();
    for fact in facts.variables.iter().filter(|f| f.state != VarState::Set) {
        fields.push(variable_field(spec, fact));
    }
    path_fields(spec, op, request, &url, &mut fields);
    let query = query_pairs(&request.url);
    for p in op
        .params
        .iter()
        .filter(|p| p.location == "query" && !spec.key_queries.contains(&p.name))
    {
        let values: Vec<&str> = query
            .iter()
            .filter(|(n, _)| *n == p.name)
            .map(|(_, v)| v.as_str())
            .collect();
        fields.push(param_field(spec, p, InputPlace::Query, &values));
    }
    for p in op.params.iter().filter(|p| {
        p.location == "header"
            && !SKIPPED_HEADERS.contains(&p.name.to_ascii_lowercase().as_str())
            && !spec
                .key_headers
                .iter()
                .any(|k| k.eq_ignore_ascii_case(&p.name))
    }) {
        let values: Vec<&str> = request
            .headers
            .iter()
            .filter(|h| h.enabled && h.name.eq_ignore_ascii_case(&p.name))
            .map(|h| h.value.as_str())
            .collect();
        fields.push(param_field(spec, p, InputPlace::Header, &values));
    }
    if let Some(body) = &op.run_body {
        fields.push(body_field(spec, body, request));
    }
    // Stable: problem fields first, each part in the order variables, path, query, header,
    // body.
    fields.sort_by_key(|f| f.problem.is_none());
    let ready = fields.iter().all(|f| f.problem.is_none());
    Some(SendRequirements {
        operation: op.key.clone(),
        summary: (!op.summary.trim().is_empty()).then(|| op.summary.clone()),
        deprecated: op.deprecated,
        spec: String::new(),
        ready,
        fields,
        servers: spec
            .servers
            .iter()
            .map(|s| ServerOption {
                url: s.url.clone(),
                description: s.description.clone(),
            })
            .collect(),
    })
}

fn missing(message: String) -> Option<InputProblem> {
    Some(InputProblem {
        kind: ProblemKind::Missing,
        message,
        details: Vec::new(),
    })
}

fn invalid_of(violations: Vec<Violation>) -> Option<InputProblem> {
    let mut it = violations.into_iter();
    let first = it.next()?;
    Some(InputProblem {
        kind: ProblemKind::Invalid,
        message: first.detail(),
        details: it.map(|v| v.detail()).collect(),
    })
}

/// A value as the form shows it: strings bare, the rest as JSON.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn description_of(spec: &Spec, schema: &Value) -> Option<String> {
    super::schema::resolve(&spec.root, schema)
        .get("description")
        .and_then(Value::as_str)
        .filter(|d| !d.trim().is_empty())
        .map(str::to_owned)
}

fn variable_field(spec: &Spec, fact: &VarFact) -> InputField {
    let name = &fact.name;
    let (schema, description, example) = if name.eq_ignore_ascii_case("baseUrl") {
        (
            serde_json::json!({ "type": "string", "format": "uri" }),
            None,
            spec.servers.first().map(|s| s.url.clone()),
        )
    } else if let Some((_, v)) = spec.server_vars.iter().find(|(n, _)| n == name) {
        let mut schema = serde_json::json!({ "type": "string" });
        for key in ["enum", "default"] {
            if let Some(x) = v.get(key) {
                schema[key] = x.clone();
            }
        }
        let description = v
            .get("description")
            .and_then(Value::as_str)
            .filter(|d| !d.trim().is_empty())
            .map(str::to_owned);
        let example = v
            .get("default")
            .or_else(|| v.get("enum").and_then(|e| e.get(0)))
            .map(text_of);
        (schema, description, example)
    } else {
        (serde_json::json!({ "type": "string" }), None, None)
    };
    let message = match fact.state {
        VarState::Empty => format!("{name} is empty"),
        _ => format!("{name} has no value"),
    };
    InputField {
        id: format!("variable:{name}"),
        place: InputPlace::Variable,
        name: name.clone(),
        required: true,
        secret: fact.secret,
        auth: fact.auth,
        value: if fact.secret {
            None
        } else {
            fact.value.clone()
        },
        problem: missing(message),
        schema: Some(schema),
        description,
        example,
    }
}

fn place_id(place: InputPlace) -> &'static str {
    match place {
        InputPlace::Variable => "variable",
        InputPlace::Path => "path",
        InputPlace::Query => "query",
        InputPlace::Header => "header",
        InputPlace::Body => "body",
    }
}

/// A path, query or header parameter with the values the request sends (each occurrence).
fn param_field(spec: &Spec, p: &Param, place: InputPlace, values: &[&str]) -> InputField {
    let present: Vec<&str> = values
        .iter()
        .copied()
        .filter(|v| !v.trim().is_empty())
        .collect();
    let problem = if present.is_empty() {
        if p.required {
            missing(format!("{} is required", p.name))
        } else {
            None
        }
    } else if present.iter().any(|v| v.contains("{{")) || p.strict.is_null() {
        // A template is judged by its variable, not here.
        None
    } else {
        let style = Style {
            style: p.style.as_deref(),
            explode: p.explode,
        };
        let inst = strict::coerce(&spec.root, &p.strict, &present, &style);
        invalid_of(strict::validate(&spec.root, &p.strict, &inst, MAX_DETAILS))
    };
    param_input(
        spec,
        p,
        place,
        (!values.is_empty()).then(|| values.join(",")),
        problem,
    )
}

fn param_input(
    spec: &Spec,
    p: &Param,
    place: InputPlace,
    value: Option<String>,
    problem: Option<InputProblem>,
) -> InputField {
    let has_schema = p.strict.as_object().is_some_and(|o| !o.is_empty());
    InputField {
        id: format!("{}:{}", place_id(place), p.name),
        place,
        name: p.name.clone(),
        required: p.required,
        secret: false,
        auth: false,
        value,
        problem,
        schema: has_schema.then(|| strict::inline(&spec.root, &p.strict)),
        description: p
            .description
            .clone()
            .or_else(|| description_of(spec, &p.strict)),
        example: p
            .example
            .clone()
            .or_else(|| strict::own_example(&spec.root, &p.strict))
            .filter(|v| !v.is_null())
            .map(|v| text_of(&v)),
    }
}

/// Path parameters in the order of the template, with the value the request puts there: a
/// literal segment, a `{{template}}`, or the value of the `:name` path parameter.
fn path_fields(
    spec: &Spec,
    op: &Operation,
    request: &RequestSpec,
    url: &ReqUrl,
    out: &mut Vec<InputField>,
) {
    let k = url.segs.len().saturating_sub(op.segs.len());
    for (i, seg) in op.segs.iter().enumerate() {
        let names = template_names(seg);
        if names.is_empty() {
            continue;
        }
        let raw = url.raw.get(k + i).map(String::as_str);
        for name in &names {
            let Some(p) = op
                .params
                .iter()
                .find(|p| p.location == "path" && p.name == *name)
            else {
                continue;
            };
            let value = match raw {
                None => None,
                // `{a}.{b}`: not split; judged by nobody.
                Some(r) if names.len() > 1 => Some(r.to_owned()),
                Some(r) => {
                    let a = seg.find('{').unwrap_or(0);
                    let b = seg.rfind('}').map_or(seg.len(), |b| b + 1);
                    let (pre, post) = (&seg[..a], &seg[b..]);
                    let inner = if r.len() >= pre.len() + post.len()
                        && r.starts_with(pre)
                        && r.ends_with(post)
                    {
                        &r[pre.len()..r.len() - post.len()]
                    } else {
                        r
                    };
                    match inner.strip_prefix(':') {
                        Some(param) if !inner.contains("{{") => request
                            .params
                            .iter()
                            .find(|x| x.enabled && x.kind == ParamKind::Path && x.name == param)
                            .map(|x| x.value.clone()),
                        _ if inner.contains("{{") => Some(inner.to_owned()),
                        _ => Some(percent_decode(inner, false)),
                    }
                }
            };
            let problem = match &value {
                None => missing(format!("{} is required", p.name)),
                Some(v) if v.trim().is_empty() => missing(format!("{} is required", p.name)),
                Some(v) if v.contains("{{") || names.len() > 1 || p.strict.is_null() => None,
                Some(v) => {
                    let style = Style {
                        style: p.style.as_deref(),
                        explode: p.explode,
                    };
                    let inst = strict::coerce(&spec.root, &p.strict, &[v.as_str()], &style);
                    invalid_of(strict::validate(&spec.root, &p.strict, &inst, MAX_DETAILS))
                }
            };
            out.push(param_input(spec, p, InputPlace::Path, value, problem));
        }
    }
}

/// Names of `{name}` in a template segment.
fn template_names(seg: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = seg;
    while let Some(a) = rest.find('{') {
        let after = &rest[a + 1..];
        let Some(b) = after.find('}') else { break };
        if b > 0 {
            out.push(&after[..b]);
        }
        rest = &after[b + 1..];
    }
    out
}

/// The query string's pairs, decoded; names with templates left out.
fn query_pairs(url: &str) -> Vec<(String, String)> {
    let url = url.split('#').next().unwrap_or("");
    let Some((_, query)) = url.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|pair| {
            let (n, v) = pair.split_once('=').unwrap_or((pair, ""));
            if n.is_empty() || n.contains("{{") {
                return None;
            }
            let value = if v.contains("{{") {
                v.to_owned()
            } else {
                percent_decode(v, true)
            };
            Some((percent_decode(n, true), value))
        })
        .collect()
}

fn percent_decode(s: &str, plus: bool) -> String {
    if !(s.contains('%') || (plus && s.contains('+'))) {
        return s.to_owned();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                        continue;
                    }
                    _ => out.push(b'%'),
                }
            }
            b'+' if plus => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_json(mime: &str) -> bool {
    let m = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    m.ends_with("/json") || m.ends_with("+json")
}

fn body_text(body: &Body) -> Option<String> {
    match body {
        Body::None | Body::File { .. } => None,
        Body::Raw { data, .. } => Some(data.clone()),
        Body::GraphQl { query, .. } => Some(query.clone()),
        Body::FormUrlEncoded { fields } => Some(
            fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| format!("{}={}", f.name, f.value))
                .collect::<Vec<_>>()
                .join("&"),
        ),
        Body::Multipart { parts } => Some(
            parts
                .iter()
                .filter(|p| p.enabled)
                .map(|p| format!("{}={}", p.name, p.values.join(",")))
                .collect::<Vec<_>>()
                .join("&"),
        ),
    }
}

fn body_field(spec: &Spec, body: &RunBody, request: &RequestSpec) -> InputField {
    let mime = mime_of(&request.body, &request.headers);
    let fitting = mime
        .as_deref()
        .and_then(|m| body.media.iter().find(|x| media_fits(&x.mime, m)));
    let media: Option<&Media> = fitting
        .or_else(|| body.media.iter().find(|x| is_json(&x.mime)))
        .or_else(|| body.media.first());
    let schema_value = media.map(|m| &m.schema).filter(|s| !s.is_null());
    let json_media = media.is_some_and(|m| is_json(&m.mime));
    let example = media
        .and_then(|m| m.example.clone())
        .or_else(|| schema_value.map(|s| strict::sample(&spec.root, s)))
        .filter(|v| !v.is_null())
        .map(|v| match (&v, json_media) {
            (Value::String(s), false) => s.clone(),
            _ => serde_json::to_string_pretty(&v).unwrap_or_default(),
        });
    let problem = body_problem(spec, body, request, mime.as_deref(), fitting, schema_value);
    InputField {
        id: "body".into(),
        place: InputPlace::Body,
        name: "body".into(),
        required: body.required,
        secret: false,
        auth: false,
        value: body_text(&request.body),
        problem,
        schema: schema_value.map(|s| strict::inline(&spec.root, s)),
        description: schema_value.and_then(|s| description_of(spec, s)),
        example,
    }
}

fn body_problem(
    spec: &Spec,
    body: &RunBody,
    request: &RequestSpec,
    mime: Option<&str>,
    fitting: Option<&Media>,
    schema: Option<&Value>,
) -> Option<InputProblem> {
    let Some(mime) = mime else {
        return body
            .required
            .then(|| "a request body is required".to_owned())
            .and_then(missing);
    };
    if fitting.is_none() && !body.media.is_empty() {
        let accepted: Vec<&str> = body.media.iter().map(|m| m.mime.as_str()).collect();
        return Some(InputProblem {
            kind: ProblemKind::Invalid,
            message: format!(
                "the body is {mime}, the operation accepts {}",
                accepted.join(", ")
            ),
            details: Vec::new(),
        });
    }
    let schema = schema?;
    match &request.body {
        Body::Raw { data, .. } if is_json(mime) => match serde_json::from_str::<Value>(data) {
            Ok(inst) => {
                let found = strict::validate(&spec.root, schema, &inst, MAX_DETAILS);
                (!found.is_empty()).then(|| InputProblem {
                    kind: ProblemKind::Invalid,
                    message: "the body does not match the schema".into(),
                    details: found
                        .iter()
                        .map(|v| format!("{}: {}", v.path, v.text))
                        .collect(),
                })
            }
            // Templates outside strings: not JSON until sent. Nothing to judge.
            Err(_) if data.contains("{{") => None,
            Err(e) => Some(InputProblem {
                kind: ProblemKind::Invalid,
                message: "the body is not valid JSON".into(),
                details: vec![e.to_string()],
            }),
        },
        Body::FormUrlEncoded { fields } => {
            let names: Vec<&str> = fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| f.name.as_str())
                .collect();
            missing_fields(spec, schema, &names)
        }
        Body::Multipart { parts } => {
            let names: Vec<&str> = parts
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.name.as_str())
                .collect();
            missing_fields(spec, schema, &names)
        }
        _ => None,
    }
}

fn missing_fields(spec: &Spec, schema: &Value, names: &[&str]) -> Option<InputProblem> {
    let eff = super::schema::effective(&spec.root, schema);
    let details: Vec<String> = eff
        .required
        .iter()
        .filter(|r| !names.contains(&r.as_str()))
        .map(|r| format!("{r}: required field is missing"))
        .collect();
    (!details.is_empty()).then(|| InputProblem {
        kind: ProblemKind::Missing,
        message: "the body is missing required fields".into(),
        details,
    })
}

// ---- the response check ----

fn one_of(list: &[&str]) -> String {
    match list {
        [one] => (*one).to_owned(),
        _ => format!("one of {}", list.join(", ")),
    }
}

pub(super) fn validate_response(
    spec: &Spec,
    request: &RequestSpec,
    response: &ResponseFacts<'_>,
) -> Option<ResponseCheck> {
    let (op, _) = operation_of(spec, request)?;
    let status = response.status;
    let code = status.to_string();
    let class = format!("{}XX", status / 100);
    let reply = op
        .replies
        .iter()
        .find(|r| r.status == code)
        .or_else(|| {
            op.replies
                .iter()
                .find(|r| r.status.eq_ignore_ascii_case(&class))
        })
        .or_else(|| op.replies.iter().find(|r| r.status == "default"));
    let mut check = ResponseCheck {
        operation: op.key.clone(),
        status,
        matched: reply.map(|r| r.status.clone()),
        differences: Vec::new(),
        body_skipped: None,
    };
    let Some(reply) = reply else {
        if op.replies.is_empty() {
            check.body_skipped = Some("the spec declares no responses for this operation".into());
        } else {
            let declared: Vec<&str> = op.replies.iter().map(|r| r.status.as_str()).collect();
            check.differences.push(ResponseDifference {
                place: DifferencePlace::Status,
                path: "status".into(),
                expected: one_of(&declared),
                got: code.clone(),
            });
            check.body_skipped = Some(format!("the spec declares no response for status {code}"));
        }
        return Some(check);
    };

    let header = |name: &str| {
        response
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    for h in &reply.headers {
        if h.name.eq_ignore_ascii_case("content-type") {
            continue;
        }
        match header(&h.name) {
            None if h.required => check.differences.push(ResponseDifference {
                place: DifferencePlace::Header,
                path: h.name.clone(),
                expected: "present".into(),
                got: "missing".into(),
            }),
            Some(v) if !h.schema.is_null() => {
                let inst = strict::coerce(
                    &spec.root,
                    &h.schema,
                    &[v],
                    &Style {
                        style: None,
                        explode: None,
                    },
                );
                for found in strict::validate(&spec.root, &h.schema, &inst, 1) {
                    check.differences.push(ResponseDifference {
                        place: DifferencePlace::Header,
                        path: h.name.clone(),
                        expected: found.expected,
                        got: found.got,
                    });
                }
            }
            _ => {}
        }
    }

    let content_type = header("content-type").map(|v| {
        v.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    });
    let empty = response.body.is_some_and(<[u8]>::is_empty);
    if reply.media.is_empty() {
        if !empty && response.body.is_some() {
            check.body_skipped = Some("the spec declares no body for this response".into());
        }
        return Some(check);
    }
    let media = match &content_type {
        Some(ct) => match reply.media.iter().find(|m| media_fits(&m.mime, ct)) {
            Some(m) => m,
            None => {
                let declared: Vec<&str> = reply.media.iter().map(|m| m.mime.as_str()).collect();
                check.differences.push(ResponseDifference {
                    place: DifferencePlace::ContentType,
                    path: "Content-Type".into(),
                    expected: one_of(&declared),
                    got: ct.clone(),
                });
                check.body_skipped = Some("the content type is not the declared one".into());
                return Some(check);
            }
        },
        None => {
            if empty {
                check.body_skipped = Some("the response has no body".into());
                return Some(check);
            }
            let declared: Vec<&str> = reply.media.iter().map(|m| m.mime.as_str()).collect();
            check.differences.push(ResponseDifference {
                place: DifferencePlace::ContentType,
                path: "Content-Type".into(),
                expected: one_of(&declared),
                got: "none".into(),
            });
            match reply.media.iter().find(|m| is_json(&m.mime)) {
                Some(m) => m,
                None => return Some(check),
            }
        }
    };
    if media.schema.is_null() {
        check.body_skipped = Some("the spec gives no schema for this body".into());
        return Some(check);
    }
    let json = content_type
        .as_deref()
        .map_or(is_json(&media.mime), is_json);
    if !json {
        check.body_skipped = Some(format!(
            "only JSON bodies are checked, this one is {}",
            content_type.as_deref().unwrap_or(&media.mime)
        ));
        return Some(check);
    }
    let Some(body) = response.body else {
        check.body_skipped = Some("the body was not kept in full".into());
        return Some(check);
    };
    if body.len() > MAX_CHECKED_BODY {
        check.body_skipped = Some(format!(
            "the body is larger than {} MB",
            MAX_CHECKED_BODY / (1024 * 1024)
        ));
        return Some(check);
    }
    if body.is_empty() {
        check.body_skipped = Some("the response has no body".into());
        return Some(check);
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(inst) => {
            for v in strict::validate(&spec.root, &media.schema, &inst, MAX_DIFFERENCES) {
                check.differences.push(ResponseDifference {
                    place: DifferencePlace::Body,
                    path: v.path,
                    expected: v.expected,
                    got: v.got,
                });
            }
        }
        Err(_) => check.differences.push(ResponseDifference {
            place: DifferencePlace::Body,
            path: "$".into(),
            expected: "JSON".into(),
            got: "text that is not valid JSON".into(),
        }),
    }
    Some(check)
}
