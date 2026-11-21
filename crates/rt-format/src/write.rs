// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt_core::model` → files: the inverse of the `read` module, as format-preserving edits.
//!
//! Every field is compared with what the reader makes of the file; only fields that differ
//! are rewritten. A changed scalar keeps its place; a changed list keeps every item that is
//! still equal (matched by longest common subsequence) and edits the others in place, so
//! fields the model does not represent (descriptions, typed values, comments) survive. New
//! subtrees are written the way Bruno writes them, in Bruno's key order. Only keys the
//! reader maps are ever written, so no field outside the OpenCollection schema appears.
//!
//! After editing, the file is parsed and read again and must give exactly the model that was
//! written; otherwise the edit is refused and the file stays as it was.
//!
//! Secrets: a variable with `secret: true` is written without `value`, whatever the model
//! holds (product principle 3).

use std::collections::BTreeMap;

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, KeyValue, MultipartPart, Param, ParamKind, PartKind,
    RawKind, RequestSpec, Settings, Variable,
};

use crate::read::{self, Builder};
use crate::yaml::{Document, ItemOp, Map, Node, Val, document_text, parse, reparse};

const ROOT_ORDER: &[&str] = &[
    "meta", "info", "http", "runtime", "settings", "docs", "examples",
];
const INFO_ORDER: &[&str] = &["name", "type", "seq", "tags"];
const HTTP_ORDER: &[&str] = &["method", "url", "params", "headers", "body", "auth"];
const RUNTIME_ORDER: &[&str] = &["variables", "scripts", "assertions", "actions"];
const SETTINGS_ORDER: &[&str] = &["encodeUrl", "timeout", "followRedirects", "maxRedirects"];
const KV_ORDER: &[&str] = &["name", "value", "description", "disabled"];
const PARAM_ORDER: &[&str] = &["name", "value", "type", "description", "disabled"];
const PART_ORDER: &[&str] = &[
    "name",
    "type",
    "value",
    "contentType",
    "description",
    "disabled",
];
const VAR_ORDER: &[&str] = &["secret", "name", "value", "type", "description", "disabled"];
const BODY_ORDER: &[&str] = &["type", "data"];
const FILE_ORDER: &[&str] = &["filePath", "contentType", "selected", "description"];
const AUTH_ORDER: &[&str] = &[
    "type",
    "username",
    "password",
    "token",
    "key",
    "value",
    "placement",
];
const ENV_ORDER: &[&str] = &["name", "variables"];

type Result<T> = std::result::Result<T, String>;

// ---------------------------------------------------------------------------------------------
// Rendering model values as new subtrees

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn disabled(entries: &mut Vec<(&'static str, Val)>, enabled: bool) {
    if !enabled {
        entries.push(("disabled", Val::bool(true)));
    }
}

fn kv_val(kv: &KeyValue) -> Val {
    let mut e = vec![("name", Val::str(&kv.name)), ("value", Val::str(&kv.value))];
    disabled(&mut e, kv.enabled);
    Val::Map(e)
}

fn kind_str(kind: ParamKind) -> &'static str {
    match kind {
        ParamKind::Query => "query",
        ParamKind::Path => "path",
    }
}

fn param_val(p: &Param) -> Val {
    let mut e = vec![
        ("name", Val::str(&p.name)),
        ("value", Val::str(&p.value)),
        ("type", Val::str(kind_str(p.kind))),
    ];
    disabled(&mut e, p.enabled);
    Val::Map(e)
}

fn part_kind_str(kind: PartKind) -> &'static str {
    match kind {
        PartKind::Text => "text",
        PartKind::File => "file",
    }
}

/// A text part with one value is a string; anything else a list (as the reader expects).
fn part_value(p: &MultipartPart) -> Val {
    match (p.kind, p.values.as_slice()) {
        (PartKind::Text, [one]) => Val::str(one),
        _ => Val::Seq(p.values.iter().map(Val::str).collect()),
    }
}

fn part_val(p: &MultipartPart) -> Val {
    let mut e = vec![
        ("name", Val::str(&p.name)),
        ("type", Val::str(part_kind_str(p.kind))),
        ("value", part_value(p)),
    ];
    if let Some(ct) = &p.content_type {
        e.push(("contentType", Val::str(ct)));
    }
    disabled(&mut e, p.enabled);
    Val::Map(e)
}

/// A variable as Bruno writes it; a secret one is `secret: true` and a name, never a value.
fn var_val(v: &Variable) -> Val {
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
    disabled(&mut e, v.enabled);
    Val::Map(e)
}

fn raw_kind_str(kind: RawKind) -> &'static str {
    match kind {
        RawKind::Json => "json",
        RawKind::Text => "text",
        RawKind::Xml => "xml",
        RawKind::Sparql => "sparql",
    }
}

/// The `type` of a body in the file.
fn body_type(body: &Body) -> &'static str {
    match body {
        Body::None => "none",
        Body::Raw { kind, .. } => raw_kind_str(*kind),
        Body::FormUrlEncoded { .. } => "form-urlencoded",
        Body::Multipart { .. } => "multipart-form",
        Body::File { .. } => "file",
    }
}

fn file_entry_val(path: &str, content_type: &Option<String>) -> Val {
    let mut e = vec![("filePath", Val::str(path))];
    if let Some(ct) = content_type {
        e.push(("contentType", Val::str(ct)));
    }
    e.push(("selected", Val::bool(true)));
    Val::Map(e)
}

fn body_val(body: &Body) -> Val {
    let data = match body {
        Body::None => return Val::Map(vec![("type", Val::str("none"))]),
        Body::Raw { data, .. } => Val::str(data),
        Body::FormUrlEncoded { fields } => Val::Seq(fields.iter().map(kv_val).collect()),
        Body::Multipart { parts } => Val::Seq(parts.iter().map(part_val).collect()),
        Body::File { path, content_type } => Val::Seq(vec![file_entry_val(path, content_type)]),
    };
    Val::Map(vec![("type", Val::str(body_type(body))), ("data", data)])
}

fn auth_type(auth: &Auth) -> &str {
    match auth {
        Auth::Inherit => "inherit",
        Auth::None => "none",
        Auth::Basic { .. } => "basic",
        Auth::Bearer { .. } => "bearer",
        Auth::ApiKey { .. } => "apikey",
        Auth::Unsupported { kind } => kind,
    }
}

fn placement_str(p: ApiKeyPlacement) -> &'static str {
    match p {
        ApiKeyPlacement::Header => "header",
        ApiKeyPlacement::Query => "query",
    }
}

/// The fields of an auth mapping after `type`, in Bruno's order.
fn auth_fields(auth: &Auth) -> Vec<(&'static str, String)> {
    match auth {
        Auth::Basic { username, password } => {
            vec![
                ("username", username.clone()),
                ("password", password.clone()),
            ]
        }
        Auth::Bearer { token } => vec![("token", token.clone())],
        Auth::ApiKey {
            key,
            value,
            placement,
        } => vec![
            ("key", key.clone()),
            ("value", value.clone()),
            ("placement", placement_str(*placement).to_owned()),
        ],
        _ => Vec::new(),
    }
}

/// The value of `auth`, or `None` for no auth (written as the absence of `auth`).
fn auth_val(auth: &Auth) -> Result<Option<Val>> {
    match auth {
        Auth::None => Ok(None),
        Auth::Inherit => Ok(Some(Val::str("inherit"))),
        Auth::Unsupported { kind } => Err(format!(
            "auth type `{kind}` cannot be written; it can only be kept as it is in the file"
        )),
        _ => {
            let mut e = vec![("type", Val::str(auth_type(auth)))];
            e.extend(auth_fields(auth).into_iter().map(|(k, v)| (k, Val::Str(v))));
            Ok(Some(Val::Map(e)))
        }
    }
}

fn settings_entries(s: &Settings) -> Vec<(&'static str, Val)> {
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
    e
}

// ---------------------------------------------------------------------------------------------
// Editing helpers

/// Sets or removes `disabled` (and a legacy `enabled`) so the item reads as `enabled`.
fn set_enabled(m: &mut Map, enabled: bool, order: &[&str], nl: &str) {
    m.remove("enabled");
    if enabled {
        m.remove("disabled");
    } else {
        m.put("disabled", &Val::bool(true), order, false, nl);
    }
}

/// One step of turning an old list into a new one.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Keep(usize),
    /// Old item, new item: edit the old one in place.
    Edit(usize, usize),
    New(usize),
}

/// Pairs a longest common subsequence of items that are the `same` entry (e.g. have the same
/// name): equal ones are kept, the others edited in place. Between the pairs, old items are
/// edited by position, extra new items added and extra old items dropped.
fn align<T: PartialEq>(old: &[T], new: &[T], same: impl Fn(&T, &T) -> bool) -> Vec<Step> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if same(&old[i], &new[j]) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut matches = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same(&old[i], &new[j]) {
            matches.push((i, j));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    matches.push((n, m));
    let mut steps = Vec::new();
    let (mut a, mut c) = (0, 0);
    for (mi, mj) in matches {
        for k in 0..mj - c {
            steps.push(if k < mi - a {
                Step::Edit(a + k, c + k)
            } else {
                Step::New(c + k)
            });
        }
        if mi < n {
            steps.push(if old[mi] == new[mj] {
                Step::Keep(mi)
            } else {
                Step::Edit(mi, mj)
            });
        }
        a = mi + 1;
        c = mj + 1;
    }
    steps
}

/// How a list is written when it becomes empty.
#[derive(Clone, Copy, PartialEq)]
enum Empty {
    /// Remove the key (Bruno omits empty lists).
    Remove,
    /// `key: []`.
    Flow,
}

struct ListSpec<'f, T> {
    key: &'static str,
    order: &'f [&'f str],
    empty: Empty,
    read: &'f dyn Fn(&Node) -> T,
    /// Whether two items are the same entry, possibly edited (e.g. the same name).
    same: &'f dyn Fn(&T, &T) -> bool,
    render: &'f dyn Fn(&T) -> Val,
    /// Edits an item mapping in place from the old to the new value.
    edit: &'f dyn Fn(&mut Map, &T, &T, &str),
}

/// Makes the list under `spec.key` read as `new`, touching as little as possible.
fn sync_list<T: PartialEq>(map: &mut Map, spec: &ListSpec<'_, T>, new: &[T], nl: &str) {
    let present = map.index(spec.key).is_some();
    let old: Vec<T> = map
        .get_mut(spec.key)
        .map(|n| n.items().map(spec.read).collect())
        .unwrap_or_default();
    if old == new && (present || spec.empty == Empty::Remove) {
        return;
    }
    if new.is_empty() {
        match spec.empty {
            Empty::Remove => {
                map.remove(spec.key);
            }
            Empty::Flow => map.put(spec.key, &Val::Seq(Vec::new()), spec.order, false, nl),
        }
        return;
    }
    let rendered: Vec<Val> = new.iter().map(spec.render).collect();
    let Some(seq) = map.get_mut(spec.key).and_then(Node::block_seq_mut) else {
        map.put(spec.key, &Val::Seq(rendered), spec.order, false, nl);
        return;
    };
    let steps = align(&old, new, spec.same);
    let editable = |i: usize| matches!(seq.items[i].value, Node::Map(ref m) if !m.flow);
    let mut pairs = Vec::new();
    let ops: Vec<ItemOp<'_>> = steps
        .iter()
        .map(|s| match *s {
            Step::Keep(o) => ItemOp::Keep(o),
            Step::Edit(o, n) if editable(o) => {
                pairs.push((o, n));
                ItemOp::Edit(o)
            }
            Step::Edit(_, n) | Step::New(n) => ItemOp::New(&rendered[n]),
        })
        .collect();
    let positions = seq.rebuild(&ops, nl);
    for (pos, (o, n)) in positions.into_iter().zip(pairs) {
        let item = seq.items[pos]
            .value
            .block_map_mut()
            .expect("checked editable");
        (spec.edit)(item, &old[o], &new[n], nl);
    }
}

fn edit_kv(m: &mut Map, old: &KeyValue, new: &KeyValue, nl: &str) {
    if old.name != new.name {
        m.put("name", &Val::str(&new.name), KV_ORDER, false, nl);
    }
    if old.value != new.value {
        m.put("value", &Val::str(&new.value), KV_ORDER, false, nl);
    }
    if old.enabled != new.enabled {
        set_enabled(m, new.enabled, KV_ORDER, nl);
    }
}

fn edit_param(m: &mut Map, old: &Param, new: &Param, nl: &str) {
    if old.name != new.name {
        m.put("name", &Val::str(&new.name), PARAM_ORDER, false, nl);
    }
    if old.value != new.value {
        m.put("value", &Val::str(&new.value), PARAM_ORDER, false, nl);
    }
    if old.kind != new.kind {
        m.put(
            "type",
            &Val::str(kind_str(new.kind)),
            PARAM_ORDER,
            false,
            nl,
        );
    }
    if old.enabled != new.enabled {
        set_enabled(m, new.enabled, PARAM_ORDER, nl);
    }
}

fn edit_part(m: &mut Map, old: &MultipartPart, new: &MultipartPart, nl: &str) {
    if old.name != new.name {
        m.put("name", &Val::str(&new.name), PART_ORDER, false, nl);
    }
    if old.kind != new.kind {
        m.put(
            "type",
            &Val::str(part_kind_str(new.kind)),
            PART_ORDER,
            false,
            nl,
        );
    }
    if old.values != new.values || old.kind != new.kind {
        m.put("value", &part_value(new), PART_ORDER, false, nl);
    }
    if old.content_type != new.content_type {
        match &new.content_type {
            Some(ct) => m.put("contentType", &Val::str(ct), PART_ORDER, false, nl),
            None => {
                m.remove("contentType");
            }
        }
    }
    if old.enabled != new.enabled {
        set_enabled(m, new.enabled, PART_ORDER, nl);
    }
}

fn edit_var(m: &mut Map, old: &Variable, new: &Variable, nl: &str) {
    if new.secret != old.secret {
        if new.secret {
            m.put("secret", &Val::bool(true), VAR_ORDER, false, nl);
        } else {
            m.remove("secret");
        }
    }
    if old.name != new.name {
        m.put("name", &Val::str(&new.name), VAR_ORDER, false, nl);
    }
    if new.secret {
        m.remove("value");
    } else if old.secret || old.value != new.value {
        match &new.value {
            Some(v) => {
                // A typed value (`{type, data}`) keeps its type.
                let typed = m
                    .get_mut("value")
                    .and_then(Node::block_map_mut)
                    .filter(|t| t.index("data").is_some());
                match typed {
                    Some(t) => t.put("data", &Val::str(v), &["type", "data"], false, nl),
                    None => m.put("value", &Val::str(v), VAR_ORDER, false, nl),
                }
            }
            None => {
                m.remove("value");
            }
        }
    }
    if old.enabled != new.enabled {
        set_enabled(m, new.enabled, VAR_ORDER, nl);
    }
}

fn header_list() -> ListSpec<'static, KeyValue> {
    ListSpec {
        key: "headers",
        order: HTTP_ORDER,
        empty: Empty::Remove,
        read: &read::key_value,
        same: &|a: &KeyValue, b: &KeyValue| a.name == b.name,
        render: &kv_val,
        edit: &edit_kv,
    }
}

fn var_list(
    key: &'static str,
    order: &'static [&'static str],
    empty: Empty,
) -> ListSpec<'static, Variable> {
    ListSpec {
        key,
        order,
        empty,
        read: &read::variable,
        same: &|a: &Variable, b: &Variable| a.name == b.name,
        render: &var_val,
        edit: &edit_var,
    }
}

/// Removes a non-empty `value` from every variable marked secret in the list under `key`
/// (edited secret variables lose any `value`, see `edit_var`).
fn strip_secret_values(map: &mut Map, key: &str) {
    let Some(seq) = map.get_mut(key).and_then(Node::block_seq_mut) else {
        return;
    };
    for item in &mut seq.items {
        if let Some(m) = item.value.block_map_mut()
            && m.get("secret").and_then(Node::as_bool) == Some(true)
            && !read::text(m.get("value")).is_empty()
        {
            m.remove("value");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Requests

/// Text of a new request file with name, type, seq, method and URL; the rest is added by
/// [`apply_request`].
pub(crate) fn new_request_text(spec: &RequestSpec) -> String {
    let mut info = vec![("name", Val::str(&spec.name)), ("type", Val::str("http"))];
    if let Some(seq) = spec.seq {
        info.push(("seq", Val::Plain(number(seq))));
    }
    document_text(
        &[
            ("info", Val::Map(info)),
            (
                "http",
                Val::Map(vec![
                    ("method", Val::str(&spec.method)),
                    ("url", Val::str(&spec.url)),
                ]),
            ),
        ],
        true,
    )
}

/// What the file can hold of `spec`: secret variables have no value.
fn normalized(spec: &RequestSpec) -> RequestSpec {
    let mut spec = spec.clone();
    for v in &mut spec.vars {
        if v.secret {
            v.value = None;
        }
    }
    spec
}

fn read_request(doc: &Document, path: &str) -> Option<RequestSpec> {
    let docs: BTreeMap<&str, &Document> = BTreeMap::from([(path, doc)]);
    let mut builder = Builder::new(&docs);
    builder.request(path).map(|r| r.spec)
}

/// Edits the request file `doc` (at `path`) so it reads as `spec`. Returns the edited and
/// re-parsed document; `doc` itself is not changed.
pub(crate) fn apply_request(doc: &Document, path: &str, spec: &RequestSpec) -> Result<Document> {
    let want = normalized(spec);
    let old = read_request(doc, path).ok_or("not an HTTP request")?;
    if old == want {
        return Ok(doc.clone());
    }
    let mut doc = doc.clone();
    let nl = doc.newline;
    let root = doc
        .root
        .block_map_mut()
        .ok_or("the file is not a YAML mapping")?;
    write_info(root, &old, &want, nl);
    write_http(root, path, &old, &want, nl)?;
    write_runtime(root, &old, &want, nl);
    write_settings(root, &old, &want, nl);

    let doc = reparse(&doc)?;
    match read_request(&doc, path) {
        Some(got) if got == want => Ok(doc),
        got => Err(format!(
            "internal error: the edited file does not read back as the request \
             (wanted {want:?}, got {got:?})"
        )),
    }
}

fn write_info(root: &mut Map, old: &RequestSpec, new: &RequestSpec, nl: &str) {
    if old.name == new.name && old.seq == new.seq {
        return;
    }
    let Some(info) = root.get_mut("info").and_then(Node::block_map_mut) else {
        let mut e = vec![("name", Val::str(&new.name)), ("type", Val::str("http"))];
        if let Some(seq) = new.seq {
            e.push(("seq", Val::Plain(number(seq))));
        }
        root.put("info", &Val::Map(e), ROOT_ORDER, true, nl);
        return;
    };
    if old.name != new.name {
        info.put("name", &Val::str(&new.name), INFO_ORDER, false, nl);
    }
    if old.seq != new.seq {
        match new.seq {
            Some(seq) => info.put("seq", &Val::Plain(number(seq)), INFO_ORDER, false, nl),
            None => {
                info.remove("seq");
            }
        }
    }
}

fn http_val(spec: &RequestSpec) -> Result<Val> {
    let mut e = vec![
        ("method", Val::str(&spec.method)),
        ("url", Val::str(&spec.url)),
    ];
    if !spec.params.is_empty() {
        e.push((
            "params",
            Val::Seq(spec.params.iter().map(param_val).collect()),
        ));
    }
    if !spec.headers.is_empty() {
        e.push((
            "headers",
            Val::Seq(spec.headers.iter().map(kv_val).collect()),
        ));
    }
    if spec.body != Body::None {
        e.push(("body", body_val(&spec.body)));
    }
    if let Some(auth) = auth_val(&spec.auth)? {
        e.push(("auth", auth));
    }
    Ok(Val::Map(e))
}

fn write_http(
    root: &mut Map,
    path: &str,
    old: &RequestSpec,
    new: &RequestSpec,
    nl: &str,
) -> Result<()> {
    let unchanged_auth = old.auth == new.auth;
    if let Auth::Unsupported { .. } = &new.auth
        && !unchanged_auth
    {
        auth_val(&new.auth)?;
    }
    let Some(http) = root.get_mut("http").and_then(Node::block_map_mut) else {
        root.put("http", &http_val(new)?, ROOT_ORDER, true, nl);
        return Ok(());
    };
    if old.method != new.method {
        http.put("method", &Val::str(&new.method), HTTP_ORDER, false, nl);
    }
    if old.url != new.url {
        http.put("url", &Val::str(&new.url), HTTP_ORDER, false, nl);
    }
    sync_list(
        http,
        &ListSpec {
            key: "params",
            order: HTTP_ORDER,
            empty: Empty::Remove,
            read: &read::param,
            same: &|a: &Param, b: &Param| a.name == b.name && a.kind == b.kind,
            render: &param_val,
            edit: &edit_param,
        },
        &new.params,
        nl,
    );
    sync_list(http, &header_list(), &new.headers, nl);
    if old.body != new.body {
        write_body(http, path, new, nl);
    }
    if !unchanged_auth {
        write_auth(http, &old.auth, &new.auth, nl)?;
    }
    Ok(())
}

fn write_body(http: &mut Map, path: &str, new: &RequestSpec, nl: &str) {
    // A list of body variants: edit the selected one.
    if let Some(seq) = http.get_mut("body").and_then(Node::block_seq_mut) {
        let selected = seq
            .items
            .iter()
            .position(|i| read::flag(i.value.get("selected")) == Some(true))
            .unwrap_or(0);
        if let Some(variant) = seq
            .items
            .get_mut(selected)
            .and_then(|i| i.value.block_map_mut())
        {
            let old = Builder::new(&BTreeMap::new()).body(path, variant.get("body"));
            write_body_in(variant, &old, &new.body, true, nl);
            return;
        }
    }
    let old = Builder::new(&BTreeMap::new()).body(path, http.get("body"));
    write_body_in(http, &old, &new.body, false, nl);
}

/// Writes `new` as the `body` of `map` (the `http` mapping or a body variant).
fn write_body_in(map: &mut Map, old: &Body, new: &Body, variant: bool, nl: &str) {
    if old == new {
        return;
    }
    if *new == Body::None && !variant {
        map.remove("body");
        return;
    }
    let order = if variant {
        &["title", "selected", "body"][..]
    } else {
        HTTP_ORDER
    };
    let same_type = map
        .get("body")
        .and_then(|b| b.get("type"))
        .and_then(Node::text)
        == Some(body_type(new));
    let body = map
        .get_mut("body")
        .and_then(Node::block_map_mut)
        .filter(|_| same_type && *new != Body::None);
    let Some(body) = body else {
        map.put("body", &body_val(new), order, false, nl);
        return;
    };
    match new {
        Body::None => {}
        Body::Raw { data, .. } => body.put("data", &Val::str(data), BODY_ORDER, false, nl),
        Body::FormUrlEncoded { fields } => sync_list(
            body,
            &ListSpec {
                key: "data",
                order: BODY_ORDER,
                empty: Empty::Flow,
                read: &read::key_value,
                same: &|a: &KeyValue, b: &KeyValue| a.name == b.name,
                render: &kv_val,
                edit: &edit_kv,
            },
            fields,
            nl,
        ),
        Body::Multipart { parts } => sync_list(
            body,
            &ListSpec {
                key: "data",
                order: BODY_ORDER,
                empty: Empty::Flow,
                read: &read::part,
                same: &|a: &MultipartPart, b: &MultipartPart| a.name == b.name,
                render: &part_val,
                edit: &edit_part,
            },
            parts,
            nl,
        ),
        Body::File { path, content_type } => {
            let entry = body
                .get_mut("data")
                .and_then(Node::block_seq_mut)
                .and_then(|seq| {
                    let selected = seq
                        .items
                        .iter()
                        .position(|i| read::flag(i.value.get("selected")) == Some(true))
                        .unwrap_or(0);
                    seq.items.get_mut(selected)?.value.block_map_mut()
                });
            let Some(entry) = entry else {
                body.put(
                    "data",
                    &Val::Seq(vec![file_entry_val(path, content_type)]),
                    BODY_ORDER,
                    false,
                    nl,
                );
                return;
            };
            if read::text(entry.get("filePath")) != *path {
                entry.put("filePath", &Val::str(path), FILE_ORDER, false, nl);
            }
            let old_ct = entry
                .get("contentType")
                .and_then(Node::text)
                .map(str::to_owned);
            if old_ct != *content_type {
                match content_type {
                    Some(ct) => entry.put("contentType", &Val::str(ct), FILE_ORDER, false, nl),
                    None => {
                        entry.remove("contentType");
                    }
                }
            }
        }
    }
}

fn write_auth(http: &mut Map, old: &Auth, new: &Auth, nl: &str) -> Result<()> {
    let Some(val) = auth_val(new)? else {
        http.remove("auth");
        return Ok(());
    };
    let same_kind = std::mem::discriminant(old) == std::mem::discriminant(new)
        && matches!(
            new,
            Auth::Basic { .. } | Auth::Bearer { .. } | Auth::ApiKey { .. }
        );
    let existing = http
        .get_mut("auth")
        .and_then(Node::block_map_mut)
        .filter(|m| m.index("type").is_some());
    match existing {
        Some(auth) if same_kind => {
            let old_fields = auth_fields(old);
            for ((key, value), (_, old_value)) in auth_fields(new).into_iter().zip(old_fields) {
                if value != old_value {
                    auth.put(key, &Val::Str(value), AUTH_ORDER, false, nl);
                }
            }
        }
        _ => http.put("auth", &val, HTTP_ORDER, false, nl),
    }
    Ok(())
}

fn write_runtime(root: &mut Map, old: &RequestSpec, new: &RequestSpec, nl: &str) {
    if old.vars == new.vars {
        return;
    }
    let list = var_list("variables", RUNTIME_ORDER, Empty::Remove);
    match root.get_mut("runtime").and_then(Node::block_map_mut) {
        Some(runtime) => {
            sync_list(runtime, &list, &new.vars, nl);
            strip_secret_values(runtime, "variables");
            if runtime.is_empty() {
                root.remove("runtime");
            }
        }
        None if new.vars.is_empty() => {}
        None => root.put(
            "runtime",
            &Val::Map(vec![(
                "variables",
                Val::Seq(new.vars.iter().map(var_val).collect()),
            )]),
            ROOT_ORDER,
            true,
            nl,
        ),
    }
}

fn write_settings(root: &mut Map, old: &RequestSpec, new: &RequestSpec, nl: &str) {
    let (old, new) = (&old.settings, &new.settings);
    if old == new {
        return;
    }
    let Some(settings) = root.get_mut("settings").and_then(Node::block_map_mut) else {
        let entries = settings_entries(new);
        if entries.is_empty() {
            root.remove("settings");
        } else {
            root.put("settings", &Val::Map(entries), ROOT_ORDER, true, nl);
        }
        return;
    };
    let fields: [(&'static str, Option<String>, Option<String>); 4] = [
        (
            "encodeUrl",
            old.encode_url.map(|b| b.to_string()),
            new.encode_url.map(|b| b.to_string()),
        ),
        (
            "timeout",
            old.timeout_ms.map(|t| t.to_string()),
            new.timeout_ms.map(|t| t.to_string()),
        ),
        (
            "followRedirects",
            old.follow_redirects.map(|b| b.to_string()),
            new.follow_redirects.map(|b| b.to_string()),
        ),
        (
            "maxRedirects",
            old.max_redirects.map(|m| m.to_string()),
            new.max_redirects.map(|m| m.to_string()),
        ),
    ];
    for (key, o, n) in fields {
        if o == n {
            continue;
        }
        match n {
            Some(raw) => settings.put(key, &Val::Plain(raw), SETTINGS_ORDER, false, nl),
            None => {
                settings.remove(key);
            }
        }
    }
    if settings.is_empty() {
        root.remove("settings");
    }
}

// ---------------------------------------------------------------------------------------------
// Environments

/// What the file can hold of `env`: secret variables have no value.
pub(crate) fn normalized_env(env: &Environment) -> Environment {
    let mut env = env.clone();
    for v in &mut env.variables {
        if v.secret {
            v.value = None;
        }
    }
    env
}

/// Text of a new environment file.
pub(crate) fn new_environment_text(name: &str) -> String {
    document_text(
        &[
            ("name", Val::str(name)),
            ("variables", Val::Seq(Vec::new())),
        ],
        false,
    )
}

/// Edits the environment mapping `env_map` so it reads as `env`. `fallback_name` is the name
/// the reader uses when the mapping has none (the file stem).
pub(crate) fn apply_environment(
    env_map: &mut Map,
    env: &Environment,
    fallback_name: &str,
    nl: &str,
) {
    let old_name = env_map
        .get("name")
        .and_then(Node::text)
        .unwrap_or(fallback_name)
        .to_owned();
    if old_name != env.name {
        env_map.put("name", &Val::str(&env.name), ENV_ORDER, false, nl);
    }
    sync_list(
        env_map,
        &var_list("variables", ENV_ORDER, Empty::Flow),
        &env.variables,
        nl,
    );
    // Whatever the file held before: a secret's value never stays in it.
    strip_secret_values(env_map, "variables");
}

/// Edits a whole environment file and checks that it reads back as `env`.
pub(crate) fn apply_environment_doc(
    doc: &Document,
    env: &Environment,
    fallback_name: &str,
) -> Result<Document> {
    let mut doc = match doc.root {
        Node::Map(ref m) if !m.flow => doc.clone(),
        _ => parse(&new_environment_text(&env.name)).map_err(|e| e.to_string())?,
    };
    let nl = doc.newline;
    let root = doc.root.block_map_mut().expect("checked above");
    apply_environment(root, env, fallback_name, nl);
    let doc = reparse(&doc)?;
    check_environment(&doc.root, env, fallback_name)?;
    Ok(doc)
}

/// Checks that `node` reads as the environment `env`.
pub(crate) fn check_environment(node: &Node, env: &Environment, fallback_name: &str) -> Result<()> {
    let want = normalized_env(env);
    let got = read::environment(node, fallback_name);
    if got == want {
        Ok(())
    } else {
        Err(format!(
            "internal error: the edited environment does not read back as written \
             (wanted {want:?}, got {got:?})"
        ))
    }
}
