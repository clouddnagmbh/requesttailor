// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer for OpenAPI 3.0, 3.1 and 3.2 descriptions (JSON or YAML), and Swagger 2.0 after
//! conversion. Owner: wave 4, strand B.
//!
//! An API description is not a collection, so the import builds one request per operation:
//! - Folders: one per operation's first tag (in the order of the top-level `tags`, with their
//!   descriptions as folder docs); operations without tags at the root.
//! - URL: `{{baseUrl}}` + the path, `{id}` → `:id` (path parameters with their example or
//!   default). Required query parameters are in the URL, optional ones are disabled
//!   parameters; header parameters become headers (disabled when optional).
//! - `baseUrl`: a collection variable from the first server; every server becomes an
//!   environment with its `baseUrl` and server variables.
//! - Request bodies: the media type's example, or one generated from the schema (reported).
//! - Security: the first scheme of the requirement maps to auth with `{{variables}}`
//!   (`{{username}}`, `{{password}}`, `{{token}}`, `{{apiKey}}`, ...), which are added to
//!   every environment, secrets as `secret: true` without value.
//! - Explicit response examples become request examples; schemas, response definitions,
//!   callbacks and webhooks are documentation only and reported once.
//!
//! References: local `$ref`s (`#/components/...`) are resolved, with cycles and depth bounded;
//! references to other files are reported. A multi-file spec arrives bundled into one document
//! (`spec_files`), Swagger 2.0 converted to 3.0 (`swagger2`).
//!
//! Wizard choices ([`OpenApiOptions`], since the OpenAPI import wizard): which operations
//! (`METHOD /path` keys), folders by tag, first path segment or none, request names from the
//! summary or the `operationId`, examples from the spec or generated from the schema, which
//! servers become environments under which names, the server of `baseUrl`, the collection
//! name, and the kept spec (`extensions.requesttailor.contract.spec`). The defaults give the
//! plain import byte for byte. [`overview`] describes a spec with the same names and order.

use std::collections::{BTreeSet, HashSet};

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, KeyValue, MultipartPart, Param, ParamKind, PartKind,
    RawKind, RequestSpec, Variable,
};

use super::auth::{self, OAuth2};
use super::common::{Losses, finish, invalid, location, plural, raw_kind};
use super::out::{AuthOut, CollectionOut, DefaultsOut, ExampleOut, FolderOut, ItemOut, RequestOut};
use super::value::Value;
use super::{
    Import, ImportError, ImportFormat, Loss, LossKind, OpenApiExamples, OpenApiGrouping,
    OpenApiNaming, OpenApiOptions, SpecOperation, SpecSecurityScheme, SpecServer,
    SpecServerVariable, SpecTag, stem,
};

const F: ImportFormat = ImportFormat::Openapi;

/// Operation keys of a path item (`query` is new in 3.2).
pub(super) const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace", "query",
];

/// Deepest `$ref` chain and schema nesting followed.
const MAX_REF_DEPTH: usize = 32;
const MAX_SAMPLE_DEPTH: usize = 8;

/// An OpenAPI 3.x document to convert (Swagger 2.0 is converted to it first, external files
/// are bundled into it; see `spec_files` and `swagger2`).
pub(super) struct Source<'v> {
    pub root: &'v Value,
    /// Name of the main file, for the collection name when the spec has no title.
    pub file_name: &'v str,
    /// Report references to other files at the collection (a single file). `false` when
    /// the files were bundled: the bundler reported what it could not resolve.
    pub report_external: bool,
    /// Losses of reading, bundling and converting, reported with the import's.
    pub losses: Vec<Loss>,
}

/// Refuses what is not OpenAPI 3.0, 3.1 or 3.2.
pub(super) fn check_version(root: &Value) -> Result<(), ImportError> {
    if !root.is_obj() {
        return Err(invalid(F, "expected a JSON or YAML object"));
    }
    let version = root.string("openapi");
    if !(version.starts_with("3.0") || version.starts_with("3.1") || version.starts_with("3.2")) {
        return Err(ImportError::UnsupportedVersion {
            format: F,
            version: if version.is_empty() {
                "unknown".into()
            } else {
                version
            },
        });
    }
    Ok(())
}

/// Converts `src` with the wizard's choices; `contract_spec` names the spec kept in the
/// collection (`extensions.requesttailor.contract.spec`).
pub(super) fn build(
    src: Source<'_>,
    options: &OpenApiOptions,
    contract_spec: Option<String>,
) -> Result<Import, ImportError> {
    check_version(src.root)?;
    let mut cx = Cx {
        root: src.root,
        opts: options,
        losses: Losses(src.losses),
        secrets: Vec::new(),
        plain_vars: Vec::new(),
        external_refs: BTreeSet::new(),
    };
    cx.build(src.file_name, src.report_external, contract_spec)
}

struct Cx<'a> {
    root: &'a Value,
    opts: &'a OpenApiOptions,
    losses: Losses,
    /// Variables the auth uses, to add to every environment: `(name, secret)`.
    secrets: Vec<(String, bool)>,
    plain_vars: Vec<String>,
    external_refs: BTreeSet<String>,
}

/// JSON pointer `#/a/b~1c` → the value.
pub(super) fn pointer<'v>(root: &'v Value, reference: &str) -> Option<&'v Value> {
    let path = reference.strip_prefix('#')?;
    let mut cur = root;
    for part in path.split('/').skip(1) {
        let key = part.replace("~1", "/").replace("~0", "~");
        cur = match cur {
            Value::Arr(items) => items.get(key.parse::<usize>().ok()?)?,
            _ => cur.get(&key)?,
        };
    }
    Some(cur)
}

/// The collection name the import gives: `info.title`, else the file name.
pub(super) fn spec_title(root: &Value, file_name: &str) -> String {
    let title = root
        .get("info")
        .map(|i| i.string("title"))
        .unwrap_or_default();
    if title.trim().is_empty() {
        stem(
            file_name,
            &[".openapi.json", ".openapi.yaml", ".json", ".yaml", ".yml"],
        )
        .to_owned()
    } else {
        title
    }
}

/// The servers of the document.
pub(super) fn servers(root: &Value) -> &[Value] {
    root.get("servers").map(Value::items).unwrap_or_default()
}

/// The environment name the import suggests for server `i` (not yet unique).
fn suggested_name(s: &Value, i: usize) -> String {
    let url = braces_to_vars(&s.string("url"));
    let mut env_name = s
        .string("description")
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    if env_name.is_empty() {
        env_name = url
            .split_once("://")
            .map_or(url.as_str(), |(_, r)| r)
            .trim_end_matches('/')
            .to_owned();
    }
    if env_name.is_empty() || env_name.chars().any(char::is_control) {
        env_name = format!("Server {}", i + 1);
    }
    env_name.chars().take(60).collect()
}

/// `name`, or `name (2)`, `name (3)`, ... if `taken` has it.
fn unique(name: String, taken: &[String]) -> String {
    let mut out = name.clone();
    let mut n = 2;
    while taken.contains(&out) {
        out = format!("{name} ({n})");
        n += 1;
    }
    out
}

/// The environment of each server (`None`: no environment), as the import names them.
fn environment_names(root: &Value, opts: &OpenApiOptions) -> Vec<Option<String>> {
    let mut taken: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (i, s) in servers(root).iter().enumerate() {
        let wanted = match &opts.servers {
            None => Some(suggested_name(s, i)),
            Some(choices) => choices
                .iter()
                .find(|c| c.index as usize == i)
                .and_then(|c| c.environment.as_deref())
                .map(|n| {
                    let n: String = n.trim().chars().take(60).collect();
                    if n.is_empty() || n.chars().any(char::is_control) {
                        suggested_name(s, i)
                    } else {
                        n
                    }
                }),
        };
        let name = wanted.map(|w| unique(w, &taken));
        if let Some(n) = &name {
            taken.push(n.clone());
        }
        out.push(name);
    }
    out
}

/// The operations of a path item in source order: `(method as written, operation)`.
pub(super) fn item_operations(item: &Value) -> Vec<(&String, &Value)> {
    // 3.2 lists operations with other methods under `additionalOperations`.
    let extra = item
        .get("additionalOperations")
        .map(Value::entries)
        .unwrap_or_default();
    item.entries()
        .iter()
        .filter(|(m, _)| METHODS.contains(&m.as_str()))
        .chain(extra.iter())
        .filter(|(_, op)| op.is_obj())
        .map(|(m, op)| (m, op))
        .collect()
}

/// The collection-level loss for references that were not resolved.
pub(super) fn external_refs_detail(refs: &[String]) -> String {
    let shown: Vec<String> = refs.iter().take(5).map(|r| format!("`{r}`")).collect();
    format!(
        "references to other files not resolved ({}{})",
        shown.join(", "),
        if refs.len() > 5 { ", ..." } else { "" }
    )
}

/// `GET /pets/{petId}`.
pub(super) fn operation_key(method: &str, path: &str) -> String {
    format!("{} {path}", method.to_ascii_uppercase())
}

/// The first tag of an operation.
fn first_tag(op: &Value) -> Option<String> {
    op.get("tags")
        .and_then(|t| t.items().first())
        .and_then(Value::text)
}

/// The folder of an operation for `grouping`; `None`: the collection root.
fn folder_of(op: &Value, path: &str, grouping: OpenApiGrouping) -> Option<String> {
    match grouping {
        OpenApiGrouping::Tag => first_tag(op),
        OpenApiGrouping::Path => path
            .split('/')
            .find(|s| !s.is_empty())
            .map(|s| s.chars().take(80).collect()),
        OpenApiGrouping::Flat => None,
    }
}

impl<'a> Cx<'a> {
    /// Follows `$ref`s; an unresolvable reference gives `None`.
    fn resolve(&mut self, v: &'a Value) -> Option<&'a Value> {
        let mut cur = v;
        for _ in 0..MAX_REF_DEPTH {
            let Some(r) = cur.get("$ref").and_then(Value::as_str) else {
                return Some(cur);
            };
            if !r.starts_with('#') {
                self.external_refs.insert(r.to_owned());
                return None;
            }
            cur = pointer(self.root, r)?;
        }
        None
    }

    fn schema_examples(&self) -> bool {
        self.opts.examples == OpenApiExamples::Schema
    }

    fn build(
        &mut self,
        file_name: &str,
        report_external: bool,
        contract_spec: Option<String>,
    ) -> Result<Import, ImportError> {
        let root = self.root;
        let opts = self.opts;
        let info = root.get("info").cloned().unwrap_or(Value::Null);
        let mut name = spec_title(root, file_name);
        if let Some(n) = opts
            .collection_name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            name = n.to_owned();
        }
        let mut docs = Vec::new();
        if let Some(v) = info.str_of("version") {
            docs.push(format!("Version {v}"));
        }
        if let Some(d) = info.str_of("description").filter(|d| !d.trim().is_empty()) {
            docs.push(d);
        }

        // Servers → baseUrl and environments.
        let mut environments = Vec::new();
        let servers = servers(root);
        let default_server = opts
            .default_server
            .map(|i| i as usize)
            .filter(|i| *i < servers.len())
            .unwrap_or(0);
        let env_names = environment_names(root, opts);
        let mut base = String::new();
        let mut server_vars: Vec<Variable> = Vec::new();
        for (i, s) in servers.iter().enumerate() {
            let (url, vars) = self.server(s);
            if i == default_server {
                base = url.clone();
                server_vars = vars.clone();
                if !url.contains("://") && !url.starts_with("{{") {
                    self.losses.approximated(
                        "collection",
                        LossKind::Variable,
                        format!(
                            "server URL `{url}` is relative; set `baseUrl` to the full address"
                        ),
                    );
                }
            }
            let Some(env_name) = env_names.get(i).cloned().flatten() else {
                continue;
            };
            let mut variables = vec![Variable::new("baseUrl", url)];
            variables.extend(vars);
            environments.push(Environment {
                client_certificates: Vec::new(),
                external_secrets: None,
                name: env_name,
                variables,
            });
        }
        if servers.is_empty() {
            self.losses.approximated(
                "collection",
                LossKind::Variable,
                "the description names no server; set `baseUrl` in the collection variables",
            );
        }

        // Folders from tags (or path segments).
        let mut folders: Vec<FolderOut> = Vec::new();
        if opts.grouping == OpenApiGrouping::Tag {
            for t in root.get("tags").map(Value::items).unwrap_or_default() {
                if let Some(n) = t.str_of("name")
                    && !folders.iter().any(|f| f.name == n)
                {
                    folders.push(FolderOut {
                        name: n,
                        docs: t.str_of("description"),
                        ..FolderOut::default()
                    });
                }
            }
        }
        let wanted: Option<HashSet<&str>> = opts
            .operations
            .as_ref()
            .map(|ops| ops.iter().map(String::as_str).collect());
        let global_security = root.get("security");
        let collection_auth = match global_security {
            Some(sec) => self.security(sec, "collection"),
            None => AuthOut::Model(Auth::None),
        };
        let mut loose = Vec::new();
        let mut callbacks = 0;
        let paths = root.get("paths").map(Value::entries).unwrap_or_default();
        for (path, item) in paths {
            let Some(item) = self.resolve(item) else {
                self.losses.dropped(
                    path,
                    LossKind::Other,
                    "path item with an external reference not imported",
                );
                continue;
            };
            let common: Vec<&Value> = item
                .get("parameters")
                .map(Value::items)
                .unwrap_or_default()
                .iter()
                .collect();
            let path_servers = item.get("servers");
            for (method, op) in item_operations(item) {
                if let Some(wanted) = &wanted
                    && !wanted.contains(operation_key(method, path).as_str())
                {
                    continue;
                }
                if op.get("callbacks").is_some_and(|c| !c.entries().is_empty()) {
                    callbacks += 1;
                }
                let folder = folder_of(op, path, opts.grouping);
                let folder_path: Vec<String> = folder.iter().cloned().collect();
                let r = self.operation(
                    path,
                    method,
                    op,
                    &common,
                    path_servers,
                    &folder_path,
                    global_security,
                );
                match folder {
                    Some(folder) => {
                        let i = match folders.iter().position(|f| f.name == folder) {
                            Some(i) => i,
                            None => {
                                folders.push(FolderOut {
                                    name: folder.clone(),
                                    ..FolderOut::default()
                                });
                                folders.len() - 1
                            }
                        };
                        folders[i].items.push(ItemOut::Request(r));
                    }
                    None => loose.push(ItemOut::Request(r)),
                }
            }
        }
        let mut items: Vec<ItemOut> = folders
            .into_iter()
            .filter(|f| !f.items.is_empty())
            .map(ItemOut::Folder)
            .collect();
        items.extend(loose);

        if callbacks > 0 {
            self.losses.dropped(
                "collection",
                LossKind::Protocol,
                format!(
                    "callbacks of {callbacks} {} not imported",
                    plural(callbacks, "operation", "operations")
                ),
            );
        }
        let webhooks = root.get("webhooks").map(|w| w.entries().len()).unwrap_or(0);
        if webhooks > 0 {
            self.losses.dropped(
                "collection",
                LossKind::Protocol,
                format!(
                    "{webhooks} {} not imported",
                    plural(webhooks, "webhook", "webhooks")
                ),
            );
        }
        let has_responses = paths.iter().any(|(path, item)| {
            item.entries().iter().any(|(m, op)| {
                METHODS.contains(&m.as_str())
                    && op.get("responses").is_some()
                    && wanted
                        .as_ref()
                        .is_none_or(|w| w.contains(operation_key(m, path).as_str()))
            })
        });
        let has_models = root
            .get("components")
            .is_some_and(|c| c.get("schemas").is_some_and(|s| !s.entries().is_empty()));
        if has_models || has_responses {
            self.losses.dropped(
                "collection",
                LossKind::Documentation,
                "schemas and response definitions are API documentation and not imported; \
                 explicit response examples are kept as request examples",
            );
        }
        if report_external && !self.external_refs.is_empty() {
            let refs: Vec<String> = self.external_refs.iter().cloned().collect();
            self.losses
                .dropped("collection", LossKind::Other, external_refs_detail(&refs));
        }

        // Variables the auth needs.
        let mut collection_vars = vec![Variable::new("baseUrl", base)];
        collection_vars.extend(server_vars);
        for name in &self.plain_vars {
            if !collection_vars.iter().any(|v| v.name == *name) {
                collection_vars.push(Variable::new(name.clone(), ""));
            }
        }
        let secrets: Vec<(String, bool)> = self.secrets.clone();
        if !secrets.is_empty() && environments.is_empty() {
            environments.push(Environment {
                client_certificates: Vec::new(),
                external_secrets: None,
                name: "Default".into(),
                variables: Vec::new(),
            });
        }
        for env in &mut environments {
            for (name, secret) in &secrets {
                if !env.variables.iter().any(|v| v.name == *name) {
                    env.variables.push(Variable {
                        name: name.clone(),
                        value: if *secret { None } else { Some(String::new()) },
                        enabled: true,
                        secret: *secret,
                    });
                }
            }
        }
        let collection = CollectionOut {
            name,
            docs: (!docs.is_empty()).then(|| docs.join("\n\n")),
            defaults: DefaultsOut {
                auth: collection_auth,
                vars: collection_vars,
                ..DefaultsOut::default()
            },
            items,
            environments,
            contract_spec,
        };
        let losses = std::mem::take(&mut self.losses);
        finish(F, collection, losses)
    }

    /// Server URL with `{var}` → `{{var}}`, and its variables with their defaults.
    fn server(&mut self, s: &Value) -> (String, Vec<Variable>) {
        let url = s.string("url");
        let mut vars = Vec::new();
        for (name, v) in s.get("variables").map(Value::entries).unwrap_or_default() {
            vars.push(Variable::new(name.clone(), v.string("default")));
        }
        (braces_to_vars(&url), vars)
    }

    #[allow(clippy::too_many_arguments)]
    fn operation(
        &mut self,
        path: &str,
        method: &str,
        op: &'a Value,
        common: &[&'a Value],
        path_servers: Option<&'a Value>,
        folder: &[String],
        global_security: Option<&'a Value>,
    ) -> RequestOut {
        let method_upper = method.to_ascii_uppercase();
        let summary = op.str_of("summary").filter(|s| !s.trim().is_empty());
        let name = match self.opts.naming {
            OpenApiNaming::Summary => summary.or_else(|| op.str_of("operationId")),
            OpenApiNaming::OperationId => op
                .str_of("operationId")
                .filter(|s| !s.trim().is_empty())
                .or(summary),
        }
        .unwrap_or_else(|| format!("{method_upper} {path}"));
        let name: String = name
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(120)
            .collect();
        let here = location(folder, &name);

        // Operation parameters override path-level ones of the same name and location.
        let mut params: Vec<&Value> = Vec::new();
        let own: Vec<&Value> = op
            .get("parameters")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .collect();
        for p in common.iter().chain(own.iter()) {
            let Some(p) = self.resolve(p) else { continue };
            let key = (p.string("name"), p.string("in"));
            params.retain(|q| (q.string("name"), q.string("in")) != key);
            params.push(p);
        }
        let server = op
            .get("servers")
            .or(path_servers)
            .and_then(|s| s.items().first())
            .map(|s| self.server(s).0);
        let mut url = format!(
            "{}{}",
            server.unwrap_or_else(|| "{{baseUrl}}".into()),
            path_to_url(path)
        );
        let mut query = Vec::new();
        let mut out_params = Vec::new();
        let mut param_docs = Vec::new();
        let mut headers = Vec::new();
        let mut header_docs = Vec::new();
        let mut cookies = Vec::new();
        let mut path_params = Vec::new();
        for p in &params {
            let pname = p.string("name");
            let value = self.param_value(p);
            let required = p.flag("required");
            let doc = p.str_of("description").filter(|d| !d.trim().is_empty());
            match p.string("in").as_str() {
                "path" => path_params.push((pname, value, doc)),
                "query" => {
                    if required {
                        query.push(format!("{pname}={value}"));
                    }
                    out_params.push(Param {
                        name: pname,
                        value,
                        kind: ParamKind::Query,
                        enabled: required,
                    });
                    param_docs.push(doc);
                }
                "header" => {
                    if ["accept", "content-type", "authorization"]
                        .contains(&pname.to_ascii_lowercase().as_str())
                    {
                        // OpenAPI ignores these as parameters.
                        continue;
                    }
                    headers.push(KeyValue {
                        name: pname,
                        value,
                        enabled: required,
                    });
                    header_docs.push(doc);
                }
                "cookie" => cookies.push(format!("{pname}={value}")),
                _ => {}
            }
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        // Path parameters after query parameters, in URL order.
        for seg in url.split(['/', '?']).map(str::to_owned).collect::<Vec<_>>() {
            if let Some(n) = seg.strip_prefix(':')
                && !out_params
                    .iter()
                    .any(|p: &Param| p.kind == ParamKind::Path && p.name == n)
            {
                let (value, doc) = path_params
                    .iter()
                    .find(|(pn, _, _)| pn == n)
                    .map(|(_, v, d)| (v.clone(), d.clone()))
                    .unwrap_or_default();
                out_params.push(Param {
                    name: n.to_owned(),
                    value,
                    kind: ParamKind::Path,
                    enabled: true,
                });
                param_docs.push(doc);
            }
        }
        if !cookies.is_empty() {
            self.losses.approximated(
                &here,
                LossKind::Other,
                "cookie parameters imported as a disabled `Cookie` header",
            );
            headers.push(KeyValue {
                name: "Cookie".into(),
                value: cookies.join("; "),
                enabled: false,
            });
            header_docs.push(None);
        }
        let body = match op.get("requestBody") {
            Some(rb) => self.request_body(rb, &here, &mut headers, &mut header_docs),
            None => Body::None,
        };
        let auth = match op.get("security") {
            Some(sec) if Some(sec) != global_security => self.security(sec, &here),
            _ => AuthOut::Model(Auth::Inherit),
        };
        let mut docs = Vec::new();
        if op.flag("deprecated") {
            docs.push("**Deprecated.**".to_owned());
        }
        if let Some(d) = op.str_of("description").filter(|d| !d.trim().is_empty()) {
            docs.push(d);
        } else if let Some(s) = op.str_of("summary").filter(|s| s.lines().count() > 1) {
            docs.push(s);
        }
        let spec = RequestSpec {
            protocol: Default::default(),
            actions: Vec::new(),
            assertions: Vec::new(),
            scripts: Vec::new(),
            name,
            seq: None,
            method: method_upper,
            url,
            headers,
            params: out_params,
            body,
            auth: Auth::Inherit,
            vars: Vec::new(),
            settings: Default::default(),
        };
        let mut r = RequestOut::new(spec);
        r.auth = auth;
        r.docs = (!docs.is_empty()).then(|| docs.join("\n\n"));
        r.param_docs = param_docs;
        r.header_docs = header_docs;
        r.examples = self.examples(op);
        r
    }

    /// Example, then default, then first enum value of a parameter.
    /// With [`OpenApiExamples::Schema`] only the schema's default and first enum value.
    fn param_value(&mut self, p: &'a Value) -> String {
        let spec = !self.schema_examples();
        if spec && let Some(v) = p.field("example") {
            return scalar_text(v);
        }
        if spec
            && let Some(first) = p
                .get("examples")
                .map(Value::entries)
                .and_then(|e| e.first())
            && let Some(ex) = self.resolve(&first.1)
            && let Some(v) = ex.field("value")
        {
            return scalar_text(v);
        }
        let Some(schema) = p.get("schema").and_then(|s| self.resolve(s)) else {
            return String::new();
        };
        for key in ["example", "default"] {
            if (spec || key == "default")
                && let Some(v) = schema.field(key)
            {
                return scalar_text(v);
            }
        }
        if spec && let Some(v) = schema.get("examples").and_then(|e| e.items().first()) {
            return scalar_text(v);
        }
        if let Some(v) = schema.get("enum").and_then(|e| e.items().first()) {
            return scalar_text(v);
        }
        String::new()
    }

    fn request_body(
        &mut self,
        rb: &'a Value,
        here: &str,
        headers: &mut Vec<KeyValue>,
        header_docs: &mut Vec<Option<String>>,
    ) -> Body {
        let Some(rb) = self.resolve(rb) else {
            self.losses.dropped(
                here,
                LossKind::Body,
                "request body with an external reference not imported",
            );
            return Body::None;
        };
        let content = rb.get("content").map(Value::entries).unwrap_or_default();
        if content.is_empty() {
            return Body::None;
        }
        let pick = content
            .iter()
            .find(|(m, _)| m.to_ascii_lowercase().contains("json"))
            .or_else(|| content.first())
            .expect("not empty");
        let (media, mt) = (pick.0.clone(), &pick.1);
        if content.len() > 1 {
            let others: Vec<String> = content
                .iter()
                .filter(|(m, _)| *m != media)
                .map(|(m, _)| format!("`{m}`"))
                .collect();
            self.losses.approximated(
                here,
                LossKind::Body,
                format!(
                    "body imported as `{media}`; other media types not imported: {}",
                    others.join(", ")
                ),
            );
        }
        let lower = media.to_ascii_lowercase();
        headers.push(KeyValue::new("Content-Type", media.clone()));
        header_docs.push(None);
        let schema = mt.get("schema").and_then(|s| self.resolve(s));
        let example = mt
            .field("example")
            .or_else(|| {
                mt.get("examples")
                    .map(Value::entries)
                    .and_then(|e| e.first())
                    .and_then(|(_, ex)| pointer_or(self.root, ex))
                    .and_then(|ex| ex.field("value"))
            })
            // Without a schema there is nothing to generate: the spec's example stays.
            .filter(|_| !self.schema_examples() || schema.is_none());
        if lower.contains("x-www-form-urlencoded") || lower.contains("multipart/form-data") {
            let sample = example
                .cloned()
                .or_else(|| schema.map(|s| self.sample(s, 0, &mut Vec::new())));
            if example.is_none() {
                self.losses.approximated(
                    here,
                    LossKind::Body,
                    "form fields generated from the schema",
                );
            }
            let fields: Vec<(String, Value)> =
                sample.map(|s| s.entries().to_vec()).unwrap_or_default();
            if lower.contains("multipart") {
                let props = schema.and_then(|s| s.get("properties"));
                let parts = fields
                    .into_iter()
                    .map(|(n, v)| {
                        let file = props
                            .and_then(|p| p.get(&n))
                            .and_then(|p| pointer_or(self.root, p))
                            .is_some_and(|p| {
                                let f = p.string("format");
                                f == "binary"
                                    || f == "base64"
                                    || p.get("contentMediaType").is_some()
                            });
                        if file {
                            MultipartPart {
                                name: n,
                                kind: PartKind::File,
                                values: Vec::new(),
                                content_type: None,
                                enabled: true,
                            }
                        } else {
                            MultipartPart {
                                name: n,
                                kind: PartKind::Text,
                                values: vec![scalar_text(&v)],
                                content_type: None,
                                enabled: true,
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                if parts.iter().any(|p| p.kind == PartKind::File) {
                    self.losses.approximated(
                        here,
                        LossKind::Body,
                        "file parts have no file selected; choose them in the request",
                    );
                }
                // The transport sets the multipart Content-Type with its boundary.
                headers.pop();
                header_docs.pop();
                return Body::Multipart { parts };
            }
            headers.pop();
            header_docs.pop();
            return Body::FormUrlEncoded {
                fields: fields
                    .into_iter()
                    .map(|(n, v)| KeyValue::new(n, scalar_text(&v)))
                    .collect(),
            };
        }
        if lower.contains("octet-stream")
            || lower.starts_with("image/")
            || lower.starts_with("audio/")
            || lower.starts_with("video/")
            || lower.contains("pdf")
        {
            self.losses.approximated(
                here,
                LossKind::Body,
                format!("`{media}` body: choose the file to send in the request"),
            );
            headers.pop();
            header_docs.pop();
            return Body::File {
                path: String::new(),
                content_type: Some(media),
            };
        }
        let kind = raw_kind(&media);
        if let Some(ex) = example {
            let data = match ex {
                Value::Str(s) if kind != RawKind::Json => s.clone(),
                v => v.to_json_pretty(),
            };
            return Body::Raw { kind, data };
        }
        match (kind, schema) {
            (RawKind::Json, Some(s)) => {
                let sample = self.sample(s, 0, &mut Vec::new());
                self.losses.approximated(
                    here,
                    LossKind::Body,
                    "request body generated from the schema; edit the example values",
                );
                Body::Raw {
                    kind,
                    data: sample.to_json_pretty(),
                }
            }
            _ => {
                self.losses.approximated(
                    here,
                    LossKind::Body,
                    format!("`{media}` body has no example; it is empty"),
                );
                Body::None
            }
        }
    }

    /// An example value for a schema.
    fn sample(&mut self, schema: &'a Value, depth: usize, stack: &mut Vec<*const Value>) -> Value {
        let Some(s) = self.resolve(schema) else {
            return Value::Null;
        };
        let ptr = s as *const Value;
        if depth > MAX_SAMPLE_DEPTH || stack.contains(&ptr) {
            return Value::Null;
        }
        let spec = !self.schema_examples();
        for key in ["example", "default", "const"] {
            if (spec || key != "example")
                && let Some(v) = s.get(key)
            {
                return v.clone();
            }
        }
        if spec && let Some(v) = s.get("examples").and_then(|e| e.items().first()) {
            return v.clone();
        }
        if let Some(v) = s.get("enum").and_then(|e| e.items().first()) {
            return v.clone();
        }
        stack.push(ptr);
        let out = self.sample_inner(s, depth, stack);
        stack.pop();
        out
    }

    fn sample_inner(&mut self, s: &'a Value, depth: usize, stack: &mut Vec<*const Value>) -> Value {
        if let Some(all) = s.get("allOf").filter(|a| a.is_arr()) {
            let mut merged = Vec::new();
            for part in all.items() {
                if let Value::Obj(entries) = self.sample(part, depth + 1, stack) {
                    for (k, v) in entries {
                        merged.retain(|(m, _): &(String, Value)| *m != k);
                        merged.push((k, v));
                    }
                }
            }
            return Value::Obj(merged);
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(first) = s.get(key).and_then(|a| a.items().first()) {
                return self.sample(first, depth + 1, stack);
            }
        }
        let ty = match s.get("type") {
            Some(Value::Arr(types)) => types
                .iter()
                .filter_map(Value::as_str)
                .find(|t| *t != "null")
                .unwrap_or("null")
                .to_owned(),
            Some(t) => t.text().unwrap_or_default(),
            None if s.get("properties").is_some() => "object".into(),
            None if s.get("items").is_some() => "array".into(),
            None => String::new(),
        };
        match ty.as_str() {
            "object" | "" if s.get("properties").is_some() || ty == "object" => {
                let mut entries = Vec::new();
                for (k, v) in s.get("properties").map(Value::entries).unwrap_or_default() {
                    if pointer_or(self.root, v).is_some_and(|p| p.flag("readOnly")) {
                        continue;
                    }
                    entries.push((k.clone(), self.sample(v, depth + 1, stack)));
                }
                Value::Obj(entries)
            }
            "array" => match s.get("items") {
                Some(items) => {
                    let one = self.sample(items, depth + 1, stack);
                    if one.is_null() {
                        Value::Arr(Vec::new())
                    } else {
                        Value::Arr(vec![one])
                    }
                }
                None => Value::Arr(Vec::new()),
            },
            "string" => Value::Str(
                match s.string("format").as_str() {
                    "date-time" => "2024-01-01T00:00:00Z",
                    "date" => "2024-01-01",
                    "time" => "00:00:00",
                    "email" => "user@example.com",
                    "uuid" => "00000000-0000-0000-0000-000000000000",
                    "uri" | "url" => "https://example.com",
                    "hostname" => "example.com",
                    "ipv4" => "192.0.2.1",
                    "ipv6" => "2001:db8::1",
                    "byte" | "binary" | "password" => "",
                    _ => "string",
                }
                .to_owned(),
            ),
            "integer" => Value::Num("0".into()),
            "number" => Value::Num("0".into()),
            "boolean" => Value::Bool(true),
            _ => Value::Null,
        }
    }

    /// Auth for a security requirement list. `[]` means no auth.
    fn security(&mut self, sec: &'a Value, here: &str) -> AuthOut {
        let reqs = sec.items();
        let Some(first) = reqs.first() else {
            return AuthOut::Model(Auth::None);
        };
        if reqs.len() > 1 {
            let others: Vec<String> = reqs[1..]
                .iter()
                .flat_map(|r| r.entries().iter().map(|(k, _)| format!("`{k}`")))
                .collect();
            if !others.is_empty() {
                self.losses.approximated(
                    here,
                    LossKind::Auth,
                    format!(
                        "alternative security schemes not imported: {}",
                        others.join(", ")
                    ),
                );
            }
        }
        let entries = first.entries();
        let Some((scheme_name, scopes)) = entries.first() else {
            // `{}`: auth is optional.
            return AuthOut::Model(Auth::None);
        };
        if entries.len() > 1 {
            let more: Vec<String> = entries[1..].iter().map(|(k, _)| format!("`{k}`")).collect();
            self.losses.dropped(
                here,
                LossKind::Auth,
                format!(
                    "additional required security schemes not imported: {}",
                    more.join(", ")
                ),
            );
        }
        let scheme = self
            .root
            .get("components")
            .and_then(|c| c.get("securitySchemes"))
            .and_then(|s| s.get(scheme_name));
        let Some(scheme) = scheme.and_then(|s| self.resolve(s)) else {
            self.losses.dropped(
                here,
                LossKind::Auth,
                format!("security scheme `{scheme_name}` is not defined; not imported"),
            );
            return AuthOut::Model(Auth::None);
        };
        let secret = |cx: &mut Self, name: &str, is_secret: bool| {
            if !cx.secrets.iter().any(|(n, _)| n == name) {
                cx.secrets.push((name.to_owned(), is_secret));
            }
            format!("{{{{{name}}}}}")
        };
        match (
            scheme.string("type").as_str(),
            scheme.string("scheme").to_ascii_lowercase().as_str(),
        ) {
            ("http", "basic") => AuthOut::Model(Auth::Basic {
                username: secret(self, "username", false),
                password: secret(self, "password", true),
            }),
            ("http", "bearer") => AuthOut::Model(Auth::Bearer {
                token: secret(self, "token", true),
            }),
            ("http", "digest") => {
                let out = auth::user_password(
                    "digest",
                    &secret(self, "username", false),
                    &secret(self, "password", true),
                );
                auth::report(&mut self.losses, here, &out, "Digest auth");
                out
            }
            ("apiKey", _) => {
                let key = scheme.string("name");
                let value = secret(self, "apiKey", true);
                match scheme.string("in").as_str() {
                    "query" => AuthOut::Model(Auth::ApiKey {
                        key,
                        value,
                        placement: ApiKeyPlacement::Query,
                    }),
                    "header" => AuthOut::Model(Auth::ApiKey {
                        key,
                        value,
                        placement: ApiKeyPlacement::Header,
                    }),
                    other => {
                        self.losses.dropped(
                            here,
                            LossKind::Auth,
                            format!("API key in `{other}` (scheme `{scheme_name}`) not imported; send it as a header instead"),
                        );
                        AuthOut::Model(Auth::None)
                    }
                }
            }
            ("oauth2", _) => {
                let flows = scheme.get("flows").map(Value::entries).unwrap_or_default();
                let Some((flow_name, flow)) = flows.first() else {
                    self.losses.dropped(
                        here,
                        LossKind::Auth,
                        format!("OAuth 2.0 scheme `{scheme_name}` without flows not imported"),
                    );
                    return AuthOut::Model(Auth::None);
                };
                if flows.len() > 1 {
                    self.losses.approximated(
                        here,
                        LossKind::Auth,
                        format!("OAuth 2.0 flow `{flow_name}` imported; other flows of `{scheme_name}` not imported"),
                    );
                }
                let mut scope_list: Vec<String> =
                    scopes.items().iter().filter_map(Value::text).collect();
                if scope_list.is_empty() {
                    scope_list = flow
                        .get("scopes")
                        .map(Value::entries)
                        .unwrap_or_default()
                        .iter()
                        .map(|(k, _)| k.clone())
                        .collect();
                }
                let flow_id = auth::oauth2_flow(flow_name).unwrap_or("authorization_code");
                let client_id = secret(self, "clientId", false);
                let client_secret = if flow_id == "implicit" {
                    String::new()
                } else {
                    secret(self, "clientSecret", true)
                };
                let (username, password) = if flow_id == "resource_owner_password_credentials" {
                    (
                        secret(self, "username", false),
                        secret(self, "password", true),
                    )
                } else {
                    (String::new(), String::new())
                };
                let out = auth::oauth2(&OAuth2 {
                    flow: flow_id,
                    authorization_url: flow.string("authorizationUrl"),
                    access_token_url: flow.string("tokenUrl"),
                    refresh_token_url: flow.string("refreshUrl"),
                    client_id,
                    client_secret,
                    scope: scope_list.join(" "),
                    username,
                    password,
                    pkce: None,
                    ..OAuth2::default()
                });
                auth::report(&mut self.losses, here, &out, "OAuth 2.0");
                out
            }
            (kind, _) => {
                let label = match kind {
                    "openIdConnect" => "OpenID Connect",
                    "mutualTLS" => "mutual TLS",
                    _ => "this security scheme",
                };
                self.losses.dropped(
                    here,
                    LossKind::Auth,
                    format!("{label} (scheme `{scheme_name}`) not imported; the request is sent without auth"),
                );
                AuthOut::Model(Auth::None)
            }
        }
    }

    /// Explicit response examples.
    fn examples(&mut self, op: &'a Value) -> Vec<ExampleOut> {
        let mut out = Vec::new();
        for (status, resp) in op.get("responses").map(Value::entries).unwrap_or_default() {
            let Some(resp) = self.resolve(resp) else {
                continue;
            };
            let code = status.parse::<u16>().ok();
            for (media, mt) in resp.get("content").map(Value::entries).unwrap_or_default() {
                let kind = body_kind(media);
                let mut found: Vec<(String, Value)> = Vec::new();
                if let Some(v) = mt.field("example") {
                    found.push((String::new(), v.clone()));
                }
                for (ex_name, ex) in mt.get("examples").map(Value::entries).unwrap_or_default() {
                    if let Some(ex) = self.resolve(ex)
                        && let Some(v) = ex.field("value")
                    {
                        found.push((ex_name.clone(), v.clone()));
                    }
                }
                for (ex_name, v) in found {
                    let data = match (&v, kind) {
                        (Value::Str(s), k) if k != "json" => s.clone(),
                        (v, _) => v.to_json_pretty(),
                    };
                    let mut name = format!(
                        "{status} {}",
                        resp.string("description")
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                    );
                    if !ex_name.is_empty() {
                        name = format!("{status} {ex_name}");
                    }
                    out.push(ExampleOut {
                        name: name.trim().chars().take(120).collect(),
                        status: code,
                        headers: vec![("Content-Type".into(), media.clone())],
                        body: Some((kind, data)),
                        ..ExampleOut::default()
                    });
                }
            }
        }
        out
    }
}

// ---- the wizard's overview ----

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

/// What the OpenAPI 3.x document `root` holds, named and ordered as [`build`] with the
/// default options would import it.
pub(super) fn overview(
    root: &Value,
    main_file: &str,
    spec_version: String,
    files: u32,
    losses: Vec<Loss>,
) -> super::SpecOverview {
    let info = root.get("info").cloned().unwrap_or(Value::Null);
    let names = environment_names(root, &OpenApiOptions::default());
    let servers = servers(root)
        .iter()
        .enumerate()
        .map(|(i, s)| SpecServer {
            url: s.string("url"),
            description: non_empty(s.str_of("description")),
            suggested_name: names.get(i).cloned().flatten().unwrap_or_default(),
            variables: s
                .get("variables")
                .map(Value::entries)
                .unwrap_or_default()
                .iter()
                .map(|(name, v)| SpecServerVariable {
                    name: name.clone(),
                    default: v.string("default"),
                    values: v
                        .get("enum")
                        .map(Value::items)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(Value::text)
                        .collect(),
                    description: non_empty(v.str_of("description")),
                })
                .collect(),
        })
        .collect();

    let mut operations = Vec::new();
    for (path, item) in root.get("paths").map(Value::entries).unwrap_or_default() {
        let Some(item) = pointer_or(root, item) else {
            continue;
        };
        for (method, op) in item_operations(item) {
            operations.push(SpecOperation {
                key: operation_key(method, path),
                method: method.to_ascii_uppercase(),
                path: path.clone(),
                tags: op
                    .get("tags")
                    .map(Value::items)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Value::text)
                    .collect(),
                summary: non_empty(op.str_of("summary")),
                operation_id: non_empty(op.str_of("operationId")),
                deprecated: op.flag("deprecated"),
            });
        }
    }

    // Folder order: the top-level tags, then first tags as used, then the other tags.
    let mut tags: Vec<SpecTag> = Vec::new();
    let add = |tags: &mut Vec<SpecTag>, name: String, description: Option<String>| {
        if !tags.iter().any(|t| t.name == name) {
            tags.push(SpecTag {
                name,
                description,
                operations: 0,
            });
        }
    };
    for t in root.get("tags").map(Value::items).unwrap_or_default() {
        if let Some(n) = t.str_of("name") {
            add(&mut tags, n, non_empty(t.str_of("description")));
        }
    }
    for op in &operations {
        if let Some(first) = op.tags.first() {
            add(&mut tags, first.clone(), None);
        }
    }
    for op in &operations {
        for t in op.tags.iter().skip(1) {
            add(&mut tags, t.clone(), None);
        }
    }
    for op in &operations {
        let mut seen: Vec<&String> = Vec::new();
        for t in &op.tags {
            if !seen.contains(&t) {
                seen.push(t);
                if let Some(tag) = tags.iter_mut().find(|x| x.name == *t) {
                    tag.operations += 1;
                }
            }
        }
    }

    let security = root
        .get("components")
        .and_then(|c| c.get("securitySchemes"))
        .map(Value::entries)
        .unwrap_or_default()
        .iter()
        .map(|(name, scheme)| match pointer_or(root, scheme) {
            Some(scheme) => scheme_overview(name, scheme),
            None => SpecSecurityScheme {
                name: name.clone(),
                kind: "other".into(),
                detail: "not resolved".into(),
                maps_to: None,
                variables: Vec::new(),
            },
        })
        .collect();

    super::SpecOverview {
        title: spec_title(root, main_file),
        version: info.string("version"),
        description: non_empty(info.str_of("description")),
        spec_version,
        main_file: main_file.to_owned(),
        files,
        servers,
        tags,
        operations,
        security,
        losses,
    }
}

/// What [`Cx::security`] makes of a scheme, in plain words.
fn scheme_overview(name: &str, scheme: &Value) -> SpecSecurityScheme {
    let vars = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let (kind, detail, maps_to, variables) = match (
        scheme.string("type").as_str(),
        scheme.string("scheme").to_ascii_lowercase().as_str(),
    ) {
        ("http", "basic") => (
            "basic",
            "HTTP Basic".to_owned(),
            Some("Basic auth {{username}} / {{password}}".to_owned()),
            vars(&["username", "password"]),
        ),
        ("http", "bearer") => (
            "bearer",
            non_empty(scheme.str_of("bearerFormat")).unwrap_or_else(|| "HTTP Bearer".into()),
            Some("Bearer token {{token}}".to_owned()),
            vars(&["token"]),
        ),
        ("http", "digest") => (
            "digest",
            "HTTP Digest".to_owned(),
            Some(
                "Digest auth {{username}} / {{password}} (kept in the file, not sent yet)"
                    .to_owned(),
            ),
            vars(&["username", "password"]),
        ),
        ("http", other) => ("other", format!("HTTP {other}"), None, Vec::new()),
        ("apiKey", _) => {
            let key = scheme.string("name");
            let place = scheme.string("in");
            let maps_to = matches!(place.as_str(), "header" | "query")
                .then(|| format!("API key {key} in {place} {{{{apiKey}}}}"));
            let variables = if maps_to.is_some() {
                vars(&["apiKey"])
            } else {
                Vec::new()
            };
            ("api-key", format!("{place} {key}"), maps_to, variables)
        }
        ("oauth2", _) => {
            let flows = scheme.get("flows").map(Value::entries).unwrap_or_default();
            match flows.first() {
                None => ("oauth2", "no flows".to_owned(), None, Vec::new()),
                Some((flow_name, _)) => {
                    let flow = auth::oauth2_flow(flow_name).unwrap_or("authorization_code");
                    let words = match flow {
                        "client_credentials" => "client credentials",
                        "resource_owner_password_credentials" => "password",
                        "implicit" => "implicit",
                        _ => "authorization code",
                    };
                    let mut v = vec!["clientId"];
                    if flow != "implicit" {
                        v.push("clientSecret");
                    }
                    if flow == "resource_owner_password_credentials" {
                        v.extend(["username", "password"]);
                    }
                    let mut detail = format!("{words} flow");
                    if flows.len() > 1 {
                        detail.push_str(&format!(" (and {} more)", flows.len() - 1));
                    }
                    (
                        "oauth2",
                        detail,
                        Some(format!("OAuth 2.0 {words} ({})", v.join(", "))),
                        vars(&v),
                    )
                }
            }
        }
        ("openIdConnect", _) => (
            "openid-connect",
            scheme.string("openIdConnectUrl"),
            None,
            Vec::new(),
        ),
        ("mutualTLS", _) => (
            "mutual-tls",
            "client certificate".to_owned(),
            None,
            Vec::new(),
        ),
        (other, _) => ("other", other.to_owned(), None, Vec::new()),
    };
    SpecSecurityScheme {
        name: name.to_owned(),
        kind: kind.to_owned(),
        detail,
        maps_to,
        variables,
    }
}

fn pointer_or<'v>(root: &'v Value, v: &'v Value) -> Option<&'v Value> {
    let mut cur = v;
    for _ in 0..MAX_REF_DEPTH {
        match cur.get("$ref").and_then(Value::as_str) {
            Some(r) => cur = pointer(root, r)?,
            None => return Some(cur),
        }
    }
    None
}

fn body_kind(media: &str) -> &'static str {
    let m = media.to_ascii_lowercase();
    if m.contains("json") {
        "json"
    } else if m.contains("xml") {
        "xml"
    } else if m.contains("html") {
        "html"
    } else {
        "text"
    }
}

/// A scalar as text; objects and arrays as compact JSON.
fn scalar_text(v: &Value) -> String {
    match v {
        Value::Obj(_) | Value::Arr(_) => v.to_json(),
        Value::Null => String::new(),
        v => v.text().unwrap_or_default(),
    }
}

/// `/pets/{petId}` → `/pets/:petId`.
fn path_to_url(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                if name.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push(':');
                    out.push_str(name);
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// `https://{region}.example.com` → `https://{{region}}.example.com`.
fn braces_to_vars(url: &str) -> String {
    let mut out = String::new();
    let mut rest = url;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                out.push_str("{{");
                out.push_str(&after[..end]);
                out.push_str("}}");
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::value::parse_any;

    #[test]
    fn paths_and_servers() {
        assert_eq!(path_to_url("/pets/{petId}/x{y}"), "/pets/:petId/x:y");
        assert_eq!(path_to_url("/a/{"), "/a/{");
        assert_eq!(braces_to_vars("https://{r}.x/{v}"), "https://{{r}}.x/{{v}}");
    }

    #[test]
    fn pointers() {
        let v = parse_any(r#"{"a": {"b/c": [1, {"d": 2}]}}"#).unwrap();
        assert_eq!(pointer(&v, "#/a/b~1c/1/d"), Some(&Value::Num("2".into())));
        assert_eq!(pointer(&v, "#/a/x"), None);
    }
}
