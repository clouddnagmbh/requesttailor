// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `$batch` bodies: multipart/mixed (V2 and V4) with changesets, and their responses.
//!
//! Layout (both versions; OData V2 part 1 §2.2.7.6 and OData V4 part 1 §11.7): one
//! `application/http` part per read, one nested `multipart/mixed` part per changeset, each
//! operation with `Content-Transfer-Encoding: binary`; operations in changesets carry a
//! `Content-ID`. Line endings are CRLF. Content-IDs count all operations of the batch from 1,
//! so `$3` means "what operation 3 created" (only within its changeset).
//!
//! A failed changeset answers with one `application/http` part instead of the nested
//! multipart; [`parse_response_for`] uses the request to tell which changeset it was.

use crate::edm::Version;

/// One operation inside a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub method: String,
    /// Relative to the service root, e.g. `Books(1)`.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// A batch: reads outside changesets, and changesets (all-or-nothing groups of writes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Read(Part),
    Changeset(Vec<Part>),
}

fn has(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

fn http_part(part: &Part, content_id: Option<usize>) -> String {
    let mut s =
        String::from("Content-Type: application/http\r\nContent-Transfer-Encoding: binary\r\n");
    if let Some(id) = content_id {
        s.push_str(&format!("Content-ID: {id}\r\n"));
    }
    s.push_str(&format!(
        "\r\n{} {} HTTP/1.1\r\n",
        part.method.to_ascii_uppercase(),
        part.url
    ));
    for (n, v) in &part.headers {
        if !n.eq_ignore_ascii_case("content-length") {
            s.push_str(&format!("{n}: {v}\r\n"));
        }
    }
    if !has(&part.headers, "Accept") {
        s.push_str("Accept: application/json\r\n");
    }
    match &part.body {
        Some(body) => {
            if !has(&part.headers, "Content-Type") {
                s.push_str("Content-Type: application/json\r\n");
            }
            s.push_str(&format!("Content-Length: {}\r\n\r\n{body}\r\n", body.len()));
        }
        None => s.push_str("\r\n\r\n"),
    }
    s
}

/// The request body and its `Content-Type` (with boundary). Changeset boundaries are
/// `changeset_<n>_<boundary>`. V2 and V4 multipart batches have the same layout.
pub fn build(items: &[Item], version: Version, boundary: &str) -> (String, String) {
    // Same layout for both versions; the parameter stays for a V4 JSON batch later.
    let _ = version;
    let mut out = String::new();
    let mut content_id = 0;
    let mut changesets = 0;
    for item in items {
        out.push_str(&format!("--{boundary}\r\n"));
        match item {
            Item::Read(part) => {
                content_id += 1;
                out.push_str(&http_part(part, None));
            }
            Item::Changeset(parts) => {
                changesets += 1;
                let cs = format!("changeset_{changesets}_{boundary}");
                out.push_str(&format!(
                    "Content-Type: multipart/mixed; boundary={cs}\r\n\r\n"
                ));
                for part in parts {
                    content_id += 1;
                    out.push_str(&format!("--{cs}\r\n"));
                    out.push_str(&http_part(part, Some(content_id)));
                }
                out.push_str(&format!("--{cs}--\r\n"));
            }
        }
    }
    out.push_str(&format!("--{boundary}--\r\n"));
    (out, format!("multipart/mixed; boundary={boundary}"))
}

/// One response of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// Index of the changeset it belongs to, `None` for a read.
    pub changeset: Option<usize>,
}

/// The responses of one item: a read, a changeset, or a failed changeset (one response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Group {
    Single(PartResponse),
    Changeset(Vec<PartResponse>),
}

/// The `boundary` parameter of a multipart content type.
pub fn boundary(content_type: &str) -> Option<String> {
    let mut params = content_type.split(';');
    let kind = params.next()?.trim();
    if !kind.to_ascii_lowercase().starts_with("multipart/") {
        return None;
    }
    params.find_map(|p| {
        let (name, value) = p.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

/// Headers and body of one MIME part (body without the line break before the delimiter).
struct Mime<'a> {
    headers: Vec<(String, String)>,
    body: &'a str,
}

fn split_head(text: &str) -> (&str, &str) {
    let crlf = text.find("\r\n\r\n").map(|i| (i, 4));
    let lf = text.find("\n\n").map(|i| (i, 2));
    let first = match (crlf, lf) {
        (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
        (a, b) => a.or(b),
    };
    match first {
        Some((i, len)) => (&text[..i], &text[i + len..]),
        // Only headers (or only a status line) and no blank line.
        None => (text, ""),
    }
}

fn headers_of(block: &str) -> Vec<(String, String)> {
    block
        .lines()
        .filter_map(|l| {
            let (n, v) = l.split_once(':')?;
            Some((n.trim().to_owned(), v.trim().to_owned()))
        })
        .collect()
}

fn strip_line_end(s: &str) -> &str {
    s.strip_suffix("\r\n")
        .or_else(|| s.strip_suffix('\n'))
        .unwrap_or(s)
}

fn mime_parts<'a>(body: &'a str, boundary: &str) -> Vec<Mime<'a>> {
    let delimiter = format!("--{boundary}");
    let mut starts = Vec::new();
    let mut from = 0;
    while let Some(i) = body[from..].find(&delimiter) {
        let at = from + i;
        // A delimiter line: at a line start, followed by the line end, blanks or `--`.
        let rest = &body[at + delimiter.len()..];
        let ends =
            rest.is_empty() || rest.starts_with("--") || rest.starts_with(['\r', '\n', ' ', '\t']);
        if (at == 0 || body[..at].ends_with('\n')) && ends {
            starts.push(at);
        }
        from = at + delimiter.len();
    }
    let mut out = Vec::new();
    for (n, start) in starts.iter().enumerate() {
        let content_start = start + delimiter.len();
        if body[content_start..].starts_with("--") {
            break;
        }
        let end = starts.get(n + 1).copied().unwrap_or(body.len());
        let content = &body[content_start..end];
        let content = content
            .strip_prefix("\r\n")
            .or_else(|| content.strip_prefix('\n'))
            .unwrap_or(content);
        let (head, part_body) = split_head(strip_line_end(content));
        out.push(Mime {
            headers: headers_of(head),
            body: part_body,
        });
    }
    out
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn http_response(mime: &Mime, changeset: Option<usize>) -> PartResponse {
    let (head, body) = split_head(mime.body);
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut headers = headers_of(&lines.collect::<Vec<_>>().join("\n"));
    if let Some(id) = header(&mime.headers, "Content-ID")
        && !has(&headers, "Content-ID")
    {
        headers.push(("Content-ID".into(), id.to_owned()));
    }
    PartResponse {
        status,
        headers,
        body: body.to_owned(),
        changeset,
    }
}

/// The responses item by item. Changeset indexes count the changesets of the response.
pub fn parse_groups(content_type: &str, body: &str) -> Vec<Group> {
    let Some(outer) = boundary(content_type) else {
        return Vec::new();
    };
    let mut changesets = 0;
    mime_parts(body, &outer)
        .iter()
        .map(|part| {
            let inner = header(&part.headers, "Content-Type").and_then(boundary);
            match inner {
                Some(inner) => {
                    let index = changesets;
                    changesets += 1;
                    Group::Changeset(
                        mime_parts(part.body, &inner)
                            .iter()
                            .map(|p| http_response(p, Some(index)))
                            .collect(),
                    )
                }
                None => Group::Single(http_response(part, None)),
            }
        })
        .collect()
}

/// Parses a multipart batch response (`content_type` with its boundary). A failed changeset
/// cannot be told from a read without the request: it comes back with `changeset: None`
/// (see [`parse_response_for`]).
pub fn parse_response(content_type: &str, body: &str) -> Vec<PartResponse> {
    flatten(parse_groups(content_type, body))
}

fn flatten(groups: Vec<Group>) -> Vec<PartResponse> {
    groups
        .into_iter()
        .flat_map(|g| match g {
            Group::Single(r) => vec![r],
            Group::Changeset(rs) => rs,
        })
        .collect()
}

/// Like [`parse_response`], with the request's items: the changeset index of every response
/// is the index of the changeset among the request's items, also for a failed changeset.
pub fn parse_response_for(items: &[Item], content_type: &str, body: &str) -> Vec<PartResponse> {
    flatten(align(items, parse_groups(content_type, body)))
}

/// Groups with the changeset indexes of the request's items.
pub fn align(items: &[Item], groups: Vec<Group>) -> Vec<Group> {
    let mut changeset = 0;
    groups
        .into_iter()
        .enumerate()
        .map(|(i, group)| {
            let index = match items.get(i) {
                Some(Item::Changeset(_)) => {
                    changeset += 1;
                    Some(changeset - 1)
                }
                _ => None,
            };
            match group {
                Group::Single(mut r) => {
                    r.changeset = index;
                    Group::Single(r)
                }
                Group::Changeset(rs) => Group::Changeset(
                    rs.into_iter()
                        .map(|mut r| {
                            r.changeset = index.or(r.changeset);
                            r
                        })
                        .collect(),
                ),
            }
        })
        .collect()
}

fn http_request(mime: &Mime) -> Option<Part> {
    let (head, body) = split_head(mime.body);
    let mut lines = head.lines();
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_owned();
    let url = request_line.next()?.to_owned();
    let headers = headers_of(&lines.collect::<Vec<_>>().join("\n"))
        .into_iter()
        .filter(|(n, _)| !n.eq_ignore_ascii_case("content-length"))
        .collect();
    Some(Part {
        method,
        url,
        headers,
        body: (!body.is_empty()).then(|| body.to_owned()),
    })
}

/// Parses a multipart batch request back into items (for the response hook and the tab).
pub fn parse_request(content_type: &str, body: &str) -> Option<Vec<Item>> {
    let outer = boundary(content_type)?;
    mime_parts(body, &outer)
        .iter()
        .map(
            |part| match header(&part.headers, "Content-Type").and_then(boundary) {
                Some(inner) => Some(Item::Changeset(
                    mime_parts(part.body, &inner)
                        .iter()
                        .map(http_request)
                        .collect::<Option<Vec<_>>>()?,
                )),
                None => http_request(part).map(Item::Read),
            },
        )
        .collect()
}

/// Bare `\n` line breaks as CRLF (a batch body edited by hand), `None` if there are none.
pub fn to_crlf(body: &str) -> Option<String> {
    let bare = body
        .char_indices()
        .any(|(i, c)| c == '\n' && !body[..i].ends_with('\r'));
    bare.then(|| body.replace("\r\n", "\n").replace('\n', "\r\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(method: &str, url: &str, body: Option<&str>) -> Part {
        Part {
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            body: body.map(str::to_owned),
        }
    }

    #[test]
    fn build_is_crlf_multipart_with_changesets() {
        let items = vec![
            Item::Read(part("GET", "Books(1)", None)),
            Item::Changeset(vec![
                part("POST", "Books", Some(r#"{"a":1}"#)),
                part("PATCH", "Books(2)", Some("{}")),
            ]),
        ];
        let (body, ct) = build(&items, Version::V4, "batch_x");
        assert_eq!(ct, "multipart/mixed; boundary=batch_x");
        assert!(!body.replace("\r\n", "").contains('\n'), "CRLF only");
        assert!(body.starts_with("--batch_x\r\nContent-Type: application/http\r\n"));
        assert!(body.contains("Content-Type: multipart/mixed; boundary=changeset_1_batch_x\r\n\r\n--changeset_1_batch_x\r\n"));
        assert!(body.contains("Content-ID: 2\r\n\r\nPOST Books HTTP/1.1\r\n"));
        assert!(body.contains("Content-Length: 7\r\n\r\n{\"a\":1}\r\n--changeset_1_batch_x\r\n"));
        assert!(body.ends_with("--changeset_1_batch_x--\r\n--batch_x--\r\n"));
        // And back.
        let parsed = parse_request(&ct, &body).unwrap();
        assert_eq!(parsed.len(), 2);
        let Item::Changeset(ops) = &parsed[1] else {
            panic!("changeset expected")
        };
        assert_eq!(ops[0].body.as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(ops[1].url, "Books(2)");
        let Item::Read(read) = &parsed[0] else {
            panic!("read expected")
        };
        assert_eq!(read.body, None);
    }

    #[test]
    fn responses_with_a_failed_changeset() {
        let body = "--b\r\nContent-Type: application/http\r\n\r\nHTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"x\":1}\r\n\
--b\r\nContent-Type: multipart/mixed; boundary=c\r\n\r\n--c\r\nContent-Type: application/http\r\nContent-ID: 1\r\n\r\nHTTP/1.1 201 Created\r\nETag: W/\"1\"\r\n\r\n{}\r\n--c--\r\n\
--b\r\nContent-Type: application/http\r\n\r\nHTTP/1.1 412 Precondition Failed\r\n\r\n{\"error\":{}}\r\n--b--\r\n";
        let flat = parse_response("multipart/mixed; boundary=b", body);
        assert_eq!(
            flat.iter()
                .map(|r| (r.status, r.changeset))
                .collect::<Vec<_>>(),
            vec![(200, None), (201, Some(0)), (412, None)]
        );
        assert_eq!(flat[0].body, "{\"x\":1}");
        assert_eq!(header(&flat[1].headers, "content-id"), Some("1"));
        let items = vec![
            Item::Read(part("GET", "A", None)),
            Item::Changeset(vec![part("POST", "A", Some("{}"))]),
            Item::Changeset(vec![part("PATCH", "A(1)", Some("{}"))]),
        ];
        let aligned = parse_response_for(&items, "multipart/mixed; boundary=b", body);
        assert_eq!(aligned[2].changeset, Some(1));
        assert!(parse_response("application/json", "{}").is_empty());
    }

    #[test]
    fn hand_edited_bodies_get_crlf() {
        assert_eq!(to_crlf("a\nb\r\nc").as_deref(), Some("a\r\nb\r\nc"));
        assert_eq!(to_crlf("a\r\nb"), None);
    }
}
