// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The parts of a request the search may take away, and the request that is left when some are
//! gone. A part is something the request *has*: a header, a query parameter, a cookie of the
//! `Cookie` header, a form field, a node of a JSON body (down to depth [`MAX_DEPTH`]). Nothing
//! is invented, the order of what stays is the order it had.
//!
//! A part has an id made of its kind and its name or path (`header:X-Debug`,
//! `body:$.items[3]`); ids never contain values.

use std::collections::HashMap;

use rt_core::model::{Body, KeyValue, ParamKind, RawKind, RequestSpec};

use super::json::{self, Node};

/// JSON nodes below this depth are not parts of their own (they go with their parent).
pub const MAX_DEPTH: usize = 8;

/// Headers the search never takes away, besides the ones the user keeps: the transport needs
/// them or they carry the credentials.
fn protected_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "host" | "content-length" | "x-api-key" | "api-key" | "apikey" | "x-auth-token"
    ) || lower.contains("authorization")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Loc {
    Header(usize),
    Query(usize),
    /// Pair `.1` of cookie header `.0` (index into `Plan::cookies`).
    Cookie(usize, usize),
    Json,
    Form(usize),
    Multipart(usize),
    /// The whole body (a body that is not a JSON tree).
    Body,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub id: String,
    /// `header`, `query`, `cookie`, `body`, `form`.
    pub kind: &'static str,
    /// Stays whatever the search decides.
    pub protected: bool,
    /// The container this part sits in (a JSON node); it goes when its parent goes.
    pub parent: Option<usize>,
    loc: Loc,
}

#[derive(Debug, Clone)]
struct QueryParts {
    base: String,
    pairs: Vec<String>,
    fragment: String,
}

/// A request split into parts.
#[derive(Debug, Clone)]
pub struct Plan {
    pub parts: Vec<Part>,
    spec: RequestSpec,
    query: Option<QueryParts>,
    /// Cookie headers: (index in `spec.headers`, the `name=value` pairs).
    cookies: Vec<(usize, Vec<String>)>,
    json: Option<Node>,
    /// Index of the first JSON part in `parts`, and how many there are.
    json_first: usize,
    json_count: usize,
}

fn path_key(key: &str) -> String {
    let plain = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if plain {
        format!(".{key}")
    } else {
        format!("[{}]", serde_json::to_string(key).unwrap_or_default())
    }
}

/// Makes ids unique: the second `header:x` becomes `header:x[2]`.
struct Namer(HashMap<String, usize>);

impl Namer {
    fn name(&mut self, id: String) -> String {
        let n = self.0.entry(id.clone()).or_insert(0);
        *n += 1;
        if *n == 1 { id } else { format!("{id}[{n}]") }
    }
}

fn matches_keep(part: &Part, keep: &str) -> bool {
    let lower = matches!(part.kind, "header" | "cookie");
    let (a, b) = if lower {
        (part.id.to_ascii_lowercase(), keep.to_ascii_lowercase())
    } else {
        (part.id.clone(), keep.to_owned())
    };
    a == b
        || (part.kind == "body"
            && a.starts_with(&b)
            && matches!(a.as_bytes().get(b.len()), Some(b'.' | b'[')))
}

impl Plan {
    /// Splits `spec` into parts. At most `limit` parts are listed (headers, query, cookies,
    /// form, then the JSON tree); what does not fit stays in the request and cannot go.
    /// `keep` lists parts that must stay (`header:Authorization`, `query:key`, `body:$.id`).
    pub fn new(spec: &RequestSpec, keep: &[String], limit: usize) -> Plan {
        let mut plan = Plan {
            parts: Vec::new(),
            spec: spec.clone(),
            query: None,
            cookies: Vec::new(),
            json: None,
            json_first: 0,
            json_count: 0,
        };
        let mut namer = Namer(HashMap::new());
        // Headers and cookies.
        for (i, h) in spec.headers.iter().enumerate() {
            if !h.enabled {
                continue;
            }
            if h.name.eq_ignore_ascii_case("cookie") {
                let pairs: Vec<String> = h
                    .value
                    .split(';')
                    .map(|p| p.trim().to_owned())
                    .filter(|p| !p.is_empty())
                    .collect();
                if !pairs.is_empty()
                    && pairs
                        .iter()
                        .all(|p| p.split_once('=').is_some_and(|(n, _)| !n.trim().is_empty()))
                {
                    let at = plan.cookies.len();
                    for (j, pair) in pairs.iter().enumerate() {
                        let name = pair.split_once('=').map_or("", |(n, _)| n.trim());
                        plan.push(
                            namer.name(format!("cookie:{name}")),
                            "cookie",
                            false,
                            None,
                            Loc::Cookie(at, j),
                            limit,
                        );
                    }
                    plan.cookies.push((i, pairs));
                    continue;
                }
            }
            let protected = protected_header(&h.name);
            plan.push(
                namer.name(format!("header:{}", h.name)),
                "header",
                protected,
                None,
                Loc::Header(i),
                limit,
            );
        }
        // Query parameters live in the URL (it is the truth, the `params` list only mirrors).
        if let Some((before, rest)) = spec.url.split_once('?') {
            let (query, fragment) = match rest.split_once('#') {
                Some((q, f)) => (q, format!("#{f}")),
                None => (rest, String::new()),
            };
            let pairs: Vec<String> = query
                .split('&')
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect();
            for (j, pair) in pairs.iter().enumerate() {
                let name = pair.split_once('=').map_or(pair.as_str(), |(n, _)| n);
                plan.push(
                    namer.name(format!("query:{name}")),
                    "query",
                    false,
                    None,
                    Loc::Query(j),
                    limit,
                );
            }
            plan.query = Some(QueryParts {
                base: before.to_owned(),
                pairs,
                fragment,
            });
        }
        // The body.
        match &spec.body {
            Body::FormUrlEncoded { fields } => {
                for (i, f) in fields.iter().enumerate().filter(|(_, f)| f.enabled) {
                    plan.push(
                        namer.name(format!("form:{}", f.name)),
                        "form",
                        false,
                        None,
                        Loc::Form(i),
                        limit,
                    );
                }
            }
            Body::Multipart { parts } => {
                for (i, p) in parts.iter().enumerate().filter(|(_, p)| p.enabled) {
                    plan.push(
                        namer.name(format!("form:{}", p.name)),
                        "form",
                        false,
                        None,
                        Loc::Multipart(i),
                        limit,
                    );
                }
            }
            Body::Raw {
                kind: RawKind::Json,
                data,
            } => match json::parse(data) {
                Some(root) if matches!(root, Node::Object(_) | Node::Array(_)) => {
                    plan.json_first = plan.parts.len();
                    let mut budget = limit.saturating_sub(plan.parts.len());
                    plan.collect_json(&root, "$", 0, None, &mut budget);
                    plan.json_count = plan.parts.len() - plan.json_first;
                    plan.json = Some(root);
                }
                _ => {
                    plan.push("body".into(), "body", false, None, Loc::Body, limit);
                }
            },
            Body::Raw { .. } => {
                plan.push("body".into(), "body", false, None, Loc::Body, limit);
            }
            // GraphQL needs its query and a file body is not ours to cut: both stay.
            Body::None | Body::GraphQl { .. } | Body::File { .. } => {}
        }
        plan.protect(keep);
        plan
    }

    fn push(
        &mut self,
        id: String,
        kind: &'static str,
        protected: bool,
        parent: Option<usize>,
        loc: Loc,
        limit: usize,
    ) -> Option<usize> {
        if self.parts.len() >= limit {
            return None;
        }
        self.parts.push(Part {
            id,
            kind,
            protected,
            parent,
            loc,
        });
        Some(self.parts.len() - 1)
    }

    fn collect_json(
        &mut self,
        node: &Node,
        path: &str,
        depth: usize,
        parent: Option<usize>,
        budget: &mut usize,
    ) {
        if depth >= MAX_DEPTH {
            return;
        }
        let children: Vec<(String, &Node)> = match node {
            Node::Object(members) => members
                .iter()
                .map(|(k, v)| (format!("{path}{}", path_key(k)), v))
                .collect(),
            Node::Array(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| (format!("{path}[{i}]"), v))
                .collect(),
            _ => return,
        };
        for (child_path, child) in children {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let idx = self.parts.len();
            self.parts.push(Part {
                id: format!("body:{child_path}"),
                kind: "body",
                protected: false,
                parent,
                loc: Loc::Json,
            });
            self.collect_json(child, &child_path, depth + 1, Some(idx), budget);
        }
    }

    /// Marks what the user keeps (and everything a kept part sits in).
    fn protect(&mut self, keep: &[String]) {
        for i in 0..self.parts.len() {
            if keep.iter().any(|k| matches_keep(&self.parts[i], k)) {
                self.parts[i].protected = true;
            }
        }
        for i in (0..self.parts.len()).rev() {
            if self.parts[i].protected
                && let Some(p) = self.parts[i].parent
            {
                self.parts[p].protected = true;
            }
        }
    }

    /// Which parts are really present when `kept` says so: a part whose container is gone is
    /// gone too.
    pub fn effective(&self, kept: &[bool]) -> Vec<bool> {
        let mut eff = vec![false; self.parts.len()];
        for (i, part) in self.parts.iter().enumerate() {
            let parent_ok = part.parent.is_none_or(|p| eff[p]);
            eff[i] = (kept[i] || part.protected) && parent_ok;
        }
        eff
    }

    /// The request without the parts that are not in `eff` (from [`Plan::effective`]).
    pub fn build(&self, eff: &[bool]) -> RequestSpec {
        let mut spec = self.spec.clone();
        let mut drop_headers = vec![false; spec.headers.len()];
        let mut drop_query = Vec::new();
        let mut drop_cookies: Vec<Vec<usize>> = vec![Vec::new(); self.cookies.len()];
        let mut drop_form = Vec::new();
        let mut drop_multipart = Vec::new();
        let mut drop_body = false;
        let mut json_changed = false;
        for (i, part) in self.parts.iter().enumerate() {
            if eff[i] {
                continue;
            }
            match part.loc {
                Loc::Header(h) => drop_headers[h] = true,
                Loc::Query(q) => drop_query.push(q),
                Loc::Cookie(c, p) => drop_cookies[c].push(p),
                Loc::Form(f) => drop_form.push(f),
                Loc::Multipart(m) => drop_multipart.push(m),
                Loc::Json => json_changed = true,
                Loc::Body => drop_body = true,
            }
        }
        for (c, (header, pairs)) in self.cookies.iter().enumerate() {
            if drop_cookies[c].is_empty() {
                continue;
            }
            let left: Vec<&str> = pairs
                .iter()
                .enumerate()
                .filter(|(j, _)| !drop_cookies[c].contains(j))
                .map(|(_, p)| p.as_str())
                .collect();
            if left.is_empty() {
                drop_headers[*header] = true;
            } else {
                spec.headers[*header].value = left.join("; ");
            }
        }
        let mut index = 0;
        spec.headers.retain(|_| {
            index += 1;
            !drop_headers[index - 1]
        });
        if let Some(q) = &self.query
            && !drop_query.is_empty()
        {
            let left: Vec<&str> = q
                .pairs
                .iter()
                .enumerate()
                .filter(|(j, _)| !drop_query.contains(j))
                .map(|(_, p)| p.as_str())
                .collect();
            spec.url = if left.is_empty() {
                format!("{}{}", q.base, q.fragment)
            } else {
                format!("{}?{}{}", q.base, left.join("&"), q.fragment)
            };
            // The list of query parameters only mirrors the URL: keep it in step.
            let names: Vec<&str> = left
                .iter()
                .map(|p| p.split_once('=').map_or(*p, |(n, _)| n))
                .collect();
            spec.params
                .retain(|p| p.kind != ParamKind::Query || names.contains(&p.name.as_str()));
        }
        match &mut spec.body {
            Body::FormUrlEncoded { fields } if !drop_form.is_empty() => {
                let mut i = 0;
                fields.retain(|_| {
                    i += 1;
                    !drop_form.contains(&(i - 1))
                });
            }
            Body::Multipart { parts } if !drop_multipart.is_empty() => {
                let mut i = 0;
                parts.retain(|_| {
                    i += 1;
                    !drop_multipart.contains(&(i - 1))
                });
            }
            _ => {}
        }
        if drop_body {
            spec.body = Body::None;
        } else if json_changed && let Some(root) = &self.json {
            let mut counter = 0;
            let rebuilt = self.rebuild(root, 0, &mut counter, eff);
            spec.body = Body::Raw {
                kind: RawKind::Json,
                data: json::write(&rebuilt),
            };
        }
        spec
    }

    fn rebuild(&self, node: &Node, depth: usize, counter: &mut usize, eff: &[bool]) -> Node {
        let listed = |counter: &usize| depth < MAX_DEPTH && *counter < self.json_count;
        let child = |value: &Node, counter: &mut usize| -> Option<Node> {
            if !listed(counter) {
                return Some(value.clone());
            }
            let idx = self.json_first + *counter;
            *counter += 1;
            let sub = self.rebuild(value, depth + 1, counter, eff);
            eff[idx].then_some(sub)
        };
        match node {
            Node::Object(members) => Node::Object(
                members
                    .iter()
                    .filter_map(|(k, v)| child(v, counter).map(|n| (k.clone(), n)))
                    .collect(),
            ),
            Node::Array(items) => {
                Node::Array(items.iter().filter_map(|v| child(v, counter)).collect())
            }
            leaf => leaf.clone(),
        }
    }
}

/// A value the second pass may shorten.
#[derive(Debug, Clone)]
pub struct ValueSlot {
    pub id: String,
    /// Alternatives to try, simplest first (raw text for JSON leaves, plain text elsewhere).
    pub alternatives: Vec<String>,
    loc: SlotLoc,
}

#[derive(Debug, Clone)]
enum SlotLoc {
    Header(usize),
    /// The nth query pair.
    Query(usize),
    /// A JSON leaf, by the order of leaves in a depth-first walk.
    Json(usize),
}

const LONG_VALUE: usize = 16;
const SHORT_VALUE: usize = 8;

fn shorter(text: &str) -> Option<String> {
    (text.chars().count() > LONG_VALUE).then(|| text.chars().take(SHORT_VALUE).collect())
}

fn json_leaves<'a>(node: &'a Node, path: &str, out: &mut Vec<(String, &'a Node)>) {
    match node {
        Node::Object(members) => {
            for (k, v) in members {
                json_leaves(v, &format!("{path}{}", path_key(k)), out);
            }
        }
        Node::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                json_leaves(v, &format!("{path}[{i}]"), out);
            }
        }
        leaf => out.push((path.to_owned(), leaf)),
    }
}

/// The values of `spec` the second pass may shorten: long strings become their first
/// characters, numbers other than 0 and 1 become 0 or 1. Only text the request already
/// holds is used, and credentials (protected headers, auth) are left alone.
pub fn value_slots(spec: &RequestSpec, keep: &[String]) -> Vec<ValueSlot> {
    let mut slots = Vec::new();
    let held = |id: &str| {
        keep.iter()
            .any(|k| k.eq_ignore_ascii_case(id) || id.starts_with(&format!("{k}.")))
    };
    for (i, h) in spec.headers.iter().enumerate() {
        let id = format!("header:{}", h.name);
        if !h.enabled
            || protected_header(&h.name)
            || h.name.eq_ignore_ascii_case("cookie")
            || held(&id)
        {
            continue;
        }
        if let Some(s) = shorter(&h.value) {
            slots.push(ValueSlot {
                id,
                alternatives: vec![s],
                loc: SlotLoc::Header(i),
            });
        }
    }
    if let Some((_, rest)) = spec.url.split_once('?') {
        let query = rest.split_once('#').map_or(rest, |(q, _)| q);
        for (j, pair) in query.split('&').filter(|p| !p.is_empty()).enumerate() {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            if held(&format!("query:{name}")) {
                continue;
            }
            if let Some(s) = shorter(value) {
                slots.push(ValueSlot {
                    id: format!("query:{name}"),
                    alternatives: vec![s],
                    loc: SlotLoc::Query(j),
                });
            }
        }
    }
    if let Body::Raw {
        kind: RawKind::Json,
        data,
    } = &spec.body
        && let Some(root) = json::parse(data)
    {
        let mut leaves = Vec::new();
        json_leaves(&root, "$", &mut leaves);
        for (n, (path, leaf)) in leaves.into_iter().enumerate() {
            let id = format!("body:{path}");
            if held(&id) {
                continue;
            }
            let alternatives = match leaf {
                Node::Str(raw) => serde_json::from_str::<String>(raw)
                    .ok()
                    .and_then(|s| shorter(&s))
                    .map(|s| vec![serde_json::to_string(&s).unwrap_or_default()])
                    .unwrap_or_default(),
                Node::Other(raw) if raw.parse::<f64>().is_ok() => ["0", "1"]
                    .into_iter()
                    .filter(|c| raw != c)
                    .map(str::to_owned)
                    .collect(),
                _ => Vec::new(),
            };
            if !alternatives.is_empty() {
                slots.push(ValueSlot {
                    id,
                    alternatives,
                    loc: SlotLoc::Json(n),
                });
            }
        }
    }
    slots
}

/// `spec` with `slot` set to `alternative` (`None` if the slot no longer fits the request).
pub fn with_value(spec: &RequestSpec, slot: &ValueSlot, alternative: &str) -> Option<RequestSpec> {
    let mut out = spec.clone();
    match slot.loc {
        SlotLoc::Header(i) => {
            let h: &mut KeyValue = out.headers.get_mut(i)?;
            h.value = alternative.to_owned();
        }
        SlotLoc::Query(j) => {
            let (before, rest) = out.url.split_once('?')?;
            let (query, fragment) = match rest.split_once('#') {
                Some((q, f)) => (q, format!("#{f}")),
                None => (rest, String::new()),
            };
            let mut pairs: Vec<String> = query
                .split('&')
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect();
            let pair = pairs.get_mut(j)?;
            let (name, _) = pair.split_once('=')?;
            *pair = format!("{name}={alternative}");
            out.url = format!("{before}?{}{fragment}", pairs.join("&"));
        }
        SlotLoc::Json(n) => {
            let Body::Raw { data, .. } = &out.body else {
                return None;
            };
            let mut root = json::parse(data)?;
            replace_leaf(&mut root, n, alternative)?;
            out.body = Body::Raw {
                kind: RawKind::Json,
                data: json::write(&root),
            };
        }
    }
    Some(out)
}

fn replace_leaf(node: &mut Node, wanted: usize, raw: &str) -> Option<()> {
    fn go(node: &mut Node, wanted: usize, seen: &mut usize, raw: &str) -> bool {
        match node {
            Node::Object(members) => members.iter_mut().any(|(_, v)| go(v, wanted, seen, raw)),
            Node::Array(items) => items.iter_mut().any(|v| go(v, wanted, seen, raw)),
            leaf => {
                *seen += 1;
                if *seen - 1 == wanted {
                    *leaf = if raw.starts_with('"') {
                        Node::Str(raw.to_owned())
                    } else {
                        Node::Other(raw.to_owned())
                    };
                    true
                } else {
                    false
                }
            }
        }
    }
    let mut seen = 0;
    go(node, wanted, &mut seen, raw).then_some(())
}
