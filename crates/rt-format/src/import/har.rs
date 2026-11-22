// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer for HTTP Archives (HAR 1.2, as browsers and proxies export them).
//! Owner: wave 4, strand B.
//!
//! Every entry becomes a request named `METHOD host/path`; entries of a page go into a folder
//! named after the page when the archive has several pages. The recorded response becomes
//! the request's example. HTTP/2 pseudo-headers and `Content-Length` are left out (the URL,
//! the method and the transport carry them). Recorded cookies and `Authorization` headers are
//! real credentials more often than not, so each one is reported.

use rt_core::model::{Auth, Body, KeyValue, MultipartPart, PartKind, RawKind, RequestSpec};

use super::common::{Losses, finish, invalid, location, raw_kind, url_params, utf8};
use super::curl::percent_decode;
use super::out::{CollectionOut, ExampleOut, FolderOut, ItemOut, RequestOut};
use super::value::{Value, parse_json};
use super::{Import, ImportError, ImportFormat, LossKind, SourceFile, stem};

const F: ImportFormat = ImportFormat::Har;

/// Largest response body kept as an example.
const MAX_EXAMPLE_BODY: usize = 256 * 1024;

pub(super) fn import(file: &SourceFile<'_>) -> Result<Import, ImportError> {
    let root = parse_json(&utf8(F, file.data)?).map_err(|e| invalid(F, e))?;
    let log = root
        .get("log")
        .filter(|l| l.is_obj())
        .ok_or_else(|| invalid(F, "no `log` object"))?;
    let version = log.string("version");
    if !version.is_empty() && !version.starts_with("1.") {
        return Err(ImportError::UnsupportedVersion { format: F, version });
    }
    let entries = log
        .get("entries")
        .filter(|e| e.is_arr())
        .ok_or_else(|| invalid(F, "no `log.entries` list"))?;
    let mut losses = Losses::default();
    let pages: Vec<(String, String)> = log
        .get("pages")
        .map(Value::items)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let title = p
                .str_of("title")
                .filter(|t| !t.trim().is_empty())
                .or_else(|| p.str_of("id"))?;
            Some((p.str_of("id")?, title))
        })
        .collect();
    let by_page = pages.len() > 1;
    let mut collection = CollectionOut {
        name: stem(file.name, &[".har"]).to_owned(),
        ..CollectionOut::default()
    };
    let mut folders: Vec<(String, FolderOut)> = Vec::new();
    for entry in entries.items() {
        if !entry.is_obj() {
            continue;
        }
        let page = entry.string("pageref");
        let folder_name = if by_page {
            pages
                .iter()
                .find(|(id, _)| *id == page)
                .map(|(_, t)| t.clone())
        } else {
            None
        };
        let path: Vec<String> = folder_name.iter().cloned().collect();
        let r = request(entry, &path, &mut losses);
        match folder_name {
            Some(name) => match folders.iter_mut().find(|(n, _)| *n == name) {
                Some((_, f)) => f.items.push(ItemOut::Request(r)),
                None => folders.push((
                    name.clone(),
                    FolderOut {
                        name,
                        items: vec![ItemOut::Request(r)],
                        ..FolderOut::default()
                    },
                )),
            },
            None => collection.items.push(ItemOut::Request(r)),
        }
    }
    let mut items: Vec<ItemOut> = folders
        .into_iter()
        .map(|(_, f)| ItemOut::Folder(f))
        .collect();
    items.append(&mut collection.items);
    collection.items = items;
    finish(F, collection, losses)
}

fn name_value(v: &Value) -> Option<(String, String)> {
    Some((v.str_of("name")?, v.string("value")))
}

fn request(entry: &Value, path: &[String], losses: &mut Losses) -> RequestOut {
    let req = entry.get("request").cloned().unwrap_or(Value::Null);
    let method = {
        let m = req.string("method").trim().to_ascii_uppercase();
        if m.is_empty() { "GET".to_owned() } else { m }
    };
    let url = req.string("url");
    let name = {
        let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
        let rest = rest
            .split(['?', '#'])
            .next()
            .unwrap_or(rest)
            .trim_end_matches('/');
        format!("{method} {}", rest.chars().take(120).collect::<String>())
    };
    let here = location(path, &name);
    let mut headers: Vec<KeyValue> = Vec::new();
    for h in req.get("headers").map(Value::items).unwrap_or_default() {
        let Some((n, v)) = name_value(h) else {
            continue;
        };
        if n.starts_with(':') || n.eq_ignore_ascii_case("content-length") {
            continue;
        }
        headers.push(KeyValue::new(n, v));
    }
    let cookies: Vec<String> = req
        .get("cookies")
        .map(Value::items)
        .unwrap_or_default()
        .iter()
        .filter_map(name_value)
        .map(|(n, v)| format!("{n}={v}"))
        .collect();
    if !cookies.is_empty()
        && !headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("cookie"))
    {
        headers.push(KeyValue::new("Cookie", cookies.join("; ")));
    }
    let body = body(req.get("postData"), &here, losses);
    let spec = RequestSpec {
        name: name.clone(),
        seq: None,
        method,
        params: url_params(&url, &[]),
        url,
        headers,
        body,
        auth: Auth::None,
        vars: Vec::new(),
        settings: Default::default(),
    };
    losses.literal_credentials(&here, &spec.auth, &spec.headers);
    let mut r = RequestOut::new(spec);
    r.docs = entry.str_of("comment").filter(|c| !c.trim().is_empty());
    if let Some(resp) = entry.get("response").filter(|r| r.is_obj()) {
        let mut x = ExampleOut {
            name: "Recorded response".into(),
            status: resp
                .get("status")
                .and_then(Value::as_f64)
                .filter(|s| (1.0..1000.0).contains(s))
                .map(|s| s as u16),
            status_text: resp.str_of("statusText"),
            headers: resp
                .get("headers")
                .map(Value::items)
                .unwrap_or_default()
                .iter()
                .filter_map(name_value)
                .filter(|(n, _)| !n.starts_with(':'))
                .collect(),
            ..ExampleOut::default()
        };
        let content = resp.get("content");
        if let Some(file) = content
            .and_then(|c| c.str_of("_file"))
            .filter(|f| !f.is_empty())
        {
            losses.dropped(
                &here,
                LossKind::Example,
                format!(
                    "recorded response body is stored in the separate file `{file}`; not imported"
                ),
            );
        }
        if let Some(text) = content.and_then(|c| c.str_of("text")) {
            let mime = content
                .map(|c| c.string("mimeType"))
                .unwrap_or_default()
                .to_ascii_lowercase();
            let base64 = content.is_some_and(|c| c.string("encoding") == "base64");
            if text.len() > MAX_EXAMPLE_BODY {
                losses.dropped(
                    &here,
                    LossKind::Example,
                    format!(
                        "recorded response body ({} KB) not imported; larger than {} KB",
                        text.len() / 1024,
                        MAX_EXAMPLE_BODY / 1024
                    ),
                );
            } else if base64 {
                x.body = Some(("binary", text));
                losses.approximated(
                    &here,
                    LossKind::Example,
                    "recorded binary response kept as base64 text",
                );
            } else {
                let kind = if mime.contains("json") {
                    "json"
                } else if mime.contains("html") {
                    "html"
                } else if mime.contains("xml") {
                    "xml"
                } else {
                    "text"
                };
                x.body = Some((kind, text));
            }
        }
        if x.status.is_some() || x.body.is_some() || !x.headers.is_empty() {
            r.examples.push(x);
        }
    }
    r
}

fn body(post: Option<&Value>, here: &str, losses: &mut Losses) -> Body {
    let Some(post) = post.filter(|p| p.is_obj()) else {
        return Body::None;
    };
    let mime = post.string("mimeType");
    let lower = mime.to_ascii_lowercase();
    let text = post.string("text");
    let params = post.get("params").map(Value::items).unwrap_or_default();
    if lower.contains("multipart/form-data") && !params.is_empty() {
        let parts = params
            .iter()
            .filter_map(|p| {
                let name = p.str_of("name")?;
                let content_type = p.str_of("contentType").filter(|c| !c.is_empty());
                Some(match p.str_of("fileName").filter(|f| !f.is_empty()) {
                    Some(file) => {
                        losses.approximated(
                            here,
                            LossKind::Body,
                            format!(
                                "file `{file}` for `{name}` was not recorded; put it into the \
                                 collection folder and update the path"
                            ),
                        );
                        MultipartPart {
                            name,
                            kind: PartKind::File,
                            values: vec![file],
                            content_type,
                            enabled: true,
                        }
                    }
                    None => MultipartPart {
                        name,
                        kind: PartKind::Text,
                        values: vec![p.string("value")],
                        content_type,
                        enabled: true,
                    },
                })
            })
            .collect();
        return Body::Multipart { parts };
    }
    if lower.contains("multipart/form-data") && !text.is_empty() {
        losses.approximated(
            here,
            LossKind::Body,
            "multipart body kept as recorded text; its boundary must match the Content-Type header",
        );
        return Body::Raw {
            kind: RawKind::Text,
            data: text,
        };
    }
    if lower.contains("x-www-form-urlencoded") {
        let fields: Vec<KeyValue> = if text.is_empty() {
            params
                .iter()
                .filter_map(|p| {
                    Some(KeyValue::new(
                        percent_decode(&p.str_of("name")?),
                        percent_decode(&p.string("value")),
                    ))
                })
                .collect()
        } else {
            text.split('&')
                .filter(|p| !p.is_empty())
                .map(|pair| {
                    let (n, v) = pair.split_once('=').unwrap_or((pair, ""));
                    KeyValue::new(percent_decode(n), percent_decode(v))
                })
                .collect()
        };
        return Body::FormUrlEncoded { fields };
    }
    if text.is_empty() {
        return Body::None;
    }
    if post.string("encoding") == "base64" {
        losses.dropped(
            here,
            LossKind::Body,
            "base64-encoded (binary) request body not imported",
        );
        return Body::None;
    }
    Body::Raw {
        kind: raw_kind(&mime),
        data: text,
    }
}
