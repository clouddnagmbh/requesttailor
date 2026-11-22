// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! An OpenAPI document read for the contract check: operations with their parameters, request
//! body and responses; local `$ref`s are followed lazily (see `schema`).

use serde_json::Value;

use super::ContractError;
use crate::yaml::{self, Node, Resolved};

const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

#[derive(Debug, Clone)]
pub(super) struct Param {
    pub name: String,
    /// `query`, `path`, `header` or `cookie`.
    pub location: String,
    pub required: bool,
    pub schema: Value,
}

#[derive(Debug, Clone)]
pub(super) struct Body {
    pub required: bool,
    /// Media type and schema (`Null` if the media type has none).
    pub content: Vec<(String, Value)>,
}

#[derive(Debug, Clone)]
pub(super) struct Operation {
    /// `GET /orders/{id}`.
    pub key: String,
    pub method: String,
    /// Segments of the path template.
    pub segs: Vec<String>,
    pub params: Vec<Param>,
    pub body: Option<Body>,
    /// Status (`200`, `default`, `2XX`) and, per media type, the schema.
    pub responses: Vec<(String, Vec<(String, Value)>)>,
    pub summary: String,
    pub deprecated: bool,
}

#[derive(Debug, Clone)]
pub(super) struct Spec {
    pub root: Value,
    pub title: String,
    pub version: String,
    /// Path prefixes of the `servers` URLs, as segments (`/v1` is `["v1"]`).
    pub base_paths: Vec<Vec<String>>,
    pub ops: Vec<Operation>,
    /// Names of API keys in headers and query strings (`securitySchemes`).
    pub key_headers: Vec<String>,
    pub key_queries: Vec<String>,
}

fn node_to_json(node: &Node) -> Value {
    match node {
        Node::Scalar(s) => match s.resolve() {
            Resolved::Null => Value::Null,
            Resolved::Bool(b) => Value::Bool(b),
            Resolved::Int(i) => Value::from(i),
            Resolved::Float(f) => serde_json::Number::from_f64(f)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(s.value.clone())),
            Resolved::Str => Value::String(s.value.clone()),
        },
        Node::Map(m) => Value::Object(
            m.entries
                .iter()
                .map(|e| (e.key.value.clone(), node_to_json(&e.value)))
                .collect(),
        ),
        Node::Seq(s) => Value::Array(s.items.iter().map(|i| node_to_json(&i.value)).collect()),
    }
}

/// JSON or YAML text as a JSON value.
pub(super) fn parse_document(bytes: &[u8]) -> Result<Value, ContractError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ContractError::Spec("the spec is not UTF-8 text".into()))?;
    if text.trim_start().starts_with(['{', '['])
        && let Ok(v) = serde_json::from_str::<Value>(text)
    {
        return Ok(v);
    }
    let doc = yaml::parse(text)
        .map_err(|e| ContractError::Spec(format!("the spec is not valid YAML or JSON: {e}")))?;
    Ok(node_to_json(&doc.root))
}

fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The path of a `servers` URL (`https://h/v1` → `v1`), or of a relative one.
fn server_path(url: &str) -> Vec<String> {
    let rest = match url.find("://") {
        Some(i) => {
            let after = &url[i + 3..];
            after.find('/').map_or("", |j| &after[j..])
        }
        None => url,
    };
    segments(rest)
}

impl Spec {
    pub fn parse(bytes: &[u8]) -> Result<Self, ContractError> {
        let root = parse_document(bytes)?;
        if !root.is_object() {
            return Err(ContractError::Spec("the spec is not an object".into()));
        }
        let openapi = text_of(root.get("openapi"));
        let swagger = text_of(root.get("swagger"));
        if !(openapi.starts_with('3') || swagger.starts_with('2')) {
            return Err(ContractError::Spec(
                "not an OpenAPI document (no `openapi: 3.x` or `swagger: 2.0`)".into(),
            ));
        }
        let info = root.get("info");
        let title = text_of(info.and_then(|i| i.get("title")));
        let version = text_of(info.and_then(|i| i.get("version")));
        let mut spec = Spec {
            title,
            version,
            base_paths: Vec::new(),
            ops: Vec::new(),
            key_headers: Vec::new(),
            key_queries: Vec::new(),
            root,
        };
        spec.read_servers();
        spec.read_security();
        spec.read_operations();
        Ok(spec)
    }

    fn read_servers(&mut self) {
        let mut out = Vec::new();
        if let Some(Value::Array(servers)) = self.root.get("servers") {
            for s in servers {
                let p = server_path(&text_of(s.get("url")));
                if !p.is_empty() && !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        // Swagger 2.0.
        if let Some(Value::String(b)) = self.root.get("basePath") {
            let p = segments(b);
            if !p.is_empty() && !out.contains(&p) {
                out.push(p);
            }
        }
        self.base_paths = out;
    }

    fn read_security(&mut self) {
        let schemes = self
            .root
            .pointer("/components/securitySchemes")
            .or_else(|| self.root.get("securityDefinitions"));
        let Some(Value::Object(map)) = schemes else {
            return;
        };
        for scheme in map.values() {
            if text_of(scheme.get("type")) != "apiKey" {
                continue;
            }
            let name = text_of(scheme.get("name"));
            match text_of(scheme.get("in")).as_str() {
                "header" => self.key_headers.push(name),
                "query" => self.key_queries.push(name),
                _ => {}
            }
        }
    }

    fn read_operations(&mut self) {
        let Some(Value::Object(paths)) = self.root.get("paths") else {
            return;
        };
        let mut ops = Vec::new();
        for (path, item) in paths {
            let item = super::schema::resolve(&self.root, item);
            let shared = self.params_of(item.get("parameters"));
            for method in METHODS {
                let Some(op) = item.get(*method) else {
                    continue;
                };
                let mut params = shared.clone();
                for p in self.params_of(op.get("parameters")) {
                    params.retain(|q| !(q.name == p.name && q.location == p.location));
                    params.push(p);
                }
                let upper = method.to_uppercase();
                ops.push(Operation {
                    key: format!("{upper} {path}"),
                    method: upper,
                    segs: segments(path),
                    params,
                    body: self.body_of(op),
                    responses: self.responses_of(op),
                    summary: text_of(op.get("summary")),
                    deprecated: op.get("deprecated") == Some(&Value::Bool(true)),
                });
            }
        }
        self.ops = ops;
    }

    fn params_of(&self, list: Option<&Value>) -> Vec<Param> {
        let Some(Value::Array(list)) = list else {
            return Vec::new();
        };
        list.iter()
            .map(|p| super::schema::resolve(&self.root, p))
            .filter_map(|p| {
                let name = text_of(p.get("name"));
                if name.is_empty() {
                    return None;
                }
                let location = text_of(p.get("in"));
                let required = location == "path" || p.get("required") == Some(&Value::Bool(true));
                let mut schema = p.get("schema").cloned().unwrap_or(Value::Null);
                if schema.is_null() {
                    // Swagger 2.0 puts the type on the parameter.
                    schema = serde_json::json!({ "type": p.get("type").cloned().unwrap_or(Value::Null),
                        "format": p.get("format").cloned().unwrap_or(Value::Null) });
                }
                Some(Param {
                    name,
                    location,
                    required,
                    schema,
                })
            })
            .collect()
    }

    fn body_of(&self, op: &Value) -> Option<Body> {
        let body = super::schema::resolve(&self.root, op.get("requestBody")?);
        let content = media_schemas(body.get("content"));
        Some(Body {
            required: body.get("required") == Some(&Value::Bool(true)),
            content,
        })
    }

    fn responses_of(&self, op: &Value) -> Vec<(String, Vec<(String, Value)>)> {
        let Some(Value::Object(map)) = op.get("responses") else {
            return Vec::new();
        };
        map.iter()
            .map(|(status, r)| {
                let r = super::schema::resolve(&self.root, r);
                (status.clone(), media_schemas(r.get("content")))
            })
            .collect()
    }
}

fn media_schemas(content: Option<&Value>) -> Vec<(String, Value)> {
    match content {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(m, v)| (m.clone(), v.get("schema").cloned().unwrap_or(Value::Null)))
            .collect(),
        _ => Vec::new(),
    }
}
