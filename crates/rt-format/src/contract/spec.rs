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
    /// For the run wizard: the full schema (Swagger 2.0: built from the parameter's own
    /// keywords; OpenAPI 3: `schema`, or the schema of its first `content` entry).
    pub strict: Value,
    pub description: Option<String>,
    /// `example`, else the first of `examples`.
    pub example: Option<Value>,
    /// `style` (Swagger 2.0 `collectionFormat` mapped: `ssv` → `spaceDelimited`, `pipes` →
    /// `pipeDelimited`, `multi` → `form` with `explode`).
    pub style: Option<String>,
    pub explode: Option<bool>,
}

/// One media type of a request body or response, for the run wizard.
#[derive(Debug, Clone)]
pub(super) struct Media {
    pub mime: String,
    /// `Null` if the media type has none.
    pub schema: Value,
    /// `example`, else the first of `examples`.
    pub example: Option<Value>,
}

/// The request body of an operation for the run wizard: OpenAPI 3 `requestBody`, Swagger 2.0
/// `in: body` or `formData` parameters.
#[derive(Debug, Clone)]
pub(super) struct RunBody {
    pub required: bool,
    pub media: Vec<Media>,
}

#[derive(Debug, Clone)]
pub(super) struct HeaderSpec {
    pub name: String,
    pub required: bool,
    pub schema: Value,
}

/// One declared response, for the response check.
#[derive(Debug, Clone)]
pub(super) struct Reply {
    /// `200`, `2XX`, `default`.
    pub status: String,
    pub media: Vec<Media>,
    pub headers: Vec<HeaderSpec>,
}

/// A server URL with `{{var}}` templates, as the importer writes it.
#[derive(Debug, Clone)]
pub(super) struct Server {
    pub url: String,
    pub description: Option<String>,
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
    /// For the run wizard and the response check (Swagger 2.0 included, unlike `body` and
    /// `responses`, which the contract check and the change list use).
    pub run_body: Option<RunBody>,
    pub replies: Vec<Reply>,
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
    /// Servers as the importer writes them (Swagger 2.0: `schemes`, `host`, `basePath`).
    pub servers: Vec<Server>,
    /// Variables of the servers (`name` → the variable object), first definition wins.
    pub server_vars: Vec<(String, Value)>,
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
            servers: Vec::new(),
            server_vars: Vec::new(),
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
        self.read_server_options();
    }

    fn read_server_options(&mut self) {
        let mut servers = Vec::new();
        let mut vars: Vec<(String, Value)> = Vec::new();
        if let Some(Value::Array(list)) = self.root.get("servers") {
            for s in list {
                let url = text_of(s.get("url"));
                if url.is_empty() {
                    continue;
                }
                let description = Some(text_of(s.get("description"))).filter(|d| !d.is_empty());
                servers.push(Server {
                    url: braces_to_vars(&url),
                    description,
                });
                if let Some(Value::Object(map)) = s.get("variables") {
                    for (name, v) in map {
                        if !vars.iter().any(|(n, _)| n == name) {
                            vars.push((name.clone(), v.clone()));
                        }
                    }
                }
            }
        }
        if text_of(self.root.get("swagger")).starts_with('2') {
            let host = text_of(self.root.get("host"));
            let base = text_of(self.root.get("basePath"));
            let base = base.trim_end_matches('/');
            if host.is_empty() {
                if !base.is_empty() {
                    servers.push(Server {
                        url: base.to_owned(),
                        description: None,
                    });
                }
            } else {
                let mut schemes: Vec<String> = match self.root.get("schemes") {
                    Some(Value::Array(s)) => s
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                    _ => Vec::new(),
                };
                if schemes.is_empty() {
                    schemes.push("https".into());
                }
                for scheme in schemes {
                    servers.push(Server {
                        url: format!("{scheme}://{host}{base}"),
                        description: None,
                    });
                }
            }
        }
        self.servers = servers;
        self.server_vars = vars;
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
                    run_body: self.run_body_of(op, item.get("parameters")),
                    replies: self.replies_of(op),
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
                let v2 = schema.is_null() && p.get("content").is_none();
                let strict = if v2 {
                    v2_schema(p)
                } else if schema.is_null() {
                    // OpenAPI 3: a parameter with `content` instead of `schema`.
                    media_list(p.get("content"))
                        .into_iter()
                        .next()
                        .map(|m| m.schema)
                        .unwrap_or(Value::Null)
                } else {
                    schema.clone()
                };
                if schema.is_null() {
                    // Swagger 2.0 puts the type on the parameter.
                    schema = serde_json::json!({ "type": p.get("type").cloned().unwrap_or(Value::Null),
                        "format": p.get("format").cloned().unwrap_or(Value::Null) });
                }
                let (style, explode) = if v2 {
                    match text_of(p.get("collectionFormat")).as_str() {
                        "ssv" => (Some("spaceDelimited".to_owned()), Some(false)),
                        "pipes" => (Some("pipeDelimited".to_owned()), Some(false)),
                        "tsv" => (Some("tabDelimited".to_owned()), Some(false)),
                        "multi" => (Some("form".to_owned()), Some(true)),
                        _ => (None, Some(false)),
                    }
                } else {
                    (
                        Some(text_of(p.get("style"))).filter(|s| !s.is_empty()),
                        p.get("explode").and_then(Value::as_bool),
                    )
                };
                Some(Param {
                    name,
                    location,
                    required,
                    schema,
                    strict,
                    description: Some(text_of(p.get("description"))).filter(|d| !d.is_empty()),
                    example: example_of(p),
                    style,
                    explode,
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

impl Spec {
    /// The request body for the run wizard: `requestBody`, or Swagger 2.0 `in: body` and
    /// `formData` parameters (path-level ones included).
    fn run_body_of(&self, op: &Value, shared: Option<&Value>) -> Option<RunBody> {
        if let Some(body) = op.get("requestBody") {
            let body = super::schema::resolve(&self.root, body);
            return Some(RunBody {
                required: body.get("required") == Some(&Value::Bool(true)),
                media: media_list(body.get("content")),
            });
        }
        let mut params: Vec<&Value> = Vec::new();
        for list in [shared, op.get("parameters")] {
            if let Some(Value::Array(list)) = list {
                for p in list {
                    let p = super::schema::resolve(&self.root, p);
                    let name = p.get("name");
                    params.retain(|q| !(q.get("name") == name && q.get("in") == p.get("in")));
                    params.push(p);
                }
            }
        }
        let consumes = self.mimes(op, "consumes");
        if let Some(body) = params.iter().find(|p| text_of(p.get("in")) == "body") {
            let schema = body.get("schema").cloned().unwrap_or(Value::Null);
            let example = body.get("x-example").cloned();
            let mimes = if consumes.is_empty() {
                vec!["application/json".to_owned()]
            } else {
                consumes
            };
            return Some(RunBody {
                required: body.get("required") == Some(&Value::Bool(true)),
                media: mimes
                    .into_iter()
                    .map(|mime| Media {
                        mime,
                        schema: schema.clone(),
                        example: example.clone(),
                    })
                    .collect(),
            });
        }
        let form: Vec<&&Value> = params
            .iter()
            .filter(|p| text_of(p.get("in")) == "formData")
            .collect();
        if form.is_empty() {
            return None;
        }
        let mut props = serde_json::Map::new();
        let mut required = Vec::new();
        for p in &form {
            let name = text_of(p.get("name"));
            if p.get("required") == Some(&Value::Bool(true)) {
                required.push(Value::String(name.clone()));
            }
            props.insert(name, v2_schema(p));
        }
        let mut schema = serde_json::json!({ "type": "object", "properties": props });
        if !required.is_empty() {
            schema["required"] = Value::Array(required.clone());
        }
        let mimes: Vec<String> = consumes
            .into_iter()
            .filter(|m| m.contains("form"))
            .collect();
        let mimes = if mimes.is_empty() {
            vec!["application/x-www-form-urlencoded".to_owned()]
        } else {
            mimes
        };
        Some(RunBody {
            required: !required.is_empty(),
            media: mimes
                .into_iter()
                .map(|mime| Media {
                    mime,
                    schema: schema.clone(),
                    example: None,
                })
                .collect(),
        })
    }

    /// `consumes` or `produces` of the operation, else of the document (Swagger 2.0).
    fn mimes(&self, op: &Value, key: &str) -> Vec<String> {
        let list = match op.get(key) {
            Some(Value::Array(l)) => l,
            _ => match self.root.get(key) {
                Some(Value::Array(l)) => l,
                _ => return Vec::new(),
            },
        };
        list.iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    }

    fn replies_of(&self, op: &Value) -> Vec<Reply> {
        let Some(Value::Object(map)) = op.get("responses") else {
            return Vec::new();
        };
        let v2 = text_of(self.root.get("swagger")).starts_with('2');
        map.iter()
            .map(|(status, r)| {
                let r = super::schema::resolve(&self.root, r);
                let media = if v2 {
                    match r.get("schema") {
                        Some(schema) => {
                            let mut mimes = self.mimes(op, "produces");
                            if mimes.is_empty() {
                                mimes.push("application/json".into());
                            }
                            mimes
                                .into_iter()
                                .map(|mime| Media {
                                    mime,
                                    schema: schema.clone(),
                                    example: None,
                                })
                                .collect()
                        }
                        None => Vec::new(),
                    }
                } else {
                    media_list(r.get("content"))
                };
                let headers = match r.get("headers") {
                    Some(Value::Object(h)) => h
                        .iter()
                        .map(|(name, h)| {
                            let h = super::schema::resolve(&self.root, h);
                            HeaderSpec {
                                name: name.clone(),
                                required: h.get("required") == Some(&Value::Bool(true)),
                                schema: if v2 {
                                    v2_schema(h)
                                } else {
                                    h.get("schema").cloned().unwrap_or(Value::Null)
                                },
                            }
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                Reply {
                    status: status.clone(),
                    media,
                    headers,
                }
            })
            .collect()
    }
}

/// `{name}` → `{{name}}` in a server URL.
fn braces_to_vars(url: &str) -> String {
    let mut out = String::with_capacity(url.len() + 8);
    let mut rest = url;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(inner) = after.strip_prefix('{') {
            // Already a template.
            out.push_str("{{");
            rest = inner;
            continue;
        }
        match after.find('}') {
            Some(j) if j > 0 => {
                out.push_str("{{");
                out.push_str(&after[..j]);
                out.push_str("}}");
                rest = &after[j + 1..];
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

/// A Swagger 2.0 parameter or header as a JSON Schema: its own validation keywords.
fn v2_schema(p: &Value) -> Value {
    const KEYS: &[&str] = &[
        "type",
        "format",
        "items",
        "enum",
        "default",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minLength",
        "maxLength",
        "pattern",
        "multipleOf",
        "minItems",
        "maxItems",
        "uniqueItems",
        "description",
    ];
    let mut out = serde_json::Map::new();
    for key in KEYS {
        if let Some(v) = p.get(*key)
            && !v.is_null()
        {
            let v = if *key == "items" {
                v2_schema(v)
            } else {
                v.clone()
            };
            out.insert((*key).to_owned(), v);
        }
    }
    // Swagger 2.0 `file` has no JSON Schema counterpart.
    if out.get("type").and_then(Value::as_str) == Some("file") {
        out.insert("type".into(), Value::String("string".into()));
        out.insert("format".into(), Value::String("binary".into()));
    }
    Value::Object(out)
}

/// `example`, else the `value` of the first of `examples` (OpenAPI 3), else `x-example`.
fn example_of(v: &Value) -> Option<Value> {
    if let Some(e) = v.get("example") {
        return Some(e.clone());
    }
    if let Some(Value::Object(examples)) = v.get("examples")
        && let Some(first) = examples.values().next()
        && let Some(value) = first.get("value")
    {
        return Some(value.clone());
    }
    v.get("x-example").cloned()
}

fn media_list(content: Option<&Value>) -> Vec<Media> {
    match content {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(m, v)| Media {
                mime: m.clone(),
                schema: v.get("schema").cloned().unwrap_or(Value::Null),
                example: example_of(v),
            })
            .collect(),
        _ => Vec::new(),
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
