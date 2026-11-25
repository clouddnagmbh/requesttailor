// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The notebook file format, one reader and one writer (the module documentation of
//! [`super`] has the grammar). Prose is kept exactly as written, so reading and writing an
//! unchanged file gives the same bytes; a request block is written in one canonical form, and a
//! file that already is in it comes back byte for byte (a hand-written block with other
//! spacing, comments or key order is normalised once and is stable afterwards).

use rt_core::model::{Body, KeyValue, RawKind, RequestSpec};

use super::{Block, MAX_BLOCKS, Notebook, Show};
use crate::AppError;

/// The info string of a request block.
pub const FENCE_INFO: &str = "rt-request";

fn bad(message: impl Into<String>) -> AppError {
    AppError::Shell(format!("notebook: {}", message.into()))
}

/// The id a block without one gets: `block-<n>`, n counting request blocks from 1.
pub fn default_id(ordinal: usize) -> String {
    format!("block-{ordinal}")
}

/// A fence line of an ordinary code block: marker character and length.
fn fence_marker(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// Reads a notebook file's text.
pub fn parse(text: &str) -> Result<Notebook, AppError> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut prose = String::new();
    let mut title = String::new();
    let mut ordinal = 0usize;
    // Inside an ordinary code block: its marker, so a request fence shown as an example stays text.
    let mut open: Option<(char, usize)> = None;
    let mut lines = text.split_inclusive('\n').enumerate().peekable();
    while let Some((number, line)) = lines.next() {
        let bare = line.trim_end_matches(['\n', '\r']);
        if let Some((c, n)) = open {
            if let Some((c2, n2)) = fence_marker(line)
                && c2 == c
                && n2 >= n
                && bare.trim().chars().all(|x| x == c)
            {
                open = None;
            }
            prose.push_str(line);
            continue;
        }
        if bare.trim_end() == format!("```{FENCE_INFO}") {
            if !prose.is_empty() {
                blocks.push(Block::Text {
                    markdown: std::mem::take(&mut prose),
                });
            }
            let mut body = String::new();
            let mut closed = false;
            for (_, inner) in lines.by_ref() {
                if inner.trim_end_matches(['\n', '\r']).trim_end() == "```" {
                    closed = true;
                    break;
                }
                body.push_str(inner);
            }
            if !closed {
                return Err(bad(format!(
                    "the {FENCE_INFO} block on line {} is not closed",
                    number + 1
                )));
            }
            ordinal += 1;
            blocks.push(parse_request(&body, ordinal, number + 1)?);
            continue;
        }
        if let Some(marker) = fence_marker(line) {
            open = Some(marker);
        } else if title.is_empty()
            && let Some(heading) = bare.strip_prefix("# ")
        {
            title = heading.trim().to_owned();
        }
        prose.push_str(line);
    }
    if !prose.is_empty() {
        blocks.push(Block::Text { markdown: prose });
    }
    if ordinal > MAX_BLOCKS {
        return Err(bad(format!(
            "a notebook runs at most {MAX_BLOCKS} request blocks"
        )));
    }
    let mut seen = std::collections::BTreeSet::new();
    for block in &blocks {
        if let Block::Request { id, .. } = block
            && !seen.insert(id.clone())
        {
            return Err(bad(format!("the block id {id:?} is used twice")));
        }
    }
    Ok(Notebook { title, blocks })
}

/// `key: value` of a line (the first colon splits; a bare `key:` has an empty value).
fn split_key(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim()))
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn parse_request(body: &str, ordinal: usize, line: usize) -> Result<Block, AppError> {
    let at = |m: String| bad(format!("block on line {line}: {m}"));
    let lines: Vec<&str> = body.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let mut id: Option<String> = None;
    let mut request_ref: Option<String> = None;
    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<KeyValue> = Vec::new();
    let mut raw_body: Option<String> = None;
    let mut set: Vec<(String, String)> = Vec::new();
    let mut show: Vec<Show> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        if indent_of(l) != 0 {
            return Err(at(format!("unexpected indented line {l:?}")));
        }
        let (key, value) =
            split_key(l).ok_or_else(|| at(format!("expected `key: value`, got {l:?}")))?;
        i += 1;
        // The indented lines under the key.
        let mut child: Vec<&str> = Vec::new();
        while i < lines.len()
            && (lines[i].trim().is_empty()
                || indent_of(lines[i]) > 0
                // YAML lets a list sit at the key's own indent.
                || (key == "show" && lines[i].starts_with("- ")))
        {
            child.push(lines[i]);
            i += 1;
        }
        while child.last().is_some_and(|l| l.trim().is_empty()) {
            child.pop();
        }
        let plain = |what: &str| -> Result<String, AppError> {
            if value.is_empty() || !child.is_empty() {
                Err(at(format!("{what} needs a value on the same line")))
            } else {
                Ok(value.to_owned())
            }
        };
        match key {
            "id" => id = Some(plain("id")?),
            "ref" => request_ref = Some(plain("ref")?),
            "method" => method = Some(plain("method")?.to_ascii_uppercase()),
            "url" => url = Some(plain("url")?),
            "headers" => {
                for c in &child {
                    if c.trim().is_empty() {
                        continue;
                    }
                    let (n, v) =
                        split_key(c).ok_or_else(|| at(format!("bad header line {c:?}")))?;
                    if n.is_empty() {
                        return Err(at(format!("bad header line {c:?}")));
                    }
                    headers.push(KeyValue::new(n, v));
                }
            }
            "body" => {
                if value == "|" {
                    let text: Vec<&str> = child
                        .iter()
                        .map(|c| c.strip_prefix("  ").unwrap_or(c.trim_start()))
                        .collect();
                    raw_body = Some(text.join("\n"));
                } else if child.is_empty() && !value.is_empty() {
                    raw_body = Some(value.to_owned());
                } else {
                    return Err(at(
                        "body is `body: <text>` or `body: |` and indented lines".into()
                    ));
                }
            }
            "set" => {
                for c in &child {
                    if c.trim().is_empty() {
                        continue;
                    }
                    let (n, p) = split_key(c).ok_or_else(|| at(format!("bad set line {c:?}")))?;
                    if n.is_empty() || p.is_empty() {
                        return Err(at(format!("bad set line {c:?}: expected `name: $.path`")));
                    }
                    set.push((n.to_owned(), p.to_owned()));
                }
            }
            "show" => {
                for c in &child {
                    let t = c.trim();
                    if t.is_empty() {
                        continue;
                    }
                    let item = t
                        .strip_prefix('-')
                        .map(str::trim)
                        .ok_or_else(|| at(format!("bad show line {c:?}")))?;
                    show.push(parse_show(item).map_err(at)?);
                }
            }
            other => return Err(at(format!("unknown key {other:?}"))),
        }
    }
    let id = id.unwrap_or_else(|| default_id(ordinal));
    if id.is_empty() || id.chars().any(|c| c.is_whitespace() || c == '#') {
        return Err(at(format!("{id:?} is not a usable block id")));
    }
    let embedded = match (&request_ref, &url) {
        (Some(_), None) if method.is_none() && headers.is_empty() && raw_body.is_none() => None,
        (Some(_), _) => return Err(at("`ref` cannot be mixed with an embedded request".into())),
        (None, Some(url)) => Some(RequestSpec {
            name: id.clone(),
            method: method.unwrap_or_else(|| "GET".into()),
            url: url.clone(),
            headers,
            body: match raw_body {
                None => Body::None,
                Some(data) => {
                    let kind = if data.trim_start().starts_with(['{', '[']) {
                        RawKind::Json
                    } else {
                        RawKind::Text
                    };
                    Body::Raw { kind, data }
                }
            },
            ..RequestSpec::default()
        }),
        (None, None) => return Err(at("a block needs `ref: <request file>` or `url:`".into())),
    };
    Ok(Block::Request {
        id,
        request_ref,
        embedded,
        set,
        show,
    })
}

fn parse_show(item: &str) -> Result<Show, String> {
    if item == "response" {
        return Ok(Show::Response);
    }
    match split_key(item) {
        Some(("table", path)) if !path.is_empty() => Ok(Show::Table { path: path.into() }),
        Some(("chart", path)) if !path.is_empty() => Ok(Show::Chart { path: path.into() }),
        _ => Err(format!(
            "bad show item {item:?}: expected `response`, `table: <path>` or `chart: <path>`"
        )),
    }
}

/// Writes a notebook back (module documentation above).
pub fn render(notebook: &Notebook) -> Result<String, AppError> {
    if notebook
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Request { .. }))
        .count()
        > MAX_BLOCKS
    {
        return Err(bad(format!(
            "a notebook runs at most {MAX_BLOCKS} request blocks"
        )));
    }
    let mut out = String::new();
    let mut ordinal = 0usize;
    for block in &notebook.blocks {
        match block {
            Block::Text { markdown } => out.push_str(markdown),
            Block::Request {
                id,
                request_ref,
                embedded,
                set,
                show,
            } => {
                ordinal += 1;
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&format!("```{FENCE_INFO}\n"));
                if *id != default_id(ordinal) {
                    out.push_str(&format!("id: {id}\n"));
                }
                match (request_ref, embedded) {
                    (Some(path), None) => out.push_str(&format!("ref: {path}\n")),
                    (None, Some(spec)) => render_embedded(spec, &mut out)?,
                    _ => {
                        return Err(bad(format!(
                            "block {id:?} needs exactly one of ref and an embedded request"
                        )));
                    }
                }
                if !set.is_empty() {
                    out.push_str("set:\n");
                    for (name, path) in set {
                        out.push_str(&format!("  {name}: {path}\n"));
                    }
                }
                if !show.is_empty() {
                    out.push_str("show:\n");
                    for s in show {
                        match s {
                            Show::Response => out.push_str("  - response\n"),
                            Show::Table { path } => out.push_str(&format!("  - table: {path}\n")),
                            Show::Chart { path } => out.push_str(&format!("  - chart: {path}\n")),
                        }
                    }
                }
                out.push_str("```\n");
            }
        }
    }
    Ok(out)
}

fn render_embedded(spec: &RequestSpec, out: &mut String) -> Result<(), AppError> {
    out.push_str(&format!("method: {}\nurl: {}\n", spec.method, spec.url));
    if !spec.headers.is_empty() {
        out.push_str("headers:\n");
        for h in spec.headers.iter().filter(|h| h.enabled) {
            out.push_str(&format!("  {}: {}\n", h.name, h.value));
        }
    }
    match &spec.body {
        Body::None => {}
        Body::Raw { data, .. } => {
            if !data.contains('\n')
                && !data.starts_with('|')
                && !data.trim().is_empty()
                && data.trim() == data
            {
                out.push_str(&format!("body: {data}\n"));
            } else {
                out.push_str("body: |\n");
                for line in data.split('\n') {
                    if line.is_empty() {
                        out.push('\n');
                    } else {
                        out.push_str(&format!("  {line}\n"));
                    }
                }
            }
        }
        _ => {
            return Err(bad(
                "an embedded request supports a raw body only; use `ref:` for the others",
            ));
        }
    }
    Ok(())
}
