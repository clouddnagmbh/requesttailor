// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer for Insomnia exports: v4 (JSON or YAML, a flat `resources` list) and v5 (YAML,
//! a tree). Owner: wave 4, strand B.
//!
//! Mapping:
//! - Request groups become folders (with their headers, auth and environment as folder
//!   defaults), ordered by Insomnia's sort keys. Several workspaces in one v4 export become
//!   one top-level folder each.
//! - Insomnia appends `parameters` to the URL when sending; the import writes them into the
//!   URL (enabled ones) so the request sends the same.
//! - Templates: `{{ _.name }}` and `{{ name }}` become `{{name}}`. Template tags
//!   (`{% uuid %}`, `{% response ... %}`, ...) and filters stay as text and are reported.
//! - The base environment becomes the collection variables, sub-environments environments.
//!   Nested values are flattened (`user.name`). Private environments are secrets: their
//!   values are never written.
//! - Scripts, unit tests, gRPC/WebSocket requests, API specs and cookie jars are reported.

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, KeyValue, MultipartPart, Param, ParamKind, PartKind,
    RawKind, RequestSpec, Settings, Variable,
};

use super::auth::{self, AwsV4, OAuth1, OAuth2};
use super::common::{Losses, finish, invalid, location, plural, raw_kind, url_params, utf8};
use super::out::{AuthOut, CollectionOut, DefaultsOut, FolderOut, ItemOut, RequestOut};
use super::value::{Value, parse_any};
use super::{Import, ImportError, ImportFormat, LossKind, SourceFile, stem};

const F: ImportFormat = ImportFormat::Insomnia;

pub(super) fn import(file: &SourceFile<'_>) -> Result<Import, ImportError> {
    let root = parse_any(&utf8(F, file.data)?).map_err(|e| invalid(F, e))?;
    if !root.is_obj() {
        return Err(invalid(F, "expected a JSON or YAML object"));
    }
    let mut cx = Cx::default();
    let fallback = stem(
        file.name,
        &[".insomnia.json", ".insomnia.yaml", ".json", ".yaml", ".yml"],
    )
    .to_owned();
    let kind = root.string("type");
    let collection = if root.get("resources").is_some() || root.get("__export_format").is_some() {
        let format = root.string("__export_format");
        if !format.is_empty() && format != "4" {
            return Err(ImportError::UnsupportedVersion {
                format: F,
                version: format,
            });
        }
        cx.v4(&root, &fallback)?
    } else if let Some(version) = kind
        .strip_prefix("collection.insomnia.rest/")
        .or_else(|| kind.strip_prefix("spec.insomnia.rest/"))
        .or_else(|| kind.strip_prefix("environment.insomnia.rest/"))
    {
        if !version.starts_with('5') {
            return Err(ImportError::UnsupportedVersion {
                format: F,
                version: version.to_owned(),
            });
        }
        cx.v5(&root, &fallback)
    } else {
        return Err(invalid(F, "neither an Insomnia v4 export nor a v5 file"));
    };
    finish(F, collection, cx.losses)
}

#[derive(Default)]
struct Cx {
    losses: Losses,
    /// Template tags already reported, per location.
    seen_tags: Vec<(String, String)>,
}

/// The parts of an item that differ between v4 and v5.
#[derive(Clone, Copy)]
struct Item<'a> {
    v: &'a Value,
    v5: bool,
}

impl<'a> Item<'a> {
    fn description(&self) -> Option<String> {
        let d = if self.v5 {
            self.v.get("meta").and_then(|m| m.str_of("description"))
        } else {
            self.v.str_of("description")
        };
        d.filter(|d| !d.trim().is_empty())
    }

    fn sort_key(&self) -> Option<f64> {
        if self.v5 {
            self.v
                .get("meta")
                .and_then(|m| m.get("sortKey"))
                .and_then(Value::as_f64)
        } else {
            self.v.get("metaSortKey").and_then(Value::as_f64)
        }
    }

    fn id(&self) -> String {
        if self.v5 {
            self.v
                .get("meta")
                .map(|m| m.string("id"))
                .unwrap_or_default()
        } else {
            self.v.string("_id")
        }
    }

    fn scripts(&self) -> (String, String) {
        if self.v5 {
            let s = self.v.get("scripts");
            (
                s.map(|s| s.string("preRequest")).unwrap_or_default(),
                s.map(|s| s.string("afterResponse")).unwrap_or_default(),
            )
        } else {
            (
                self.v.string("preRequestScript"),
                self.v.string("afterResponseScript"),
            )
        }
    }
}

fn sorted(mut items: Vec<Item<'_>>) -> Vec<Item<'_>> {
    // Stable: items without a key keep their place relative to each other.
    items.sort_by(|a, b| match (a.sort_key(), b.sort_key()) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        _ => std::cmp::Ordering::Equal,
    });
    items
}

/// `{{ _.a.b }}` / `{{a}}` → `{{a.b}}` / `{{a}}`; anything else in braces stays.
fn convert_template(text: &str) -> String {
    if !text.contains("{{") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = after[..end].trim();
        let name = inner.strip_prefix("_.").unwrap_or(inner);
        let simple = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '$'));
        if simple {
            out.push_str("{{");
            out.push_str(name);
            out.push_str("}}");
        } else {
            out.push_str(&rest[start..start + 2 + end + 2]);
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Flattens environment data: nested objects to `a.b` names, arrays to JSON text.
fn flatten(prefix: &str, data: &Value, out: &mut Vec<(String, String)>, nested: &mut bool) {
    for (k, v) in data.entries() {
        let name = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            Value::Obj(_) => {
                *nested = true;
                flatten(&name, v, out, nested);
            }
            Value::Arr(_) => {
                *nested = true;
                out.push((name, v.to_json()));
            }
            Value::Null => out.push((name, String::new())),
            v => out.push((name, v.text().unwrap_or_default())),
        }
    }
}

impl Cx {
    /// Converts a template and reports tags and filters it keeps as text.
    fn template(&mut self, text: &str, here: &str) -> String {
        let converted = convert_template(text);
        let mut rest = converted.as_str();
        while let Some(s) = rest.find("{%") {
            let after = &rest[s + 2..];
            let tag = after
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches("%}")
                .to_owned();
            self.report_tag(here, &format!("{{% {tag} %}}"));
            rest = after;
        }
        let mut rest = converted.as_str();
        while let Some(s) = rest.find("{{") {
            let after = &rest[s + 2..];
            let Some(e) = after.find("}}") else { break };
            if after[..e].contains('|') || after[..e].contains('(') {
                self.report_tag(here, &format!("{{{{{}}}}}", after[..e].trim()));
            }
            rest = &after[e..];
        }
        converted
    }

    fn report_tag(&mut self, here: &str, tag: &str) {
        let key = (here.to_owned(), tag.to_owned());
        if self.seen_tags.contains(&key) {
            return;
        }
        self.seen_tags.push(key);
        self.losses.approximated(
            here,
            LossKind::Variable,
            format!("template tag `{tag}` kept as text; RequestTailor does not evaluate it"),
        );
    }

    fn variables(&mut self, data: Option<&Value>, here: &str, private: bool) -> Vec<Variable> {
        let Some(data) = data.filter(|d| d.is_obj()) else {
            return Vec::new();
        };
        let mut flat = Vec::new();
        let mut nested = false;
        flatten("", data, &mut flat, &mut nested);
        if nested {
            self.losses.approximated(
                here,
                LossKind::Variable,
                "nested values imported as flat variables (`a.b`); lists as JSON text",
            );
        }
        flat.into_iter()
            .filter(|(n, _)| !n.is_empty())
            .map(|(name, value)| {
                if private {
                    if !value.is_empty() {
                        self.losses.secret_value(here, &name);
                    }
                    Variable {
                        name,
                        value: None,
                        enabled: true,
                        secret: true,
                    }
                } else {
                    let value = self.template(&value, here);
                    Variable::new(name, value)
                }
            })
            .collect()
    }

    /// Variables of an environment object (`data`, or `kvPairData` for key-value ones).
    fn environment_vars(&mut self, env: &Value, here: &str) -> Vec<Variable> {
        let private = env.flag("isPrivate") || env.get("meta").is_some_and(|m| m.flag("isPrivate"));
        if env.string("environmentType") == "kv" {
            let mut vars = Vec::new();
            for kv in env.get("kvPairData").map(Value::items).unwrap_or_default() {
                let name = kv.string("name");
                if name.is_empty() {
                    continue;
                }
                let secret = private || kv.string("type") == "secret";
                let value = kv.string("value");
                if secret && !value.is_empty() {
                    self.losses.secret_value(here, &name);
                }
                vars.push(Variable {
                    value: if secret {
                        None
                    } else {
                        Some(self.template(&value, here))
                    },
                    name,
                    enabled: kv.get("enabled").and_then(Value::as_bool) != Some(false),
                    secret,
                });
            }
            return vars;
        }
        self.variables(env.get("data"), here, private)
    }

    fn v4(&mut self, root: &Value, fallback: &str) -> Result<CollectionOut, ImportError> {
        let resources = root
            .get("resources")
            .filter(|r| r.is_arr())
            .ok_or_else(|| invalid(F, "no `resources` list"))?
            .items();
        let item = |v| Item { v, v5: false };
        let workspaces: Vec<&Value> = resources
            .iter()
            .filter(|r| r.string("_type") == "workspace")
            .collect();
        let children = |parent: &str| -> Vec<Item<'_>> {
            sorted(
                resources
                    .iter()
                    .filter(|r| r.string("parentId") == parent)
                    .map(item)
                    .collect(),
            )
        };
        let mut collection = CollectionOut::default();
        let multi = workspaces.len() > 1;
        // Resources nobody points to (no workspace) are imported at the root.
        let roots: Vec<String> = if workspaces.is_empty() {
            let ids: Vec<String> = resources.iter().map(|r| r.string("_id")).collect();
            let mut parents: Vec<String> = resources
                .iter()
                .map(|r| r.string("parentId"))
                .filter(|p| !ids.contains(p))
                .collect();
            parents.dedup();
            parents
        } else {
            workspaces.iter().map(|w| w.string("_id")).collect()
        };
        for (n, ws_id) in roots.iter().enumerate() {
            let ws = workspaces.get(n).copied();
            let ws_name = ws
                .map(|w| w.string("name"))
                .filter(|n| !n.trim().is_empty());
            let path: Vec<String> = if multi {
                ws_name.iter().cloned().collect()
            } else {
                Vec::new()
            };
            let here = if multi {
                location(&[], ws_name.as_deref().unwrap_or("workspace"))
            } else {
                "collection".to_owned()
            };
            // Environments: base (child of the workspace) and its sub-environments.
            let mut base_vars = Vec::new();
            for env in resources
                .iter()
                .filter(|r| r.string("_type") == "environment" && r.string("parentId") == *ws_id)
            {
                let base_here = format!("environment {}", env.string("name"));
                base_vars.extend(self.environment_vars(env, &base_here));
                let base_id = env.string("_id");
                for sub in sorted(
                    resources
                        .iter()
                        .filter(|r| {
                            r.string("_type") == "environment" && r.string("parentId") == base_id
                        })
                        .map(item)
                        .collect(),
                ) {
                    let name = unique_env(&collection.environments, &sub.v.string("name"));
                    let sub_here = format!("environment {name}");
                    let variables = self.environment_vars(sub.v, &sub_here);
                    collection.environments.push(Environment {
                        client_certificates: Vec::new(),
                        external_secrets: None,
                        name,
                        variables,
                    });
                }
            }
            let jar_cookies: usize = resources
                .iter()
                .filter(|r| r.string("_type") == "cookie_jar" && r.string("parentId") == *ws_id)
                .map(|j| j.get("cookies").map(|c| c.items().len()).unwrap_or(0))
                .sum();
            if jar_cookies > 0 {
                self.losses.dropped(
                    &here,
                    LossKind::Setting,
                    format!(
                        "{jar_cookies} {} of the cookie jar not imported",
                        plural(jar_cookies, "cookie", "cookies")
                    ),
                );
            }
            for spec in resources
                .iter()
                .filter(|r| r.string("_type") == "api_spec" && r.string("parentId") == *ws_id)
            {
                if !spec.string("contents").trim().is_empty() {
                    self.losses.dropped(
                        &here,
                        LossKind::Documentation,
                        "API design document not imported; import it as an OpenAPI file",
                    );
                }
            }
            let items = self.v4_items(resources, &children(ws_id), &path, &children);
            match (multi, ws) {
                (true, _) => {
                    collection.items.push(ItemOut::Folder(FolderOut {
                        name: ws_name.unwrap_or_else(|| "Workspace".into()),
                        docs: ws.and_then(|w| item(w).description()),
                        defaults: DefaultsOut {
                            vars: base_vars,
                            ..DefaultsOut::default()
                        },
                        items,
                    }));
                }
                (false, ws) => {
                    collection.name = ws_name.unwrap_or_else(|| fallback.to_owned());
                    collection.docs = ws.and_then(|w| item(w).description());
                    collection.defaults.vars = base_vars;
                    collection.items = items;
                }
            }
        }
        if multi {
            collection.name = fallback.to_owned();
            self.losses.approximated(
                "collection",
                LossKind::Variable,
                "several workspaces imported as one folder each; their base environments are folder variables",
            );
        }
        if collection.name.trim().is_empty() {
            collection.name = fallback.to_owned();
        }
        Ok(collection)
    }

    fn v4_items<'a>(
        &mut self,
        all: &'a [Value],
        items: &[Item<'a>],
        path: &[String],
        children: &dyn Fn(&str) -> Vec<Item<'a>>,
    ) -> Vec<ItemOut> {
        let _ = all;
        let mut out = Vec::new();
        for it in items {
            let name = display_name(it.v);
            let here = location(path, &name);
            match it.v.string("_type").as_str() {
                "request_group" => {
                    let mut sub = path.to_vec();
                    sub.push(name.clone());
                    let kids = children(&it.id());
                    let folder_items = self.v4_items(all, &kids, &sub, children);
                    out.push(ItemOut::Folder(self.folder(
                        *it,
                        &name,
                        &here,
                        folder_items,
                    )));
                }
                "request" => out.push(ItemOut::Request(self.request(*it, &here))),
                "grpc_request" => {
                    self.losses
                        .dropped(&here, LossKind::Protocol, "gRPC request not imported")
                }
                "websocket_request" => {
                    self.losses
                        .dropped(&here, LossKind::Protocol, "WebSocket request not imported")
                }
                "unit_test_suite" => {
                    let tests = all
                        .iter()
                        .filter(|t| {
                            t.string("_type") == "unit_test" && t.string("parentId") == it.id()
                        })
                        .count();
                    self.losses.dropped(
                        &here,
                        LossKind::Test,
                        format!(
                            "unit test suite ({tests} {}) not imported",
                            plural(tests, "test", "tests")
                        ),
                    );
                }
                "mock_server" | "mock_route" => {
                    self.losses
                        .dropped(&here, LossKind::Other, "mock server not imported")
                }
                // Environments, cookie jars, specs, payloads and proto files are handled
                // elsewhere or belong to dropped items.
                _ => {}
            }
        }
        out
    }

    fn v5(&mut self, root: &Value, fallback: &str) -> CollectionOut {
        let mut name = root.string("name");
        if name.trim().is_empty() {
            name = fallback.to_owned();
        }
        let mut collection = CollectionOut {
            name,
            docs: Item { v: root, v5: true }.description(),
            ..CollectionOut::default()
        };
        if let Some(envs) = root.get("environments").filter(|e| e.is_obj()) {
            let base_here = format!("environment {}", envs.string("name"));
            collection.defaults.vars = self.environment_vars(envs, &base_here);
            let subs: Vec<Item<'_>> = envs
                .get("subEnvironments")
                .map(Value::items)
                .unwrap_or_default()
                .iter()
                .map(|v| Item { v, v5: true })
                .collect();
            for sub in sorted(subs) {
                let name = unique_env(&collection.environments, &sub.v.string("name"));
                let here = format!("environment {name}");
                let variables = self.environment_vars(sub.v, &here);
                collection.environments.push(Environment {
                    client_certificates: Vec::new(),
                    external_secrets: None,
                    name,
                    variables,
                });
            }
        }
        let cookies = root
            .get("cookieJar")
            .and_then(|j| j.get("cookies"))
            .map(|c| c.items().len())
            .unwrap_or(0);
        if cookies > 0 {
            self.losses.dropped(
                "collection",
                LossKind::Setting,
                format!(
                    "{cookies} {} of the cookie jar not imported",
                    plural(cookies, "cookie", "cookies")
                ),
            );
        }
        if root.get("spec").is_some_and(|s| !s.entries().is_empty()) {
            self.losses.dropped(
                "collection",
                LossKind::Documentation,
                "API design document not imported; import it as an OpenAPI file",
            );
        }
        if let Some(tests) = root.get("testSuites").filter(|t| !t.items().is_empty()) {
            let n = tests.items().len();
            self.losses.dropped(
                "collection",
                LossKind::Test,
                format!(
                    "{n} unit test {} not imported",
                    plural(n, "suite", "suites")
                ),
            );
        }
        let items: Vec<Item<'_>> = root
            .get("collection")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .map(|v| Item { v, v5: true })
            .collect();
        collection.items = self.v5_items(&items, &[]);
        collection
    }

    fn v5_items(&mut self, items: &[Item<'_>], path: &[String]) -> Vec<ItemOut> {
        let mut out = Vec::new();
        for it in sorted(items.to_vec()) {
            if !it.v.is_obj() {
                continue;
            }
            let name = display_name(it.v);
            let here = location(path, &name);
            let id = it.id();
            if let Some(children) = it.v.get("children") {
                let mut sub = path.to_vec();
                sub.push(name.clone());
                let kids: Vec<Item<'_>> = children
                    .items()
                    .iter()
                    .map(|v| Item { v, v5: true })
                    .collect();
                let folder_items = self.v5_items(&kids, &sub);
                out.push(ItemOut::Folder(self.folder(it, &name, &here, folder_items)));
            } else if id.starts_with("ws-req") {
                self.losses
                    .dropped(&here, LossKind::Protocol, "WebSocket request not imported");
            } else if id.starts_with("greq") || it.v.get("protoMethodName").is_some() {
                self.losses
                    .dropped(&here, LossKind::Protocol, "gRPC request not imported");
            } else if it.v.get("url").is_some() || it.v.get("method").is_some() {
                out.push(ItemOut::Request(self.request(it, &here)));
            } else {
                self.losses
                    .dropped(&here, LossKind::Other, "item of unknown kind not imported");
            }
        }
        out
    }

    fn folder(&mut self, it: Item<'_>, name: &str, here: &str, items: Vec<ItemOut>) -> FolderOut {
        let (pre, post) = it.scripts();
        self.losses.script(here, "pre-request script", &pre);
        self.losses.script(here, "after-response script", &post);
        let (headers, header_docs) = self.headers(it.v.get("headers"), here);
        let auth = match it.v.get("authentication") {
            Some(a) if !a.entries().is_empty() => self.auth(a, here),
            _ => AuthOut::Model(Auth::Inherit),
        };
        if let AuthOut::Model(a) = &auth {
            self.losses.literal_credentials(here, a, &headers);
        }
        let vars = self.variables(it.v.get("environment"), here, false);
        FolderOut {
            name: name.to_owned(),
            docs: it.description(),
            defaults: DefaultsOut {
                headers,
                header_docs,
                auth,
                vars,
                ..DefaultsOut::default()
            },
            items,
        }
    }

    fn headers(&mut self, v: Option<&Value>, here: &str) -> (Vec<KeyValue>, Vec<Option<String>>) {
        let mut list = Vec::new();
        let mut docs = Vec::new();
        for h in v.map(Value::items).unwrap_or_default() {
            let name = h.string("name");
            if name.is_empty() && h.string("value").is_empty() {
                continue;
            }
            list.push(KeyValue {
                name: self.template(&name, here),
                value: self.template(&h.string("value"), here),
                enabled: !h.flag("disabled"),
            });
            docs.push(h.str_of("description").filter(|d| !d.trim().is_empty()));
        }
        (list, docs)
    }

    fn request(&mut self, it: Item<'_>, here: &str) -> RequestOut {
        let v = it.v;
        let (pre, post) = it.scripts();
        self.losses.script(here, "pre-request script", &pre);
        self.losses.script(here, "after-response script", &post);
        let mut url = self.template(&v.string("url"), here);
        // Insomnia appends `parameters` when sending; here they become part of the URL.
        let mut query = Vec::new();
        let mut params: Vec<Param> = Vec::new();
        let mut param_docs = Vec::new();
        for p in v.get("parameters").map(Value::items).unwrap_or_default() {
            let name = self.template(&p.string("name"), here);
            if name.is_empty() {
                continue;
            }
            let value = self.template(&p.string("value"), here);
            let enabled = !p.flag("disabled");
            if enabled {
                query.push(if value.is_empty() {
                    name.clone()
                } else {
                    format!("{name}={value}")
                });
            }
            params.push(Param {
                name,
                value,
                kind: ParamKind::Query,
                enabled,
            });
            param_docs.push(p.str_of("description").filter(|d| !d.trim().is_empty()));
        }
        // Query parameters already written into the URL come first.
        let in_url: Vec<Param> = url_params(&url, &[])
            .into_iter()
            .filter(|p| p.kind == ParamKind::Query)
            .collect();
        if !query.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&query.join("&"));
        }
        let mut all_params: Vec<Param> = in_url;
        let mut all_docs: Vec<Option<String>> = vec![None; all_params.len()];
        all_params.extend(params);
        all_docs.extend(param_docs);
        let path_values: Vec<(String, String)> = v
            .get("pathParameters")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .map(|p| (p.string("name"), p.string("value")))
            .collect();
        for p in url_params(&url, &path_values)
            .into_iter()
            .filter(|p| p.kind == ParamKind::Path)
        {
            all_params.push(p);
            all_docs.push(None);
        }
        let (headers, header_docs) = self.headers(v.get("headers"), here);
        let body = self.body(v.get("body"), here);
        let auth = match v.get("authentication") {
            Some(a) if !a.entries().is_empty() => self.auth(a, here),
            _ => AuthOut::Model(Auth::Inherit),
        };
        let settings = self.settings(it, here);
        let mut method = v.string("method").trim().to_ascii_uppercase();
        if method.is_empty() {
            method = "GET".into();
        }
        let name = display_name(v);
        let spec = RequestSpec {
            name,
            seq: None,
            method,
            url,
            headers,
            params: all_params,
            body,
            auth: Auth::Inherit,
            vars: Vec::new(),
            settings,
        };
        let mut texts: Vec<&str> = vec![&spec.url];
        texts.extend(spec.headers.iter().map(|h| h.value.as_str()));
        if let Body::Raw { data, .. } = &spec.body {
            texts.push(data);
        }
        self.losses.dynamic_vars(here, texts);
        let model_auth = match &auth {
            AuthOut::Model(a) => a.clone(),
            AuthOut::Other { .. } => Auth::Inherit,
        };
        self.losses
            .literal_credentials(here, &model_auth, &spec.headers);
        let mut r = RequestOut::new(spec);
        r.auth = auth;
        r.docs = it.description();
        r.header_docs = header_docs;
        r.param_docs = all_docs;
        r
    }

    fn settings(&mut self, it: Item<'_>, here: &str) -> Settings {
        let v = it.v;
        let (follow, encode, send, store, render, rebuild) = if it.v5 {
            let s = v.get("settings");
            let get = |k: &str| s.and_then(|s| s.get(k));
            let cookies = s.and_then(|s| s.get("cookies"));
            (
                get("followRedirects").and_then(Value::text),
                get("encodeUrl").and_then(Value::as_bool),
                cookies.and_then(|c| c.get("send")).and_then(Value::as_bool),
                cookies
                    .and_then(|c| c.get("store"))
                    .and_then(Value::as_bool),
                get("renderRequestBody").and_then(Value::as_bool),
                get("rebuildPath").and_then(Value::as_bool),
            )
        } else {
            (
                v.str_of("settingFollowRedirects"),
                v.get("settingEncodeUrl").and_then(Value::as_bool),
                v.get("settingSendCookies").and_then(Value::as_bool),
                v.get("settingStoreCookies").and_then(Value::as_bool),
                v.get("settingDisableRenderRequestBody")
                    .and_then(Value::as_bool)
                    .map(|d| !d),
                v.get("settingRebuildPath").and_then(Value::as_bool),
            )
        };
        let mut dropped = Vec::new();
        if send == Some(false) {
            dropped.push("do not send cookies");
        }
        if store == Some(false) {
            dropped.push("do not store cookies");
        }
        if render == Some(false) {
            dropped.push("do not render the body");
        }
        if rebuild == Some(false) {
            dropped.push("do not rebuild the path");
        }
        if !dropped.is_empty() {
            self.losses.dropped(
                here,
                LossKind::Setting,
                format!("request settings not imported: {}", dropped.join(", ")),
            );
        }
        Settings {
            follow_redirects: match follow.as_deref() {
                Some("on") => Some(true),
                Some("off") => Some(false),
                _ => None,
            },
            encode_url: encode.filter(|e| !e),
            ..Settings::default()
        }
    }

    fn body(&mut self, v: Option<&Value>, here: &str) -> Body {
        let Some(b) = v.filter(|b| b.is_obj() && !b.entries().is_empty()) else {
            return Body::None;
        };
        let mime = b.string("mimeType");
        let lower = mime.to_ascii_lowercase();
        let text = b.string("text");
        let params = b.get("params").map(Value::items).unwrap_or_default();
        if lower.contains("x-www-form-urlencoded") {
            return Body::FormUrlEncoded {
                fields: params
                    .iter()
                    .map(|p| KeyValue {
                        name: self.template(&p.string("name"), here),
                        value: self.template(&p.string("value"), here),
                        enabled: !p.flag("disabled"),
                    })
                    .collect(),
            };
        }
        if lower.contains("multipart/form-data") {
            let mut parts = Vec::new();
            for p in params {
                let name = self.template(&p.string("name"), here);
                if p.string("type") == "file" {
                    let file = p.string("fileName");
                    if file.is_empty() {
                        self.losses.approximated(
                            here,
                            LossKind::Body,
                            format!(
                                "file part `{name}` has no file selected; choose one in the request"
                            ),
                        );
                    } else {
                        self.losses.approximated(here, LossKind::Body, format!("file `{file}` for `{name}` refers to the original computer; copy it into the collection folder and update the path"));
                    }
                    parts.push(MultipartPart {
                        name,
                        kind: PartKind::File,
                        values: if file.is_empty() {
                            Vec::new()
                        } else {
                            vec![file]
                        },
                        content_type: None,
                        enabled: !p.flag("disabled"),
                    });
                } else {
                    parts.push(MultipartPart {
                        name,
                        kind: PartKind::Text,
                        values: vec![self.template(&p.string("value"), here)],
                        content_type: None,
                        enabled: !p.flag("disabled"),
                    });
                }
            }
            return Body::Multipart { parts };
        }
        if let Some(file) = b.str_of("fileName").filter(|f| !f.is_empty()) {
            self.losses.approximated(here, LossKind::Body, format!("body file `{file}` refers to the original computer; copy it into the collection folder and update the path"));
            return Body::File {
                path: file,
                content_type: (!mime.is_empty()).then_some(mime),
            };
        }
        if lower.contains("graphql") {
            self.losses.approximated(
                here,
                LossKind::Body,
                "GraphQL body imported as a JSON body ({\"query\", \"variables\"})",
            );
            return Body::Raw {
                kind: RawKind::Json,
                data: self.template(&text, here),
            };
        }
        if text.is_empty() {
            return Body::None;
        }
        Body::Raw {
            kind: if mime.is_empty() {
                RawKind::Text
            } else {
                raw_kind(&mime)
            },
            data: self.template(&text, here),
        }
    }

    fn auth(&mut self, a: &Value, here: &str) -> AuthOut {
        let kind = a.string("type");
        let t = |cx: &mut Self, k: &str| cx.template(&a.string(k), here);
        if a.flag("disabled") && kind != "none" {
            self.losses.dropped(
                here,
                LossKind::Auth,
                format!("disabled {kind} auth not imported"),
            );
            return AuthOut::Model(Auth::None);
        }
        match kind.as_str() {
            "none" => AuthOut::Model(Auth::None),
            "basic" => AuthOut::Model(Auth::Basic {
                username: t(self, "username"),
                password: t(self, "password"),
            }),
            "bearer" => {
                let prefix = a.string("prefix");
                if !prefix.is_empty() && prefix != "Bearer" {
                    self.losses.approximated(
                        here,
                        LossKind::Auth,
                        format!("bearer prefix `{prefix}` not imported; `Bearer` is sent"),
                    );
                }
                AuthOut::Model(Auth::Bearer {
                    token: t(self, "token"),
                })
            }
            "apikey" => {
                let key = t(self, "key");
                let value = t(self, "value");
                match a.string("addTo").as_str() {
                    "queryParams" => AuthOut::Model(Auth::ApiKey {
                        key,
                        value,
                        placement: ApiKeyPlacement::Query,
                    }),
                    "cookie" => {
                        self.losses.dropped(
                            here,
                            LossKind::Auth,
                            "API key in a cookie not imported; send it as a header instead",
                        );
                        AuthOut::Model(Auth::None)
                    }
                    _ => AuthOut::Model(Auth::ApiKey {
                        key,
                        value,
                        placement: ApiKeyPlacement::Header,
                    }),
                }
            }
            "digest" => {
                let out = auth::user_password("digest", &t(self, "username"), &t(self, "password"));
                auth::report(&mut self.losses, here, &out, "Digest auth");
                out
            }
            "ntlm" => {
                let out = auth::ntlm(
                    &t(self, "username"),
                    &t(self, "password"),
                    &t(self, "domain"),
                );
                auth::report(&mut self.losses, here, &out, "NTLM auth");
                out
            }
            "iam" => {
                let out = auth::awsv4(&AwsV4 {
                    access_key_id: t(self, "accessKeyId"),
                    secret_access_key: t(self, "secretAccessKey"),
                    session_token: t(self, "sessionToken"),
                    service: t(self, "service"),
                    region: t(self, "region"),
                });
                auth::report(&mut self.losses, here, &out, "AWS Signature v4");
                out
            }
            "oauth1" => {
                let o = OAuth1 {
                    consumer_key: t(self, "consumerKey"),
                    consumer_secret: t(self, "consumerSecret"),
                    access_token: t(self, "tokenKey"),
                    access_token_secret: t(self, "tokenSecret"),
                    callback_url: t(self, "callback"),
                    verifier: t(self, "verifier"),
                    signature_method: a.string("signatureMethod"),
                    timestamp: t(self, "timestamp"),
                    nonce: t(self, "nonce"),
                    version: t(self, "version"),
                    realm: t(self, "realm"),
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
                if !a.string("privateKey").is_empty() {
                    self.losses
                        .dropped(here, LossKind::Auth, "OAuth 1.0 private key not imported");
                }
                let out = auth::oauth1(&o);
                auth::report(&mut self.losses, here, &out, "OAuth 1.0");
                out
            }
            "oauth2" => {
                let grant = a.string("grantType");
                let flow = auth::oauth2_flow(&grant).unwrap_or("authorization_code");
                if !grant.is_empty() && auth::oauth2_flow(&grant).is_none() {
                    self.losses.approximated(
                        here,
                        LossKind::Auth,
                        format!("OAuth 2.0 grant type `{grant}` imported as authorization code"),
                    );
                }
                let pkce = (flow == "authorization_code" && a.flag("usePkce")).then(|| {
                    if a.string("pkceMethod") == "plain" {
                        "plain"
                    } else {
                        "S256"
                    }
                });
                let o = OAuth2 {
                    flow,
                    authorization_url: t(self, "authorizationUrl"),
                    access_token_url: t(self, "accessTokenUrl"),
                    refresh_token_url: String::new(),
                    callback_url: t(self, "redirectUrl"),
                    client_id: t(self, "clientId"),
                    client_secret: t(self, "clientSecret"),
                    credentials_in_body: a.flag("credentialsInBody"),
                    scope: t(self, "scope"),
                    state: t(self, "state"),
                    username: t(self, "username"),
                    password: t(self, "password"),
                    pkce,
                };
                let extra: Vec<String> = [
                    "audience",
                    "resource",
                    "origin",
                    "tokenPrefix",
                    "responseType",
                ]
                .iter()
                .filter(|k| {
                    let v = a.string(k);
                    // Defaults Insomnia writes on its own carry no information.
                    let default = match **k {
                        "tokenPrefix" => v == "Bearer",
                        "responseType" => matches!(v.as_str(), "code" | "token"),
                        _ => false,
                    };
                    !v.is_empty() && !default
                })
                .map(|k| format!("`{k}`"))
                .collect();
                if !extra.is_empty() {
                    self.losses.dropped(
                        here,
                        LossKind::Auth,
                        format!("OAuth 2.0 settings not imported: {}", extra.join(", ")),
                    );
                }
                let out = auth::oauth2(&o);
                auth::report(&mut self.losses, here, &out, "OAuth 2.0");
                out
            }
            other => {
                let label = match other {
                    "hawk" => "Hawk auth",
                    "asap" => "ASAP auth",
                    "netrc" => "netrc auth",
                    _ => "auth",
                };
                self.losses.dropped(
                    here,
                    LossKind::Auth,
                    format!("{label} (`{other}`) not imported: OpenCollection cannot hold it; the request is sent without auth"),
                );
                AuthOut::Model(Auth::None)
            }
        }
    }
}

/// The name of an item; requests without one are named after their URL.
fn display_name(v: &Value) -> String {
    let name = v.string("name");
    if !name.trim().is_empty() {
        return name;
    }
    let url = v.string("url");
    if url.trim().is_empty() {
        String::new()
    } else {
        convert_template(url.trim())
    }
}

fn unique_env(existing: &[Environment], name: &str) -> String {
    let base = if name.trim().is_empty() || name.chars().any(char::is_control) {
        "Environment".to_owned()
    } else {
        name.trim().to_owned()
    };
    let mut unique = base.clone();
    let mut n = 2;
    while existing.iter().any(|e| e.name == unique) {
        unique = format!("{base} ({n})");
        n += 1;
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates() {
        assert_eq!(
            convert_template("{{ _.base_url }}/a/{{token}}"),
            "{{base_url}}/a/{{token}}"
        );
        assert_eq!(convert_template("{{ _.a.b }}"), "{{a.b}}");
        assert_eq!(
            convert_template("{{ _.a | upper }} {% uuid %}"),
            "{{ _.a | upper }} {% uuid %}"
        );
        assert_eq!(convert_template("{{ open"), "{{ open");
    }
}
