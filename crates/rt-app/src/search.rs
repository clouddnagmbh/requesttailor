// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Full-text search over the open collection, including unsaved changes.
//!
//! Case-insensitive substring search (Unicode lowercase) over every request's name, URL,
//! params, headers (name and value), body (raw text, form fields, multipart text parts), auth
//! fields and variables. Secret values are never searched or shown: secret variables match by
//! name only, and a Basic password is skipped (it is often typed in literally).
//!
//! One hit per matching field (each param, header, form field, … is a field; a body is one
//! field, shown at its first matching line). Hits are ranked name > URL > everything else,
//! then in tree order. The snippet is one line of at most [`SNIPPET_CHARS`] characters
//! around the first match; `ranges` are UTF-16 offsets into it (JavaScript string indexes).
//!
//! Owner: wave 4, strand E. Changes to the public types are interface changes.

use rt_core::model::{Auth, Body, Item, PartKind, RequestSpec};
use serde::{Deserialize, Serialize};

use crate::Workspace;

/// Hits returned at most.
pub const SEARCH_LIMIT: usize = 200;

/// Longest snippet, in characters (without the `…` marks).
pub const SNIPPET_CHARS: usize = 80;
/// Characters of context kept before the first match when a snippet is cut.
const CONTEXT_BEFORE: usize = 24;
const ELLIPSIS: &str = "…";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    /// Request name.
    pub name: String,
    pub method: String,
    pub field: SearchField,
    /// The matching text with some context, on one line.
    pub snippet: String,
    /// Matches in `snippet` as `[start, end)` UTF-16 offsets (JavaScript string indexes).
    pub ranges: Vec<(u32, u32)>,
}

/// Where a hit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchField {
    Name,
    Url,
    Param,
    Header,
    Body,
    Auth,
    Variable,
}

impl SearchField {
    /// Ranking bucket: name, URL, the rest.
    fn rank(self) -> usize {
        match self {
            SearchField::Name => 0,
            SearchField::Url => 1,
            _ => 2,
        }
    }
}

/// Requests of `workspace` matching `query` (case-insensitive), best first, at most `limit`
/// (and never more than [`SEARCH_LIMIT`]). An empty query finds nothing.
pub fn search(workspace: &Workspace, query: &str, limit: usize) -> Vec<SearchHit> {
    let limit = limit.min(SEARCH_LIMIT);
    let Some(needle) = Needle::new(query.trim()) else {
        return Vec::new();
    };
    if limit == 0 {
        return Vec::new();
    }
    let mut buckets: [Vec<SearchHit>; 3] = Default::default();
    let mut requests = Vec::new();
    collect_requests(&workspace.collection().items, &mut requests);
    for (path, spec) in requests {
        // Enough of the best kind of hit: nothing later can rank higher.
        if buckets[0].len() >= limit {
            break;
        }
        let mut add = |field: SearchField, text: &str| {
            let bucket = &mut buckets[field.rank()];
            if bucket.len() >= limit {
                return;
            }
            if let Some((snippet, ranges)) = needle.snippet(text) {
                bucket.push(SearchHit {
                    path: path.to_owned(),
                    name: spec.name.clone(),
                    method: spec.method.clone(),
                    field,
                    snippet,
                    ranges,
                });
            }
        };
        request_fields(spec, &mut add);
    }
    buckets.into_iter().flatten().take(limit).collect()
}

/// Requests in tree order (the order of the model's items).
fn collect_requests<'a>(items: &'a [Item], out: &mut Vec<(&'a str, &'a RequestSpec)>) {
    for item in items {
        match item {
            Item::Request(r) => out.push((&r.path, &r.spec)),
            Item::Folder(f) => collect_requests(&f.items, out),
        }
    }
}

/// Calls `add` with every searchable text of `spec`, in display order.
fn request_fields(spec: &RequestSpec, add: &mut impl FnMut(SearchField, &str)) {
    add(SearchField::Name, &spec.name);
    add(SearchField::Url, &spec.url);
    for p in &spec.params {
        add(SearchField::Param, &format!("{}={}", p.name, p.value));
    }
    for h in &spec.headers {
        add(SearchField::Header, &format!("{}: {}", h.name, h.value));
    }
    match &spec.body {
        Body::Raw { data, .. } => add(SearchField::Body, data),
        Body::FormUrlEncoded { fields } => {
            for f in fields {
                add(SearchField::Body, &format!("{}={}", f.name, f.value));
            }
        }
        Body::Multipart { parts } => {
            for p in parts {
                match p.kind {
                    PartKind::Text => add(
                        SearchField::Body,
                        &format!("{}={}", p.name, p.values.join(", ")),
                    ),
                    PartKind::File => add(SearchField::Body, &p.name),
                }
            }
        }
        Body::None | Body::File { .. } => {}
    }
    match &spec.auth {
        Auth::Basic { username, .. } => add(SearchField::Auth, &format!("username: {username}")),
        Auth::Bearer { token } => add(SearchField::Auth, &format!("token: {token}")),
        Auth::ApiKey { key, value, .. } => add(SearchField::Auth, &format!("{key}: {value}")),
        Auth::Inherit | Auth::None | Auth::Unsupported { .. } => {}
    }
    for v in &spec.vars {
        match (&v.value, v.secret) {
            (Some(value), false) => add(SearchField::Variable, &format!("{}={value}", v.name)),
            _ => add(SearchField::Variable, &v.name),
        }
    }
}

/// The lowercased query.
struct Needle {
    lower: String,
    chars: Vec<char>,
    ascii: Option<Vec<u8>>,
}

impl Needle {
    fn new(query: &str) -> Option<Self> {
        if query.is_empty() {
            return None;
        }
        let lower = fold(query);
        Some(Self {
            ascii: lower.is_ascii().then(|| lower.as_bytes().to_vec()),
            chars: lower.chars().collect(),
            lower,
        })
    }

    /// Byte ranges of the non-overlapping matches in `text`; with `first`, only the first.
    fn find(&self, text: &str, first: bool) -> Vec<(usize, usize)> {
        match &self.ascii {
            Some(needle) if text.is_ascii() => find_ascii(text.as_bytes(), needle, first),
            // ASCII lowercases to ASCII: a needle with other characters cannot match.
            None if text.is_ascii() => Vec::new(),
            _ => self.find_folded(text, first),
        }
    }

    /// Unicode path: compares lowercase characters, mapped back to `text`'s bytes.
    fn find_folded(&self, text: &str, first: bool) -> Vec<(usize, usize)> {
        // Cheap rejection first (most texts do not match).
        if !fold(text).contains(&self.lower) {
            return Vec::new();
        }
        // Each folded char remembers the byte range of the original char it came from (one
        // char can lowercase to several, e.g. `İ` → `i̇`).
        let mut folded: Vec<char> = Vec::with_capacity(text.len());
        let mut origin: Vec<(usize, usize)> = Vec::with_capacity(text.len());
        for (i, c) in text.char_indices() {
            for l in c.to_lowercase() {
                folded.push(if l == FINAL_SIGMA { SIGMA } else { l });
                origin.push((i, i + c.len_utf8()));
            }
        }
        let n = self.chars.len();
        let mut out = Vec::new();
        let mut i = 0;
        while i + n <= folded.len() {
            if folded[i..i + n] == self.chars[..] {
                out.push((origin[i].0, origin[i + n - 1].1));
                if first {
                    break;
                }
                i += n;
            } else {
                i += 1;
            }
        }
        out
    }

    /// Snippet and UTF-16 ranges for the first match in `text`, or `None` if it does not
    /// match.
    fn snippet(&self, text: &str) -> Option<(String, Vec<(u32, u32)>)> {
        let &(first, _) = self.find(text, true).first()?;
        // The line of the first match, without leading and trailing whitespace.
        let line_start = text[..first].rfind(['\n', '\r']).map_or(0, |i| i + 1);
        let line_end = text[first..]
            .find(['\n', '\r'])
            .map_or(text.len(), |i| first + i);
        let line = &text[line_start..line_end];
        let trimmed = line.trim_start();
        let lead = line_start + (line.len() - trimmed.len());
        let line = trimmed.trim_end();
        let first = first - lead;
        let matches = self.find(line, false);

        // Window of at most SNIPPET_CHARS chars, starting a little before the first match.
        let total = line.chars().count();
        let (start, end) = if total <= SNIPPET_CHARS {
            (0, line.len())
        } else {
            let before = line[..first].chars().count();
            let skip = before
                .saturating_sub(CONTEXT_BEFORE)
                .min(total - SNIPPET_CHARS);
            let start = line.char_indices().nth(skip).map_or(0, |(i, _)| i);
            let end = line[start..]
                .char_indices()
                .nth(SNIPPET_CHARS)
                .map_or(line.len(), |(i, _)| start + i);
            // Never cut the first match.
            (start, matches.first().map_or(end, |m| end.max(m.1)))
        };

        let mut snippet = String::with_capacity(end - start + 2 * ELLIPSIS.len());
        if start > 0 {
            snippet.push_str(ELLIPSIS);
        }
        let offset = utf16_len(&snippet);
        // One line: tabs and other control characters become spaces.
        snippet.extend(
            line[start..end]
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c }),
        );
        if end < line.len() {
            snippet.push_str(ELLIPSIS);
        }
        let ranges = matches
            .iter()
            .filter(|&&(s, e)| s >= start && e <= end)
            .map(|&(s, e)| {
                let s16 = offset + utf16_len(&line[start..s]);
                (s16, s16 + utf16_len(&line[s..e]))
            })
            .collect();
        Some((snippet, ranges))
    }
}

const SIGMA: char = 'σ';
const FINAL_SIGMA: char = 'ς';

/// Lowercase for comparing: `str::to_lowercase` (fast on ASCII), with the word-final sigma it
/// produces for a capital sigma folded to the normal one, so the result equals lowercasing
/// char by char (which the match positions are computed with).
fn fold(text: &str) -> String {
    let lower = text.to_lowercase();
    if lower.contains(FINAL_SIGMA) {
        lower.replace(FINAL_SIGMA, "σ")
    } else {
        lower
    }
}

fn find_ascii(hay: &[u8], needle: &[u8], first: bool) -> Vec<(usize, usize)> {
    let n = needle.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= hay.len() {
        if hay[i].to_ascii_lowercase() == needle[0] && hay[i..i + n].eq_ignore_ascii_case(needle) {
            out.push((i, i + n));
            if first {
                break;
            }
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}
