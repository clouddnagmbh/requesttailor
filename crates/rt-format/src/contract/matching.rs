// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Which operation of the spec a request talks to: method and path template, after the base
//! URL (a variable, or a literal scheme and host) and the servers' path prefix are removed.

use rt_core::model::{Collection, Item, Protocol, RequestEntry};

use super::spec::{Operation, Spec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Seg {
    Lit(String),
    /// `:name`.
    Colon(String),
    /// Contains a `{{variable}}`: matches anything.
    Var,
}

#[derive(Debug, Clone)]
pub(super) struct ReqUrl {
    pub segs: Vec<Seg>,
    /// The text of each segment of `segs`, as written.
    pub raw: Vec<String>,
    /// Names in the query string, templates left out.
    pub query: Vec<String>,
}

pub(super) fn parse_url(url: &str) -> ReqUrl {
    let url = url.trim();
    let url = url.split('#').next().unwrap_or("");
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    };
    let mut rest = path;
    if let Some(i) = rest.find("://")
        && rest[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    {
        let after = &rest[i + 3..];
        rest = after.find('/').map_or("", |j| &after[j..]);
    } else if rest.starts_with("{{")
        && let Some(end) = rest.find("}}")
    {
        rest = &rest[end + 2..];
    }
    let raw: Vec<String> = rest
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let segs = raw
        .iter()
        .map(|s| {
            if s.contains("{{") {
                Seg::Var
            } else if let Some(name) = s.strip_prefix(':') {
                Seg::Colon(name.to_owned())
            } else {
                Seg::Lit(s.to_owned())
            }
        })
        .collect();
    let query = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| p.split('=').next().unwrap_or("").to_owned())
        .filter(|n| !n.is_empty() && !n.contains("{{"))
        .collect();
    ReqUrl { segs, raw, query }
}

/// Whether a request segment fits a spec segment (a literal or `{name}`, maybe with text
/// around).
fn seg_fits(spec: &str, req: &Seg) -> bool {
    match (spec.find('{'), spec.rfind('}')) {
        (Some(a), Some(b)) if a < b => match req {
            Seg::Lit(l) => {
                let (pre, post) = (&spec[..a], &spec[b + 1..]);
                l.len() > pre.len() + post.len() && l.starts_with(pre) && l.ends_with(post)
            }
            _ => true,
        },
        _ => match req {
            Seg::Lit(l) => l == spec,
            Seg::Var => true,
            Seg::Colon(_) => false,
        },
    }
}

/// How well the request path fits the template: the number of literal segments that matched
/// (more is more specific), or `None`.
fn path_score(spec: &Spec, op: &Operation, req: &ReqUrl) -> Option<usize> {
    let (n, m) = (req.segs.len(), op.segs.len());
    if n < m {
        return None;
    }
    let k = n - m;
    let prefix_ok = |bases: &[String]| {
        req.segs[..k].iter().enumerate().all(|(i, s)| match s {
            Seg::Var => true,
            Seg::Lit(l) => bases.get(i).is_some_and(|b| b == l),
            Seg::Colon(_) => false,
        })
    };
    if k > 0 && !(prefix_ok(&[]) || spec.base_paths.iter().any(|b| prefix_ok(b))) {
        return None;
    }
    let mut score = 0;
    for (s, r) in op.segs.iter().zip(&req.segs[k..]) {
        if !seg_fits(s, r) {
            return None;
        }
        if !s.contains('{') {
            score += 1;
        }
    }
    Some(score)
}

pub(super) enum Match<'a> {
    Op(&'a Operation),
    /// The path exists, but only with these methods.
    OtherMethods(Vec<&'a str>),
    None,
}

pub(super) fn find<'a>(spec: &'a Spec, method: &str, req: &ReqUrl) -> Match<'a> {
    let method = method.to_uppercase();
    let mut best: Option<(usize, &Operation)> = None;
    let mut others: Vec<&str> = Vec::new();
    for op in &spec.ops {
        let Some(score) = path_score(spec, op, req) else {
            continue;
        };
        if op.method == method {
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, op));
            }
        } else {
            others.push(&op.method);
        }
    }
    match best {
        Some((_, op)) => Match::Op(op),
        None if others.is_empty() => Match::None,
        None => {
            others.sort_unstable();
            others.dedup();
            Match::OtherMethods(others)
        }
    }
}

/// Every HTTP request of the collection, in tree order.
pub(super) fn requests(collection: &Collection) -> Vec<&RequestEntry> {
    fn walk<'a>(items: &'a [Item], out: &mut Vec<&'a RequestEntry>) {
        for item in items {
            match item {
                Item::Request(r) => {
                    if matches!(r.spec.protocol, Protocol::Http)
                        && !matches!(r.spec.method.as_str(), "GRPC" | "WS")
                    {
                        out.push(r);
                    }
                }
                Item::Folder(f) => walk(&f.items, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(&collection.items, &mut out);
    out
}
