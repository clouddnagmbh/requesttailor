// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer for Postman collections v2.0 and v2.1, with environment and globals files.
//! Owner: wave 4, strand B.
//!
//! Mapping:
//! - Folders and requests keep their order; descriptions become `docs`, header, parameter
//!   and variable descriptions their `description`; saved responses become `examples`.
//! - A request without `auth` inherits (as in Postman); `noauth` is no auth. Basic, Bearer and
//!   API key map to the model; OAuth 2.0, OAuth 1.0, Digest, NTLM and AWS Signature v4 are
//!   kept as OpenCollection auth (reported: not executable yet); Hawk, Akamai EdgeGrid, JWT
//!   and ASAP are dropped (reported).
//! - `protocolProfileBehavior` of the collection and folders is passed down to each request:
//!   redirects and URL encoding map to settings, the rest is reported.
//! - Collection variables become collection variables, environments environment files,
//!   globals collection variables (Postman resolves globals below collection variables, so a
//!   collection variable of the same name wins). Secret values are never written.
//! - Scripts and tests are reported with their line count (scripting arrives later).

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, KeyValue, MultipartPart, Param, ParamKind, PartKind,
    RawKind, RequestSpec, Settings, Variable,
};

use super::auth::{self, AwsV4, OAuth1, OAuth2};
use super::common::{Losses, finish, invalid, location, raw_kind, url_params, utf8};
use super::out::{AuthOut, CollectionOut, DefaultsOut, ExampleOut, FolderOut, ItemOut, RequestOut};
use super::value::{Value, parse_json};
use super::{Import, ImportError, ImportFormat, LossKind, SourceFile, stem};

const F: ImportFormat = ImportFormat::Postman;

pub(super) fn import(
    main: &SourceFile<'_>,
    envs: &[SourceFile<'_>],
) -> Result<Import, ImportError> {
    let root = parse_json(&utf8(F, main.data)?).map_err(|e| invalid(F, e))?;
    if !root.is_obj() {
        return Err(invalid(F, "expected a JSON object"));
    }
    if root.get("requests").is_some() && root.get("item").is_none() {
        return Err(ImportError::UnsupportedVersion {
            format: F,
            version: "1.0 (export the collection as v2.1 in Postman)".into(),
        });
    }
    let info = root.get("info").cloned().unwrap_or(Value::Null);
    let schema = info.string("schema");
    if let Some(v) = ["v1.0.0", "/v1/"].iter().find(|v| schema.contains(*v)) {
        return Err(ImportError::UnsupportedVersion {
            format: F,
            version: v.trim_matches('/').into(),
        });
    }
    if root.get("item").is_some_and(|i| !i.is_arr()) {
        return Err(invalid(F, "`item` must be a list"));
    }

    let mut cx = Cx::default();
    let mut name = info.string("name");
    if name.trim().is_empty() {
        name = stem(main.name, &[".postman_collection.json", ".json"]).to_owned();
    }
    let here = "collection";
    let behavior = cx.behavior(
        root.get("protocolProfileBehavior"),
        here,
        &Behavior::default(),
    );
    cx.events(root.get("event"), here);
    let auth = match root.field("auth") {
        Some(a) => cx.auth(a, here, true),
        None => AuthOut::Model(Auth::None),
    };
    let (vars, var_docs) = cx.variables(root.get("variable"), here);
    let mut collection = CollectionOut {
        name,
        docs: description(info.get("description")).or_else(|| description(root.get("description"))),
        defaults: DefaultsOut {
            auth,
            vars,
            var_docs,
            ..DefaultsOut::default()
        },
        ..CollectionOut::default()
    };
    if let AuthOut::Model(a) = &collection.defaults.auth {
        cx.losses.literal_credentials(here, a, &[]);
    }
    collection.items = cx.items(root.get("item"), &[], &behavior);

    for env in envs {
        cx.environment(env, &mut collection)?;
    }
    finish(F, collection, cx.losses)
}

#[derive(Default)]
struct Cx {
    losses: Losses,
}

/// Settings inherited from `protocolProfileBehavior`.
#[derive(Debug, Clone, Default)]
struct Behavior {
    settings: Settings,
}

/// A Postman description: a string or `{content, type}`.
fn description(v: Option<&Value>) -> Option<String> {
    let v = v?;
    let text = match v {
        Value::Obj(_) => v.str_of("content")?,
        _ => v.text()?,
    };
    (!text.trim().is_empty()).then_some(text)
}

/// Auth parameters: a list of `{key, value}` (v2.1) or an object (v2.0).
fn auth_params(v: Option<&Value>) -> Vec<(String, Value)> {
    match v {
        Some(Value::Arr(items)) => items
            .iter()
            .filter_map(|p| {
                Some((
                    p.str_of("key")?,
                    p.get("value").cloned().unwrap_or(Value::Null),
                ))
            })
            .collect(),
        Some(Value::Obj(entries)) => entries.clone(),
        _ => Vec::new(),
    }
}

fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Str(s) => s.is_empty(),
        Value::Arr(a) => a.is_empty(),
        Value::Obj(o) => o.is_empty(),
        Value::Num(_) => false,
    }
}

struct Params {
    list: Vec<(String, Value)>,
    used: Vec<&'static str>,
}

impl Params {
    fn get(&mut self, key: &'static str) -> String {
        self.used.push(key);
        self.list
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.text())
            .unwrap_or_default()
    }

    /// Non-empty parameters nobody asked for, except `ignore`.
    fn rest(&self, ignore: &[&str]) -> Vec<String> {
        self.list
            .iter()
            .filter(|(k, v)| {
                !self.used.contains(&k.as_str())
                    && !ignore.contains(&k.as_str())
                    && !is_empty_value(v)
            })
            .map(|(k, _)| format!("`{k}`"))
            .collect()
    }
}

/// Parameters that only steer Postman's UI or token helper.
const UI_PARAMS: &[&str] = &[
    "showPassword",
    "saveHelperData",
    "disableRetryRequest",
    "tokenName",
    "useBrowser",
    "client_authentication",
    "grant_type",
    "tokenType",
    "addEmptyParamsToSign",
    "addParamsToHeader",
    "id",
    "refreshRequestParams",
    "authRequestParams",
    "tokenRequestParams",
];

/// Stored tokens of Postman's OAuth 2.0 helper; secrets, never written.
const TOKEN_PARAMS: &[&str] = &[
    "accessToken",
    "refreshToken",
    "id_token",
    "idToken",
    "expires_in",
];

fn kv(name: impl Into<String>, value: impl Into<String>, enabled: bool) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled,
    }
}

fn enabled(v: &Value) -> bool {
    !v.flag("disabled")
}

/// Headers: a list of `{key, value, disabled}` or `Name: value` lines.
fn headers(v: Option<&Value>) -> (Vec<KeyValue>, Vec<Option<String>>) {
    let mut list = Vec::new();
    let mut docs = Vec::new();
    match v {
        Some(Value::Arr(items)) => {
            for h in items {
                let Some(name) = h.str_of("key") else {
                    continue;
                };
                list.push(kv(name, h.string("value"), enabled(h)));
                docs.push(description(h.get("description")));
            }
        }
        Some(Value::Str(s)) => {
            for line in s.lines() {
                if let Some((n, val)) = line.split_once(':')
                    && !n.trim().is_empty()
                {
                    list.push(kv(n.trim(), val.trim(), true));
                    docs.push(None);
                }
            }
        }
        _ => {}
    }
    (list, docs)
}

fn preview_kind(lang: &str) -> &'static str {
    match lang {
        "json" => "json",
        "xml" => "xml",
        "html" => "html",
        _ => "text",
    }
}

impl Cx {
    fn behavior(&mut self, v: Option<&Value>, here: &str, parent: &Behavior) -> Behavior {
        let mut b = parent.clone();
        let Some(v) = v else { return b };
        let mut dropped = Vec::new();
        for (k, val) in v.entries() {
            match k.as_str() {
                "followRedirects" => b.settings.follow_redirects = val.as_bool(),
                "maxRedirects" => {
                    b.settings.max_redirects = val
                        .as_f64()
                        .filter(|n| n.is_finite() && *n >= 0.0)
                        .map(|n| n.min(u32::MAX as f64) as u32)
                }
                "disableUrlEncoding" => b.settings.encode_url = val.as_bool().map(|d| !d),
                _ => dropped.push(format!("`{k}`")),
            }
        }
        if !dropped.is_empty() {
            self.losses.dropped(
                here,
                LossKind::Setting,
                format!("request settings not imported: {}", dropped.join(", ")),
            );
        }
        b
    }

    fn events(&mut self, v: Option<&Value>, here: &str) {
        for e in v.map(Value::items).unwrap_or_default() {
            let script = e.get("script");
            let code = match script.and_then(|s| s.get("exec")) {
                Some(Value::Arr(lines)) => lines
                    .iter()
                    .filter_map(Value::text)
                    .collect::<Vec<_>>()
                    .join("\n"),
                Some(v) => v.text().unwrap_or_default(),
                None => String::new(),
            };
            let what = match e.string("listen").as_str() {
                "prerequest" => "pre-request script",
                "test" => "test script",
                _ => "script",
            };
            self.losses.script(here, what, &code);
        }
    }

    fn variables(&mut self, v: Option<&Value>, here: &str) -> (Vec<Variable>, Vec<Option<String>>) {
        let mut vars = Vec::new();
        let mut docs = Vec::new();
        for item in v.map(Value::items).unwrap_or_default() {
            let Some(name) = item
                .str_of("key")
                .or_else(|| item.str_of("id"))
                .or_else(|| item.str_of("name"))
            else {
                continue;
            };
            let secret = item.string("type") == "secret";
            let value = match item.get("value") {
                Some(v @ (Value::Obj(_) | Value::Arr(_))) => Some(v.to_json()),
                Some(v) => v.text(),
                None => None,
            };
            if secret && value.as_deref().is_some_and(|v| !v.is_empty()) {
                self.losses.secret_value(here, &name);
            }
            vars.push(Variable {
                name,
                value: if secret {
                    None
                } else {
                    Some(value.unwrap_or_default())
                },
                enabled: enabled(item),
                secret,
            });
            docs.push(description(item.get("description")));
        }
        (vars, docs)
    }

    fn items(&mut self, v: Option<&Value>, path: &[String], behavior: &Behavior) -> Vec<ItemOut> {
        let mut out = Vec::new();
        for item in v.map(Value::items).unwrap_or_default() {
            if !item.is_obj() {
                continue;
            }
            let name = item.string("name");
            let here = location(path, &name);
            if let Some(children) = item.get("item") {
                out.push(ItemOut::Folder(
                    self.folder(item, children, path, &name, &here, behavior),
                ));
            } else if let Some(request) = item.get("request") {
                out.push(ItemOut::Request(
                    self.request(item, request, &here, behavior),
                ));
            } else {
                self.losses.dropped(
                    &here,
                    LossKind::Other,
                    "item without a request not imported",
                );
            }
        }
        out
    }

    fn folder(
        &mut self,
        item: &Value,
        children: &Value,
        path: &[String],
        name: &str,
        here: &str,
        parent: &Behavior,
    ) -> FolderOut {
        let behavior = self.behavior(item.get("protocolProfileBehavior"), here, parent);
        self.events(item.get("event"), here);
        let auth = match item.field("auth") {
            Some(a) => self.auth(a, here, false),
            None => AuthOut::Model(Auth::Inherit),
        };
        if let AuthOut::Model(a) = &auth {
            self.losses.literal_credentials(here, a, &[]);
        }
        let (vars, var_docs) = self.variables(item.get("variable"), here);
        let mut sub = path.to_vec();
        sub.push(name.to_owned());
        FolderOut {
            name: name.to_owned(),
            docs: description(item.get("description")),
            defaults: DefaultsOut {
                auth,
                vars,
                var_docs,
                ..DefaultsOut::default()
            },
            items: self.items(Some(children), &sub, &behavior),
        }
    }

    fn request(
        &mut self,
        item: &Value,
        request: &Value,
        here: &str,
        parent: &Behavior,
    ) -> RequestOut {
        let behavior = self.behavior(item.get("protocolProfileBehavior"), here, parent);
        self.events(item.get("event"), here);
        // v2.0 allows the request to be just its URL.
        let request = match request {
            Value::Str(url) => Value::Obj(vec![("url".into(), Value::Str(url.clone()))]),
            other => other.clone(),
        };
        let (url, params, param_docs) = self.url(request.get("url"));
        let (headers, header_docs) = headers(request.get("header"));
        let body = self.body(request.get("body"), &headers, here);
        let auth = match request.field("auth") {
            Some(a) => self.auth(a, here, false),
            None => AuthOut::Model(Auth::Inherit),
        };
        let (vars, _) = self.variables(item.get("variable"), here);
        if !vars.is_empty() {
            self.losses.approximated(
                here,
                LossKind::Variable,
                "request variables imported as request variables (Postman has no such scope; \
                 they were collection-wide there)",
            );
        }
        for key in ["proxy", "certificate"] {
            if request.field(key).is_some() {
                self.losses.dropped(
                    here,
                    LossKind::Setting,
                    format!("{key} settings not imported"),
                );
            }
        }
        let mut method = request.string("method").trim().to_ascii_uppercase();
        if method.is_empty() {
            method = "GET".into();
        }
        let spec = RequestSpec {
            name: item.string("name"),
            seq: None,
            method,
            url,
            headers,
            params,
            body,
            auth: Auth::Inherit,
            vars,
            settings: behavior.settings.clone(),
        };
        let mut texts: Vec<&str> = vec![&spec.url];
        texts.extend(spec.headers.iter().map(|h| h.value.as_str()));
        texts.extend(spec.params.iter().map(|p| p.value.as_str()));
        if let Body::Raw { data, .. } = &spec.body {
            texts.push(data);
        }
        if let Body::FormUrlEncoded { fields } = &spec.body {
            texts.extend(fields.iter().map(|f| f.value.as_str()));
        }
        self.losses.dynamic_vars(here, texts);
        let auth_model = match &auth {
            AuthOut::Model(a) => a.clone(),
            AuthOut::Other { .. } => Auth::Inherit,
        };
        self.losses
            .literal_credentials(here, &auth_model, &spec.headers);

        let mut r = RequestOut::new(spec);
        r.auth = auth;
        r.docs = description(request.get("description"))
            .or_else(|| description(item.get("description")));
        r.header_docs = header_docs;
        r.param_docs = param_docs;
        for (i, resp) in item
            .get("response")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            if let Some(x) = self.example(resp, i, here) {
                r.examples.push(x);
            }
        }
        r
    }

    /// The URL text, its parameters and their descriptions.
    fn url(&mut self, v: Option<&Value>) -> (String, Vec<Param>, Vec<Option<String>>) {
        let Some(u) = v else {
            return (String::new(), Vec::new(), Vec::new());
        };
        if let Some(s) = u.as_str() {
            let params = url_params(s, &[]);
            let n = params.len();
            return (s.to_owned(), params, vec![None; n]);
        }
        if !u.is_obj() {
            return (String::new(), Vec::new(), Vec::new());
        }
        let raw = u.str_of("raw").unwrap_or_else(|| build_url(u));
        let path_values: Vec<(String, String)> = u
            .get("variable")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|p| {
                Some((
                    p.str_of("key").or_else(|| p.str_of("id"))?,
                    p.string("value"),
                ))
            })
            .collect();
        let path_docs: Vec<(String, Option<String>)> = u
            .get("variable")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|p| {
                Some((
                    p.str_of("key").or_else(|| p.str_of("id"))?,
                    description(p.get("description")),
                ))
            })
            .collect();
        let derived = url_params(&raw, &path_values);
        let mut params = Vec::new();
        let mut docs = Vec::new();
        match u.get("query") {
            Some(Value::Arr(query)) => {
                for q in query {
                    let Some(name) = q.str_of("key") else {
                        continue;
                    };
                    params.push(Param {
                        name,
                        value: q.string("value"),
                        kind: ParamKind::Query,
                        enabled: enabled(q),
                    });
                    docs.push(description(q.get("description")));
                }
            }
            _ => {
                for p in derived.iter().filter(|p| p.kind == ParamKind::Query) {
                    params.push(p.clone());
                    docs.push(None);
                }
            }
        }
        for p in derived.into_iter().filter(|p| p.kind == ParamKind::Path) {
            docs.push(
                path_docs
                    .iter()
                    .find(|(n, _)| *n == p.name)
                    .and_then(|(_, d)| d.clone()),
            );
            params.push(p);
        }
        (raw, params, docs)
    }

    fn body(&mut self, v: Option<&Value>, headers: &[KeyValue], here: &str) -> Body {
        let Some(b) = v.filter(|b| b.is_obj()) else {
            return Body::None;
        };
        if b.flag("disabled") {
            self.losses
                .dropped(here, LossKind::Body, "disabled body not imported");
            return Body::None;
        }
        let content_type = headers
            .iter()
            .find(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| h.value.clone());
        match b.string("mode").as_str() {
            "raw" => {
                let data = b.string("raw");
                let language = b
                    .get("options")
                    .and_then(|o| o.get("raw"))
                    .map(|r| r.string("language"))
                    .unwrap_or_default();
                let kind = match language.as_str() {
                    "json" => RawKind::Json,
                    "xml" => RawKind::Xml,
                    "" => content_type.as_deref().map_or(RawKind::Text, raw_kind),
                    _ => RawKind::Text,
                };
                if data.is_empty() {
                    return Body::None;
                }
                Body::Raw { kind, data }
            }
            "urlencoded" => Body::FormUrlEncoded {
                fields: b
                    .get("urlencoded")
                    .map(Value::items)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|f| Some(kv(f.str_of("key")?, f.string("value"), enabled(f))))
                    .collect(),
            },
            "formdata" => {
                let mut parts = Vec::new();
                for p in b.get("formdata").map(Value::items).unwrap_or_default() {
                    let Some(name) = p.str_of("key") else {
                        continue;
                    };
                    let content_type = p.str_of("contentType").filter(|c| !c.is_empty());
                    if p.string("type") == "file" {
                        let values: Vec<String> = match p.get("src") {
                            Some(Value::Arr(a)) => a.iter().filter_map(Value::text).collect(),
                            Some(v) => v.text().into_iter().filter(|s| !s.is_empty()).collect(),
                            None => Vec::new(),
                        };
                        self.file_paths(here, &name, &values);
                        parts.push(MultipartPart {
                            name,
                            kind: PartKind::File,
                            values,
                            content_type,
                            enabled: enabled(p),
                        });
                    } else {
                        parts.push(MultipartPart {
                            name,
                            kind: PartKind::Text,
                            values: vec![p.string("value")],
                            content_type,
                            enabled: enabled(p),
                        });
                    }
                }
                Body::Multipart { parts }
            }
            "file" => {
                let file = b.get("file");
                let src = file.map(|f| f.string("src")).unwrap_or_default();
                if src.is_empty() {
                    if let Some(content) = file.and_then(|f| f.str_of("content")) {
                        self.losses.approximated(
                            here,
                            LossKind::Body,
                            "inline file content imported as a text body",
                        );
                        return Body::Raw {
                            kind: RawKind::Text,
                            data: content,
                        };
                    }
                    self.losses.dropped(
                        here,
                        LossKind::Body,
                        "file body without a file not imported",
                    );
                    return Body::None;
                }
                self.file_paths(here, "body", std::slice::from_ref(&src));
                Body::File {
                    path: src,
                    content_type,
                }
            }
            "graphql" => {
                let g = b.get("graphql");
                let query = g.map(|g| g.string("query")).unwrap_or_default();
                let variables = g.map(|g| g.string("variables")).unwrap_or_default();
                self.losses.approximated(
                    here,
                    LossKind::Body,
                    "GraphQL body imported as a JSON body ({\"query\", \"variables\"})",
                );
                Body::Raw {
                    kind: RawKind::Json,
                    data: graphql_json(&query, &variables, &mut self.losses, here),
                }
            }
            "" => Body::None,
            other => {
                self.losses.dropped(
                    here,
                    LossKind::Body,
                    format!("body of type `{other}` not imported"),
                );
                Body::None
            }
        }
    }

    fn file_paths(&mut self, here: &str, part: &str, values: &[String]) {
        if values.is_empty() {
            self.losses.approximated(
                here,
                LossKind::Body,
                format!("file part `{part}` has no file selected; choose one in the request"),
            );
        } else {
            self.losses.approximated(
                here,
                LossKind::Body,
                format!(
                    "file {} for `{part}` refers to the original computer; copy it into the \
                     collection folder and update the path",
                    values
                        .iter()
                        .map(|v| format!("`{v}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
    }

    fn example(&mut self, resp: &Value, index: usize, here: &str) -> Option<ExampleOut> {
        if !resp.is_obj() {
            return None;
        }
        let mut name = resp.string("name");
        if name.trim().is_empty() {
            name = format!("Example {}", index + 1);
        }
        let mut x = ExampleOut {
            name: name.clone(),
            status: resp
                .get("code")
                .and_then(Value::as_f64)
                .filter(|c| (0.0..1000.0).contains(c))
                .map(|c| c as u16),
            status_text: resp.str_of("status"),
            ..ExampleOut::default()
        };
        let (hs, _) = headers(resp.get("header"));
        x.headers = hs.into_iter().map(|h| (h.name, h.value)).collect();
        if let Some(body) = resp.str_of("body") {
            let lang = resp.string("_postman_previewlanguage");
            let ct = x
                .headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.to_ascii_lowercase())
                .unwrap_or_default();
            let kind = if lang.is_empty() || lang == "raw" || lang == "text" {
                if ct.contains("json") {
                    "json"
                } else if ct.contains("xml") {
                    "xml"
                } else if ct.contains("html") {
                    "html"
                } else {
                    "text"
                }
            } else {
                preview_kind(&lang)
            };
            x.body = Some((kind, body));
        }
        if resp.get("cookie").is_some_and(|c| !c.items().is_empty()) {
            self.losses.dropped(
                here,
                LossKind::Example,
                format!("cookies of example `{name}` not imported"),
            );
        }
        if let Some(orig) = resp.field("originalRequest") {
            let orig = match orig {
                Value::Str(url) => Value::Obj(vec![("url".into(), Value::Str(url.clone()))]),
                o => o.clone(),
            };
            let url = match orig.get("url") {
                Some(Value::Str(s)) => Some(s.clone()),
                Some(u) if u.is_obj() => Some(u.str_of("raw").unwrap_or_else(|| build_url(u))),
                _ => None,
            };
            x.url = url;
            x.method = orig.str_of("method");
            x.request_headers = headers(orig.get("header")).0;
            if let Some(b) = orig.get("body").filter(|b| b.is_obj()) {
                match b.string("mode").as_str() {
                    "raw" => {
                        let data = b.string("raw");
                        if !data.is_empty() {
                            let ct = x
                                .request_headers
                                .iter()
                                .find(|h| h.name.eq_ignore_ascii_case("content-type"))
                                .map(|h| h.value.clone())
                                .unwrap_or_default();
                            let lang = b
                                .get("options")
                                .and_then(|o| o.get("raw"))
                                .map(|r| r.string("language"))
                                .unwrap_or_default();
                            let kind = match lang.as_str() {
                                "json" => RawKind::Json,
                                "xml" => RawKind::Xml,
                                _ => raw_kind(&ct),
                            };
                            x.request_body = Some((kind, data));
                        }
                    }
                    "" => {}
                    mode => {
                        let has_content = b.get(mode).is_some_and(|c| !is_empty_value(c));
                        if has_content {
                            self.losses.dropped(
                                here,
                                LossKind::Example,
                                format!("request body ({mode}) of example `{name}` not imported"),
                            );
                        }
                    }
                }
            }
        }
        Some(x)
    }

    /// Maps a Postman auth object. `collection`: no auth is written as its absence there.
    fn auth(&mut self, a: &Value, here: &str, collection: bool) -> AuthOut {
        let kind = a.string("type");
        let mut p = Params {
            list: auth_params(a.get(kind.as_str())),
            used: Vec::new(),
        };
        let none = || AuthOut::Model(Auth::None);
        let out = match kind.as_str() {
            "noauth" | "" => return none(),
            "inherit" => {
                return if collection {
                    none()
                } else {
                    AuthOut::Model(Auth::Inherit)
                };
            }
            "basic" => AuthOut::Model(Auth::Basic {
                username: p.get("username"),
                password: p.get("password"),
            }),
            "bearer" => AuthOut::Model(Auth::Bearer {
                token: p.get("token"),
            }),
            "apikey" => {
                let mut key = p.get("key");
                if key.is_empty() {
                    key = "X-API-Key".into();
                }
                AuthOut::Model(Auth::ApiKey {
                    key,
                    value: p.get("value"),
                    placement: if p.get("in") == "query" {
                        ApiKeyPlacement::Query
                    } else {
                        ApiKeyPlacement::Header
                    },
                })
            }
            "digest" => {
                let out = auth::user_password("digest", &p.get("username"), &p.get("password"));
                auth::report(&mut self.losses, here, &out, "Digest auth");
                out
            }
            "ntlm" => {
                let out = auth::ntlm(&p.get("username"), &p.get("password"), &p.get("domain"));
                auth::report(&mut self.losses, here, &out, "NTLM auth");
                out
            }
            "awsv4" => {
                let out = auth::awsv4(&AwsV4 {
                    access_key_id: p.get("accessKey"),
                    secret_access_key: p.get("secretKey"),
                    session_token: p.get("sessionToken"),
                    service: p.get("service"),
                    region: p.get("region"),
                });
                auth::report(&mut self.losses, here, &out, "AWS Signature v4");
                out
            }
            "oauth1" => {
                let o = OAuth1 {
                    consumer_key: p.get("consumerKey"),
                    consumer_secret: p.get("consumerSecret"),
                    access_token: p.get("token"),
                    access_token_secret: p.get("tokenSecret"),
                    callback_url: p.get("callback"),
                    verifier: p.get("verifier"),
                    signature_method: p.get("signatureMethod"),
                    timestamp: p.get("timestamp"),
                    nonce: p.get("nonce"),
                    version: p.get("version"),
                    realm: p.get("realm"),
                };
                if auth::oauth1_method_unknown(&o) {
                    self.losses.dropped(
                        here,
                        LossKind::Auth,
                        format!(
                            "OAuth 1.0 signature method `{}` not imported",
                            o.signature_method
                        ),
                    );
                }
                let out = auth::oauth1(&o);
                auth::report(&mut self.losses, here, &out, "OAuth 1.0");
                out
            }
            "oauth2" => {
                let grant = p.get("grant_type");
                let flow = auth::oauth2_flow(&grant).unwrap_or("authorization_code");
                if !grant.is_empty() && auth::oauth2_flow(&grant).is_none() {
                    self.losses.approximated(
                        here,
                        LossKind::Auth,
                        format!("OAuth 2.0 grant type `{grant}` imported as authorization code"),
                    );
                }
                let pkce = (grant == "authorization_code_with_pkce").then(|| {
                    if p.get("challengeAlgorithm") == "plain" {
                        "plain"
                    } else {
                        "S256"
                    }
                });
                let o = OAuth2 {
                    flow,
                    authorization_url: p.get("authUrl"),
                    access_token_url: p.get("accessTokenUrl"),
                    refresh_token_url: p.get("refreshTokenUrl"),
                    callback_url: p.get("redirect_uri"),
                    client_id: p.get("clientId"),
                    client_secret: p.get("clientSecret"),
                    credentials_in_body: p.get("client_authentication") == "body",
                    scope: p.get("scope"),
                    state: p.get("state"),
                    username: p.get("username"),
                    password: p.get("password"),
                    pkce,
                };
                let header_prefix = p.get("headerPrefix");
                if !header_prefix.is_empty() && header_prefix != "Bearer" {
                    p.used.retain(|k| *k != "headerPrefix");
                }
                let add_to = p.get("addTokenTo");
                if add_to == "queryParams" {
                    p.used.retain(|k| *k != "addTokenTo");
                }
                p.get("challengeAlgorithm");
                if TOKEN_PARAMS
                    .iter()
                    .any(|k| p.list.iter().any(|(n, v)| n == k && !is_empty_value(v)))
                {
                    self.losses.dropped(
                        here,
                        LossKind::Auth,
                        "stored OAuth 2.0 tokens not imported; fetch a new token",
                    );
                }
                p.used.extend(TOKEN_PARAMS);
                let out = auth::oauth2(&o);
                auth::report(&mut self.losses, here, &out, "OAuth 2.0");
                out
            }
            other => {
                let label = match other {
                    "hawk" => "Hawk auth",
                    "akamai" | "edgegrid" => "Akamai EdgeGrid auth",
                    "jwt" => "JWT bearer auth",
                    "asap" => "ASAP auth",
                    _ => "auth",
                };
                self.losses.dropped(
                    here,
                    LossKind::Auth,
                    format!(
                        "{label} (`{other}`) not imported: OpenCollection cannot hold it; the \
                         request is sent without auth"
                    ),
                );
                return none();
            }
        };
        let rest = p.rest(UI_PARAMS);
        if !rest.is_empty() {
            self.losses.dropped(
                here,
                LossKind::Auth,
                format!(
                    "{} settings not imported: {}",
                    auth_label(&kind),
                    rest.join(", ")
                ),
            );
        }
        out
    }

    fn environment(
        &mut self,
        f: &SourceFile<'_>,
        c: &mut CollectionOut,
    ) -> Result<(), ImportError> {
        let v =
            parse_json(&utf8(F, f.data)?).map_err(|e| invalid(F, format!("{}: {e}", f.name)))?;
        // Old Postman versions wrote globals as a bare list of variables.
        let legacy_globals = v.is_arr();
        if !v.is_obj() && !legacy_globals {
            return Err(invalid(F, format!("{}: expected a JSON object", f.name)));
        }
        let globals = legacy_globals || v.string("_postman_variable_scope") == "globals";
        let mut name = v.string("name");
        if name.trim().is_empty() {
            name = stem(
                f.name,
                &[
                    ".postman_environment.json",
                    ".postman_globals.json",
                    ".json",
                ],
            )
            .to_owned();
        }
        let here = if globals {
            "globals".to_owned()
        } else {
            format!("environment {name}")
        };
        let mut vars = Vec::new();
        let values = if legacy_globals {
            v.items()
        } else {
            v.get("values").map(Value::items).unwrap_or_default()
        };
        for item in values {
            let Some(key) = item.str_of("key") else {
                continue;
            };
            let secret = item.string("type") == "secret";
            let value = item.get("value").and_then(Value::text).unwrap_or_default();
            if secret && !value.is_empty() {
                self.losses.secret_value(&here, &key);
            }
            vars.push(Variable {
                name: key,
                value: if secret { None } else { Some(value) },
                enabled: item.get("enabled").and_then(Value::as_bool) != Some(false),
                secret,
            });
        }
        if globals {
            let mut added = 0;
            for var in vars {
                if c.defaults.vars.iter().any(|x| x.name == var.name) {
                    self.losses.dropped(
                        &here,
                        LossKind::Variable,
                        format!(
                            "global `{}` not imported: a collection variable of the same name \
                             takes precedence",
                            var.name
                        ),
                    );
                    continue;
                }
                c.defaults.vars.push(var);
                c.defaults.var_docs.push(None);
                added += 1;
            }
            if added > 0 {
                self.losses.approximated(
                    &here,
                    LossKind::Variable,
                    format!(
                        "{added} global {} imported as collection {}",
                        if added == 1 { "variable" } else { "variables" },
                        if added == 1 { "variable" } else { "variables" }
                    ),
                );
            }
            return Ok(());
        }
        let mut unique = name.clone();
        let mut n = 2;
        while c.environments.iter().any(|e| e.name == unique) {
            unique = format!("{name} ({n})");
            n += 1;
        }
        if unique != name {
            self.losses.approximated(
                &here,
                LossKind::Variable,
                format!("environment imported as `{unique}`: the name was already used"),
            );
        }
        if unique.trim().is_empty() || unique.chars().any(char::is_control) {
            unique = "Environment".into();
        }
        c.environments.push(Environment {
            client_certificates: Vec::new(),
            external_secrets: None,
            name: unique,
            variables: vars,
        });
        Ok(())
    }
}

/// A URL object without `raw`, put back together.
fn build_url(u: &Value) -> String {
    let join = |v: Option<&Value>, sep: &str| -> String {
        match v {
            Some(Value::Arr(parts)) => parts
                .iter()
                .filter_map(|p| p.text().or_else(|| p.str_of("value")))
                .collect::<Vec<_>>()
                .join(sep),
            Some(v) => v.text().unwrap_or_default(),
            None => String::new(),
        }
    };
    let mut out = String::new();
    let protocol = u.string("protocol");
    if !protocol.is_empty() {
        out.push_str(&protocol);
        out.push_str("://");
    }
    out.push_str(&join(u.get("host"), "."));
    let port = u.string("port");
    if !port.is_empty() {
        out.push(':');
        out.push_str(&port);
    }
    let path = join(u.get("path"), "/");
    if !path.is_empty() {
        if !path.starts_with('/') {
            out.push('/');
        }
        out.push_str(&path);
    }
    let query: Vec<String> = u
        .get("query")
        .map(Value::items)
        .unwrap_or_default()
        .iter()
        .filter(|q| enabled(q))
        .filter_map(|q| {
            let k = q.str_of("key")?;
            Some(match q.str_of("value") {
                Some(v) => format!("{k}={v}"),
                None => k,
            })
        })
        .collect();
    if !query.is_empty() {
        out.push('?');
        out.push_str(&query.join("&"));
    }
    let hash = u.string("hash");
    if !hash.is_empty() {
        out.push('#');
        out.push_str(&hash);
    }
    out
}

/// `{"query": ..., "variables": ...}` for a GraphQL request.
pub(crate) fn graphql_json(
    query: &str,
    variables: &str,
    losses: &mut Losses,
    here: &str,
) -> String {
    let vars = if variables.trim().is_empty() {
        None
    } else {
        match parse_json(variables) {
            Ok(v) => Some(v),
            Err(_) => {
                losses.dropped(
                    here,
                    LossKind::Body,
                    "GraphQL variables are not valid JSON; not imported",
                );
                None
            }
        }
    };
    let mut entries = vec![("query".to_owned(), Value::Str(query.to_owned()))];
    if let Some(v) = vars {
        entries.push(("variables".to_owned(), v));
    }
    Value::Obj(entries).to_json_pretty()
}

/// The name users know an auth type by.
fn auth_label(kind: &str) -> &str {
    match kind {
        "basic" => "Basic auth",
        "bearer" => "Bearer token",
        "apikey" => "API key",
        "digest" => "Digest auth",
        "ntlm" => "NTLM auth",
        "awsv4" => "AWS Signature v4",
        "oauth1" => "OAuth 1.0",
        "oauth2" => "OAuth 2.0",
        other => other,
    }
}
