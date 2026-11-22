// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! One OData operation against one service's in-memory data: reads with query options,
//! create, update, delete, draft actions, ETag checks and SAP-style errors. Used for plain
//! requests and for every part of a `$batch`. Transport concerns (auth, `sap-client`, CSRF,
//! sessions) are handled before, in `lib.rs`.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::services::{DraftStyle, Flavor, Generate, KeyKind, ServiceDef, SetDef, Stamp, Version};

pub type Entity = Map<String, Value>;

/// One request, already relative to the service root.
#[derive(Debug, Clone, Default)]
pub struct Op {
    pub method: String,
    /// Percent-decoded resource path after the root, e.g. `Books(ID=1,IsActiveEntity=true)`.
    pub path: String,
    /// Percent-decoded query pairs.
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Op {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    fn query(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
}

pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// The data of one service. Cloned before a changeset, restored when it fails.
#[derive(Clone)]
pub struct ServiceState {
    pub def: &'static ServiceDef,
    pub sets: BTreeMap<&'static str, Vec<Entity>>,
    /// Seconds after 10:00:00 of the server's day, for ETag timestamps.
    clock: u64,
    /// For generated GUIDs.
    seq: u64,
}

/// Where the service is reachable and in which language errors are reported.
pub struct Ctx<'a> {
    /// `http://host:port/<root>` with trailing `/`.
    pub base: &'a str,
    pub lang: &'a str,
}

impl ServiceState {
    pub fn load(def: &'static ServiceDef) -> Self {
        let data: Value = serde_json::from_str(def.data).expect("test data is JSON");
        let mut sets = BTreeMap::new();
        for set in def.sets {
            let mut rows: Vec<Entity> = data
                .get(set.data)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_object().cloned()).collect())
                .unwrap_or_default();
            if set.has_draft_keys() {
                for row in &mut rows {
                    row.insert("IsActiveEntity".into(), Value::Bool(true));
                    row.insert("HasActiveEntity".into(), Value::Bool(false));
                    row.insert("HasDraftEntity".into(), Value::Bool(false));
                }
            }
            sets.insert(set.name, rows);
        }
        Self {
            def,
            sets,
            clock: 0,
            seq: 0x100,
        }
    }

    fn stamp(&mut self, kind: Stamp) -> String {
        self.clock += 1;
        let (h, m, s) = (
            10 + self.clock / 3600,
            self.clock / 60 % 60,
            self.clock % 60,
        );
        match kind {
            Stamp::Seven => format!("2026-11-22T{h:02}:{m:02}:{s:02}.0000000Z"),
            Stamp::Milli => format!("2026-11-22T{h:02}:{m:02}:{s:02}.000Z"),
        }
    }

    fn guid(&mut self) -> String {
        self.seq += 1;
        format!("7a1d0000-0000-4000-8000-{:012x}", self.seq)
    }

    fn rows(&self, set: &SetDef) -> &[Entity] {
        self.sets.get(set.name).map_or(&[], Vec::as_slice)
    }

    fn rows_mut(&mut self, set: &SetDef) -> &mut Vec<Entity> {
        self.sets.entry(set.name).or_default()
    }
}

// -------------------------------------------------------------------------------------------
// Errors

pub fn error(def: &ServiceDef, lang: &str, status: u16, message: &str) -> Reply {
    let code = match def.flavor {
        Flavor::S4 => format!("RT_ODATA/{status:03}"),
        Flavor::Cap => status.to_string(),
    };
    let body = match def.version {
        Version::V2 => json!({
            "error": {
                "code": code,
                "message": { "lang": lang, "value": message },
                "innererror": { "transactionid": "0000000000000000000000000000RT01", "errordetails": [] }
            }
        }),
        Version::V4 => json!({ "error": { "code": code, "message": message } }),
    };
    Reply {
        status,
        headers: vec![
            ("Content-Type".into(), json_type(def.version).into()),
            version_header(def.version),
        ],
        body: body.to_string().into_bytes(),
    }
}

fn json_type(version: Version) -> &'static str {
    match version {
        Version::V2 => "application/json",
        Version::V4 => "application/json;odata.metadata=minimal",
    }
}

pub fn version_header(version: Version) -> (String, String) {
    match version {
        Version::V2 => ("DataServiceVersion".into(), "2.0".into()),
        Version::V4 => ("OData-Version".into(), "4.0".into()),
    }
}

type Fail = Box<Reply>;

fn fail(def: &ServiceDef, ctx: &Ctx, status: u16, message: impl AsRef<str>) -> Fail {
    Box::new(error(def, ctx.lang, status, message.as_ref()))
}

// -------------------------------------------------------------------------------------------
// Literals and keys

#[derive(Debug, Clone, PartialEq)]
enum Lit {
    Str(String),
    Num(String),
    Bool(bool),
    Null,
}

impl Lit {
    fn canon(&self) -> String {
        match self {
            Lit::Str(s) | Lit::Num(s) => s.clone(),
            Lit::Bool(b) => b.to_string(),
            Lit::Null => "null".into(),
        }
    }
}

fn parse_lit(raw: &str) -> Lit {
    let raw = raw.trim();
    if let Some(q) = raw.find('\'')
        && raw.ends_with('\'')
        && raw.len() >= q + 2
    {
        // `'x'`, `guid'…'`, `datetime'…'`, `datetimeoffset'…'`
        return Lit::Str(raw[q + 1..raw.len() - 1].replace("''", "'"));
    }
    match raw {
        "true" => return Lit::Bool(true),
        "false" => return Lit::Bool(false),
        "null" => return Lit::Null,
        _ => {}
    }
    let num = raw.trim_end_matches(['M', 'm', 'd', 'D', 'L', 'l', 'f', 'F']);
    if !num.is_empty() && num.parse::<f64>().is_ok() {
        return Lit::Num(num.to_owned());
    }
    // V4 GUIDs and dates are bare.
    Lit::Str(raw.to_owned())
}

/// Splits at `sep` outside single quotes.
fn split_outside_quotes(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_quote = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if c == '\'' {
            in_quote = !in_quote;
        } else if c == sep && !in_quote {
            out.push(&s[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&s[start..]);
    out
}

fn value_canon(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// The key predicate `(…)` of an entity, without the parentheses resolved against `set`.
fn key_values(set: &SetDef, predicate: &str) -> Option<Vec<(&'static str, String)>> {
    let parts: Vec<&str> = split_outside_quotes(predicate, ',')
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect();
    let mut out = Vec::new();
    if parts.len() == 1 && !parts[0].contains('=') {
        // `(1)`: the single key, or the first key with `IsActiveEntity=true`.
        let (first, _) = set.keys[0];
        out.push((first, parse_lit(parts[0]).canon()));
        for (k, _) in &set.keys[1..] {
            if *k != "IsActiveEntity" {
                return None;
            }
            out.push(("IsActiveEntity", "true".into()));
        }
        return Some(out);
    }
    for part in parts {
        let (name, lit) = part.split_once('=')?;
        let (key, _) = set.keys.iter().find(|(k, _)| *k == name.trim())?;
        out.push((*key, parse_lit(lit).canon()));
    }
    (out.len() == set.keys.len()).then_some(out)
}

fn find(rows: &[Entity], key: &[(&str, String)]) -> Option<usize> {
    rows.iter().position(|e| {
        key.iter()
            .all(|(k, v)| e.get(*k).map(value_canon).as_deref() == Some(v.as_str()))
    })
}

fn format_lit(kind: KeyKind, version: Version, v: &Value) -> String {
    let s = value_canon(v);
    match (kind, version) {
        (KeyKind::Str, _) => format!("'{}'", s.replace('\'', "''")),
        (KeyKind::Guid, Version::V2) => format!("guid'{s}'"),
        _ => s,
    }
}

/// `Set(key)` of an entity, as the service writes it.
pub fn entity_path(def: &ServiceDef, set: &SetDef, e: &Entity) -> String {
    let null = Value::Null;
    let lit =
        |(k, kind): &(&str, KeyKind)| format_lit(*kind, def.version, e.get(*k).unwrap_or(&null));
    if set.keys.len() == 1 {
        format!("{}({})", set.name, lit(&set.keys[0]))
    } else {
        let parts: Vec<String> = set
            .keys
            .iter()
            .map(|k| format!("{}={}", k.0, lit(k)))
            .collect();
        format!("{}({})", set.name, parts.join(","))
    }
}

pub fn etag(def: &ServiceDef, set: &SetDef, e: &Entity) -> Option<String> {
    let (prop, _) = set.etag?;
    let v = value_canon(e.get(prop)?);
    Some(match (def.flavor, def.version) {
        (Flavor::S4, Version::V2) => format!("W/\"datetimeoffset'{}'\"", v.replace(':', "%3A")),
        _ => format!("W/\"{v}\""),
    })
}

fn is_draft(e: &Entity) -> bool {
    e.get("IsActiveEntity") == Some(&Value::Bool(false))
}

// -------------------------------------------------------------------------------------------
// Resources

enum Target<'s> {
    /// Rows of `set` (all, or the targets of a to-many navigation).
    Collection(&'s SetDef, Vec<usize>, Option<NavParent<'s>>),
    Entity(&'s SetDef, usize),
    /// A to-one navigation without a target.
    Nothing,
    Count(&'s SetDef, Vec<usize>),
    Action(&'s SetDef, usize, String),
}

/// The parent of a to-many navigation, for creating through it.
struct NavParent<'s> {
    set: &'s SetDef,
    index: usize,
    on: &'static [(&'static str, &'static str)],
}

fn split_segment(segment: &str) -> (&str, Option<&str>) {
    match segment.find('(') {
        Some(i) if segment.ends_with(')') => {
            (&segment[..i], Some(&segment[i + 1..segment.len() - 1]))
        }
        _ => (segment, None),
    }
}

fn related(
    state: &ServiceState,
    from: &Entity,
    nav_on: &[(&str, &str)],
    target: &SetDef,
) -> Vec<usize> {
    state
        .rows(target)
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            nav_on.iter().all(|(a, b)| {
                from.get(*a).map(value_canon) == t.get(*b).map(value_canon)
                    && from.get(*a).is_some_and(|v| !v.is_null())
            })
        })
        .map(|(i, _)| i)
        .collect()
}

fn resolve<'s>(state: &ServiceState, ctx: &Ctx, path: &str) -> Result<Target<'s>, Fail> {
    let def: &'static ServiceDef = state.def;
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let Some(first) = segments.first() else {
        return Err(fail(def, ctx, 404, "No resource"));
    };
    let (name, predicate) = split_segment(first);
    let Some(set) = def.set(name) else {
        return Err(fail(
            def,
            ctx,
            404,
            format!("Resource not found for segment '{name}'"),
        ));
    };
    let all = || (0..state.rows(set).len()).collect::<Vec<_>>();
    let Some(predicate) = predicate else {
        return match segments.get(1..).unwrap_or_default() {
            [] => Ok(Target::Collection(set, all(), None)),
            ["$count"] => Ok(Target::Count(set, all())),
            _ => Err(fail(
                def,
                ctx,
                404,
                format!("Resource not found for segment '{path}'"),
            )),
        };
    };
    let index = key_values(set, predicate)
        .and_then(|key| find(state.rows(set), &key))
        .ok_or_else(|| {
            fail(
                def,
                ctx,
                404,
                format!("Resource not found for segment '{first}'"),
            )
        })?;
    let Some(second) = segments.get(1) else {
        return Ok(Target::Entity(set, index));
    };
    if let Some(nav) = set.nav(second) {
        let target = def.set(nav.target).expect("navigation targets exist");
        let rows = related(state, &state.rows(set)[index], nav.on, target);
        return match (nav.many, segments.get(2)) {
            (true, None) => Ok(Target::Collection(
                target,
                rows,
                Some(NavParent {
                    set,
                    index,
                    on: nav.on,
                }),
            )),
            (true, Some(&"$count")) => Ok(Target::Count(target, rows)),
            (false, None) => Ok(rows
                .first()
                .map_or(Target::Nothing, |i| Target::Entity(target, *i))),
            _ => Err(fail(
                def,
                ctx,
                404,
                format!("Resource not found for segment '{path}'"),
            )),
        };
    }
    if segments.len() == 2 {
        let action = second.rsplit('.').next().unwrap_or(second);
        return Ok(Target::Action(set, index, action.to_owned()));
    }
    Err(fail(
        def,
        ctx,
        404,
        format!("Resource not found for segment '{path}'"),
    ))
}

// -------------------------------------------------------------------------------------------
// Formatting

struct Shape {
    select: Option<Vec<String>>,
    expand: Vec<String>,
}

impl Shape {
    fn of(op: &Op) -> Self {
        let list = |name| {
            op.query(name).map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
        };
        Self {
            select: list("$select").filter(|s| !s.iter().any(|x| x == "*")),
            expand: list("$expand")
                .unwrap_or_default()
                .into_iter()
                // One level: `ToItems`, also from `ToItems($select=…)` and `ToItems/ToX`.
                .map(|e| e.split(['(', '/']).next().unwrap_or_default().to_owned())
                .collect(),
        }
    }

    fn none() -> Self {
        Self {
            select: None,
            expand: Vec::new(),
        }
    }
}

fn format_entity(
    state: &ServiceState,
    ctx: &Ctx,
    set: &SetDef,
    e: &Entity,
    shape: &Shape,
) -> Value {
    let def = state.def;
    let mut out = Map::new();
    for (k, v) in e {
        let keep = shape.select.as_ref().is_none_or(|s| s.contains(k))
            || (def.version == Version::V4 && set.keys.iter().any(|(key, _)| key == k));
        if keep {
            out.insert(k.clone(), v.clone());
        }
    }
    let path = entity_path(def, set, e);
    for nav in set.navs {
        let target = def.set(nav.target).expect("navigation targets exist");
        if shape.expand.iter().any(|x| x == nav.name) {
            let rows = related(state, e, nav.on, target);
            let inner: Vec<Value> = rows
                .iter()
                .map(|i| format_entity(state, ctx, target, &state.rows(target)[*i], &Shape::none()))
                .collect();
            let value = match (nav.many, def.version) {
                (true, Version::V2) => json!({ "results": inner }),
                (true, Version::V4) => Value::Array(inner),
                (false, _) => inner.into_iter().next().unwrap_or(Value::Null),
            };
            out.insert(nav.name.into(), value);
        } else if def.version == Version::V2
            && shape
                .select
                .as_ref()
                .is_none_or(|s| s.iter().any(|x| x == nav.name))
        {
            out.insert(
                nav.name.into(),
                json!({ "__deferred": { "uri": format!("{}{path}/{}", ctx.base, nav.name) } }),
            );
        }
    }
    let etag = etag(def, set, e);
    match def.version {
        Version::V2 => {
            let uri = format!("{}{path}", ctx.base);
            let mut meta = json!({ "id": uri, "uri": uri, "type": set.entity_type });
            if let Some(etag) = etag {
                meta["etag"] = Value::String(etag);
            }
            out.insert("__metadata".into(), meta);
        }
        Version::V4 => {
            if let Some(etag) = etag {
                out.insert("@odata.etag".into(), Value::String(etag));
            }
        }
    }
    Value::Object(out)
}

fn json_reply(def: &ServiceDef, status: u16, body: &Value) -> Reply {
    Reply {
        status,
        headers: vec![
            ("Content-Type".into(), json_type(def.version).into()),
            version_header(def.version),
        ],
        body: body.to_string().into_bytes(),
    }
}

fn entity_reply(
    state: &ServiceState,
    ctx: &Ctx,
    set: &SetDef,
    index: usize,
    status: u16,
    shape: &Shape,
) -> Reply {
    let def = state.def;
    let e = &state.rows(set)[index];
    let mut value = format_entity(state, ctx, set, e, shape);
    let body = match def.version {
        Version::V2 => json!({ "d": value }),
        Version::V4 => {
            value["@odata.context"] = Value::String(format!("$metadata#{}/$entity", set.name));
            value
        }
    };
    let mut reply = json_reply(def, status, &body);
    if let Some(etag) = etag(def, set, e) {
        reply.headers.push(("ETag".into(), etag));
    }
    reply
}

fn no_content(state: &ServiceState, set: &SetDef, index: Option<usize>) -> Reply {
    let mut headers = vec![version_header(state.def.version)];
    if let Some(etag) = index.and_then(|i| etag(state.def, set, &state.rows(set)[i])) {
        headers.push(("ETag".into(), etag));
    }
    Reply {
        status: 204,
        headers,
        body: Vec::new(),
    }
}

// -------------------------------------------------------------------------------------------
// $filter, $orderby

fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for c in s.chars() {
        if c == '\'' {
            in_quote = !in_quote;
            cur.push(c);
        } else if c.is_whitespace() && !in_quote {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

type Cond = (String, String, Lit);

/// `a eq 1 and b ne 'x' or c gt 2`: or-groups of and-terms, one property per term.
fn parse_filter(s: &str) -> Option<Vec<Vec<Cond>>> {
    let toks = tokens(s);
    let mut groups = vec![Vec::new()];
    let mut i = 0;
    while i < toks.len() {
        let (prop, op, lit) = (toks.get(i)?, toks.get(i + 1)?, toks.get(i + 2)?);
        let op = op.to_ascii_lowercase();
        if !["eq", "ne", "gt", "ge", "lt", "le"].contains(&op.as_str()) {
            return None;
        }
        groups.last_mut()?.push((prop.clone(), op, parse_lit(lit)));
        i += 3;
        match toks.get(i).map(|t| t.to_ascii_lowercase()).as_deref() {
            None => break,
            Some("and") => {}
            Some("or") => groups.push(Vec::new()),
            Some(_) => return None,
        }
        i += 1;
    }
    Some(groups)
}

fn compare(v: &Value, lit: &Lit) -> Option<Ordering> {
    if matches!(lit, Lit::Null) || v.is_null() {
        return (matches!(lit, Lit::Null) == v.is_null()).then_some(Ordering::Equal);
    }
    if let Lit::Num(n) = lit {
        let a = match v {
            Value::Number(x) => x.as_f64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        };
        if let (Some(a), Ok(b)) = (a, n.parse::<f64>()) {
            return a.partial_cmp(&b);
        }
    }
    Some(value_canon(v).cmp(&lit.canon()))
}

fn matches(e: &Entity, groups: &[Vec<Cond>]) -> bool {
    groups.iter().any(|group| {
        group.iter().all(|(prop, op, lit)| {
            let v = e.get(prop).unwrap_or(&Value::Null);
            let ord = compare(v, lit);
            match op.as_str() {
                "eq" => ord == Some(Ordering::Equal),
                "ne" => ord != Some(Ordering::Equal),
                "gt" => ord == Some(Ordering::Greater),
                "ge" => matches!(ord, Some(Ordering::Greater | Ordering::Equal)),
                "lt" => ord == Some(Ordering::Less),
                "le" => matches!(ord, Some(Ordering::Less | Ordering::Equal)),
                _ => false,
            }
        })
    })
}

fn order_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        _ => value_canon(a).cmp(&value_canon(b)),
    }
}

fn list(state: &ServiceState, ctx: &Ctx, op: &Op, set: &SetDef, mut rows: Vec<usize>) -> Reply {
    let def = state.def;
    let data = state.rows(set);
    if let Some(filter) = op.query("$filter") {
        let Some(groups) = parse_filter(filter) else {
            return error(
                def,
                ctx.lang,
                400,
                &format!("Unsupported $filter: {filter}"),
            );
        };
        rows.retain(|i| matches(&data[*i], &groups));
    }
    if let Some(order) = op.query("$orderby") {
        let keys: Vec<(String, bool)> = order
            .split(',')
            .map(|p| {
                let mut it = p.split_whitespace();
                let name = it.next().unwrap_or_default().to_owned();
                (
                    name,
                    it.next().is_some_and(|d| d.eq_ignore_ascii_case("desc")),
                )
            })
            .collect();
        rows.sort_by(|a, b| {
            for (name, desc) in &keys {
                let null = Value::Null;
                let o = order_values(
                    data[*a].get(name).unwrap_or(&null),
                    data[*b].get(name).unwrap_or(&null),
                );
                let o = if *desc { o.reverse() } else { o };
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        });
    }
    let count = rows.len();
    let skip = op.query("$skip").and_then(|s| s.parse().ok()).unwrap_or(0);
    let top = op
        .query("$top")
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let shape = Shape::of(op);
    let values: Vec<Value> = rows
        .into_iter()
        .skip(skip)
        .take(top)
        .map(|i| format_entity(state, ctx, set, &data[i], &shape))
        .collect();
    let body = match def.version {
        Version::V2 => {
            let mut d = json!({ "results": values });
            if op.query("$inlinecount") == Some("allpages") {
                d["__count"] = Value::String(count.to_string());
            }
            json!({ "d": d })
        }
        Version::V4 => {
            let mut b =
                json!({ "@odata.context": format!("$metadata#{}", set.name), "value": values });
            if op.query("$count") == Some("true") {
                b["@odata.count"] = json!(count);
            }
            b
        }
    };
    json_reply(def, 200, &body)
}

// -------------------------------------------------------------------------------------------
// Writes

/// Checks `If-Match` against the entity's ETag. `required`: a write without `If-Match` fails
/// with 428 (S/4 on active instances).
fn check_etag(
    state: &ServiceState,
    ctx: &Ctx,
    op: &Op,
    set: &SetDef,
    index: usize,
) -> Result<(), Fail> {
    let def = state.def;
    let Some(current) = etag(def, set, &state.rows(set)[index]) else {
        return Ok(());
    };
    let required = def.flavor == Flavor::S4 && !is_draft(&state.rows(set)[index]);
    match op.header("If-Match") {
        None if required => Err(fail(
            def,
            ctx,
            428,
            "Precondition required: send If-Match with the ETag of the entity",
        )),
        None => Ok(()),
        Some(v) if v.trim() == "*" || v.split(',').any(|t| t.trim() == current) => Ok(()),
        Some(_) => Err(fail(
            def,
            ctx,
            412,
            "Precondition failed: the entity was changed in the meantime (ETag mismatch)",
        )),
    }
}

fn body_object(def: &ServiceDef, ctx: &Ctx, op: &Op) -> Result<Entity, Fail> {
    if op.body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Entity::new());
    }
    let value: Value = serde_json::from_slice(&op.body).map_err(|e| {
        fail(
            def,
            ctx,
            400,
            format!("The request body is not valid JSON: {e}"),
        )
    })?;
    let mut object = match value {
        Value::Object(mut o) if o.len() == 1 && o.get("d").is_some_and(Value::is_object) => o
            .remove("d")
            .and_then(|d| d.as_object().cloned())
            .unwrap_or_default(),
        Value::Object(o) => o,
        _ => {
            return Err(fail(
                def,
                ctx,
                400,
                "The request body must be a JSON object",
            ));
        }
    };
    object.retain(|k, _| k != "__metadata" && !k.starts_with("@odata.") && !k.contains("@odata."));
    Ok(object)
}

fn writable_fields(set: &SetDef, body: Entity) -> impl Iterator<Item = (String, Value)> {
    body.into_iter().filter(|(k, _)| {
        !set.computed.contains(&k.as_str())
            && set.nav(k).is_none()
            && !["IsActiveEntity", "HasActiveEntity", "HasDraftEntity"].contains(&k.as_str())
    })
}

fn touch(state: &mut ServiceState, set: &SetDef, index: usize) {
    if let Some((prop, kind)) = set.etag {
        let now = state.stamp(kind);
        state.rows_mut(set)[index].insert(prop.into(), Value::String(now));
    }
}

fn create(
    state: &mut ServiceState,
    ctx: &Ctx,
    op: &Op,
    set: &SetDef,
    parent: Option<NavParent>,
) -> Reply {
    let def = state.def;
    if !set.insertable {
        return error(
            def,
            ctx.lang,
            405,
            &format!("Entity set {} does not allow creating entities", set.name),
        );
    }
    let body = match body_object(def, ctx, op) {
        Ok(b) => b,
        Err(e) => return *e,
    };
    let direct_active = body.get("IsActiveEntity") == Some(&Value::Bool(true));
    let mut e: Entity = writable_fields(set, body).collect();
    if let Some(p) = parent {
        let from = state.rows(p.set)[p.index].clone();
        for (a, b) in p.on {
            if let Some(v) = from.get(*a) {
                e.insert((*b).into(), v.clone());
            }
        }
    }
    for (prop, generator) in set.generate {
        let missing = e.get(*prop).is_none_or(Value::is_null) || set.computed.contains(prop);
        if !missing {
            continue;
        }
        let value = match generator {
            Generate::Guid => Value::String(state.guid()),
            Generate::Number10 => {
                let max = state
                    .rows(set)
                    .iter()
                    .filter_map(|r| r.get(*prop).map(value_canon)?.parse::<u64>().ok())
                    .max()
                    .unwrap_or(0);
                Value::String(format!("{:010}", max + 1))
            }
            Generate::NextInt => {
                let max = state
                    .rows(set)
                    .iter()
                    .filter_map(|r| r.get(*prop)?.as_i64())
                    .max()
                    .unwrap_or(0);
                json!(max + 1)
            }
            Generate::ItemPosition => {
                let (first, _) = set.keys[0];
                let parent = e.get(first).map(value_canon);
                let max = state
                    .rows(set)
                    .iter()
                    .filter(|r| r.get(first).map(value_canon) == parent)
                    .filter_map(|r| r.get(*prop).map(value_canon)?.parse::<u64>().ok())
                    .max()
                    .unwrap_or(0);
                Value::String(format!("{:06}", (max / 10 + 1) * 10))
            }
        };
        e.insert((*prop).into(), value);
    }
    for prop in set.computed {
        if prop.to_ascii_lowercase().starts_with("created") {
            let now = state.stamp(set.etag.map_or(Stamp::Milli, |(_, k)| k));
            e.insert((*prop).into(), Value::String(now));
        }
    }
    if set.has_draft_keys() {
        let active = !set.draft_root || direct_active;
        e.insert("IsActiveEntity".into(), Value::Bool(active));
        e.insert("HasActiveEntity".into(), Value::Bool(false));
        e.insert("HasDraftEntity".into(), Value::Bool(false));
    }
    if let Some((k, _)) = set
        .keys
        .iter()
        .find(|(k, _)| e.get(*k).is_none_or(Value::is_null))
    {
        return error(def, ctx.lang, 400, &format!("Key property {k} is missing"));
    }
    let key: Vec<(&str, String)> = set
        .keys
        .iter()
        .map(|(k, _)| (*k, value_canon(&e[*k])))
        .collect();
    if find(state.rows(set), &key).is_some() {
        return error(def, ctx.lang, 409, "Entity already exists (duplicate key)");
    }
    state.rows_mut(set).push(e);
    let index = state.rows(set).len() - 1;
    touch(state, set, index);
    let mut reply = entity_reply(state, ctx, set, index, 201, &Shape::none());
    let location = format!(
        "{}{}",
        ctx.base,
        entity_path(def, set, &state.rows(set)[index])
    );
    reply.headers.push(("Location".into(), location));
    reply
}

fn update(state: &mut ServiceState, ctx: &Ctx, op: &Op, set: &SetDef, index: usize) -> Reply {
    let def = state.def;
    if !set.updatable {
        return error(
            def,
            ctx.lang,
            405,
            &format!("Entity set {} does not allow updates", set.name),
        );
    }
    if let Err(e) = check_etag(state, ctx, op, set, index) {
        return *e;
    }
    let body = match body_object(def, ctx, op) {
        Ok(b) => b,
        Err(e) => return *e,
    };
    for (k, v) in writable_fields(set, body) {
        if set.keys.iter().any(|(key, _)| *key == k) {
            continue;
        }
        state.rows_mut(set)[index].insert(k, v);
    }
    touch(state, set, index);
    let minimal = op
        .header("Prefer")
        .is_some_and(|p| p.contains("return=minimal"));
    match def.version {
        Version::V4 if !minimal => entity_reply(state, ctx, set, index, 200, &Shape::none()),
        _ => no_content(state, set, Some(index)),
    }
}

/// The active instance of a draft, or the draft of an active instance.
fn sibling(state: &ServiceState, set: &SetDef, index: usize) -> Option<usize> {
    let e = &state.rows(set)[index];
    let want = Value::Bool(is_draft(e));
    state.rows(set).iter().position(|o| {
        o.get("IsActiveEntity") == Some(&want)
            && set
                .keys
                .iter()
                .filter(|(k, _)| *k != "IsActiveEntity")
                .all(|(k, _)| o.get(*k) == e.get(*k))
    })
}

fn set_flag(state: &mut ServiceState, set: &SetDef, index: usize, flag: &str, value: bool) {
    state.rows_mut(set)[index].insert(flag.into(), Value::Bool(value));
}

fn discard(state: &mut ServiceState, set: &SetDef, index: usize) -> Reply {
    if let Some(active) = sibling(state, set, index) {
        set_flag(state, set, active, "HasDraftEntity", false);
    }
    state.rows_mut(set).remove(index);
    no_content(state, set, None)
}

fn delete(state: &mut ServiceState, ctx: &Ctx, op: &Op, set: &SetDef, index: usize) -> Reply {
    let def = state.def;
    if is_draft(&state.rows(set)[index]) {
        if let Err(e) = check_etag(state, ctx, op, set, index) {
            return *e;
        }
        return discard(state, set, index);
    }
    if !set.deletable {
        return error(
            def,
            ctx.lang,
            405,
            &format!("Entity set {} does not allow deleting entities", set.name),
        );
    }
    if let Err(e) = check_etag(state, ctx, op, set, index) {
        return *e;
    }
    if let Some(draft) = sibling(state, set, index) {
        state.rows_mut(set).remove(draft.max(index));
        state.rows_mut(set).remove(draft.min(index));
    } else {
        state.rows_mut(set).remove(index);
    }
    no_content(state, set, None)
}

fn action(
    state: &mut ServiceState,
    ctx: &Ctx,
    op: &Op,
    set: &SetDef,
    index: usize,
    name: &str,
) -> Reply {
    let def = state.def;
    let style = def.draft.filter(|_| set.draft_root);
    let draft = is_draft(&state.rows(set)[index]);
    let which = match (style, name) {
        (Some(DraftStyle::Rap), "Edit") | (Some(DraftStyle::Cap), "draftEdit") => "edit",
        (Some(DraftStyle::Rap), "Prepare") | (Some(DraftStyle::Cap), "draftPrepare") => "prepare",
        (Some(DraftStyle::Rap), "Activate") | (Some(DraftStyle::Cap), "draftActivate") => {
            "activate"
        }
        (Some(DraftStyle::Rap), "Discard") => "discard",
        _ => {
            return error(
                def,
                ctx.lang,
                400,
                &format!(
                    "Action {name} is not supported on {} by the test server",
                    set.name
                ),
            );
        }
    };
    if op.method != "POST" {
        return error(def, ctx.lang, 405, "Actions are called with POST");
    }
    if (which == "edit") == draft {
        let wanted = if which == "edit" {
            "an active entity"
        } else {
            "a draft"
        };
        return error(
            def,
            ctx.lang,
            400,
            &format!(
                "Action {name} needs {wanted} (IsActiveEntity={})",
                which != "edit"
            ),
        );
    }
    if let Err(e) = check_etag(state, ctx, op, set, index) {
        return *e;
    }
    let body = match body_object(def, ctx, op) {
        Ok(b) => b,
        Err(e) => return *e,
    };
    match which {
        "edit" => {
            let mut index = index;
            if let Some(existing) = sibling(state, set, index) {
                if body.get("PreserveChanges") == Some(&Value::Bool(true)) {
                    return error(def, ctx.lang, 409, "A draft of this entity exists already");
                }
                state.rows_mut(set).remove(existing);
                if existing < index {
                    index -= 1;
                }
            }
            let mut copy = state.rows(set)[index].clone();
            copy.insert("IsActiveEntity".into(), Value::Bool(false));
            copy.insert("HasActiveEntity".into(), Value::Bool(true));
            copy.insert("HasDraftEntity".into(), Value::Bool(false));
            set_flag(state, set, index, "HasDraftEntity", true);
            state.rows_mut(set).push(copy);
            let draft = state.rows(set).len() - 1;
            touch(state, set, draft);
            let status = if style == Some(DraftStyle::Cap) {
                201
            } else {
                200
            };
            entity_reply(state, ctx, set, draft, status, &Shape::none())
        }
        "prepare" => entity_reply(state, ctx, set, index, 200, &Shape::none()),
        "activate" => {
            let mut fields = state.rows(set)[index].clone();
            fields.insert("IsActiveEntity".into(), Value::Bool(true));
            fields.insert("HasActiveEntity".into(), Value::Bool(false));
            fields.insert("HasDraftEntity".into(), Value::Bool(false));
            let active = match sibling(state, set, index) {
                Some(active) => {
                    state.rows_mut(set)[active] = fields;
                    state.rows_mut(set).remove(index);
                    if active > index { active - 1 } else { active }
                }
                None => {
                    state.rows_mut(set)[index] = fields;
                    index
                }
            };
            touch(state, set, active);
            entity_reply(state, ctx, set, active, 200, &Shape::none())
        }
        _ => discard(state, set, index),
    }
}

// -------------------------------------------------------------------------------------------

/// Runs one operation.
pub fn execute(state: &mut ServiceState, ctx: &Ctx, op: &Op) -> Reply {
    let def = state.def;
    let target = match resolve(state, ctx, &op.path) {
        Ok(t) => t,
        Err(e) => return *e,
    };
    let method = op.method.as_str();
    match (method, target) {
        ("GET" | "HEAD", Target::Collection(set, rows, _)) => list(state, ctx, op, set, rows),
        ("GET" | "HEAD", Target::Entity(set, index)) => {
            entity_reply(state, ctx, set, index, 200, &Shape::of(op))
        }
        ("GET" | "HEAD", Target::Nothing) => match def.version {
            Version::V2 => error(
                def,
                ctx.lang,
                404,
                "No entity at the end of this navigation",
            ),
            Version::V4 => Reply {
                status: 204,
                headers: vec![version_header(def.version)],
                body: Vec::new(),
            },
        },
        ("GET" | "HEAD", Target::Count(set, rows)) => {
            let n = match op.query("$filter").and_then(parse_filter) {
                Some(groups) => rows
                    .iter()
                    .filter(|i| matches(&state.rows(set)[**i], &groups))
                    .count(),
                None => rows.len(),
            };
            Reply {
                status: 200,
                headers: vec![
                    ("Content-Type".into(), "text/plain".into()),
                    version_header(def.version),
                ],
                body: n.to_string().into_bytes(),
            }
        }
        ("POST", Target::Collection(set, _, parent)) => create(state, ctx, op, set, parent),
        ("POST", Target::Action(set, index, name)) => action(state, ctx, op, set, index, &name),
        ("PATCH" | "MERGE" | "PUT", Target::Entity(set, index)) => {
            update(state, ctx, op, set, index)
        }
        ("DELETE", Target::Entity(set, index)) => delete(state, ctx, op, set, index),
        (_, Target::Action(set, index, name)) => action(state, ctx, op, set, index, &name),
        (m, _) => error(
            def,
            ctx.lang,
            405,
            &format!("Method {m} is not allowed on this resource"),
        ),
    }
}

/// The service document (`GET <root>`).
pub fn service_document(state: &ServiceState, ctx: &Ctx) -> Reply {
    let def = state.def;
    let names: Vec<&str> = def.sets.iter().map(|s| s.name).collect();
    let body = match def.version {
        Version::V2 => json!({ "d": { "EntitySets": names } }),
        Version::V4 => json!({
            "@odata.context": format!("{}$metadata", ctx.base),
            "value": names.iter().map(|n| json!({ "name": n, "kind": "EntitySet", "url": n })).collect::<Vec<_>>()
        }),
    };
    json_reply(def, 200, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_filters() {
        assert_eq!(parse_lit("'it''s'"), Lit::Str("it's".into()));
        assert_eq!(parse_lit("guid'ab-c'"), Lit::Str("ab-c".into()));
        assert_eq!(parse_lit("12.5M"), Lit::Num("12.5".into()));
        assert_eq!(parse_lit("true"), Lit::Bool(true));
        let groups = parse_filter("stock gt 100 and currency_code eq 'USD' or ID eq 201").unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 2);
        assert!(parse_filter("stock between 1").is_none());
    }

    #[test]
    fn keys_resolve_named_bare_and_composite() {
        let cap = &crate::services::SERVICES[2];
        let books = cap.set("Books").unwrap();
        assert_eq!(
            key_values(books, "1").unwrap(),
            vec![
                ("ID", "1".to_owned()),
                ("IsActiveEntity", "true".to_owned())
            ]
        );
        assert_eq!(
            key_values(books, "ID=1,IsActiveEntity=false").unwrap()[1],
            ("IsActiveEntity", "false".to_owned())
        );
        let gw = &crate::services::SERVICES[0];
        let items = gw.set("SalesOrderItemSet").unwrap();
        assert!(key_values(items, "SalesOrderID='1',ItemPosition='000010'").is_some());
        assert!(key_values(items, "'1'").is_none());
    }
}
