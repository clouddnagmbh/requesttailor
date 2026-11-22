// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Body encoding.
//!
//! Follows Bruno's `prepareRequest` and `interpolateVars`
//! (`packages/bruno-electron/src/ipc/network/`): raw bodies are interpolated as text;
//! form and multipart fields interpolate their values (not their names); a file body is sent
//! as read. Default content types are only set when no `Content-Type` header is present.
//!
//! Multipart: the boundary comes from a user `Content-Type: multipart/...; boundary=...`
//! header if there is one. Otherwise it is derived from the content, so the same request
//! gives the same bytes, and it is checked not to occur in any part. A user header
//! `multipart/...` without a boundary gets ours appended (Bruno does the same).

use bytes::Bytes;
use url::form_urlencoded::byte_serialize;

use super::{FileSource, Interp, PrepareError};
use crate::model::{Body, MultipartPart, PartKind, RawKind};
use crate::transport::HttpBody;

const OCTET_STREAM: &str = "application/octet-stream";

pub(super) fn encode(
    body: &Body,
    headers: &mut [(String, String)],
    interp: &mut Interp<'_>,
    files: &dyn FileSource,
) -> Result<Option<HttpBody>, PrepareError> {
    let user_type = headers
        .iter()
        .position(|(n, _)| n.eq_ignore_ascii_case("content-type"));
    let default_type = |ct: String| if user_type.is_some() { None } else { Some(ct) };

    let (content_type, bytes) = match body {
        Body::None => return Ok(None),
        Body::Raw { kind, data } => {
            let ct = match kind {
                RawKind::Json => "application/json",
                RawKind::Text => "text/plain",
                RawKind::Xml => "application/xml",
                RawKind::Sparql => "application/sparql-query",
            };
            (default_type(ct.into()), interp.text(data).into_bytes())
        }
        Body::GraphQl { query, variables } => {
            let query = interp.text(query);
            let variables = interp.text(variables);
            (
                default_type("application/json".into()),
                graphql_payload(&query, &variables).into_bytes(),
            )
        }
        Body::FormUrlEncoded { fields } => {
            let encoded: Vec<String> = fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| {
                    let value = interp.text(&f.value);
                    format!(
                        "{}={}",
                        byte_serialize(f.name.as_bytes()).collect::<String>(),
                        byte_serialize(value.as_bytes()).collect::<String>()
                    )
                })
                .collect();
            (
                default_type("application/x-www-form-urlencoded".into()),
                encoded.join("&").into_bytes(),
            )
        }
        Body::Multipart { parts } => {
            let parts = multipart_parts(parts, interp, files)?;
            let user = user_type.map(|i| &mut headers[i].1);
            let (content_type, boundary) = match user {
                Some(value) => match boundary_param(value) {
                    Some(b) => (None, b.to_owned()),
                    None => {
                        let b = boundary_for(&parts);
                        if media_type(value).starts_with("multipart/") {
                            value.push_str("; boundary=");
                            value.push_str(&b);
                        }
                        (None, b)
                    }
                },
                None => {
                    let b = boundary_for(&parts);
                    (Some(format!("multipart/form-data; boundary={b}")), b)
                }
            };
            (content_type, assemble(&parts, &boundary))
        }
        Body::File { path, content_type } => {
            let bytes = read(files, path)?;
            let ct = content_type
                .as_deref()
                .filter(|c| !c.trim().is_empty())
                .unwrap_or(OCTET_STREAM);
            (default_type(ct.into()), bytes)
        }
    };
    Ok(Some(HttpBody {
        content_type,
        bytes: Bytes::from(bytes),
    }))
}

fn read(files: &dyn FileSource, path: &str) -> Result<Vec<u8>, PrepareError> {
    files.read(path).map_err(|message| PrepareError::BodyFile {
        path: path.to_owned(),
        message,
    })
}

/// One encoded part: its header lines and its content, without boundaries.
struct Part {
    head: String,
    content: Vec<u8>,
}

fn multipart_parts(
    parts: &[MultipartPart],
    interp: &mut Interp<'_>,
    files: &dyn FileSource,
) -> Result<Vec<Part>, PrepareError> {
    let mut out = Vec::new();
    for part in parts.iter().filter(|p| p.enabled) {
        let name = quote(&part.name);
        let content_type = part
            .content_type
            .as_deref()
            .filter(|c| !c.trim().is_empty());
        for value in &part.values {
            let value = interp.text(value);
            match part.kind {
                PartKind::Text => {
                    let mut head = format!("Content-Disposition: form-data; name=\"{name}\"\r\n");
                    if let Some(ct) = content_type {
                        head.push_str(&format!("Content-Type: {ct}\r\n"));
                    }
                    out.push(Part {
                        head,
                        content: value.into_bytes(),
                    });
                }
                PartKind::File => {
                    let path = value.trim();
                    let content = read(files, path)?;
                    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
                    out.push(Part {
                        head: format!(
                            "Content-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\n\
                             Content-Type: {}\r\n",
                            quote(file_name),
                            content_type.unwrap_or(OCTET_STREAM)
                        ),
                        content,
                    });
                }
            }
        }
    }
    Ok(out)
}

/// Escapes a name for a quoted `Content-Disposition` parameter (WHATWG HTML form encoding).
fn quote(s: &str) -> String {
    s.replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn assemble(parts: &[Part], boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(format!("--{boundary}\r\n{}\r\n", part.head).as_bytes());
        out.extend_from_slice(&part.content);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

/// A boundary derived from the content (FNV-1a), retried until it occurs in no part.
fn boundary_for(parts: &[Part]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for b in part.head.bytes().chain(part.content.iter().copied()) {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    let mut attempt: u64 = 0;
    loop {
        let boundary = format!(
            "----RequestTailorBoundary{:016x}",
            hash.wrapping_add(attempt.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        );
        let delimiter = format!("--{boundary}");
        let clashes = parts
            .iter()
            .any(|p| contains(p.head.as_bytes(), &delimiter) || contains(&p.content, &delimiter));
        if !clashes {
            return boundary;
        }
        attempt += 1;
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    let needle = needle.as_bytes();
    haystack.len() >= needle.len() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// The media type of a `Content-Type` value, lowercased, without parameters.
fn media_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// Bruno's `extractBoundaryFromContentType`: `boundary="..."` or `boundary=...`.
fn boundary_param(value: &str) -> Option<&str> {
    value.split(';').skip(1).find_map(|param| {
        let (name, v) = param.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("boundary") {
            return None;
        }
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(v);
        (!v.is_empty()).then_some(v)
    })
}

/// `{"query": …, "variables": …}`: empty variables are omitted, valid JSON is embedded,
/// anything else goes as a string (the server reports the error).
fn graphql_payload(query: &str, variables: &str) -> String {
    let mut payload = serde_json::Map::new();
    payload.insert("query".into(), serde_json::Value::String(query.into()));
    let trimmed = variables.trim();
    if !trimmed.is_empty() {
        let value = serde_json::from_str(trimmed)
            .unwrap_or_else(|_| serde_json::Value::String(variables.into()));
        payload.insert("variables".into(), value);
    }
    serde_json::Value::Object(payload).to_string()
}

#[cfg(test)]
mod graphql_tests {
    use super::graphql_payload;

    #[test]
    fn variables_are_embedded_as_json_or_omitted() {
        assert_eq!(
            graphql_payload("{ a }", " {\"id\": 1} "),
            r#"{"query":"{ a }","variables":{"id":1}}"#
        );
        assert_eq!(graphql_payload("{ a }", "  "), r#"{"query":"{ a }"}"#);
    }
}
