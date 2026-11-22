// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Which script APIs an imported script uses, and which of them the shims of `rt-script`
//! do not support (wave 7). Imported scripts are kept as they are; the loss report names
//! the unsupported APIs with their location.
//!
//! The scan is static and conservative: comments and string contents are ignored, an API is
//! a member path from a known root (`pm.environment.get`), a called root (`xml2Json()`), a
//! module (`require('xml2js')`) or a chai word (`expect().cookie`). It is the Rust twin of
//! `apiUses` in `scripts/shim-frequency.mjs`; both read the list in `shim-apis.txt`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The script dialect: which globals count as API roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    Postman,
    Insomnia,
}

/// Globals any sandbox script may use.
const COMMON: &[&str] = &[
    "require",
    "atob",
    "btoa",
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "fetch",
    "URL",
    "URLSearchParams",
    "Buffer",
    "process",
    "TextEncoder",
    "TextDecoder",
];

const POSTMAN: &[&str] = &[
    "pm",
    "postman",
    "tests",
    "responseBody",
    "responseCode",
    "responseHeaders",
    "responseTime",
    "responseCookies",
    "environment",
    "globals",
    "iteration",
    "request",
    "xml2Json",
    "xmlToJson",
    "CryptoJS",
    "_",
    "cheerio",
    "tv4",
    "expect",
];

const INSOMNIA: &[&str] = &["insomnia", "expect"];

/// Roots that count when used bare (not called, no member).
const BARE: &[&str] = &[
    "responseBody",
    "responseCode",
    "responseHeaders",
    "responseTime",
    "responseCookies",
    "CryptoJS",
    "_",
    "iteration",
];

/// chai language chains: words without effect.
const CHAINS: &[&str] = &[
    "to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same",
    "but", "does", "still", "also",
];

/// Members of the legacy `request` global; other members are a user variable's.
const REQUEST_MEMBERS: &[&str] = &[
    "url",
    "method",
    "headers",
    "data",
    "name",
    "id",
    "body",
    "variables",
];

/// Roots whose unlisted members are reported too (they are the tools' own objects).
const OWN_ROOTS: &[&str] = &["pm", "postman", "insomnia"];

/// Support status of an API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Supported,
    Partial,
    Unsupported,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Entry {
    pub status: Status,
    pub note: &'static str,
}

/// `shim-apis.txt`, parsed once.
fn list() -> &'static HashMap<&'static str, Entry> {
    static LIST: OnceLock<HashMap<&'static str, Entry>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut map = HashMap::new();
        for line in include_str!("shim-apis.txt").lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, ' ');
            let (Some(api), Some(status)) = (parts.next(), parts.next()) else {
                continue;
            };
            let status = match status {
                "supported" => Status::Supported,
                "partial" => Status::Partial,
                "unsupported" => Status::Unsupported,
                _ => continue,
            };
            map.insert(
                api,
                Entry {
                    status,
                    note: parts.next().unwrap_or("").trim(),
                },
            );
        }
        map
    })
}

/// The list entry that covers `api`: the longest listed member-path prefix; `insomnia.*`
/// falls back to the `pm.*` entry; an unknown module to `require('*')`. Returns the entry's
/// name with it.
pub(crate) fn coverage_of(api: &str) -> Option<(String, Entry)> {
    let list = list();
    let lookup = |name: &str| -> Option<(String, Entry)> {
        if let Some(e) = list.get(name) {
            return Some((name.to_owned(), *e));
        }
        let parts: Vec<&str> = name.split('.').collect();
        (1..parts.len()).rev().find_map(|k| {
            let prefix = parts[..k].join(".");
            list.get(prefix.as_str()).map(|e| (prefix, *e))
        })
    };
    if let Some(found) = lookup(api) {
        return Some(found);
    }
    if api.starts_with("require(") {
        return list
            .get("require('*')")
            .map(|e| ("require('*')".to_owned(), *e));
    }
    let rest = api.strip_prefix("insomnia.")?;
    let (name, e) = lookup(&format!("pm.{rest}"))?;
    Some((format!("insomnia.{}", &name[3..]), e))
}

/// An API the shims do not support, as the loss report names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Gap {
    pub api: String,
    /// The list's note; empty if there is none or the API is not in the list.
    pub note: &'static str,
}

/// The unsupported APIs `code` uses, each once, in the order of first use. An unknown member
/// of the tools' own objects (`pm.somethingNew`) counts as unsupported.
pub(crate) fn gaps(code: &str, dialect: Dialect) -> Vec<Gap> {
    let mut out: Vec<Gap> = Vec::new();
    for api in api_uses(code, dialect) {
        let gap = match coverage_of(&api) {
            Some((name, e)) if e.status == Status::Unsupported => Gap {
                // Unknown modules keep their name.
                api: if name == "require('*')" { api } else { name },
                note: e.note,
            },
            Some(_) => continue,
            None if OWN_ROOTS.iter().any(|r| api.starts_with(&format!("{r}."))) => {
                Gap { api, note: "" }
            }
            None => continue,
        };
        if !out.iter().any(|g| g.api == gap.api) {
            out.push(gap);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Scanning

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn is_ident(c: u8) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// `code` with comments and the contents of string, template and regex literals replaced
/// by spaces (offsets and newlines kept). `${...}` inside templates stays code.
fn blank_literals(code: &str) -> Vec<u8> {
    let src = code.as_bytes();
    let n = src.len();
    let mut out = src.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for b in &mut out[from.min(n)..to.min(n)] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    };
    let regex_allowed = |at: usize| {
        let mut k = at;
        while k > 0 && src[k - 1].is_ascii_whitespace() {
            k -= 1;
        }
        if k == 0 {
            return true;
        }
        let prev = src[k - 1];
        b"(,=:[!&|?{};+-*%<>~^".contains(&prev)
            || [&b"return"[..], b"typeof", b"case"]
                .iter()
                .any(|w| src[..k].ends_with(w) && (k == w.len() || !is_ident(src[k - w.len() - 1])))
    };
    let mut i = 0;
    while i < n {
        let c = src[i];
        let d = src.get(i + 1).copied();
        if c == b'/' && d == Some(b'/') {
            let stop = src[i..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(n, |p| i + p);
            blank(&mut out, i, stop);
            i = stop;
        } else if c == b'/' && d == Some(b'*') {
            let stop = src[i + 2..]
                .windows(2)
                .position(|w| w == b"*/")
                .map_or(n, |p| i + 2 + p + 2);
            blank(&mut out, i, stop);
            i = stop;
        } else if c == b'\'' || c == b'"' {
            let mut k = i + 1;
            while k < n && src[k] != c && src[k] != b'\n' {
                k += if src[k] == b'\\' { 2 } else { 1 };
            }
            blank(&mut out, i + 1, k);
            i = k + 1;
        } else if c == b'`' {
            let mut k = i + 1;
            let mut from = k;
            while k < n && src[k] != b'`' {
                if src[k] == b'\\' {
                    k += 2;
                } else if src[k] == b'$' && src.get(k + 1) == Some(&b'{') {
                    blank(&mut out, from, k);
                    let mut depth = 1;
                    k += 2;
                    while k < n && depth > 0 {
                        match src[k] {
                            b'{' => depth += 1,
                            b'}' => depth -= 1,
                            _ => {}
                        }
                        k += 1;
                    }
                    from = k;
                } else {
                    k += 1;
                }
            }
            blank(&mut out, from, k);
            i = k + 1;
        } else if c == b'/' && regex_allowed(i) {
            let mut k = i + 1;
            let mut class = false;
            while k < n && src[k] != b'\n' {
                match src[k] {
                    b'\\' => k += 1,
                    b'[' => class = true,
                    b']' => class = false,
                    b'/' if !class => break,
                    _ => {}
                }
                k += 1;
            }
            if k < n && src[k] == b'/' {
                blank(&mut out, i + 1, k);
                i = k + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

fn skip_space(t: &[u8], mut k: usize) -> usize {
    while k < t.len() && t[k].is_ascii_whitespace() {
        k += 1;
    }
    k
}

fn ident_at(t: &[u8], k: usize) -> Option<&str> {
    if k >= t.len() || !is_ident_start(t[k]) {
        return None;
    }
    let end = t[k..]
        .iter()
        .position(|&b| !is_ident(b))
        .map_or(t.len(), |p| k + p);
    std::str::from_utf8(&t[k..end]).ok()
}

/// Index after the bracket that closes the one at `open`.
fn closing(t: &[u8], open: usize) -> usize {
    let close = |b: u8| match b {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        _ => None,
    };
    let mut stack: Vec<u8> = close(t[open]).into_iter().collect();
    let mut k = open + 1;
    while k < t.len() && !stack.is_empty() {
        if let Some(c) = close(t[k]) {
            stack.push(c);
        } else if Some(&t[k]) == stack.last() {
            stack.pop();
        }
        k += 1;
    }
    k
}

/// The member chain at `k`: its segments, where it ends, and whether a call follows.
fn chain_at(t: &[u8], mut k: usize) -> (Vec<&str>, usize, bool) {
    let mut segments = Vec::new();
    loop {
        let j = skip_space(t, k);
        let dot = match t.get(j) {
            Some(b'.') => 1,
            Some(b'?') if t.get(j + 1) == Some(&b'.') => 2,
            Some(b'(') => return (segments, j, true),
            _ => return (segments, k, false),
        };
        let at = skip_space(t, j + dot);
        let Some(id) = ident_at(t, at) else {
            return (segments, k, false);
        };
        segments.push(id);
        k = at + id.len();
    }
}

/// Assertion words of an `expect(...)` chain from `k`: `.to.not.equal(1)` → `not`, `equal`.
fn expect_words(t: &[u8], mut k: usize) -> Vec<&str> {
    let mut words = Vec::new();
    loop {
        let (segments, after, called) = chain_at(t, k);
        words.extend(segments.into_iter().filter(|s| !CHAINS.contains(s)));
        if !called {
            return words;
        }
        k = closing(t, after);
    }
}

/// Every API use in `code`, as `shim-apis.txt` names them.
pub(crate) fn api_uses(code: &str, dialect: Dialect) -> Vec<String> {
    let t = blank_literals(code);
    let own = match dialect {
        Dialect::Postman => POSTMAN,
        Dialect::Insomnia => INSOMNIA,
    };
    let mut found = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if !is_ident_start(t[i]) || (i > 0 && (is_ident(t[i - 1]) || t[i - 1] == b'.')) {
            i += 1;
            continue;
        }
        let Some(root) = ident_at(&t, i) else {
            i += 1;
            continue;
        };
        let start = i + root.len();
        let at = i;
        i = start;
        if !own.contains(&root) && !COMMON.contains(&root) {
            continue;
        }
        // Declarations and parameters (`const request = …`) are not uses.
        let before = std::str::from_utf8(&t[..at]).unwrap_or("").trim_end();
        if ["const", "let", "var", "function"].iter().any(|w| {
            before.ends_with(w)
                && before[..before.len() - w.len()]
                    .bytes()
                    .last()
                    .is_none_or(|b| !is_ident(b))
        }) {
            continue;
        }
        let after = skip_space(&t, start);
        let next = t.get(after).copied();
        if root == "require" {
            if next != Some(b'(') {
                continue;
            }
            let end = closing(&t, after);
            let arg = code
                .get(after + 1..end.saturating_sub(1))
                .unwrap_or("")
                .trim();
            let name = arg
                .strip_prefix(['\'', '"', '`'])
                .and_then(|a| a.strip_suffix(['\'', '"', '`']))
                .unwrap_or("?");
            found.push(format!("require('{name}')"));
            continue;
        }
        if next == Some(b'=') && !matches!(t.get(after + 1), Some(b'=' | b'>')) {
            continue;
        }
        if next == Some(b'[') {
            found.push(format!("{root}[]"));
            continue;
        }
        let (segments, end, called) = chain_at(&t, start);
        let via_root =
            (root == "pm" || root == "insomnia") && segments.len() == 1 && segments[0] == "expect";
        if root == "expect" || (via_root && called) {
            if !called {
                continue;
            }
            if via_root {
                found.push(format!("{root}.expect"));
            }
            let close = closing(&t, end);
            found.extend(
                expect_words(&t, close)
                    .into_iter()
                    .map(|w| format!("expect().{w}")),
            );
            continue;
        }
        if segments.is_empty() {
            if called {
                found.push(format!("{root}()"));
            } else if BARE.contains(&root) {
                found.push(root.to_owned());
            }
            continue;
        }
        if (root == "pm" || root == "insomnia")
            && segments[0] == "response"
            && segments.get(1) == Some(&"to")
        {
            let word = segments[2..]
                .iter()
                .find(|w| !CHAINS.contains(w) && **w != "not");
            let mut api = format!("{root}.response.to");
            if let Some(w) = word {
                api.push('.');
                api.push_str(w);
            }
            found.push(api);
            continue;
        }
        if root == "request" && !REQUEST_MEMBERS.contains(&segments[0]) {
            continue;
        }
        found.push(format!("{root}.{}", segments.join(".")));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uses(code: &str) -> Vec<String> {
        api_uses(code, Dialect::Postman)
    }

    #[test]
    fn member_paths_calls_and_words() {
        assert_eq!(
            uses(
                "pm.environment.set('a', 1); // pm.globals.get('x')\nconst s = \"pm.vault.get()\";"
            ),
            ["pm.environment.set"]
        );
        assert_eq!(
            uses("pm.expect(x).to.not.have.property('a');"),
            ["pm.expect", "expect().not", "expect().property"]
        );
        assert_eq!(
            uses("pm.response.to.have.status(200);"),
            ["pm.response.to.status"]
        );
        assert_eq!(
            uses("tests['ok'] = responseCode.code === 200;"),
            ["tests[]", "responseCode.code"]
        );
        assert_eq!(
            uses("const C = require('crypto-js'); const x = require(\"xml2js\");"),
            ["require('crypto-js')", "require('xml2js')"]
        );
        assert_eq!(
            uses("const x = `${pm.variables.get('a')}`;"),
            ["pm.variables.get"]
        );
        assert_eq!(
            uses("const r = /pm.vault/g; xml2Json(responseBody);"),
            ["xml2Json()", "responseBody"]
        );
        assert_eq!(
            uses("const request = {}; request.foo = 1; a.pm.x;"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn coverage_lookup() {
        assert_eq!(
            coverage_of("pm.environment.get").unwrap().1.status,
            Status::Supported
        );
        assert_eq!(
            coverage_of("pm.environment.clear").unwrap().1.status,
            Status::Unsupported
        );
        let (name, e) = coverage_of("insomnia.parentFolders.get").unwrap();
        assert_eq!(
            (name.as_str(), e.status),
            ("insomnia.parentFolders", Status::Unsupported)
        );
        assert_eq!(
            coverage_of("insomnia.environment.get").unwrap().0,
            "insomnia.environment.get"
        );
        assert_eq!(
            coverage_of("require('fs')").unwrap().1.status,
            Status::Unsupported
        );
        assert_eq!(
            coverage_of("require('crypto-js')").unwrap().1.status,
            Status::Partial
        );
        assert!(coverage_of("nothing.at.all").is_none());
    }

    #[test]
    fn gaps_once_each_in_order() {
        let code = "pm.vault.get('a'); const x = require('fs'); pm.vault.set('b');\n\
                    pm.environment.set('c', 1); pm.somethingNew(); xml2Json(responseBody);";
        let got: Vec<String> = gaps(code, Dialect::Postman)
            .into_iter()
            .map(|g| g.api)
            .collect();
        assert_eq!(
            got,
            ["pm.vault", "require('fs')", "pm.somethingNew", "xml2Json()"]
        );
        assert!(
            gaps(
                "pm.test('t', () => pm.expect(1).to.equal(1));",
                Dialect::Postman
            )
            .is_empty()
        );
    }

    #[test]
    fn the_list_parses() {
        assert!(list().len() > 300);
        assert!(
            list()
                .values()
                .all(|e| e.status != Status::Partial || !e.note.is_empty())
        );
    }
}
