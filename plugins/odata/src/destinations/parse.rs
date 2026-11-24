// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reading exported destination files: JSON (one destination, the Destination service's
//! `destinationConfiguration` wrapper, `{"destinations": [...]}` or an array of these) and
//! Java `.properties` (`Key=Value`, one destination per file).
//!
//! The export formats are assumptions written against synthetic files
//! (`testdata/destinations/SOURCES.md`); nothing here was checked against a real export.

use rt_plugin_sdk::api::PluginFile;
use rt_plugin_sdk::{Result, bail};
use serde_json::Value;

/// One destination: its properties in file order, values as text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Destination {
    pub props: Vec<(String, String)>,
}

impl Destination {
    /// The value of a property; names compare ignoring case, empty values count as absent.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    }

    /// Enough of a destination to be one. Exports spell their keys the same way every time,
    /// so this reads them case-sensitively: a `Name` and two more of `Type`, `URL`,
    /// `Authentication`, `ProxyType`, or a `Type` that is one of the destination types (a
    /// mail destination has little else).
    pub fn looks_like_one(&self) -> bool {
        let has = |key: &str| {
            self.props
                .iter()
                .any(|(k, v)| k == key && !v.trim().is_empty())
        };
        let typed = self
            .props
            .iter()
            .any(|(k, v)| k == "Type" && super::TYPES.contains(&v.trim()));
        has("Name")
            && (typed
                || ["Type", "URL", "Authentication", "ProxyType"]
                    .iter()
                    .filter(|k| has(k))
                    .count()
                    >= 2)
    }
}

/// The destinations of one file; an error names the file.
pub fn parse_file(file: &PluginFile) -> Result<Vec<Destination>> {
    let text = rt_plugin_sdk::file_text(file)?;
    let text = text.trim_start_matches('\u{feff}');
    let found = if text.trim_start().starts_with(['{', '[']) {
        let Ok(root) = serde_json::from_str::<Value>(text) else {
            bail!("{} is not valid JSON", file.name);
        };
        let mut out = Vec::new();
        collect(&root, 0, &mut out);
        out
    } else if file.name.to_ascii_lowercase().ends_with(".json") {
        bail!("{} is not valid JSON", file.name);
    } else {
        vec![Destination {
            props: properties(text),
        }]
    };
    if found.is_empty() || !found.iter().all(Destination::looks_like_one) {
        bail!("{} is not an exported BTP destination", file.name);
    }
    Ok(found)
}

fn collect(value: &Value, depth: u32, out: &mut Vec<Destination>) {
    if depth > 4 {
        return;
    }
    match value {
        Value::Array(items) => items.iter().for_each(|v| collect(v, depth + 1, out)),
        Value::Object(map) => {
            if let Some(inner @ Value::Object(_)) = map.get("destinationConfiguration") {
                collect(inner, depth + 1, out);
            } else if let Some(list @ Value::Array(_)) = map.get("destinations") {
                collect(list, depth + 1, out);
            } else {
                out.push(Destination {
                    props: map
                        .iter()
                        .filter_map(|(k, v)| scalar(v).map(|v| (k.clone(), v)))
                        .collect(),
                });
            }
        }
        // A bare number or string in an array makes the file not a destination export.
        _ => out.push(Destination::default()),
    }
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Java properties: `#`/`!` comments, `=`, `:` or whitespace as separator, `\` line
/// continuation and the escapes `\t \n \r \f \uXXXX` and `\<any>`.
pub fn properties(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(first) = lines.next() {
        let trimmed = first.trim_start();
        if trimmed.is_empty() || trimmed.starts_with(['#', '!']) {
            continue;
        }
        let mut logical = trimmed.to_owned();
        while ends_with_odd_backslashes(&logical) {
            logical.pop();
            match lines.next() {
                Some(next) => logical.push_str(next.trim_start()),
                None => break,
            }
        }
        let (key, value) = split(&logical);
        out.push((unescape(key), unescape(value)));
    }
    out
}

fn ends_with_odd_backslashes(s: &str) -> bool {
    s.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1
}

/// The raw key and value of one logical line.
fn split(line: &str) -> (&str, &str) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'=' | b':' | b' ' | b'\t' => break,
            _ => i += 1,
        }
    }
    let i = i.min(bytes.len());
    let (key, rest) = line.split_at(i);
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = rest
        .strip_prefix(['=', ':'])
        .map_or(rest, |r| r.trim_start_matches([' ', '\t']));
    (key, rest)
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = chars.clone().take(4).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) if hex.len() == 4 => {
                        out.push(ch);
                        chars.nth(3);
                    }
                    _ => out.push('u'),
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}
