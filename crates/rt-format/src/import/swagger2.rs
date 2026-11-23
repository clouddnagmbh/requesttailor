// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Swagger 2.0 → the OpenAPI 3.0 model, so the one OpenAPI importer converts both.
//!
//! - Servers: one per scheme of `schemes` (`http`, `https`; `https` if none) from `host` and
//!   `basePath` (with several, described as `host/base (HTTPS)` so the environments are told
//!   apart); without `host` the server is `basePath` (relative, reported by the importer).
//! - Parameters (path-level and operation-level, references followed): `query`, `path`,
//!   `header` keep their place with `type`/`format`/`enum`/`items`/`default` (and the other
//!   validation keywords) as `schema`, `x-example` as `example`. The `in: body` parameter
//!   becomes `requestBody` with one media type per `consumes` (`application/json` if none);
//!   `formData` parameters become a `multipart/form-data` body when one is `type: file` (then
//!   `type: string, format: binary`) or `consumes` says multipart, else
//!   `application/x-www-form-urlencoded`.
//! - Responses: `schema` under every `produces` media type (`application/json` if none),
//!   `examples` (per media type) as that media type's `example`, headers as header objects.
//! - Components: `definitions` → `schemas`, `parameters` → `parameters` (body ones →
//!   `requestBodies`), `responses` → `responses`, `securityDefinitions` → `securitySchemes`
//!   (`basic` → HTTP Basic, `apiKey`, `oauth2` with flow `implicit`, `password`, `application`
//!   → `clientCredentials`, `accessCode` → `authorizationCode`). In schemas `$ref`s
//!   `#/definitions/X` become `#/components/schemas/X`, `x-nullable` becomes `nullable`, a
//!   string `discriminator` becomes `{ propertyName }`. Files hoisted by the bundler are
//!   treated as schemas.
//!
//! What has no place in the 3.0 model is reported: unknown security types and flows, a
//! second body parameter, unresolved parameter references, WebSocket schemes.

use super::common::Losses;
use super::openapi::pointer;
use super::spec_files::BUNDLED_KEY;
use super::value::Value;
use super::{Loss, LossKind};

const METHODS: &[&str] = &["get", "put", "post", "delete", "options", "head", "patch"];

/// Validation keywords a 2.0 parameter or header shares with a schema.
const SCHEMA_KEYS: &[&str] = &[
    "type",
    "format",
    "items",
    "default",
    "enum",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "maxItems",
    "minItems",
    "uniqueItems",
    "multipleOf",
];

/// Deepest `$ref` chain followed for parameters and responses.
const MAX_REF_DEPTH: usize = 32;

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn obj(entries: Vec<(&str, Value)>) -> Value {
    Value::Obj(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

fn push(entries: &mut Vec<(String, Value)>, key: &str, v: Option<&Value>) {
    if let Some(v) = v {
        entries.push((key.to_owned(), v.clone()));
    }
}

/// A schema with its references and 2.0 keywords rewritten (recursively).
fn schema(v: &Value) -> Value {
    match v {
        Value::Obj(entries) => {
            let mut out = Vec::with_capacity(entries.len());
            let is_file = v.string("type") == "file";
            for (k, val) in entries {
                match (k.as_str(), val) {
                    ("$ref", Value::Str(r)) => {
                        let r = match r.strip_prefix("#/definitions/") {
                            Some(rest) => format!("#/components/schemas/{rest}"),
                            None => r.clone(),
                        };
                        out.push((k.clone(), Value::Str(r)));
                    }
                    ("x-nullable", nullable) => out.push(("nullable".into(), nullable.clone())),
                    ("discriminator", Value::Str(p)) => {
                        out.push(("discriminator".into(), obj(vec![("propertyName", s(p))])))
                    }
                    ("type", _) if is_file => {
                        out.push(("type".into(), s("string")));
                        out.push(("format".into(), s("binary")));
                    }
                    ("format", _) if is_file => {}
                    _ => out.push((k.clone(), schema(val))),
                }
            }
            Value::Obj(out)
        }
        Value::Arr(items) => Value::Arr(items.iter().map(schema).collect()),
        v => v.clone(),
    }
}

/// The schema of a non-body parameter or a header.
fn param_schema(p: &Value) -> Value {
    let mut out = Vec::new();
    for key in SCHEMA_KEYS {
        push(&mut out, key, p.get(key));
    }
    schema(&Value::Obj(out))
}

struct Cx<'a> {
    root: &'a Value,
    losses: Losses,
}

impl<'a> Cx<'a> {
    /// Follows `$ref`s in the 2.0 document.
    fn resolve(&self, v: &'a Value) -> Option<&'a Value> {
        let mut cur = v;
        for _ in 0..MAX_REF_DEPTH {
            match cur.get("$ref").and_then(Value::as_str) {
                Some(r) if r.starts_with('#') => cur = pointer(self.root, r)?,
                Some(_) => return None,
                None => return Some(cur),
            }
        }
        None
    }

    /// A query, path, header or cookie parameter.
    fn parameter(&self, p: &Value) -> Value {
        let mut out = Vec::new();
        for key in ["name", "in", "description", "required", "allowEmptyValue"] {
            push(&mut out, key, p.get(key));
        }
        out.push(("schema".into(), param_schema(p)));
        push(&mut out, "example", p.get("x-example"));
        Value::Obj(out)
    }

    fn media_types(&self, op: &Value, key: &str) -> Vec<String> {
        let list = op
            .get(key)
            .filter(|v| v.is_arr())
            .or_else(|| self.root.get(key))
            .map(Value::items)
            .unwrap_or_default();
        let out: Vec<String> = list.iter().filter_map(Value::text).collect();
        if out.is_empty() {
            vec!["application/json".into()]
        } else {
            out
        }
    }

    fn body(&self, p: &Value, consumes: &[String]) -> Value {
        let content = consumes
            .iter()
            .filter(|m| !m.contains("form-urlencoded") && !m.contains("multipart/"))
            .map(|m| {
                let mut mt = Vec::new();
                if let Some(sch) = p.get("schema") {
                    mt.push(("schema".to_owned(), schema(sch)));
                }
                push(
                    &mut mt,
                    "example",
                    p.get("x-examples").and_then(|e| e.get(m)),
                );
                (m.clone(), Value::Obj(mt))
            })
            .collect::<Vec<_>>();
        let content = if content.is_empty() {
            let mut mt = Vec::new();
            if let Some(sch) = p.get("schema") {
                mt.push(("schema".to_owned(), schema(sch)));
            }
            vec![("application/json".to_owned(), Value::Obj(mt))]
        } else {
            content
        };
        let mut out = Vec::new();
        push(&mut out, "description", p.get("description"));
        push(&mut out, "required", p.get("required"));
        out.push(("content".into(), Value::Obj(content)));
        Value::Obj(out)
    }

    fn form(&self, fields: &[&Value], consumes: &[String]) -> Value {
        let file = fields.iter().any(|f| f.string("type") == "file");
        let media = if file || consumes.iter().any(|m| m.contains("multipart/form-data")) {
            "multipart/form-data"
        } else {
            "application/x-www-form-urlencoded"
        };
        let mut props = Vec::new();
        let mut required = Vec::new();
        for f in fields {
            let name = f.string("name");
            let mut sch = param_schema(f);
            if let (Some(d), Value::Obj(entries)) = (f.get("description"), &mut sch) {
                entries.push(("description".into(), d.clone()));
            }
            if f.flag("required") {
                required.push(s(&name));
            }
            props.push((name, sch));
        }
        let mut sch = vec![
            ("type".to_owned(), s("object")),
            ("properties".to_owned(), Value::Obj(props)),
        ];
        if !required.is_empty() {
            sch.push(("required".into(), Value::Arr(required)));
        }
        obj(vec![(
            "content",
            obj(vec![(media, obj(vec![("schema", Value::Obj(sch))]))]),
        )])
    }

    fn response(&self, r: &Value, produces: &[String]) -> Value {
        let mut out = Vec::new();
        out.push((
            "description".to_owned(),
            r.get("description").cloned().unwrap_or_else(|| s("")),
        ));
        let mut content: Vec<(String, Value)> = Vec::new();
        if let Some(sch) = r.get("schema") {
            for m in produces {
                content.push((m.clone(), obj(vec![("schema", schema(sch))])));
            }
        }
        for (media, example) in r.get("examples").map(Value::entries).unwrap_or_default() {
            match content.iter_mut().find(|(m, _)| m == media) {
                Some((_, Value::Obj(mt))) => mt.push(("example".into(), example.clone())),
                _ => content.push((media.clone(), obj(vec![("example", example.clone())]))),
            }
        }
        if !content.is_empty() {
            out.push(("content".into(), Value::Obj(content)));
        }
        let headers: Vec<(String, Value)> = r
            .get("headers")
            .map(Value::entries)
            .unwrap_or_default()
            .iter()
            .map(|(name, h)| {
                let mut e = Vec::new();
                push(&mut e, "description", h.get("description"));
                e.push(("schema".into(), param_schema(h)));
                (name.clone(), Value::Obj(e))
            })
            .collect();
        if !headers.is_empty() {
            out.push(("headers".into(), Value::Obj(headers)));
        }
        Value::Obj(out)
    }

    fn operation(&mut self, op: &'a Value, common: &[&'a Value], here: &str) -> Value {
        let consumes = self.media_types(op, "consumes");
        let produces = self.media_types(op, "produces");
        let mut params: Vec<&Value> = Vec::new();
        let own: Vec<&Value> = op
            .get("parameters")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .collect();
        for p in common.iter().chain(own.iter()) {
            let Some(p) = self.resolve(p) else {
                self.losses.dropped(
                    here,
                    LossKind::Other,
                    format!(
                        "parameter reference `{}` not resolved",
                        p.get("$ref").and_then(Value::text).unwrap_or_default()
                    ),
                );
                continue;
            };
            let key = (p.string("name"), p.string("in"));
            params.retain(|q| (q.string("name"), q.string("in")) != key);
            params.push(p);
        }
        let mut out = Vec::new();
        for key in [
            "tags",
            "summary",
            "description",
            "externalDocs",
            "operationId",
        ] {
            push(&mut out, key, op.get(key));
        }
        let mut parameters = Vec::new();
        let mut body = None;
        let mut form: Vec<&Value> = Vec::new();
        for p in params {
            match p.string("in").as_str() {
                "body" if body.is_none() => body = Some(self.body(p, &consumes)),
                "body" => self.losses.dropped(
                    here,
                    LossKind::Body,
                    format!(
                        "second body parameter `{}` not imported (Swagger 2.0 allows one)",
                        p.string("name")
                    ),
                ),
                "formData" => form.push(p),
                _ => parameters.push(self.parameter(p)),
            }
        }
        if !parameters.is_empty() {
            out.push(("parameters".into(), Value::Arr(parameters)));
        }
        if body.is_none() && !form.is_empty() {
            body = Some(self.form(&form, &consumes));
        }
        if let Some(b) = body {
            out.push(("requestBody".into(), b));
        }
        let mut responses = Vec::new();
        for (status, r) in op.get("responses").map(Value::entries).unwrap_or_default() {
            match self.resolve(r) {
                Some(r) => responses.push((status.clone(), self.response(r, &produces))),
                None => responses.push((status.clone(), r.clone())),
            }
        }
        if op.get("responses").is_some() {
            out.push(("responses".into(), Value::Obj(responses)));
        }
        push(&mut out, "deprecated", op.get("deprecated"));
        push(&mut out, "security", op.get("security"));
        Value::Obj(out)
    }

    fn security_scheme(&mut self, name: &str, d: &Value) -> Option<Value> {
        let mut out = Vec::new();
        match d.string("type").as_str() {
            "basic" => {
                out.push(("type".to_owned(), s("http")));
                out.push(("scheme".to_owned(), s("basic")));
            }
            "apiKey" => {
                out.push(("type".to_owned(), s("apiKey")));
                push(&mut out, "name", d.get("name"));
                push(&mut out, "in", d.get("in"));
            }
            "oauth2" => {
                let flow = match d.string("flow").as_str() {
                    "implicit" => "implicit",
                    "password" => "password",
                    "application" => "clientCredentials",
                    "accessCode" => "authorizationCode",
                    other => {
                        self.losses.dropped(
                            "collection",
                            LossKind::Auth,
                            format!(
                                "OAuth 2.0 flow `{other}` of security definition `{name}` is \
                                 not a Swagger 2.0 flow; not imported"
                            ),
                        );
                        return None;
                    }
                };
                let mut f = Vec::new();
                push(&mut f, "authorizationUrl", d.get("authorizationUrl"));
                push(&mut f, "tokenUrl", d.get("tokenUrl"));
                f.push((
                    "scopes".to_owned(),
                    d.get("scopes").cloned().unwrap_or(Value::Obj(Vec::new())),
                ));
                out.push(("type".to_owned(), s("oauth2")));
                out.push(("flows".to_owned(), obj(vec![(flow, Value::Obj(f))])));
            }
            other => {
                self.losses.dropped(
                    "collection",
                    LossKind::Auth,
                    format!("security definition `{name}` of type `{other}` not imported"),
                );
                return None;
            }
        }
        push(&mut out, "description", d.get("description"));
        Some(Value::Obj(out))
    }

    fn servers(&mut self) -> Vec<Value> {
        let root = self.root;
        let base_path = root.string("basePath");
        let base_path = if base_path.is_empty() || base_path.starts_with('/') {
            base_path
        } else {
            format!("/{base_path}")
        };
        let host = root.string("host");
        if host.trim().is_empty() {
            let url = if base_path.is_empty() {
                "/".to_owned()
            } else {
                base_path
            };
            return vec![obj(vec![("url", Value::Str(url))])];
        }
        let schemes: Vec<String> = root
            .get("schemes")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::text)
            .collect();
        let ws: Vec<&String> = schemes
            .iter()
            .filter(|s| matches!(s.as_str(), "ws" | "wss"))
            .collect();
        if !ws.is_empty() {
            self.losses.dropped(
                "collection",
                LossKind::Protocol,
                "WebSocket schemes (`ws`, `wss`) of the description not imported",
            );
        }
        let mut http: Vec<&str> = schemes
            .iter()
            .map(String::as_str)
            .filter(|s| matches!(*s, "http" | "https"))
            .collect();
        if http.is_empty() {
            http.push("https");
        }
        http.dedup();
        let several = http.len() > 1;
        http.into_iter()
            .map(|scheme| {
                let url = format!("{scheme}://{host}{base_path}");
                if several {
                    // Names the environments apart: `api.example.com/v1 (HTTP)`.
                    let name = format!("{host}{base_path} ({})", scheme.to_ascii_uppercase());
                    obj(vec![
                        ("url", Value::Str(url)),
                        ("description", Value::Str(name)),
                    ])
                } else {
                    obj(vec![("url", Value::Str(url))])
                }
            })
            .collect()
    }

    fn convert(&mut self) -> Value {
        let root = self.root;
        let mut out = vec![("openapi".to_owned(), s("3.0.3"))];
        push(&mut out, "info", root.get("info"));
        out.push(("servers".into(), Value::Arr(self.servers())));
        push(&mut out, "security", root.get("security"));
        push(&mut out, "tags", root.get("tags"));
        push(&mut out, "externalDocs", root.get("externalDocs"));

        let mut paths = Vec::new();
        for (path, item) in root.get("paths").map(Value::entries).unwrap_or_default() {
            let Some(item) = self.resolve(item) else {
                // Left as written; the importer reports it.
                paths.push((path.clone(), item.clone()));
                continue;
            };
            let common: Vec<&Value> = item
                .get("parameters")
                .map(Value::items)
                .unwrap_or_default()
                .iter()
                .collect();
            let mut ops = Vec::new();
            for (method, op) in item.entries() {
                if METHODS.contains(&method.as_str()) && op.is_obj() {
                    let here = format!("{} {path}", method.to_ascii_uppercase());
                    ops.push((method.clone(), self.operation(op, &common, &here)));
                }
            }
            paths.push((path.clone(), Value::Obj(ops)));
        }
        out.push(("paths".into(), Value::Obj(paths)));

        let mut components = Vec::new();
        if let Some(defs) = root.get("definitions").filter(|d| d.is_obj()) {
            components.push(("schemas".to_owned(), schema(defs)));
        }
        let mut parameters = Vec::new();
        let mut bodies = Vec::new();
        for (name, p) in root
            .get("parameters")
            .map(Value::entries)
            .unwrap_or_default()
        {
            match p.string("in").as_str() {
                "body" => bodies.push((
                    name.clone(),
                    self.body(p, &self.media_types(root, "consumes")),
                )),
                // Form fields are merged into the request body where they are used.
                "formData" => {}
                _ => parameters.push((name.clone(), self.parameter(p))),
            }
        }
        if !parameters.is_empty() {
            components.push(("parameters".into(), Value::Obj(parameters)));
        }
        if !bodies.is_empty() {
            components.push(("requestBodies".into(), Value::Obj(bodies)));
        }
        let produces = self.media_types(root, "produces");
        let responses: Vec<(String, Value)> = root
            .get("responses")
            .map(Value::entries)
            .unwrap_or_default()
            .iter()
            .map(|(name, r)| (name.clone(), self.response(r, &produces)))
            .collect();
        if !responses.is_empty() {
            components.push(("responses".into(), Value::Obj(responses)));
        }
        let mut schemes = Vec::new();
        for (name, d) in root
            .get("securityDefinitions")
            .map(Value::entries)
            .unwrap_or_default()
        {
            if let Some(scheme) = self.security_scheme(name, d) {
                schemes.push((name.clone(), scheme));
            }
        }
        if !schemes.is_empty() {
            components.push(("securitySchemes".into(), Value::Obj(schemes)));
        }
        if !components.is_empty() {
            out.push(("components".into(), Value::Obj(components)));
        }
        if let Some(bundled) = root.get(BUNDLED_KEY) {
            out.push((BUNDLED_KEY.to_owned(), schema(bundled)));
        }
        Value::Obj(out)
    }
}

/// The OpenAPI 3.0 form of the Swagger 2.0 document `root`, with what the conversion lost.
pub(super) fn convert(root: &Value) -> (Value, Vec<Loss>) {
    let mut cx = Cx {
        root,
        losses: Losses::default(),
    };
    let v = cx.convert();
    (v, cx.losses.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::value::parse_any;

    #[test]
    fn schemas_refs_and_servers() {
        let root = parse_any(
            r##"{"swagger": "2.0", "info": {"title": "T", "version": "1"},
            "host": "api.example.com", "basePath": "/v1", "schemes": ["https", "http"],
            "consumes": ["application/json"],
            "paths": {"/pets/{id}": {"parameters": [{"name": "id", "in": "path", "required": true, "type": "integer", "format": "int64"}],
              "put": {"parameters": [{"in": "body", "name": "pet", "schema": {"$ref": "#/definitions/Pet"}}],
                      "responses": {"200": {"description": "ok", "schema": {"$ref": "#/definitions/Pet"}, "examples": {"application/json": {"id": 1}}}}},
              "post": {"consumes": ["multipart/form-data"], "parameters": [{"in": "formData", "name": "file", "type": "file"}, {"in": "formData", "name": "note", "type": "string", "required": true}]}}},
            "definitions": {"Pet": {"type": "object", "discriminator": "kind", "properties": {"tag": {"type": "string", "x-nullable": true}}}},
            "securityDefinitions": {"key": {"type": "apiKey", "name": "X-Key", "in": "header"}, "o": {"type": "oauth2", "flow": "application", "tokenUrl": "https://t"}}}"##,
        )
        .unwrap();
        let (v, losses) = convert(&root);
        assert!(losses.is_empty(), "{losses:?}");
        let urls: Vec<String> = v
            .get("servers")
            .unwrap()
            .items()
            .iter()
            .map(|s| s.string("url"))
            .collect();
        assert_eq!(
            urls,
            ["https://api.example.com/v1", "http://api.example.com/v1"]
        );
        let put = pointer(&v, "#/paths/~1pets~1{id}/put").unwrap();
        assert_eq!(
            pointer(put, "#/requestBody/content/application~1json/schema/$ref")
                .unwrap()
                .as_str(),
            Some("#/components/schemas/Pet")
        );
        assert_eq!(
            pointer(put, "#/parameters/0/schema/format")
                .unwrap()
                .as_str(),
            Some("int64")
        );
        assert_eq!(
            pointer(put, "#/responses/200/content/application~1json/example/id"),
            Some(&Value::Num("1".into()))
        );
        let post = pointer(&v, "#/paths/~1pets~1{id}/post").unwrap();
        let form = pointer(post, "#/requestBody/content/multipart~1form-data/schema").unwrap();
        assert_eq!(
            pointer(form, "#/properties/file/format").unwrap().as_str(),
            Some("binary")
        );
        assert_eq!(
            pointer(form, "#/required/0").unwrap().as_str(),
            Some("note")
        );
        let pet = pointer(&v, "#/components/schemas/Pet").unwrap();
        assert_eq!(
            pointer(pet, "#/discriminator/propertyName")
                .unwrap()
                .as_str(),
            Some("kind")
        );
        assert_eq!(
            pointer(pet, "#/properties/tag/nullable"),
            Some(&Value::Bool(true))
        );
        assert!(
            pointer(
                &v,
                "#/components/securitySchemes/o/flows/clientCredentials/tokenUrl"
            )
            .is_some()
        );
    }

    #[test]
    fn no_host_is_a_relative_server_and_unknown_auth_is_reported() {
        let root = parse_any(
            r#"{"swagger": "2.0", "basePath": "/api", "paths": {},
            "securityDefinitions": {"x": {"type": "oauth2", "flow": "magic"}}}"#,
        )
        .unwrap();
        let (v, losses) = convert(&root);
        assert_eq!(
            pointer(&v, "#/servers/0/url").unwrap().as_str(),
            Some("/api")
        );
        assert_eq!(losses.len(), 1);
        assert!(losses[0].detail.contains("magic"), "{losses:?}");
    }
}
