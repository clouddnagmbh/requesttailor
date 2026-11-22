// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Writing a [`Query`] back into a request: the URL keeps everything before `?` and the
//! fragment as written, parameters keep their place and, if their value did not change, their
//! exact text; the query rows of the request (`params` of kind `query`) follow the URL, as the
//! editor does (`apps/ui/src/editor/url.ts`).

use rt_plugin_sdk::model::{Param, ParamKind, RequestSpec};

use super::encode::{find_outside, split_outside};
use super::{Query, slot};
use crate::edm::Version;

/// A URL as `base`, query (text after the first `?`, `None` without `?`) and fragment (`#…`
/// or `""`), split outside `{{templates}}`.
pub fn split_url(url: &str) -> (&str, Option<&str>, &str) {
    let Some(q) = find_outside(url, '?') else {
        return (url, None, "");
    };
    let rest = &url[q + 1..];
    match find_outside(rest, '#') {
        Some(h) => (&url[..q], Some(&rest[..h]), &rest[h..]),
        None => (&url[..q], Some(rest), ""),
    }
}

/// The query string of `url` (without `?`), `""` if it has none.
pub fn query_of(url: &str) -> &str {
    split_url(url).1.unwrap_or("")
}

/// `url` with its query string replaced by `query`. Parameters that are in both keep their
/// position; those whose value is unchanged keep their exact text (so `%20` versus `+` or the
/// order inside the URL does not change by applying the form). New parameters are appended.
pub fn with_query(url: &str, query: &Query, version: Version) -> String {
    let (base, old, fragment) = split_url(url);
    let new = query.pieces(version);
    let mut used = vec![false; new.len()];
    let mut pieces: Vec<&str> = Vec::new();
    for raw in split_outside(old.unwrap_or(""), '&') {
        if raw.is_empty() {
            continue;
        }
        let s = slot(raw);
        let Some(j) = (0..new.len()).find(|&j| !used[j] && slot(&new[j]) == s) else {
            continue;
        };
        used[j] = true;
        pieces.push(if same(raw, &new[j]) { raw } else { &new[j] });
    }
    pieces.extend(
        new.iter()
            .zip(&used)
            .filter(|(_, u)| !**u)
            .map(|(p, _)| p.as_str()),
    );
    if pieces.is_empty() {
        format!("{base}{fragment}")
    } else {
        format!("{base}?{}{fragment}", pieces.join("&"))
    }
}

/// Same name as written (decoded) and the same meaning.
fn same(a: &str, b: &str) -> bool {
    let name = |raw: &str| super::encode::decode(super::split_pair(raw).0);
    name(a) == name(b) && Query::parse(a) == Query::parse(b)
}

/// `spec` with the URL's query replaced (see [`with_query`]) and its query rows following the
/// new URL.
pub fn update_spec(spec: &RequestSpec, query: &Query, version: Version) -> RequestSpec {
    let mut spec = spec.clone();
    spec.url = with_query(&spec.url, query, version);
    spec.params = params_from_url(&spec.url, &spec.params);
    spec
}

/// The query rows mirror the URL's pairs as written; disabled rows stay where they were, path
/// rows stay unchanged (the path is not touched). Same rules as `paramsFromUrl` of the editor.
pub fn params_from_url(url: &str, old: &[Param]) -> Vec<Param> {
    let pairs: Vec<(String, String)> = split_outside(query_of(url), '&')
        .into_iter()
        .filter(|raw| !raw.is_empty())
        .map(|raw| match find_outside(raw, '=') {
            Some(i) => (raw[..i].to_owned(), raw[i + 1..].to_owned()),
            None => (raw.to_owned(), String::new()),
        })
        .collect();
    let row = |(name, value): &(String, String)| Param {
        name: name.clone(),
        value: value.clone(),
        kind: ParamKind::Query,
        enabled: true,
    };
    let mut rows = Vec::new();
    let mut k = 0;
    for p in old.iter().filter(|p| p.kind == ParamKind::Query) {
        if !p.enabled {
            rows.push(p.clone());
        } else if !(p.name.is_empty() && p.value.is_empty()) && k < pairs.len() {
            rows.push(row(&pairs[k]));
            k += 1;
        }
    }
    rows.extend(pairs[k..].iter().map(row));
    rows.extend(old.iter().filter(|p| p.kind == ParamKind::Path).cloned());
    rows
}
