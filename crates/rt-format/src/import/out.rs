// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Writing a new collection: what an importer built ([`CollectionOut`]) → OpenCollection
//! files, laid out as Bruno writes them.
//!
//! Besides the model, the files carry what OpenCollection can hold but the model does not:
//! documentation (`docs`, `description` of headers, parameters and variables), saved
//! examples, and auth schemes the core cannot execute yet (OAuth 2.0, Digest, AWS, ...) with
//! their settings. Every request file is read back and must give exactly the request the
//! importer meant; the whole collection must read without issues.
//!
//! File names: a slug of the item name (as `CollectionDoc::add_request` names new requests),
//! made unique per folder ignoring case. Items get `seq` 1, 2, ... in source order.

use std::collections::{BTreeMap, HashSet};

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, Item, KeyValue, MultipartPart, Param, ParamKind,
    PartKind, RawKind, RequestSpec, Script, Settings, Variable,
};

use crate::runtime;
use crate::yaml::{Document, Val, document_text, parse};
use crate::{COLLECTION_FILE, FOLDER_FILE, FileSet, edit, read, write};

/// Auth as written: one the model knows, or another OpenCollection scheme with its fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AuthOut {
    Model(Auth),
    /// `type: <kind>` and further fields, e.g. an OAuth 2.0 flow. Reads as
    /// `Auth::Unsupported { kind }`.
    Other {
        kind: &'static str,
        fields: Vec<(&'static str, Val)>,
    },
}

impl Default for AuthOut {
    fn default() -> Self {
        AuthOut::Model(Auth::Inherit)
    }
}

impl AuthOut {
    fn model(&self) -> Auth {
        match self {
            AuthOut::Model(a) => a.clone(),
            AuthOut::Other { kind, .. } => Auth::Unsupported {
                kind: (*kind).to_owned(),
            },
        }
    }
}

/// Defaults of the collection or a folder.
#[derive(Debug, Clone, Default)]
pub(crate) struct DefaultsOut {
    pub headers: Vec<KeyValue>,
    pub header_docs: Vec<Option<String>>,
    /// Collection: `Auth::None` (or `Inherit`) means none. Folder: `Inherit` means none.
    pub auth: AuthOut,
    pub vars: Vec<Variable>,
    pub var_docs: Vec<Option<String>>,
    /// Scripts that run for every request below (wave 7).
    pub scripts: Vec<Script>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ExampleOut {
    pub name: String,
    pub description: Option<String>,
    pub method: Option<String>,
    pub url: Option<String>,
    pub request_headers: Vec<KeyValue>,
    pub request_body: Option<(RawKind, String)>,
    pub status: Option<u16>,
    pub status_text: Option<String>,
    pub headers: Vec<(String, String)>,
    /// `json`, `text`, `xml`, `html` or `binary`, and the data.
    pub body: Option<(&'static str, String)>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RequestOut {
    /// `seq` is assigned when writing; `auth` is taken from [`RequestOut::auth`].
    pub spec: RequestSpec,
    pub auth: AuthOut,
    pub docs: Option<String>,
    pub header_docs: Vec<Option<String>>,
    pub param_docs: Vec<Option<String>>,
    pub examples: Vec<ExampleOut>,
}

impl RequestOut {
    pub fn new(spec: RequestSpec) -> Self {
        let auth = AuthOut::Model(spec.auth.clone());
        Self {
            spec,
            auth,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FolderOut {
    pub name: String,
    pub docs: Option<String>,
    pub defaults: DefaultsOut,
    pub items: Vec<ItemOut>,
}

// Short-lived build tree; boxing every request would only add allocations.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub(crate) enum ItemOut {
    Folder(FolderOut),
    Request(RequestOut),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CollectionOut {
    pub name: String,
    pub docs: Option<String>,
    pub defaults: DefaultsOut,
    pub items: Vec<ItemOut>,
    pub environments: Vec<Environment>,
}

/// What was written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub requests: u32,
    pub folders: u32,
    pub environments: u32,
}

const ROOT_ORDER: [&str; 6] = ["info", "http", "runtime", "settings", "docs", "examples"];

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn described(e: &mut Vec<(&'static str, Val)>, doc: Option<&Option<String>>) {
    if let Some(Some(d)) = doc
        && !d.is_empty()
    {
        e.push(("description", Val::str(d)));
    }
}

fn disabled(e: &mut Vec<(&'static str, Val)>, enabled: bool) {
    if !enabled {
        e.push(("disabled", Val::bool(true)));
    }
}

fn kv_val(kv: &KeyValue, doc: Option<&Option<String>>) -> Val {
    let mut e = vec![("name", Val::str(&kv.name)), ("value", Val::str(&kv.value))];
    described(&mut e, doc);
    disabled(&mut e, kv.enabled);
    Val::Map(e)
}

fn kv_list(list: &[KeyValue], docs: &[Option<String>]) -> Val {
    Val::Seq(
        list.iter()
            .enumerate()
            .map(|(i, kv)| kv_val(kv, docs.get(i)))
            .collect(),
    )
}

fn param_val(p: &Param, doc: Option<&Option<String>>) -> Val {
    let mut e = vec![
        ("name", Val::str(&p.name)),
        ("value", Val::str(&p.value)),
        (
            "type",
            Val::str(match p.kind {
                ParamKind::Query => "query",
                ParamKind::Path => "path",
            }),
        ),
    ];
    described(&mut e, doc);
    disabled(&mut e, p.enabled);
    Val::Map(e)
}

fn part_val(p: &MultipartPart) -> Val {
    let value = match (p.kind, p.values.as_slice()) {
        (PartKind::Text, [one]) => Val::str(one),
        _ => Val::Seq(p.values.iter().map(Val::str).collect()),
    };
    let mut e = vec![
        ("name", Val::str(&p.name)),
        (
            "type",
            Val::str(match p.kind {
                PartKind::Text => "text",
                PartKind::File => "file",
            }),
        ),
        ("value", value),
    ];
    if let Some(ct) = &p.content_type {
        e.push(("contentType", Val::str(ct)));
    }
    disabled(&mut e, p.enabled);
    Val::Map(e)
}

fn var_val(v: &Variable, doc: Option<&Option<String>>) -> Val {
    let mut e = Vec::new();
    if v.secret {
        e.push(("secret", Val::bool(true)));
    }
    e.push(("name", Val::str(&v.name)));
    if !v.secret
        && let Some(value) = &v.value
    {
        e.push(("value", Val::str(value)));
    }
    described(&mut e, doc);
    disabled(&mut e, v.enabled);
    Val::Map(e)
}

fn var_list(list: &[Variable], docs: &[Option<String>]) -> Val {
    Val::Seq(
        list.iter()
            .enumerate()
            .map(|(i, v)| var_val(v, docs.get(i)))
            .collect(),
    )
}

pub(crate) fn raw_kind_str(kind: RawKind) -> &'static str {
    match kind {
        RawKind::Json => "json",
        RawKind::Text => "text",
        RawKind::Xml => "xml",
        RawKind::Sparql => "sparql",
    }
}

fn body_val(body: &Body) -> Option<Val> {
    let (kind, data) = match body {
        Body::None => return None,
        Body::Raw { kind, data } => (raw_kind_str(*kind), Val::str(data)),
        Body::FormUrlEncoded { fields } => ("form-urlencoded", kv_list(fields, &[])),
        Body::Multipart { parts } => (
            "multipart-form",
            Val::Seq(parts.iter().map(part_val).collect()),
        ),
        Body::File { path, content_type } => {
            let mut e = vec![("filePath", Val::str(path))];
            if let Some(ct) = content_type {
                e.push(("contentType", Val::str(ct)));
            }
            e.push(("selected", Val::bool(true)));
            ("file", Val::Seq(vec![Val::Map(e)]))
        }
        // Importers produce no GraphQL bodies before wave 8, strand B.
        Body::GraphQl { query, variables } => (
            "graphql",
            Val::Map(vec![
                ("query", Val::str(query)),
                ("variables", Val::str(variables)),
            ]),
        ),
    };
    Some(Val::Map(vec![("type", Val::str(kind)), ("data", data)]))
}

/// `auth` as written, or `None` for no entry. `absent` is the auth that no entry means.
fn auth_val(auth: &AuthOut, absent: &Auth) -> Option<Val> {
    let a = match auth {
        AuthOut::Other { kind, fields } => {
            let mut e = vec![("type", Val::str(*kind))];
            e.extend(fields.iter().cloned());
            return Some(Val::Map(e));
        }
        AuthOut::Model(a) => a,
    };
    if a == absent {
        return None;
    }
    let fields: Vec<(&'static str, Val)> = match a {
        Auth::Inherit => return Some(Val::str("inherit")),
        Auth::None => vec![],
        Auth::Basic { username, password } => vec![
            ("username", Val::str(username)),
            ("password", Val::str(password)),
        ],
        Auth::Bearer { token } => vec![("token", Val::str(token))],
        Auth::ApiKey {
            key,
            value,
            placement,
        } => vec![
            ("key", Val::str(key)),
            ("value", Val::str(value)),
            (
                "placement",
                Val::str(match placement {
                    ApiKeyPlacement::Header => "header",
                    ApiKeyPlacement::Query => "query",
                }),
            ),
        ],
        Auth::Unsupported { kind } => {
            return Some(Val::Map(vec![("type", Val::str(kind))]));
        }
        Auth::OAuth2(oauth) => {
            let mut e = vec![("type", Val::str("oauth2"))];
            e.extend(crate::auth::oauth2::fields(oauth).unwrap_or_default());
            return Some(Val::Map(e));
        }
        Auth::AwsSigV4(aws) => {
            let mut e = vec![("type", Val::str("awsv4"))];
            e.extend(crate::auth::awsv4::fields(aws));
            return Some(Val::Map(e));
        }
    };
    let kind = match a {
        Auth::None => "none",
        Auth::Basic { .. } => "basic",
        Auth::Bearer { .. } => "bearer",
        _ => "apikey",
    };
    let mut e = vec![("type", Val::str(kind))];
    e.extend(fields);
    Some(Val::Map(e))
}

fn settings_val(s: &Settings) -> Option<Val> {
    let mut e = Vec::new();
    if let Some(b) = s.encode_url {
        e.push(("encodeUrl", Val::bool(b)));
    }
    if let Some(t) = s.timeout_ms {
        e.push(("timeout", Val::Plain(t.to_string())));
    }
    if let Some(b) = s.follow_redirects {
        e.push(("followRedirects", Val::bool(b)));
    }
    if let Some(m) = s.max_redirects {
        e.push(("maxRedirects", Val::Plain(m.to_string())));
    }
    (!e.is_empty()).then_some(Val::Map(e))
}

fn defaults_val(d: &DefaultsOut, absent: &Auth) -> Option<Val> {
    let mut e = Vec::new();
    if !d.headers.is_empty() {
        e.push(("headers", kv_list(&d.headers, &d.header_docs)));
    }
    if let Some(a) = auth_val(&d.auth, absent) {
        e.push(("auth", a));
    }
    if !d.vars.is_empty() {
        e.push(("variables", var_list(&d.vars, &d.var_docs)));
    }
    if let Some(s) = runtime::scripts_val(&d.scripts) {
        e.push(("scripts", s));
    }
    (!e.is_empty()).then_some(Val::Map(e))
}

fn example_val(x: &ExampleOut) -> Val {
    let mut e = vec![("name", Val::str(&x.name))];
    if let Some(d) = x.description.as_ref().filter(|d| !d.is_empty()) {
        e.push(("description", Val::str(d)));
    }
    let mut req = Vec::new();
    if let Some(u) = &x.url {
        req.push(("url", Val::str(u)));
    }
    if let Some(m) = &x.method {
        req.push(("method", Val::str(m)));
    }
    if !x.request_headers.is_empty() {
        req.push(("headers", kv_list(&x.request_headers, &[])));
    }
    if let Some((kind, data)) = &x.request_body {
        req.push((
            "body",
            Val::Map(vec![
                ("type", Val::str(raw_kind_str(*kind))),
                ("data", Val::str(data)),
            ]),
        ));
    }
    if !req.is_empty() {
        e.push(("request", Val::Map(req)));
    }
    let mut res = Vec::new();
    if let Some(s) = x.status {
        res.push(("status", Val::Plain(s.to_string())));
    }
    if let Some(t) = x.status_text.as_ref().filter(|t| !t.is_empty()) {
        res.push(("statusText", Val::str(t)));
    }
    if !x.headers.is_empty() {
        res.push((
            "headers",
            Val::Seq(
                x.headers
                    .iter()
                    .map(|(n, v)| Val::Map(vec![("name", Val::str(n)), ("value", Val::str(v))]))
                    .collect(),
            ),
        ));
    }
    if let Some((kind, data)) = &x.body {
        res.push((
            "body",
            Val::Map(vec![("type", Val::str(*kind)), ("data", Val::str(data))]),
        ));
    }
    if !res.is_empty() {
        e.push(("response", Val::Map(res)));
    }
    Val::Map(e)
}

fn request_text(r: &RequestOut, spec: &RequestSpec) -> String {
    let mut info = vec![("name", Val::str(&spec.name)), ("type", Val::str("http"))];
    if let Some(seq) = spec.seq {
        info.push(("seq", Val::Plain(number(seq))));
    }
    let mut http = vec![
        ("method", Val::str(&spec.method)),
        ("url", Val::str(&spec.url)),
    ];
    if !spec.params.is_empty() {
        http.push((
            "params",
            Val::Seq(
                spec.params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| param_val(p, r.param_docs.get(i)))
                    .collect(),
            ),
        ));
    }
    if !spec.headers.is_empty() {
        http.push(("headers", kv_list(&spec.headers, &r.header_docs)));
    }
    if let Some(b) = body_val(&spec.body) {
        http.push(("body", b));
    }
    if let Some(a) = auth_val(&r.auth, &Auth::None) {
        http.push(("auth", a));
    }
    let mut root = vec![("info", Val::Map(info)), ("http", Val::Map(http))];
    let mut rt = Vec::new();
    if !spec.vars.is_empty() {
        rt.push(("variables", var_list(&spec.vars, &[])));
    }
    if let Some(s) = runtime::scripts_val(&spec.scripts) {
        rt.push(("scripts", s));
    }
    if let Some(a) = runtime::assertions_val(&spec.assertions) {
        rt.push(("assertions", a));
    }
    if let Some(a) = runtime::actions_val(&spec.actions) {
        rt.push(("actions", a));
    }
    if !rt.is_empty() {
        root.push(("runtime", Val::Map(rt)));
    }
    if let Some(s) = settings_val(&spec.settings) {
        root.push(("settings", s));
    }
    if let Some(d) = r.docs.as_ref().filter(|d| !d.trim().is_empty()) {
        root.push(("docs", Val::str(d)));
    }
    if !r.examples.is_empty() {
        root.push((
            "examples",
            Val::Seq(r.examples.iter().map(example_val).collect()),
        ));
    }
    debug_assert!(root.iter().all(|(k, _)| ROOT_ORDER.contains(k)));
    document_text(&root, true)
}

/// Names already used in one folder, ignoring case.
#[derive(Default)]
struct Names(HashSet<String>);

impl Names {
    fn claim(&mut self, stem: &str, ext: &str) -> String {
        let mut n = 1;
        loop {
            let name = if n == 1 {
                format!("{stem}{ext}")
            } else {
                format!("{stem}-{n}{ext}")
            };
            if self.0.insert(name.to_lowercase()) {
                return name;
            }
            n += 1;
        }
    }
}

struct Writer {
    files: BTreeMap<String, Vec<u8>>,
    /// Request path → what it must read back as.
    expected: Vec<(String, RequestSpec)>,
    counts: Counts,
}

fn normalized(spec: &RequestSpec) -> RequestSpec {
    let mut spec = spec.clone();
    for v in &mut spec.vars {
        if v.secret {
            v.value = None;
        }
    }
    spec
}

impl Writer {
    fn items(&mut self, dir: &str, items: &[ItemOut]) {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut names = Names::default();
        if dir.is_empty() {
            names.0.insert(read::ENVIRONMENTS_DIR.to_owned());
            names.0.insert(COLLECTION_FILE.to_owned());
        }
        names.0.insert(FOLDER_FILE.to_owned());
        for (i, item) in items.iter().enumerate() {
            let seq = (i + 1) as f64;
            match item {
                ItemOut::Folder(f) => {
                    let name = if f.name.trim().is_empty() {
                        "Untitled folder"
                    } else {
                        &f.name
                    };
                    let sub = format!("{prefix}{}", names.claim(&edit::slug(name), ""));
                    let info = vec![
                        ("name", Val::str(name)),
                        ("type", Val::str("folder")),
                        ("seq", Val::Plain(number(seq))),
                    ];
                    let mut root = vec![("info", Val::Map(info))];
                    if let Some(d) = defaults_val(&f.defaults, &Auth::Inherit) {
                        root.push(("request", d));
                    }
                    if let Some(d) = f.docs.as_ref().filter(|d| !d.trim().is_empty()) {
                        root.push(("docs", Val::str(d)));
                    }
                    self.files.insert(
                        format!("{sub}/{FOLDER_FILE}"),
                        document_text(&root, true).into_bytes(),
                    );
                    self.counts.folders += 1;
                    self.items(&sub, &f.items);
                }
                ItemOut::Request(r) => {
                    let mut spec = r.spec.clone();
                    if spec.name.trim().is_empty() {
                        spec.name = "Untitled request".into();
                    }
                    if spec.method.trim().is_empty() {
                        spec.method = "GET".into();
                    }
                    spec.seq = Some(seq);
                    spec.auth = r.auth.model();
                    let path = format!("{prefix}{}", names.claim(&edit::slug(&spec.name), ".yml"));
                    self.files
                        .insert(path.clone(), request_text(r, &spec).into_bytes());
                    self.expected.push((path, normalized(&spec)));
                    self.counts.requests += 1;
                }
            }
        }
    }
}

fn collection_text(c: &CollectionOut) -> String {
    let mut root = vec![
        ("opencollection", Val::Plain("\"1.0.0\"".into())),
        (
            "info",
            Val::Map(vec![("name", Val::str(collection_name(&c.name)))]),
        ),
    ];
    let mut defaults = c.defaults.clone();
    if defaults.auth == AuthOut::Model(Auth::Inherit) {
        defaults.auth = AuthOut::Model(Auth::None);
    }
    if let Some(d) = defaults_val(&defaults, &Auth::None) {
        root.push(("request", d));
    }
    if let Some(d) = c.docs.as_ref().filter(|d| !d.trim().is_empty()) {
        root.push(("docs", Val::str(d)));
    }
    root.push(("bundled", Val::bool(false)));
    document_text(&root, true)
}

pub(crate) fn collection_name(name: &str) -> &str {
    let name = name.trim();
    if name.is_empty() {
        "Imported collection"
    } else {
        name
    }
}

fn environment_doc(env: &Environment, stem: &str) -> Result<Document, String> {
    let initial = parse(&write::new_environment_text(&env.name)).map_err(|e| e.to_string())?;
    write::apply_environment_doc(&initial, env, stem)
}

/// Renders the collection and checks that it reads back exactly. An error here is a bug in
/// an importer, never the user's input.
pub(crate) fn write_collection(c: &CollectionOut) -> Result<(FileSet, Counts), String> {
    let mut w = Writer {
        files: BTreeMap::new(),
        expected: Vec::new(),
        counts: Counts::default(),
    };
    w.files
        .insert(COLLECTION_FILE.to_owned(), collection_text(c).into_bytes());
    w.items("", &c.items);
    let mut env_names = Names::default();
    let mut seen = HashSet::new();
    for env in &c.environments {
        if !seen.insert(env.name.clone()) {
            return Err(format!("duplicate environment {:?}", env.name));
        }
        let stem = env_names.claim(&edit::file_name(&env.name), "");
        let doc = environment_doc(env, &stem)?;
        w.files.insert(
            format!("{}/{stem}.yml", read::ENVIRONMENTS_DIR),
            doc.to_text().into_bytes(),
        );
        w.counts.environments += 1;
    }
    let files = FileSet { files: w.files };

    let doc = crate::read_collection(&files).map_err(|e| e.to_string())?;
    if let Some(issue) = doc.issues().first() {
        return Err(issue.to_string());
    }
    let mut got = BTreeMap::new();
    collect(&doc.model().items, &mut got);
    for (path, want) in &w.expected {
        match got.get(path.as_str()) {
            Some(spec) if *spec == want => {}
            other => {
                return Err(format!(
                    "{path} does not read back as imported (wanted {want:?}, got {other:?})"
                ));
            }
        }
    }
    for env in &c.environments {
        let want = write::normalized_env(env);
        if doc.model().environment(&env.name) != Some(&want) {
            return Err(format!("environment {:?} does not read back", env.name));
        }
    }
    if doc.model().defaults.vars
        != c.defaults
            .vars
            .iter()
            .map(|v| {
                let mut v = v.clone();
                if v.secret {
                    v.value = None;
                }
                v
            })
            .collect::<Vec<_>>()
    {
        return Err("collection variables do not read back".into());
    }
    if doc.model().defaults.scripts != c.defaults.scripts {
        return Err("collection scripts do not read back".into());
    }
    let mut folders = Vec::new();
    folder_scripts(&c.items, &mut folders);
    let mut got_folders = Vec::new();
    model_folder_scripts(&doc.model().items, &mut got_folders);
    if folders != got_folders {
        return Err("folder scripts do not read back".into());
    }
    Ok((files, w.counts))
}

/// Folder scripts in the order the folders are written.
fn folder_scripts<'a>(items: &'a [ItemOut], out: &mut Vec<&'a [Script]>) {
    for item in items {
        if let ItemOut::Folder(f) = item {
            out.push(&f.defaults.scripts);
            folder_scripts(&f.items, out);
        }
    }
}

fn model_folder_scripts<'a>(items: &'a [Item], out: &mut Vec<&'a [Script]>) {
    for item in items {
        if let Item::Folder(f) = item {
            out.push(&f.defaults.scripts);
            model_folder_scripts(&f.items, out);
        }
    }
}

fn collect<'a>(items: &'a [Item], out: &mut BTreeMap<&'a str, &'a RequestSpec>) {
    for item in items {
        match item {
            Item::Folder(f) => collect(&f.items, out),
            Item::Request(r) => {
                out.insert(&r.path, &r.spec);
            }
        }
    }
}
