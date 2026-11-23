// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What a task tells the model: built from the open workspace and the response store, masked
//! before it is even assembled. Secret values are never read here (the snapshot with secret
//! values is not used): requests are the specs as written (templates), reports are the
//! masked reports of `rt_core::send`, and credentials that are not `{{variables}}` are
//! replaced by `***` by the name they sit under (see `mask`).

use rt_core::model::{Auth, Body, Collection, Folder, Item, RequestSpec, Variable};
use rt_core::send::{ReportBody, SendReport};
use serde::Serialize;
use serde_json::{Value, json};

use super::{AiTask, mask};
use crate::{AppError, ResponseStore, SendInput, Workspace};

/// Requests of the target folder shown for `CreateRequest`.
const MAX_SIBLINGS: usize = 12;
/// Bytes of a request body in the context.
const REQUEST_BODY_BYTES: usize = 8 * 1024;
/// Bytes of a sibling's body.
const SIBLING_BODY_BYTES: usize = 1024;
/// Bytes of a response body.
const RESPONSE_BODY_BYTES: usize = 16 * 1024;
/// Bytes of one script.
const SCRIPT_BYTES: usize = 4 * 1024;
/// Script log lines.
const MAX_LOGS: usize = 20;

/// The masked context of one task. Owned, so a shell can build it under its locks and send
/// without holding them.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AiContext {
    /// Collection name.
    pub collection: String,
    /// Selected environment.
    pub environment: Option<String>,
    /// Variables in scope, by name; secret ones as `name (secret)`, never with a value.
    pub variables: Vec<String>,
    /// `CreateRequest`: the target folder (`""` = the collection root).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// `CreateRequest`: the folder's requests (masked).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub siblings: Vec<Value>,
    /// `ExplainError`, `SuggestTests`: the request as written (masked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
    /// `ExplainError`, `SuggestTests`: what was sent and received (masked report).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Value>,
}

impl AiContext {
    /// The context of `task` in `workspace` (unsaved edits included) with the responses of
    /// `responses`. `input` selects the environment; its values are not read.
    pub fn new(
        workspace: &Workspace,
        responses: &ResponseStore,
        task: &AiTask,
        input: &SendInput,
    ) -> Result<Self, AppError> {
        let collection = workspace.collection();
        let environment = input.environment.clone();
        let mut context = AiContext {
            collection: collection.name.clone(),
            environment: environment.clone(),
            ..Default::default()
        };
        match task {
            AiTask::CreateRequest { folder, .. }
            | AiTask::RequestFromSnippet { folder, .. }
            | AiTask::SuggestNames {
                target: super::AiNameTarget::Folder,
                path: folder,
            } => {
                let (chain, items) = folder_chain(collection, folder)
                    .ok_or_else(|| AppError::FolderNotFound(folder.clone()))?;
                context.variables = variables(collection, environment.as_deref(), &chain, None);
                context.folder = Some(folder.clone());
                context.siblings = items
                    .iter()
                    .filter_map(|i| match i {
                        Item::Request(r) => Some(sibling_view(&r.spec)),
                        Item::Folder(_) => None,
                    })
                    .take(MAX_SIBLINGS)
                    .collect();
            }
            AiTask::ExplainError { path, response }
            | AiTask::SuggestTests { path, response }
            | AiTask::ExplainResponse { path, response }
            | AiTask::SuggestAssertions { path, response } => {
                let found = collection
                    .request(path)
                    .ok_or_else(|| AppError::RequestNotFound(path.clone()))?;
                context.variables = variables(
                    collection,
                    environment.as_deref(),
                    &found.folders,
                    Some(&found.request.spec),
                );
                let spec = workspace.request(path)?;
                context.request = Some(request_view(&spec));
                context.report = Some(report_view(responses.report(*response)?));
            }
            // Wave 16: the request without a response.
            AiTask::SuggestNames { path, .. } | AiTask::DraftDocs { path } => {
                let found = collection
                    .request(path)
                    .ok_or_else(|| AppError::RequestNotFound(path.clone()))?;
                context.variables = variables(
                    collection,
                    environment.as_deref(),
                    &found.folders,
                    Some(&found.request.spec),
                );
                let spec = workspace.request(path)?;
                context.request = Some(request_view(&spec));
            }
        }
        Ok(context)
    }
}

/// The folders from the root to `path` and the items of the last one (the root's items for
/// `""`).
fn folder_chain<'a>(
    collection: &'a Collection,
    path: &str,
) -> Option<(Vec<&'a Folder>, &'a [Item])> {
    if path.is_empty() {
        return Some((Vec::new(), &collection.items));
    }
    fn find<'a>(items: &'a [Item], path: &str, chain: &mut Vec<&'a Folder>) -> bool {
        for item in items {
            if let Item::Folder(f) = item {
                if f.path == path {
                    chain.push(f);
                    return true;
                }
                if path.starts_with(&format!("{}/", f.path)) {
                    chain.push(f);
                    if find(&f.items, path, chain) {
                        return true;
                    }
                    chain.pop();
                }
            }
        }
        false
    }
    let mut chain = Vec::new();
    if !find(&collection.items, path, &mut chain) {
        return None;
    }
    let items = &chain.last()?.items;
    Some((chain, items))
}

/// Names of the variables in scope, lowest precedence first, each once.
fn variables(
    collection: &Collection,
    environment: Option<&str>,
    folders: &[&Folder],
    request: Option<&RequestSpec>,
) -> Vec<String> {
    let env = environment.and_then(|e| collection.environment(e));
    let all = collection
        .defaults
        .vars
        .iter()
        .chain(env.into_iter().flat_map(|e| e.variables.iter()))
        .chain(folders.iter().flat_map(|f| f.defaults.vars.iter()))
        .chain(request.into_iter().flat_map(|r| r.vars.iter()));
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let list: Vec<&Variable> = all.filter(|v| v.enabled).collect();
    // A name is secret if any layer declares it secret.
    for v in &list {
        if seen.contains(&v.name.as_str()) {
            continue;
        }
        seen.push(&v.name);
        let secret = list.iter().any(|w| w.name == v.name && w.secret);
        out.push(if secret {
            format!("{} (secret)", v.name)
        } else {
            v.name.clone()
        });
    }
    out
}

fn headers_view(headers: &[rt_core::model::KeyValue], keep_templates: bool) -> Value {
    Value::Array(
        headers
            .iter()
            .filter(|h| h.enabled)
            .map(|h| json!({ "name": h.name, "value": mask::header_value(&h.name, &h.value, keep_templates) }))
            .collect(),
    )
}

fn body_view(body: &Body, max: usize) -> Value {
    let field = |name: &str, value: &str| {
        let value = if mask::credential_name(name) {
            mask::credential_value(value, true)
        } else {
            value.to_owned()
        };
        json!({ "name": name, "value": value })
    };
    match body {
        Body::None => json!({ "type": "none" }),
        Body::Raw { kind, data } => {
            let data = match kind {
                rt_core::model::RawKind::Json => mask::json_text(data, true),
                _ => data.clone(),
            };
            json!({ "type": kind, "data": mask::cut(&data, max) })
        }
        Body::FormUrlEncoded { fields } => json!({
            "type": "form",
            "fields": fields.iter().filter(|f| f.enabled).map(|f| field(&f.name, &f.value)).collect::<Vec<_>>(),
        }),
        Body::Multipart { parts } => json!({
            "type": "multipart",
            "parts": parts.iter().filter(|p| p.enabled).map(|p| p.name.clone()).collect::<Vec<_>>(),
        }),
        Body::File { path, content_type } => {
            json!({ "type": "file", "path": path, "content_type": content_type })
        }
        Body::GraphQl { query, variables } => json!({
            "type": "graphql",
            "query": mask::cut(query, max),
            "variables": mask::cut(&mask::json_text(variables, true), max),
        }),
    }
}

fn auth_view(auth: &Auth) -> Value {
    let keep = |v: &str| mask::credential_value(v, true);
    match auth {
        Auth::Inherit => json!("inherit"),
        Auth::None => json!("none"),
        Auth::Basic { username, password } => {
            json!({ "type": "basic", "username": username, "password": keep(password) })
        }
        Auth::Bearer { token } => json!({ "type": "bearer", "token": keep(token) }),
        Auth::ApiKey {
            key,
            value,
            placement,
        } => json!({ "type": "apikey", "key": key, "value": keep(value), "placement": placement }),
        Auth::OAuth2(_) => json!({ "type": "oauth2" }),
        Auth::AwsSigV4(_) => json!({ "type": "awsv4" }),
        Auth::Unsupported { kind } => json!({ "type": kind }),
    }
}

fn params_view(spec: &RequestSpec) -> Value {
    Value::Array(
        spec.params
            .iter()
            .filter(|p| p.enabled)
            .map(|p| {
                let value = if mask::credential_name(&p.name) {
                    mask::credential_value(&p.value, true)
                } else {
                    p.value.clone()
                };
                json!({ "name": p.name, "value": value, "kind": p.kind })
            })
            .collect(),
    )
}

/// A request as written, credentials that are not templates masked.
fn request_view(spec: &RequestSpec) -> Value {
    json!({
        "name": spec.name,
        "method": spec.method,
        "url": mask::url(&spec.url, true),
        "headers": headers_view(&spec.headers, true),
        "params": params_view(spec),
        "body": body_view(&spec.body, REQUEST_BODY_BYTES),
        "auth": auth_view(&spec.auth),
        "scripts": spec.scripts.iter().map(|s| json!({ "phase": s.phase, "code": mask::cut(&s.code, SCRIPT_BYTES) })).collect::<Vec<_>>(),
        "assertions": spec.assertions.iter().filter(|a| a.enabled).map(|a| json!({
            "expression": a.expression, "operator": a.operator, "value": a.value,
        })).collect::<Vec<_>>(),
    })
}

/// A sibling request: enough to copy its style (host variable, headers, body shape).
fn sibling_view(spec: &RequestSpec) -> Value {
    json!({
        "name": spec.name,
        "method": spec.method,
        "url": mask::url(&spec.url, true),
        "headers": headers_view(&spec.headers, true),
        "body": body_view(&spec.body, SIBLING_BODY_BYTES),
        "auth": auth_view(&spec.auth),
    })
}

fn pairs_view(headers: &[(String, String)]) -> Value {
    Value::Array(
        headers
            .iter()
            .map(|(n, v)| json!({ "name": n, "value": mask::header_value(n, v, false) }))
            .collect(),
    )
}

/// What was sent and received. The core masked secret values in the request part; here
/// credential headers, credential JSON fields of the body and credential query parameters
/// are masked too, and the body is cut.
fn report_view(report: &SendReport) -> Value {
    let request = report.request.as_ref().map(|r| {
        json!({
            "method": r.method,
            "url": mask::url(&r.url, false),
            "headers": pairs_view(&r.headers),
            "body_size": r.body_size,
        })
    });
    let response = report.response.as_ref().map(|r| {
        let body = match &r.body {
            ReportBody::Utf8(text) => mask::cut(&mask::json_text(text, false), RESPONSE_BODY_BYTES),
            ReportBody::Base64(_) => format!("(binary body, {} bytes)", r.size),
        };
        json!({
            "status": r.status,
            "url": mask::url(&r.url, false),
            "headers": pairs_view(&r.headers),
            "size": r.size,
            "time_ms": r.total.as_millis() as u64,
            "body": body,
            "body_cut_at_send_limit": r.truncated,
        })
    });
    json!({
        "environment": report.environment,
        "request": request,
        "response": response,
        "error": report.error,
        "unresolved_variables": report.unresolved,
        "notes": report.notes.iter().map(|n| n.text.clone()).collect::<Vec<_>>(),
        "tests": report.tests.iter().map(|t| json!({ "name": t.name, "passed": t.passed, "error": t.error })).collect::<Vec<_>>(),
        "assertions": report.assertions,
        "logs": report.logs.iter().take(MAX_LOGS).map(|l| mask::cut(&l.text, 1024)).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::model::KeyValue;

    #[test]
    fn literal_credentials_in_a_spec_are_masked_templates_kept() {
        let spec = RequestSpec {
            name: "Login".into(),
            method: "POST".into(),
            url: "{{base}}/login?api_key=literal-key&page=1".into(),
            headers: vec![
                KeyValue::new("Authorization", "Bearer literal-token"),
                KeyValue::new("X-Api-Key", "{{apiKey}}"),
                KeyValue::new("Accept", "application/json"),
            ],
            body: Body::Raw {
                kind: rt_core::model::RawKind::Json,
                data: r#"{"user":"{{user}}","password":"literal-pw"}"#.into(),
            },
            auth: Auth::Basic {
                username: "ann".into(),
                password: "literal-basic".into(),
            },
            ..Default::default()
        };
        let text = serde_json::to_string(&request_view(&spec)).unwrap();
        for literal in [
            "literal-key",
            "literal-token",
            "literal-pw",
            "literal-basic",
        ] {
            assert!(!text.contains(literal), "{literal} in {text}");
        }
        for kept in [
            "{{apiKey}}",
            "{{user}}",
            "{{base}}/login",
            "application/json",
            "page=1",
        ] {
            assert!(text.contains(kept), "{kept} not in {text}");
        }
    }

    #[test]
    fn credential_headers_and_fields_of_a_response_are_masked() {
        let report: SendReport = serde_json::from_value(json!({
            "version": 1, "request_path": "a.yml", "environment": null,
            "request": { "method": "GET", "url": "https://h/a?token=abc", "headers": [["Cookie", "sid=1"]], "body_size": 0 },
            "response": {
                "status": 401, "version": "HTTP/1.1", "headers": [["Set-Cookie", "sid=2"], ["Content-Type", "application/json"]],
                "url": "https://h/a", "redirects": [], "body": { "encoding": "utf8", "data": "{\"error\":\"bad\",\"refresh_token\":\"rt-1\"}" },
                "size": 10, "truncated": false, "timing": { "dns": null, "connect": null, "tls": null, "ttfb": 1 }, "total": 2
            },
            "variables": [], "unresolved": [], "error": null
        }))
        .unwrap();
        let text = serde_json::to_string(&report_view(&report)).unwrap();
        for value in ["sid=1", "sid=2", "rt-1", "token=abc"] {
            assert!(!text.contains(value), "{value} in {text}");
        }
        assert!(text.contains("401") && text.contains("bad"), "{text}");
    }
}
