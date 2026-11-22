// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `$batch` as multipart/mixed (V2 and V4): reads outside changesets, changesets as nested
//! multipart/mixed, all-or-nothing. A failing operation rolls the whole changeset back and the
//! changeset answers with that one error (an `application/http` part in its place). `$<n>`
//! at the start of a URL inside a changeset refers to the entity created by `Content-ID: n`.
//!
//! Written independently of the plugin's `write::batch`, so the tests check two
//! implementations against each other. Accepts CRLF and bare LF line endings.

use std::collections::HashMap;

use crate::odata::{self, Ctx, Op, Reply, ServiceState};
use crate::services::Version;

/// A MIME part: headers and body.
struct Part<'a> {
    headers: Vec<(String, String)>,
    body: &'a str,
}

/// The `boundary` parameter of a `multipart/*` content type.
pub fn boundary(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|p| {
        let (name, value) = p.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

/// Splits a header block from its body at the first empty line.
fn split_head(text: &str) -> (&str, &str) {
    let crlf = text.find("\r\n\r\n").map(|i| (i, 4));
    let lf = text.find("\n\n").map(|i| (i, 2));
    let first = match (crlf, lf) {
        (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
        (a, b) => a.or(b),
    };
    match first {
        Some((i, len)) => (&text[..i], &text[i + len..]),
        None => (text, ""),
    }
}

fn parse_headers(block: &str) -> Vec<(String, String)> {
    block
        .lines()
        .filter_map(|l| {
            let (n, v) = l.split_once(':')?;
            Some((n.trim().to_owned(), v.trim().to_owned()))
        })
        .collect()
}

fn parts<'a>(body: &'a str, boundary: &str) -> Option<Vec<Part<'a>>> {
    let delimiter = format!("--{boundary}");
    let mut out = Vec::new();
    // Positions of delimiter lines (at the start or after a line break).
    let mut starts = Vec::new();
    let mut from = 0;
    while let Some(i) = body[from..].find(&delimiter) {
        let at = from + i;
        if at == 0 || body[..at].ends_with('\n') {
            starts.push(at);
        }
        from = at + delimiter.len();
    }
    for w in starts.windows(2) {
        let after = &body[w[0] + delimiter.len()..w[1]];
        if after.starts_with("--") {
            break;
        }
        let content = after
            .strip_prefix("\r\n")
            .or_else(|| after.strip_prefix('\n'))
            .unwrap_or(after);
        let content = content
            .strip_suffix("\r\n")
            .or_else(|| content.strip_suffix('\n'))
            .unwrap_or(content);
        let (head, body) = split_head(content);
        out.push(Part {
            headers: parse_headers(head),
            body,
        });
    }
    let last = starts.last()?;
    body[last + delimiter.len()..]
        .starts_with("--")
        .then_some(out)
}

/// An `application/http` request inside a batch, relative to the service root.
fn parse_request(text: &str, root: &str) -> Option<Op> {
    let (head, body) = split_head(text);
    let mut lines = head.lines();
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_ascii_uppercase();
    let url = request_line.next()?;
    let headers = parse_headers(&lines.collect::<Vec<_>>().join("\n"));
    let mut path = url;
    if let Some(i) = path.find("://") {
        path = path[i + 3..].find('/').map_or("", |j| &path[i + 3 + j..]);
    }
    let path = path
        .strip_prefix(root)
        .or_else(|| path.strip_prefix(root.trim_end_matches('/')))
        .unwrap_or(path)
        .trim_start_matches('/');
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let body = body
        .strip_suffix("\r\n")
        .or_else(|| body.strip_suffix('\n'))
        .unwrap_or(body);
    Some(Op {
        method,
        path: crate::percent_decode(path),
        query: crate::parse_query(query),
        headers,
        body: body.as_bytes().to_vec(),
    })
}

fn http_part(reply: &Reply, content_id: Option<&str>) -> String {
    let mut s =
        String::from("Content-Type: application/http\r\nContent-Transfer-Encoding: binary\r\n");
    if let Some(id) = content_id {
        s.push_str(&format!("Content-ID: {id}\r\n"));
    }
    s.push_str(&format!(
        "\r\nHTTP/1.1 {} {}\r\n",
        reply.status,
        crate::reason(reply.status)
    ));
    for (n, v) in &reply.headers {
        s.push_str(&format!("{n}: {v}\r\n"));
    }
    s.push_str(&format!("Content-Length: {}\r\n\r\n", reply.body.len()));
    s.push_str(&String::from_utf8_lossy(&reply.body));
    s.push_str("\r\n");
    s
}

/// Handles `POST <root>$batch`.
pub fn handle(
    state: &mut ServiceState,
    ctx: &Ctx,
    content_type: &str,
    body: &[u8],
    n: u64,
) -> Reply {
    let def = state.def;
    let fail = |status, msg: &str| odata::error(def, ctx.lang, status, msg);
    let Some(outer) = boundary(content_type).filter(|_| {
        content_type
            .to_ascii_lowercase()
            .starts_with("multipart/mixed")
    }) else {
        return fail(
            400,
            "A $batch request needs Content-Type multipart/mixed with a boundary (JSON batch is not supported by the test server)",
        );
    };
    let text = String::from_utf8_lossy(body);
    let Some(parts) = parts(&text, &outer) else {
        return fail(
            400,
            "Malformed multipart body: the closing delimiter is missing",
        );
    };
    let out_boundary = format!("batch_rt_{n}");
    let mut out = String::new();
    for (ci, part) in parts.iter().enumerate() {
        let ct = odata::header(&part.headers, "Content-Type").unwrap_or_default();
        out.push_str(&format!("--{out_boundary}\r\n"));
        if ct.to_ascii_lowercase().starts_with("multipart/mixed") {
            let Some(inner) = boundary(ct).and_then(|b| parts_owned(part.body, &b)) else {
                return fail(400, "Malformed changeset");
            };
            out.push_str(&changeset(
                state,
                ctx,
                &inner,
                &format!("changeset_rt_{n}_{}", ci + 1),
            ));
        } else {
            let reply = match parse_request(part.body, def.root) {
                None => fail(400, "Malformed request inside the batch"),
                Some(op) if op.method != "GET" && def.version == Version::V2 => {
                    fail(400, "Modifying requests must be inside a changeset")
                }
                Some(op) => odata::execute(state, ctx, &op),
            };
            out.push_str(&http_part(&reply, None));
        }
    }
    out.push_str(&format!("--{out_boundary}--\r\n"));
    Reply {
        status: 202,
        headers: vec![
            (
                "Content-Type".into(),
                format!("multipart/mixed; boundary={out_boundary}"),
            ),
            odata::version_header(def.version),
        ],
        body: out.into_bytes(),
    }
}

/// Headers and body of a part.
type OwnedPart = (Vec<(String, String)>, String);

/// Nested parts with owned bodies (the changeset text is borrowed from the outer part).
fn parts_owned(body: &str, boundary: &str) -> Option<Vec<OwnedPart>> {
    Some(
        parts(body, boundary)?
            .into_iter()
            .map(|p| (p.headers, p.body.to_owned()))
            .collect(),
    )
}

fn changeset(state: &mut ServiceState, ctx: &Ctx, ops: &[OwnedPart], boundary: &str) -> String {
    let def = state.def;
    let snapshot = state.clone();
    let mut created: HashMap<String, String> = HashMap::new();
    let mut replies = Vec::new();
    for (headers, text) in ops {
        let Some(mut op) = parse_request(text, def.root) else {
            *state = snapshot;
            return http_part(
                &odata::error(def, ctx.lang, 400, "Malformed request inside the changeset"),
                None,
            );
        };
        let content_id = odata::header(headers, "Content-ID")
            .or_else(|| op.header("Content-ID"))
            .map(str::to_owned);
        if let Some(rest) = op.path.strip_prefix('$') {
            let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
            if let Some(path) = created.get(id) {
                op.path = if tail.is_empty() {
                    path.clone()
                } else {
                    format!("{path}/{tail}")
                };
            }
        }
        let reply = if op.method == "GET" {
            odata::error(def, ctx.lang, 400, "GET is not allowed inside a changeset")
        } else {
            odata::execute(state, ctx, &op)
        };
        if reply.status >= 400 {
            *state = snapshot;
            return http_part(&reply, content_id.as_deref());
        }
        if let Some(id) = &content_id {
            let path = reply
                .header("Location")
                .and_then(|l| l.strip_prefix(ctx.base))
                .map_or_else(|| op.path.clone(), str::to_owned);
            created.insert(id.clone(), path);
        }
        replies.push((reply, content_id));
    }
    let mut s = format!("Content-Type: multipart/mixed; boundary={boundary}\r\n\r\n");
    for (reply, id) in &replies {
        s.push_str(&format!("--{boundary}\r\n"));
        s.push_str(&http_part(reply, id.as_deref()));
    }
    s.push_str(&format!("--{boundary}--\r\n"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_with_crlf_and_lf() {
        let crlf =
            "--b\r\nContent-Type: application/http\r\n\r\nGET A HTTP/1.1\r\n\r\n\r\n--b--\r\n";
        let p = parts(crlf, "b").unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].body, "GET A HTTP/1.1\r\n\r\n");
        let lf = "--b\nContent-Type: application/http\n\nGET A HTTP/1.1\n\n\n--b--\n";
        assert_eq!(parts(lf, "b").unwrap()[0].body, "GET A HTTP/1.1\n\n");
        assert!(parts("--b\r\n\r\nx\r\n", "b").is_none());
        assert_eq!(
            boundary("multipart/mixed; boundary=\"x y\"").as_deref(),
            Some("x y")
        );
    }

    #[test]
    fn request_urls_are_made_relative() {
        let op = parse_request(
            "PATCH /odata/v4/admin/Books(1)?x=1 HTTP/1.1\r\nIf-Match: *\r\n\r\n{}",
            "/odata/v4/admin/",
        )
        .unwrap();
        assert_eq!(op.path, "Books(1)");
        assert_eq!(op.query, vec![("x".to_owned(), "1".to_owned())]);
        assert_eq!(op.header("if-match"), Some("*"));
        assert_eq!(op.body, b"{}");
    }
}
