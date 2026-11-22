// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: websocket` items: `websocket.{url, headers, message, auth}` and `settings.{timeout, keepAliveInterval}` (message variants). Wave 8, strand A.
//!
//! Mapping (OpenCollection `WebSocketRequest`):
//! - `info.{name, seq}` and `websocket.{url, headers, auth}` as for HTTP items; `method` is
//!   `WS` (display only), no params, no body, no HTTP settings.
//! - `websocket.message` is either one message `{type, data}` (read as one variant with an
//!   empty title, not selected) or a list of variants `{title, selected, message}`. The writer
//!   keeps the single form for exactly one untitled, unselected variant, the list otherwise.
//!   Unknown message types read as text.
//! - `settings.timeout` and `settings.keepAliveInterval`: numbers (ms); `inherit` or a missing
//!   key is `None`.
//! - `runtime.{variables, scripts}`; WebSocket items have no assertions or actions.
//! - `info.{description, tags}`, `docs` and anything else stay in the file as they are.

use rt_core::model::{
    Auth, Body, MessageKind, MessageVariant, Protocol, RequestEntry, RequestSpec, Settings,
    WebSocketSpec,
};

use super::Result;
use crate::read::{self, Builder};
use crate::write::{
    self, Empty, ListSpec, auth_val, header_list, number, strip_secret_values, sync_list, var_list,
    var_val,
};
use crate::yaml::{Document, Map, Node, Val, document_text, reparse};

const ROOT_ORDER: &[&str] = &["info", "websocket", "runtime", "settings", "docs"];
const INFO_ORDER: &[&str] = &["name", "description", "type", "seq", "tags"];
const WS_ORDER: &[&str] = &["url", "headers", "message", "auth"];
const VARIANT_ORDER: &[&str] = &["title", "selected", "message"];
const MESSAGE_ORDER: &[&str] = &["type", "data"];
const RUNTIME_ORDER: &[&str] = &["variables", "scripts"];
const SETTINGS_ORDER: &[&str] = &["timeout", "keepAliveInterval"];

/// The method shown for WebSocket requests.
const METHOD: &str = "WS";

pub(crate) fn read(builder: &mut Builder<'_>, path: &str) -> Option<RequestEntry> {
    let root = builder.root(path)?;
    let info = root.get("info");
    let details = root.get("websocket");
    let field = |k: &str| details.and_then(|d| d.get(k));
    let runtime = root.get("runtime");
    let settings = root.get("settings");
    let name = info
        .and_then(|i| i.get("name"))
        .and_then(Node::text)
        .unwrap_or_else(|| read::file_stem(path))
        .to_owned();
    let spec = RequestSpec {
        name,
        seq: info.and_then(|i| i.get("seq")).and_then(Node::as_f64),
        method: METHOD.to_owned(),
        url: read::text(field("url")),
        headers: read::key_values(field("headers")),
        params: Vec::new(),
        body: Body::None,
        auth: builder.auth(path, field("auth"), Auth::None),
        vars: read::variables(runtime.and_then(|r| r.get("variables"))),
        settings: Settings::default(),
        scripts: crate::runtime::scripts(runtime.and_then(|r| r.get("scripts"))),
        assertions: Vec::new(),
        actions: Vec::new(),
        protocol: Protocol::WebSocket(Box::new(WebSocketSpec {
            messages: messages(field("message")),
            timeout_ms: millis(settings.and_then(|s| s.get("timeout"))),
            keep_alive_ms: millis(settings.and_then(|s| s.get("keepAliveInterval"))),
        })),
    };
    Some(RequestEntry {
        path: path.to_owned(),
        spec,
    })
}

/// A number of milliseconds; `inherit`, anything else or nothing is `None`.
fn millis(node: Option<&Node>) -> Option<u64> {
    node.and_then(Node::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| n as u64)
}

fn kind(node: Option<&Node>) -> MessageKind {
    match node.and_then(Node::text) {
        Some("json") => MessageKind::Json,
        Some("xml") => MessageKind::Xml,
        Some("binary") => MessageKind::Binary,
        _ => MessageKind::Text,
    }
}

fn kind_str(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::Text => "text",
        MessageKind::Json => "json",
        MessageKind::Xml => "xml",
        MessageKind::Binary => "binary",
    }
}

fn messages(node: Option<&Node>) -> Vec<MessageVariant> {
    match node {
        Some(n) if n.is_seq() => n.items().map(variant).collect(),
        Some(n) if n.is_map() => vec![MessageVariant {
            title: String::new(),
            selected: false,
            kind: kind(n.get("type")),
            data: read::text(n.get("data")),
        }],
        _ => Vec::new(),
    }
}

fn variant(node: &Node) -> MessageVariant {
    let message = node.get("message");
    MessageVariant {
        title: read::text(node.get("title")),
        selected: read::flag(node.get("selected")) == Some(true),
        kind: kind(message.and_then(|m| m.get("type"))),
        data: read::text(message.and_then(|m| m.get("data"))),
    }
}

/// Whether the messages are written as one `{type, data}` rather than a list of variants.
fn single(messages: &[MessageVariant]) -> bool {
    matches!(messages, [m] if m.title.is_empty() && !m.selected)
}

fn message_val(m: &MessageVariant) -> Val {
    Val::Map(vec![
        ("type", Val::str(kind_str(m.kind))),
        ("data", Val::str(&m.data)),
    ])
}

fn variant_val(m: &MessageVariant) -> Val {
    let mut e = vec![("title", Val::str(&m.title))];
    if m.selected {
        e.push(("selected", Val::bool(true)));
    }
    e.push(("message", message_val(m)));
    Val::Map(e)
}

fn messages_val(messages: &[MessageVariant]) -> Val {
    if single(messages) {
        message_val(&messages[0])
    } else {
        Val::Seq(messages.iter().map(variant_val).collect())
    }
}

fn spec_of(spec: &RequestSpec) -> &WebSocketSpec {
    match &spec.protocol {
        Protocol::WebSocket(ws) => ws,
        _ => unreachable!("only WebSocket specs are written here"),
    }
}

/// Text of a new file: name, type, seq and URL; the rest is added by [`apply`].
pub(crate) fn new_text(spec: &RequestSpec) -> String {
    let mut info = vec![
        ("name", Val::str(&spec.name)),
        ("type", Val::str("websocket")),
    ];
    if let Some(seq) = spec.seq {
        info.push(("seq", Val::Plain(number(seq))));
    }
    document_text(
        &[
            ("info", Val::Map(info)),
            ("websocket", Val::Map(vec![("url", Val::str(&spec.url))])),
        ],
        true,
    )
}

/// What a WebSocket item cannot hold, as an error naming the field.
fn check(spec: &RequestSpec) -> Result<()> {
    let refuse = |what: &str| Err(format!("a WebSocket request has no {what}"));
    if spec.method != METHOD {
        return Err(format!(
            "a WebSocket request has no method (it is always {METHOD}, not {})",
            spec.method
        ));
    }
    if !spec.params.is_empty() {
        return refuse("params (put the query into the URL)");
    }
    if spec.body != Body::None {
        return refuse("body (messages are sent in the session)");
    }
    if !spec.assertions.is_empty() {
        return refuse("assertions");
    }
    if !spec.actions.is_empty() {
        return refuse("actions");
    }
    if spec.settings != Settings::default() {
        return refuse("HTTP settings (only timeout and keep-alive interval)");
    }
    Ok(())
}

fn read_back(doc: &Document, path: &str) -> Option<RequestSpec> {
    let docs = std::collections::BTreeMap::from([(path, doc)]);
    Builder::new(&docs).request(path).map(|r| r.spec)
}

/// Edits the WebSocket item `doc` so it reads as `spec` (already normalized: secret
/// variables without value). Unchanged parts stay byte-identical; the result must read back
/// as `spec`.
pub(crate) fn apply(doc: &Document, path: &str, spec: &RequestSpec) -> Result<Document> {
    check(spec)?;
    let old = read_back(doc, path).ok_or("not a request")?;
    if !matches!(old.protocol, Protocol::WebSocket(_)) {
        return Err("the file is not a WebSocket request; its type cannot be changed".into());
    }
    if old == *spec {
        return Ok(doc.clone());
    }
    if let Auth::Unsupported { .. } = &spec.auth
        && old.auth != spec.auth
    {
        auth_val(&spec.auth)?;
    }
    let mut doc = doc.clone();
    let nl = doc.newline;
    let root = doc
        .root
        .block_map_mut()
        .ok_or("the file is not a YAML mapping")?;
    write_info(root, &old, spec, nl);
    write_details(root, &old, spec, nl)?;
    write_runtime(root, &old, spec, nl)?;
    write_settings(root, spec_of(&old), spec_of(spec), nl);

    let doc = reparse(&doc)?;
    match read_back(&doc, path) {
        Some(got) if got == *spec => Ok(doc),
        got => Err(format!(
            "internal error: the edited file does not read back as the request \
             (wanted {spec:?}, got {got:?})"
        )),
    }
}

fn write_info(root: &mut Map, old: &RequestSpec, new: &RequestSpec, nl: &str) {
    if old.name == new.name && old.seq == new.seq {
        return;
    }
    let Some(info) = root.get_mut("info").and_then(Node::block_map_mut) else {
        let mut e = vec![
            ("name", Val::str(&new.name)),
            ("type", Val::str("websocket")),
        ];
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

fn details_val(spec: &RequestSpec) -> Result<Val> {
    let mut e = vec![("url", Val::str(&spec.url))];
    if !spec.headers.is_empty() {
        e.push((
            "headers",
            Val::Seq(spec.headers.iter().map(write::kv_val).collect()),
        ));
    }
    let messages = &spec_of(spec).messages;
    if !messages.is_empty() {
        e.push(("message", messages_val(messages)));
    }
    if let Some(auth) = auth_val(&spec.auth)? {
        e.push(("auth", auth));
    }
    Ok(Val::Map(e))
}

fn write_details(root: &mut Map, old: &RequestSpec, new: &RequestSpec, nl: &str) -> Result<()> {
    let Some(details) = root.get_mut("websocket").and_then(Node::block_map_mut) else {
        root.put("websocket", &details_val(new)?, ROOT_ORDER, true, nl);
        return Ok(());
    };
    if old.url != new.url {
        details.put("url", &Val::str(&new.url), WS_ORDER, false, nl);
    }
    sync_list(
        details,
        &ListSpec {
            order: WS_ORDER,
            ..header_list()
        },
        &new.headers,
        nl,
    );
    let (old_ws, new_ws) = (spec_of(old), spec_of(new));
    if old_ws.messages != new_ws.messages {
        write_messages(details, &old_ws.messages, &new_ws.messages, nl);
    }
    if old.auth != new.auth {
        match auth_val(&new.auth)? {
            Some(val) if details.index("auth").is_none() => {
                details.put("auth", &val, WS_ORDER, false, nl);
            }
            _ => write::write_auth(details, &old.auth, &new.auth, nl)?,
        }
    }
    Ok(())
}

fn write_messages(details: &mut Map, old: &[MessageVariant], new: &[MessageVariant], nl: &str) {
    if new.is_empty() {
        details.remove("message");
        return;
    }
    if single(new) {
        // One message: edit `{type, data}` in place when the file has that form.
        if let Some(message) = details.get_mut("message").and_then(Node::block_map_mut)
            && single(old)
        {
            edit_message(message, &old[0], &new[0], nl);
            return;
        }
        details.put("message", &messages_val(new), WS_ORDER, false, nl);
        return;
    }
    let is_list = details.get("message").is_some_and(Node::is_seq);
    if !is_list {
        details.put("message", &messages_val(new), WS_ORDER, false, nl);
        return;
    }
    sync_list(
        details,
        &ListSpec {
            key: "message",
            order: WS_ORDER,
            empty: Empty::Remove,
            read: &variant,
            same: &|a: &MessageVariant, b: &MessageVariant| a.title == b.title,
            render: &variant_val,
            edit: &edit_variant,
        },
        new,
        nl,
    );
}

/// Edits `{type, data}` of one message in place.
fn edit_message(message: &mut Map, old: &MessageVariant, new: &MessageVariant, nl: &str) {
    if old.kind != new.kind {
        message.put(
            "type",
            &Val::str(kind_str(new.kind)),
            MESSAGE_ORDER,
            false,
            nl,
        );
    }
    if old.data != new.data {
        message.put("data", &Val::str(&new.data), MESSAGE_ORDER, false, nl);
    }
}

fn edit_variant(m: &mut Map, old: &MessageVariant, new: &MessageVariant, nl: &str) {
    if old.title != new.title {
        m.put("title", &Val::str(&new.title), VARIANT_ORDER, false, nl);
    }
    if old.selected != new.selected {
        if new.selected {
            m.put("selected", &Val::bool(true), VARIANT_ORDER, false, nl);
        } else {
            m.remove("selected");
        }
    }
    if old.kind != new.kind || old.data != new.data {
        match m.get_mut("message").and_then(Node::block_map_mut) {
            Some(message) => edit_message(message, old, new, nl),
            None => m.put("message", &message_val(new), VARIANT_ORDER, false, nl),
        }
    }
}

/// `runtime.{variables, scripts}`.
fn write_runtime(root: &mut Map, old: &RequestSpec, new: &RequestSpec, nl: &str) -> Result<()> {
    let vars = old.vars != new.vars;
    let scripts = old.scripts != new.scripts;
    if !(vars || scripts) {
        return Ok(());
    }
    let fixed = root
        .get("runtime")
        .is_some_and(|n| crate::runtime::has_content(n) && !matches!(n, Node::Map(m) if !m.flow));
    match root.get_mut("runtime").and_then(Node::block_map_mut) {
        Some(runtime) => {
            if vars {
                sync_list(
                    runtime,
                    &var_list("variables", RUNTIME_ORDER, Empty::Remove),
                    &new.vars,
                    nl,
                );
                strip_secret_values(runtime, "variables");
            }
            if scripts {
                crate::runtime::write_scripts(runtime, RUNTIME_ORDER, &new.scripts, nl)?;
            }
            if runtime.is_empty() {
                root.remove("runtime");
            }
        }
        None if fixed => {
            return Err("`runtime` is not a block mapping; it cannot be edited".into());
        }
        None => {
            let mut entries = Vec::new();
            if !new.vars.is_empty() {
                entries.push((
                    "variables",
                    Val::Seq(new.vars.iter().map(var_val).collect()),
                ));
            }
            entries.extend(crate::runtime::scripts_val(&new.scripts).map(|v| ("scripts", v)));
            if !entries.is_empty() {
                root.put("runtime", &Val::Map(entries), ROOT_ORDER, true, nl);
            }
        }
    }
    Ok(())
}

/// `settings.{timeout, keepAliveInterval}`; `None` removes the key (inherit).
fn write_settings(root: &mut Map, old: &WebSocketSpec, new: &WebSocketSpec, nl: &str) {
    let fields = [
        ("timeout", old.timeout_ms, new.timeout_ms),
        ("keepAliveInterval", old.keep_alive_ms, new.keep_alive_ms),
    ];
    if fields.iter().all(|(_, o, n)| o == n) {
        return;
    }
    let Some(settings) = root.get_mut("settings").and_then(Node::block_map_mut) else {
        let entries: Vec<(&'static str, Val)> = fields
            .iter()
            .filter_map(|(k, _, n)| n.map(|n| (*k, Val::Plain(n.to_string()))))
            .collect();
        if entries.is_empty() {
            root.remove("settings");
        } else {
            root.put("settings", &Val::Map(entries), ROOT_ORDER, true, nl);
        }
        return;
    };
    for (key, o, n) in fields {
        if o == n {
            continue;
        }
        match n {
            Some(ms) => settings.put(key, &Val::Plain(ms.to_string()), SETTINGS_ORDER, false, nl),
            None => {
                settings.remove(key);
            }
        }
    }
    if settings.is_empty() {
        root.remove("settings");
    }
}
