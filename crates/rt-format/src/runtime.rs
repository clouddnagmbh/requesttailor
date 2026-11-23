// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts, assertions and actions (wave 7): `runtime.scripts`, `runtime.assertions` and
//! `runtime.actions` of request files, `request.scripts` of `opencollection.yml` and
//! `folder.yml`.
//!
//! Reading and writing both live here. Writing edits only what changed, like the rest of the
//! `write` module: unchanged blocks stay byte-identical, edited items keep their place and the
//! fields the model does not hold. Script entries of phases RequestTailor does not run (the
//! gRPC ones, `grpc:before-call-start`, ...) and actions other than `set-variable` are not
//! part of the model; they stay in the file untouched, in their place among the others.

use rt_core::model::{Assertion, Folder, Item, Script, ScriptPhase, VarAction, VarScope};
use rt_core::script::phase_name;

use crate::read::{enabled, text};
use crate::write::{self, Empty, ListSpec, Step, set_enabled, sync_list};
use crate::yaml::{Document, ItemOp, Map, Node, Val, parse, reparse};
use crate::{COLLECTION_FILE, CollectionDoc, FOLDER_FILE, FormatError, Source};

type Result<T> = std::result::Result<T, String>;

const SCRIPT_ORDER: &[&str] = &["type", "code"];
const ASSERTION_ORDER: &[&str] = &["expression", "operator", "value", "description", "disabled"];
const ACTION_ORDER: &[&str] = &["type", "phase", "selector", "variable", "disabled"];
const SELECTOR_ORDER: &[&str] = &["expression", "method"];
const VARIABLE_ORDER: &[&str] = &["name", "scope"];
/// Keys of `request` in `opencollection.yml` and `folder.yml`.
pub(crate) const DEFAULTS_ORDER: &[&str] = &["auth", "headers", "variables", "scripts", "settings"];
const COLLECTION_ORDER: &[&str] = &[
    "opencollection",
    "info",
    "config",
    "request",
    "docs",
    "bundled",
    "extensions",
];
const FOLDER_ORDER: &[&str] = &["info", "request", "docs"];

// ---------------------------------------------------------------------------------------------
// Reading

/// The phase of an OpenCollection script `type`; `None` for types RequestTailor does not run
/// (gRPC phases), which stay in the file untouched.
pub(crate) fn phase(kind: &str) -> Option<ScriptPhase> {
    match kind {
        "before-request" => Some(ScriptPhase::BeforeRequest),
        "after-response" => Some(ScriptPhase::AfterResponse),
        "tests" => Some(ScriptPhase::Tests),
        _ => None,
    }
}

/// One script entry; `None` for a phase RequestTailor does not run (or no `type`).
fn script(s: &Node) -> Option<Script> {
    let phase = phase(s.get("type").and_then(Node::text)?)?;
    Some(Script {
        phase,
        code: text(s.get("code")),
    })
}

pub(crate) fn scripts(list: Option<&Node>) -> Vec<Script> {
    list.map(|l| l.items().filter_map(script).collect())
        .unwrap_or_default()
}

fn assertion(a: &Node) -> Assertion {
    Assertion {
        expression: text(a.get("expression")),
        operator: text(a.get("operator")),
        value: text(a.get("value")),
        description: a.get("description").and_then(Node::text).map(str::to_owned),
        enabled: enabled(a),
    }
}

pub(crate) fn assertions(list: Option<&Node>) -> Vec<Assertion> {
    list.map(|l| l.items().map(assertion).collect())
        .unwrap_or_default()
}

fn scope_of(name: Option<&str>) -> VarScope {
    match name {
        Some("environment") => VarScope::Environment,
        Some("collection") => VarScope::Collection,
        Some("global") => VarScope::Global,
        _ => VarScope::Runtime,
    }
}

fn scope_name(scope: VarScope) -> &'static str {
    match scope {
        VarScope::Runtime => "runtime",
        VarScope::Environment => "environment",
        VarScope::Collection => "collection",
        VarScope::Global => "global",
    }
}

/// A `set-variable` action; `None` for other action types (kept in the file, not modelled).
fn action(a: &Node) -> Option<VarAction> {
    if a.get("type").and_then(Node::text) != Some("set-variable") {
        return None;
    }
    let selector = a.get("selector");
    let variable = a.get("variable");
    Some(VarAction {
        phase: a
            .get("phase")
            .and_then(Node::text)
            .and_then(phase)
            .unwrap_or(ScriptPhase::AfterResponse),
        expression: text(selector.and_then(|s| s.get("expression"))),
        method: text(selector.and_then(|s| s.get("method"))),
        variable: text(variable.and_then(|v| v.get("name"))),
        scope: scope_of(variable.and_then(|v| v.get("scope")).and_then(Node::text)),
        enabled: enabled(a),
    })
}

/// `set-variable` actions; other action types are kept in the file, not modelled.
pub(crate) fn actions(list: Option<&Node>) -> Vec<VarAction> {
    list.map(|l| l.items().filter_map(action).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Rendering new entries (also for the importers' writer of new files)

fn script_val(s: &Script) -> Val {
    Val::Map(vec![
        ("type", Val::str(phase_name(s.phase))),
        ("code", Val::str(&s.code)),
    ])
}

fn assertion_val(a: &Assertion) -> Val {
    let mut e = vec![
        ("expression", Val::str(&a.expression)),
        ("operator", Val::str(&a.operator)),
    ];
    if !a.value.is_empty() {
        e.push(("value", Val::str(&a.value)));
    }
    if let Some(d) = &a.description {
        e.push(("description", Val::str(d)));
    }
    if !a.enabled {
        e.push(("disabled", Val::bool(true)));
    }
    Val::Map(e)
}

fn selector_val(a: &VarAction) -> Val {
    Val::Map(vec![
        ("expression", Val::str(&a.expression)),
        ("method", Val::str(&a.method)),
    ])
}

fn variable_val(a: &VarAction) -> Val {
    Val::Map(vec![
        ("name", Val::str(&a.variable)),
        ("scope", Val::str(scope_name(a.scope))),
    ])
}

fn action_val(a: &VarAction) -> Val {
    let mut e = vec![
        ("type", Val::str("set-variable")),
        ("phase", Val::str(phase_name(a.phase))),
        ("selector", selector_val(a)),
        ("variable", variable_val(a)),
    ];
    if !a.enabled {
        e.push(("disabled", Val::bool(true)));
    }
    Val::Map(e)
}

/// `scripts` of a new file (`runtime.scripts` of a request, `request.scripts` of a collection
/// or folder); `None` when empty.
pub(crate) fn scripts_val(scripts: &[Script]) -> Option<Val> {
    (!scripts.is_empty()).then(|| Val::Seq(scripts.iter().map(script_val).collect()))
}

/// `runtime.assertions` of a new request file; `None` when empty.
pub(crate) fn assertions_val(assertions: &[Assertion]) -> Option<Val> {
    (!assertions.is_empty()).then(|| Val::Seq(assertions.iter().map(assertion_val).collect()))
}

/// `runtime.actions` of a new request file; `None` when empty.
pub(crate) fn actions_val(actions: &[VarAction]) -> Option<Val> {
    (!actions.is_empty()).then(|| Val::Seq(actions.iter().map(action_val).collect()))
}

// ---------------------------------------------------------------------------------------------
// Editing

fn edit_script(m: &mut Map, old: &Script, new: &Script, nl: &str) {
    if old.phase != new.phase {
        m.put(
            "type",
            &Val::str(phase_name(new.phase)),
            SCRIPT_ORDER,
            false,
            nl,
        );
    }
    if old.code != new.code {
        m.put("code", &Val::str(&new.code), SCRIPT_ORDER, false, nl);
    }
}

fn edit_assertion(m: &mut Map, old: &Assertion, new: &Assertion, nl: &str) {
    let mut set = |key: &'static str, old: &str, new: &str| {
        if old != new {
            m.put(key, &Val::str(new), ASSERTION_ORDER, false, nl);
        }
    };
    set("expression", &old.expression, &new.expression);
    set("operator", &old.operator, &new.operator);
    if old.value != new.value {
        if new.value.is_empty() {
            m.remove("value");
        } else {
            m.put("value", &Val::str(&new.value), ASSERTION_ORDER, false, nl);
        }
    }
    if old.description != new.description {
        match &new.description {
            Some(d) => m.put("description", &Val::str(d), ASSERTION_ORDER, false, nl),
            None => {
                m.remove("description");
            }
        }
    }
    if old.enabled != new.enabled {
        set_enabled(m, new.enabled, ASSERTION_ORDER, nl);
    }
}

/// Edits the mapping under `key` field by field (`(field, old, new)`), or writes `whole` if
/// it is not a block mapping.
fn edit_sub(
    m: &mut Map,
    key: &'static str,
    order: &[&str],
    fields: &[(&'static str, &str, &str)],
    whole: &Val,
    nl: &str,
) {
    match m.get_mut(key).and_then(Node::block_map_mut) {
        Some(sub) => {
            for (k, old, new) in fields {
                if old != new {
                    sub.put(k, &Val::str(*new), order, false, nl);
                }
            }
        }
        None => m.put(key, whole, ACTION_ORDER, false, nl),
    }
}

fn edit_action(m: &mut Map, old: &VarAction, new: &VarAction, nl: &str) {
    if old.phase != new.phase {
        m.put(
            "phase",
            &Val::str(phase_name(new.phase)),
            ACTION_ORDER,
            false,
            nl,
        );
    }
    if old.expression != new.expression || old.method != new.method {
        let fields = [
            (
                "expression",
                old.expression.as_str(),
                new.expression.as_str(),
            ),
            ("method", old.method.as_str(), new.method.as_str()),
        ];
        edit_sub(
            m,
            "selector",
            SELECTOR_ORDER,
            &fields,
            &selector_val(new),
            nl,
        );
    }
    if old.variable != new.variable || old.scope != new.scope {
        let fields = [
            ("name", old.variable.as_str(), new.variable.as_str()),
            ("scope", scope_name(old.scope), scope_name(new.scope)),
        ];
        edit_sub(
            m,
            "variable",
            VARIABLE_ORDER,
            &fields,
            &variable_val(new),
            nl,
        );
    }
    if old.enabled != new.enabled {
        set_enabled(m, new.enabled, ACTION_ORDER, nl);
    }
}

/// A list whose items are partly modelled (`read` gives `Some`) and partly not (kept as they
/// are, in their place).
struct Mixed<'f, T> {
    key: &'static str,
    order: &'f [&'f str],
    read: &'f dyn Fn(&Node) -> Option<T>,
    /// Whether two items are the same entry, possibly edited.
    same: &'f dyn Fn(&T, &T) -> bool,
    render: &'f dyn Fn(&T) -> Val,
    edit: &'f dyn Fn(&mut Map, &T, &T, &str),
}

/// Makes the modelled items of the list under `spec.key` read as `new`, touching as little
/// as possible. Unmodelled items stay untouched and keep their position relative to the
/// modelled items around them.
fn sync_mixed<T: PartialEq>(map: &mut Map, spec: &Mixed<'_, T>, new: &[T], nl: &str) -> Result<()> {
    let mut known: Vec<(usize, T)> = Vec::new();
    let mut foreign: Vec<usize> = Vec::new();
    if let Some(list) = map.get(spec.key) {
        for (i, item) in list.items().enumerate() {
            match (spec.read)(item) {
                Some(t) => known.push((i, t)),
                None => foreign.push(i),
            }
        }
    }
    let (at, old): (Vec<usize>, Vec<T>) = known.into_iter().unzip();
    if old == new {
        return Ok(());
    }
    if new.is_empty() && foreign.is_empty() {
        map.remove(spec.key);
        return Ok(());
    }
    let rendered: Vec<Val> = new.iter().map(spec.render).collect();
    let Some(seq) = map.get_mut(spec.key).and_then(Node::block_seq_mut) else {
        if !foreign.is_empty() {
            return Err(format!(
                "`{}` is not a block list; it cannot be edited without losing entries",
                spec.key
            ));
        }
        map.put(spec.key, &Val::Seq(rendered), spec.order, false, nl);
        return Ok(());
    };
    let editable: Vec<bool> = seq
        .items
        .iter()
        .map(|i| matches!(i.value, Node::Map(ref m) if !m.flow))
        .collect();
    let mut ops: Vec<ItemOp<'_>> = Vec::new();
    let mut pairs = Vec::new();
    let mut rest = foreign.iter().copied().peekable();
    let steps = write::align(&old, new, spec.same);
    // Each step's place in the file: its old item, or for a new item the next old item that
    // stays (new items go as late as possible, after unmodelled ones before them).
    let mut anchor = vec![usize::MAX; steps.len()];
    let mut next = usize::MAX;
    for (k, step) in steps.iter().enumerate().rev() {
        if let Step::Keep(o) | Step::Edit(o, _) = *step {
            next = at[o];
        }
        anchor[k] = next;
    }
    for (step, anchor) in steps.into_iter().zip(anchor) {
        while let Some(f) = rest.next_if(|f| *f < anchor) {
            ops.push(ItemOp::Keep(f));
        }
        match step {
            Step::Keep(o) => ops.push(ItemOp::Keep(at[o])),
            Step::Edit(o, n) if editable[at[o]] => {
                pairs.push((o, n));
                ops.push(ItemOp::Edit(at[o]));
            }
            Step::Edit(_, n) | Step::New(n) => ops.push(ItemOp::New(&rendered[n])),
        }
    }
    ops.extend(rest.map(ItemOp::Keep));
    let positions = seq.rebuild(&ops, nl);
    for (pos, (o, n)) in positions.into_iter().zip(pairs) {
        let item = seq.items[pos]
            .value
            .block_map_mut()
            .expect("checked editable");
        (spec.edit)(item, &old[o], &new[n], nl);
    }
    Ok(())
}

/// Makes `scripts` of `map` (`runtime` of a request file, `request` of a collection or
/// folder file) read as `new`; entries of other phases stay.
pub(crate) fn write_scripts(
    map: &mut Map,
    order: &'static [&'static str],
    new: &[Script],
    nl: &str,
) -> Result<()> {
    let spec = Mixed {
        key: "scripts",
        order,
        read: &script,
        same: &|a: &Script, b: &Script| a.phase == b.phase,
        render: &script_val,
        edit: &edit_script,
    };
    sync_mixed(map, &spec, new, nl)
}

/// Makes `runtime.assertions` read as `new`.
pub(crate) fn write_assertions(
    runtime: &mut Map,
    order: &'static [&'static str],
    new: &[Assertion],
    nl: &str,
) {
    let spec = ListSpec {
        key: "assertions",
        order,
        empty: Empty::Remove,
        read: &assertion,
        same: &|a: &Assertion, b: &Assertion| a.expression == b.expression,
        render: &assertion_val,
        edit: &edit_assertion,
    };
    sync_list(runtime, &spec, new, nl);
}

/// Makes the `set-variable` entries of `runtime.actions` read as `new`; other action types
/// stay.
pub(crate) fn write_actions(
    runtime: &mut Map,
    order: &'static [&'static str],
    new: &[VarAction],
    nl: &str,
) -> Result<()> {
    let spec = Mixed {
        key: "actions",
        order,
        read: &action,
        same: &|a: &VarAction, b: &VarAction| a.variable == b.variable,
        render: &action_val,
        edit: &edit_action,
    };
    sync_mixed(runtime, &spec, new, nl)
}

/// Whether a node holds anything (an empty value, `{}` and `[]` do not).
pub(crate) fn has_content(node: &Node) -> bool {
    match node {
        Node::Scalar(s) => !s.value.is_empty(),
        Node::Map(m) => !m.entries.is_empty(),
        Node::Seq(s) => !s.items.is_empty(),
    }
}

/// `request.scripts` of a collection or folder file.
fn default_scripts(doc: &Document) -> Vec<Script> {
    scripts(doc.root.get("request").and_then(|r| r.get("scripts")))
}

/// Edits `request.scripts` of a collection (`collection`) or folder file so it reads as
/// `new`. Returns the edited, re-parsed document; `doc` itself is not changed.
pub(crate) fn apply_default_scripts(
    doc: &Document,
    collection: bool,
    new: &[Script],
) -> Result<Document> {
    apply_request_defaults(
        doc,
        collection,
        &RequestPart {
            what: "scripts",
            new_is_empty: new.is_empty(),
            write: &|request, nl| write_scripts(request, DEFAULTS_ORDER, new, nl),
            fresh: &|| scripts_val(new).expect("not empty"),
            same: &|doc| default_scripts(doc) == new,
            describe: &|doc| format!("wanted {new:?}, got {:?}", default_scripts(doc)),
        },
    )
}

/// One part of `request` in a collection or folder file (scripts, variables), for
/// [`apply_request_defaults`].
pub(crate) struct RequestPart<'a> {
    /// The key under `request`.
    pub what: &'static str,
    pub new_is_empty: bool,
    /// Makes the part of an existing `request` block mapping read as the new value.
    pub write: &'a dyn Fn(&mut Map, &str) -> Result<()>,
    /// The new value, for a file without `request` (only called if not empty).
    pub fresh: &'a dyn Fn() -> Val,
    /// Whether `doc` already reads as the new value.
    pub same: &'a dyn Fn(&Document) -> bool,
    /// Wanted and got, for the read-back error.
    pub describe: &'a dyn Fn(&Document) -> String,
}

/// Edits one part of `request` of a collection (`collection`) or folder file. Everything
/// else in the file stays as it was; the edit is read back and must give the new value.
pub(crate) fn apply_request_defaults(
    doc: &Document,
    collection: bool,
    part: &RequestPart<'_>,
) -> Result<Document> {
    if (part.same)(doc) {
        return Ok(doc.clone());
    }
    let mut doc = doc.clone();
    let nl = doc.newline;
    let root = doc
        .root
        .block_map_mut()
        .ok_or("the file is not a YAML mapping")?;
    // Something that is not a block mapping cannot be edited without losing it.
    let fixed = root
        .get("request")
        .is_some_and(|n| has_content(n) && !matches!(n, Node::Map(m) if !m.flow));
    match root.get_mut("request").and_then(Node::block_map_mut) {
        Some(request) => {
            (part.write)(request, nl)?;
            if request.is_empty() {
                root.remove("request");
            }
        }
        None if part.new_is_empty => {}
        None if fixed => {
            return Err("`request` is not a block mapping; it cannot be edited".into());
        }
        None => {
            let order = if collection {
                COLLECTION_ORDER
            } else {
                FOLDER_ORDER
            };
            root.put(
                "request",
                &Val::Map(vec![(part.what, (part.fresh)())]),
                order,
                true,
                nl,
            );
        }
    }
    let doc = reparse(&doc)?;
    if !(part.same)(&doc) {
        return Err(format!(
            "internal error: the edited file does not read back as the {} ({})",
            part.what,
            (part.describe)(&doc)
        ));
    }
    Ok(doc)
}

fn find_folder<'a>(items: &'a [Item], path: &str) -> Option<&'a Folder> {
    items.iter().find_map(|i| match i {
        Item::Folder(f) if f.path == path => Some(f),
        Item::Folder(f) => find_folder(&f.items, path),
        Item::Request(_) => None,
    })
}

impl CollectionDoc {
    pub(crate) fn edit_default_scripts(
        &mut self,
        folder: &str,
        new: &[Script],
    ) -> std::result::Result<(), FormatError> {
        self.edit_request_defaults(folder, new.is_empty(), &|doc, collection| {
            apply_default_scripts(doc, collection, new)
        })
    }

    /// Applies `apply` to the collection file (`folder` is `""`) or to the `folder.yml` of
    /// `folder` (created if the folder has none and `new_is_empty` is false).
    pub(crate) fn edit_request_defaults(
        &mut self,
        folder: &str,
        new_is_empty: bool,
        apply: &dyn Fn(&Document, bool) -> Result<Document>,
    ) -> std::result::Result<(), FormatError> {
        let invalid = |path: &str, message: String| FormatError::Invalid {
            path: path.to_owned(),
            message,
        };
        let path = if folder.is_empty() {
            COLLECTION_FILE.to_owned()
        } else {
            format!("{folder}/{FOLDER_FILE}")
        };
        let created;
        let doc = match self.files.get(&path) {
            _ if !folder.is_empty() && find_folder(&self.model.items, folder).is_none() => {
                return Err(invalid(folder, "folder not found".into()));
            }
            Some(Source::Yaml(doc)) => doc,
            Some(Source::Unparsed(_)) => {
                return Err(invalid(&path, "the file could not be parsed".into()));
            }
            None if new_is_empty => return Ok(()),
            None if folder.is_empty() => {
                return Err(invalid(&path, "no collection file".into()));
            }
            // A folder without `folder.yml`: write one with what the folder reads as.
            None => {
                let f = find_folder(&self.model.items, folder).expect("checked above");
                created = parse(&write::folder_text(&f.name, f.seq))
                    .map_err(|e| invalid(&path, format!("internal error: {e}")))?;
                &created
            }
        };
        let edited = apply(doc, folder.is_empty()).map_err(|e| invalid(&path, e))?;
        self.files.insert(path, Source::Yaml(edited));
        self.rebuild();
        Ok(())
    }
}
