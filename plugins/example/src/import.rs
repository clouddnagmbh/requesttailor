// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer `url-list`: text files with one request per line.
//!
//! ```text
//! # Comments and blank lines are skipped.
//! GET https://api.example.com/users
//! https://api.example.com/health        (no method: GET)
//! post https://api.example.com/users    (methods are uppercased)
//! ```
//!
//! Every other line is reported as lost. The output is an OpenCollection collection: an
//! `opencollection.yml` and one request file per line, named `<n>-<slug>.yml`.

use rt_plugin_sdk::api::{ImportReply, PluginFile};
use rt_plugin_sdk::{ImportFormat, Loss, LossEffect, LossKind, LossReport, Result, bail};

const IMPORTER: &str = "url-list";
const EXTENSIONS: [&str; 2] = ["urls", "txt"];

struct Line<'a> {
    method: String,
    url: &'a str,
}

enum Parsed<'a> {
    Skip,
    Request(Line<'a>),
    Invalid,
}

fn parse_line(line: &str) -> Parsed<'_> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Parsed::Skip;
    }
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let (method, url) = match tokens.as_slice() {
        [url] => ("GET", *url),
        [method, url] if method.chars().all(|c| c.is_ascii_alphabetic()) => (*method, *url),
        _ => return Parsed::Invalid,
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Parsed::Invalid;
    }
    Parsed::Request(Line {
        method: method.to_ascii_uppercase(),
        url,
    })
}

fn check_importer(importer: &str) -> Result<()> {
    if importer != IMPORTER {
        bail!("unknown importer: {importer}");
    }
    Ok(())
}

fn extension_ok(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Matches if every file has a known extension and consists of request lines only (and at
/// least one).
pub fn detect(importer: &str, files: &[PluginFile]) -> Result<bool> {
    check_importer(importer)?;
    if files.is_empty() {
        return Ok(false);
    }
    for file in files {
        if !extension_ok(&file.name) {
            return Ok(false);
        }
        let Ok(text) = rt_plugin_sdk::file_text(file) else {
            return Ok(false);
        };
        let lines: Vec<Parsed> = text.lines().map(parse_line).collect();
        let requests = lines
            .iter()
            .filter(|l| matches!(l, Parsed::Request(_)))
            .count();
        if requests == 0 || lines.iter().any(|l| matches!(l, Parsed::Invalid)) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn import(importer: &str, files: &[PluginFile]) -> Result<ImportReply> {
    check_importer(importer)?;
    let collection_name = files
        .first()
        .map(|f| stem(&f.name).to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "URL list".to_owned());

    let mut out = vec![(
        "opencollection.yml".to_owned(),
        format!(
            "opencollection: 1.0.0\n\ninfo:\n  name: {}\n",
            quote(&collection_name)
        ),
    )];
    let mut losses = Vec::new();
    let mut seq = 0u32;
    for file in files {
        let text = rt_plugin_sdk::file_text(file)?;
        for (index, line) in text.lines().enumerate() {
            match parse_line(line) {
                Parsed::Skip => {}
                Parsed::Invalid => losses.push(Loss {
                    location: format!("{} line {}", file.name, index + 1),
                    kind: LossKind::Other,
                    effect: LossEffect::Dropped,
                    detail: format!("not a request line: {}", shorten(line.trim(), 80)),
                }),
                Parsed::Request(request) => {
                    seq += 1;
                    let name = format!("{} {}", request.method, path_of(request.url));
                    out.push((
                        format!("{seq}-{}.yml", slug(&name)),
                        request_file(&name, seq, &request),
                    ));
                }
            }
        }
    }
    Ok(ImportReply {
        collection_name,
        files: out,
        report: LossReport {
            format: ImportFormat::Plugin,
            requests: seq,
            folders: 0,
            environments: 0,
            losses,
        },
    })
}

fn request_file(name: &str, seq: u32, request: &Line) -> String {
    format!(
        "info:\n  name: {}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {}\n  url: {}\n  auth: inherit\n",
        quote(name),
        request.method,
        quote(request.url)
    )
}

/// A YAML double-quoted scalar.
fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `/users/7` of `https://api.example.com/users/7?x=1`.
fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.find('/').map_or("/", |i| &rest[i..]);
    path.split(['?', '#']).next().unwrap_or("/")
}

fn stem(name: &str) -> &str {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    base.rsplit_once('.').map_or(base, |(stem, _)| stem)
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(60).collect();
    if out.is_empty() {
        "request".to_owned()
    } else {
        out.trim_end_matches('-').to_owned()
    }
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        assert_eq!(path_of("https://a.example/users/7?x=1#f"), "/users/7");
        assert_eq!(path_of("https://a.example"), "/");
        assert_eq!(path_of("https://a.example?x"), "/");
        assert_eq!(slug("GET /users/7"), "get-users-7");
        assert_eq!(slug("GET /"), "get");
        assert_eq!(stem("dir/users.urls"), "users");
        assert_eq!(quote("a\"b\\c\td"), r#""a\"b\\c\x09d""#);
    }
}
