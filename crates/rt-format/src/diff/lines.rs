// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Where a field is in its file: the 1-based line the PR review comments at. The model keeps
//! no positions, so this looks for the section key and then for the entry's name. It is a
//! guide, not a proof: when nothing matches precisely, the section's line is used, and
//! `None` when the file has no such section.

use super::FieldPath;

/// File keys that hold the section a model segment lives in.
fn section_keys(segment: &str) -> &'static [&'static str] {
    match segment {
        "name" => &["name"],
        "seq" => &["seq"],
        "method" => &["method"],
        "url" => &["url"],
        "headers" => &["headers"],
        "params" => &["params"],
        "body" => &["body"],
        "auth" => &["auth"],
        "settings" => &["settings"],
        "scripts" => &["scripts"],
        "assertions" => &["assertions"],
        "actions" => &["actions"],
        "config" => &["config"],
        "vars" => &["vars", "variables"],
        "variables" => &["variables"],
        "protocol" => &["grpc", "websocket", "message", "messages"],
        _ => &[],
    }
}

fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in s.chars() {
        if c == '_' || c == '-' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn key_line(line: &str, key: &str) -> bool {
    let t = line.trim_start().trim_start_matches("- ").trim_start();
    t.strip_prefix(key).is_some_and(|r| r.starts_with(':'))
}

/// The line of `path` in `text`, if it can be told.
pub(super) fn locate(text: &str, path: &FieldPath) -> Option<u32> {
    let mut segments = path.as_slice();
    if segments.first().map(String::as_str) == Some("defaults") {
        segments = &segments[1..];
    }
    let first = segments.first()?;
    let lines: Vec<&str> = text.lines().collect();
    let keys = section_keys(first);
    let start = lines
        .iter()
        .position(|l| keys.iter().any(|k| key_line(l, k)))?;
    let mut found = start;
    // A scalar of the top level (url, method, name, ...) is its own line; for a list entry
    // or a field of a section, look for the entry's name after the section line.
    for seg in &segments[1..] {
        if seg.starts_with('@') || seg == "type" && segments.len() == 2 {
            break;
        }
        let seg = seg.split('#').next().unwrap_or(seg);
        let camel = camel(seg);
        let hit = lines[found..].iter().take(400).position(|l| {
            let t = l.trim();
            key_line(l, seg) || key_line(l, &camel) || (!seg.is_empty() && t.contains(seg))
        });
        match hit {
            Some(i) => found += i,
            None => break,
        }
    }
    u32::try_from(found + 1).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_scalars_and_entries() {
        let text = "info:\n  name: A\n\nhttp:\n  method: GET\n  url: x\n  headers:\n    - name: Accept\n      value: a\n    - name: X-Trace\n      value: b\n";
        let p = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(locate(text, &p(&["url"])), Some(6));
        assert_eq!(locate(text, &p(&["headers", "X-Trace"])), Some(10));
        assert_eq!(
            locate(text, &p(&["headers", "X-Trace", "@enabled"])),
            Some(10)
        );
        assert_eq!(locate(text, &p(&["body"])), None);
    }
}
