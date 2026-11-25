// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Paged lists for the paging tests (wave 18, strand C): `GET /pages/link/{n}`,
//! `/pages/url/{n}` and `/pages/cursor/{n}` serve `n` pages of `size` items each (defaults
//! 10); every item is `{"id": i, "name": "item-i", "pad": "xxx…"}` with `pad` bytes of
//! padding (default 0), ids counting up from 1 across the pages.
//!
//! | Route | The next page is named by |
//! | --- | --- |
//! | `/pages/link/{n}?page=K` | `Link: <url>; rel="next"` (also `rel="prev"` and `rel="last"`) |
//! | `/pages/url/{n}?page=K` | the body's `next` (a URL or `null`), the page in `page` |
//! | `/pages/cursor/{n}` | the body's `meta.cursor` (`c2`, `c3`, … or `null`); the request sends it back as the query `cursor`, the header `x-cursor` or the JSON body field `after` |
//!
//! Switches (query): `size`, `pad`, `relative=1` (relative next URL), `foreign=1` (next URL on
//! `localhost` instead of the host of the request: another origin), `fail_at=K` (page K
//! answers 500), `loop=1` (the cursor and the next URL name the page just served again).
//! Every body also carries `method` (the method of the request) and `total` (all items).

use http_body_util::BodyExt as _;
use hyper::body::Incoming;
use hyper::header::{HeaderValue, LINK};
use hyper::{Request, Response, StatusCode};

use super::{Body, full, json, query_param};

const MAX_PAGES: usize = 5_000;
const MAX_SIZE: usize = 100_000;
const MAX_PAD: usize = 16 * 1024 * 1024;

pub(super) async fn route(
    req: Request<Incoming>,
    kind: &str,
    n: &str,
    query: &str,
) -> Response<Body> {
    let Ok(pages) = n.parse::<usize>() else {
        return json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "bad page count" }),
        );
    };
    let number = |key: &str, default: usize| {
        query_param(query, key)
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(default)
    };
    let pages = pages.clamp(1, MAX_PAGES);
    let size = number("size", 10).min(MAX_SIZE);
    let pad = number("pad", 0).min(MAX_PAD);
    let fail_at = number("fail_at", 0);
    let looping = query_param(query, "loop").as_deref() == Some("1");
    let relative = query_param(query, "relative").as_deref() == Some("1");
    let foreign = query_param(query, "foreign").as_deref() == Some("1");
    let method = req.method().to_string();
    let host = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1")
        .to_owned();
    // `foreign` swaps the host name for another one that reaches the same server.
    let base_host = if foreign {
        match host.split_once(':') {
            Some((_, port)) => format!("localhost:{port}"),
            None => "localhost".to_owned(),
        }
    } else {
        host.clone()
    };
    let origin = if relative {
        String::new()
    } else {
        format!("http://{base_host}")
    };
    let tail = format!("size={size}&pad={pad}");
    let tail = if fail_at > 0 {
        format!("{tail}&fail_at={fail_at}")
    } else {
        tail
    };

    // The page: the query `page`, or for the cursor kind the number inside `cN`.
    let cursor_in = match kind {
        "cursor" => cursor_of(req, query).await,
        _ => None,
    };
    let page = match kind {
        "cursor" => cursor_in
            .as_deref()
            .and_then(|c| c.strip_prefix('c'))
            .and_then(|c| c.parse::<usize>().ok())
            .unwrap_or(1),
        _ => number("page", 1),
    }
    .clamp(1, pages);
    if fail_at == page {
        return json(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "boom", "page": page }),
        );
    }
    let next_page = if looping { page } else { page + 1 };
    let has_next = looping || page < pages;
    let url = |p: usize| format!("{origin}/pages/{kind}/{pages}?page={p}&{tail}");
    let items = items(page, size, pad);
    let total = pages * size;

    match kind {
        "link" => {
            let body =
                format!(r#"{{"total":{total},"page":{page},"method":"{method}","items":{items}}}"#);
            let mut response = full(StatusCode::OK, "application/json", body);
            let mut links = Vec::new();
            if has_next {
                links.push(format!("<{}>; rel=\"next\"", url(next_page)));
            }
            if page > 1 {
                links.push(format!("<{}>; rel=\"prev\"", url(page - 1)));
            }
            links.push(format!("<{}>; rel=\"last\"", url(pages)));
            if let Ok(v) = HeaderValue::from_str(&links.join(", ")) {
                response.headers_mut().insert(LINK, v);
            }
            response
        }
        "url" => {
            let next = if has_next {
                serde_json::Value::String(url(next_page))
            } else {
                serde_json::Value::Null
            };
            let body = format!(
                r#"{{"total":{total},"page":{page},"method":"{method}","next":{next},"items":{items}}}"#
            );
            full(StatusCode::OK, "application/json", body)
        }
        "cursor" => {
            let next = if has_next {
                serde_json::Value::String(format!("c{next_page}"))
            } else {
                serde_json::Value::Null
            };
            let body = format!(
                r#"{{"total":{total},"method":"{method}","meta":{{"cursor":{next}}},"items":{items}}}"#
            );
            full(StatusCode::OK, "application/json", body)
        }
        _ => json(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "no such pages route" }),
        ),
    }
}

/// The cursor a request carries: query `cursor`, header `x-cursor` or JSON body field `after`.
async fn cursor_of(req: Request<Incoming>, query: &str) -> Option<String> {
    if let Some(c) = query_param(query, "cursor") {
        return Some(c);
    }
    if let Some(c) = req.headers().get("x-cursor").and_then(|v| v.to_str().ok()) {
        return Some(c.to_owned());
    }
    let bytes = req.into_body().collect().await.ok()?.to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("after")?.as_str().map(str::to_owned)
}

fn items(page: usize, size: usize, pad: usize) -> String {
    let first = (page - 1) * size + 1;
    let padding = "x".repeat(pad);
    let mut out = String::with_capacity(2 + size * (40 + pad));
    out.push('[');
    for (k, id) in (first..first + size).enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            r#"{{"id":{id},"name":"item-{id}","pad":"{padding}"}}"#
        ));
    }
    out.push(']');
    out
}
