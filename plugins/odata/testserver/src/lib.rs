// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OData V2/V4 test server for the OData plugin (wave 6, strand C). Serves the synthetic
//! services of `plugins/odata/testdata` (see `SOURCES.md` there) with in-memory data and the
//! behaviour the plugin must handle. It imitates SAP Gateway, RAP and CAP from their public
//! documentation; no SAP code is used.
//! - `$metadata` (the EDMX files as they are) and the service document (`GET <root>`);
//! - entity sets, keys (V2 `('id')`, composite `(A='x',B='y')`, V4 `(ID=1,IsActiveEntity=true)`
//!   and bare `(1)`), one level of navigation, `$count`, `$filter` (`eq ne gt ge lt le` on
//!   one property per term, joined by `and`/`or`), `$select`, `$expand` (one level),
//!   `$orderby`, `$top`, `$skip`, `$inlinecount=allpages` (V2) and `$count=true` (V4);
//! - V2 JSON (`d`, `d.results`, `__metadata` with `uri`/`type`/`etag`, `__count`) and V4
//!   JSON (`@odata.context`, `value`, `@odata.etag`, `@odata.count`);
//! - create (`POST`, `201` with `Location`), update (`PATCH`, `MERGE`, `PUT`, also tunnelled as
//!   `POST` with `X-HTTP-Method`), delete; `PUT` merges like `PATCH` (a simplification);
//! - ETags from `ChangedAt` / `LastChangedAt` / `modifiedAt` (weak, `W/"…"`) in `ETag`,
//!   `__metadata.etag` and `@odata.etag`. Writes with `If-Match` that does not match: `412`.
//!   On the S/4 services, writes to active entities without `If-Match`: `428`;
//! - CSRF on all services: a `GET` with `X-CSRF-Token: Fetch` returns a token bound to a
//!   session cookie (`SAP_SESSIONID_RT1_100` on S/4, `JSESSIONID` on CAP); a modifying request
//!   without that token and cookie gets `403` with `X-CSRF-Token: Required`;
//! - `$batch` as multipart/mixed (V2 and V4) with changesets, all-or-nothing, `Content-ID`
//!   references (`$1`) inside a changeset (see [`batch`]);
//! - drafts on the V4 services: RAP (`Edit`, `Prepare`, `Activate`, `Discard`) and CAP
//!   (`draftEdit`, `draftPrepare`, `draftActivate`, `DELETE` on the draft discards). `POST`
//!   to a draft root creates a draft unless the body says `"IsActiveEntity": true`. Active
//!   instances may also be patched directly (a simplification);
//! - `sap-client` (query or header; `100` is the only client, others get `401` with an
//!   SAP-style error) and `sap-language` (the `lang` of V2 errors); Basic auth
//!   [`S4_USER`] / [`S4_PASS`] on the S/4 services, none on CAP;
//! - SAP-style error bodies: V2 `{"error":{"code","message":{"lang","value"}}}`, V4
//!   `{"error":{"code","message"}}`.
//!
//! Every spawned server has its own data. Not supported: function imports, V4 JSON batch,
//! `$search`, nested `$expand`, deep insert, functions in `$filter`.

pub mod batch;
pub mod client;
pub mod odata;
pub mod services;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Incoming};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use tokio::net::TcpListener;

use odata::{Ctx, Op, Reply, ServiceState};
use services::Flavor;

/// Default port of `rt-odata-testserver` (rt-testserver uses 47811).
pub const DEFAULT_PORT: u16 = 47830;

/// Basic auth of the S/4-style services (test credentials only).
pub const S4_USER: &str = "RT_USER";
pub const S4_PASS: &str = "RT_PASS";

/// The only client of the S/4-style system.
pub const S4_CLIENT: &str = "100";

/// Session cookie of the S/4-style services (client 100).
pub const S4_SESSION_COOKIE: &str = "SAP_SESSIONID_RT1_100";
/// Session cookie of the CAP services (as behind an application router).
pub const CAP_SESSION_COOKIE: &str = "JSESSIONID";

/// Service roots (paths) and their EDMX, as in `testdata/SOURCES.md` (interface of the wave 6
/// interface commit; [`services::SERVICES`] has the full descriptions).
pub const SERVICES: &[(&str, &str)] = &[
    (
        "/sap/opu/odata/sap/ZRT_SALES_SRV/",
        include_str!("../../testdata/s4/ZRT_SALES_SRV.edmx"),
    ),
    (
        "/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/",
        include_str!("../../testdata/s4/zrt_salesorder_0001.edmx"),
    ),
    (
        "/odata/v4/admin/",
        include_str!("../../testdata/cap-bookshop/admin-v4.edmx"),
    ),
    (
        "/odata/v2/admin/",
        include_str!("../../testdata/cap-bookshop/admin-v2.edmx"),
    ),
];

/// Binds `addr` and serves until the runtime ends. Every server has its own data.
pub async fn spawn(addr: SocketAddr) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let state = Arc::new(Mutex::new(State::new()));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let state = state.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |r| handle(state.clone(), r));
                let _ = auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    Ok(local)
}

/// Starts the server on a random local port inside the current tokio runtime.
pub async fn spawn_local() -> SocketAddr {
    spawn(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind OData test server")
}

async fn handle(
    state: Arc<Mutex<State>>,
    request: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let (parts, body) = request.into_parts();
    let body = body
        .collect()
        .await
        .map(|b| b.to_bytes())
        .unwrap_or_default();
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(n, v)| {
            (
                n.as_str().to_owned(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let head = parts.method == hyper::Method::HEAD;
    let reply = state.lock().expect("state lock").handle(
        parts.method.as_str(),
        parts.uri.path(),
        parts.uri.query().unwrap_or_default(),
        &headers,
        &body,
    );
    let mut response = Response::builder().status(reply.status);
    for (n, v) in &reply.headers {
        response = response.header(n.as_str(), v.as_str());
    }
    let body = if head { Vec::new() } else { reply.body };
    Ok(response
        .body(Full::new(Bytes::from(body)))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::from_static(b"bad response")))))
}

struct State {
    services: Vec<ServiceState>,
    /// Session id → CSRF token.
    sessions: HashMap<String, String>,
    counter: u64,
}

fn plain(status: u16, text: &str) -> Reply {
    Reply {
        status,
        headers: vec![("Content-Type".into(), "text/plain".into())],
        body: text.as_bytes().to_vec(),
    }
}

impl State {
    fn new() -> Self {
        Self {
            services: services::SERVICES.iter().map(ServiceState::load).collect(),
            sessions: HashMap::new(),
            counter: 0,
        }
    }

    fn token(&mut self) -> String {
        self.counter += 1;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        let mut x = nanos ^ self.counter.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        format!("{x:016x}{:06x}", self.counter)
    }

    fn handle(
        &mut self,
        method: &str,
        raw_path: &str,
        raw_query: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Reply {
        let header = |name| odata::header(headers, name);
        let Some(index) = services::SERVICES
            .iter()
            .position(|s| raw_path.starts_with(s.root) || raw_path == s.root.trim_end_matches('/'))
        else {
            return plain(404, "No OData service at this path");
        };
        let def = &services::SERVICES[index];
        let query = parse_query(raw_query);
        let q = |name: &str| {
            query
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
        };
        let lang = q("sap-language")
            .or_else(|| header("sap-language"))
            .unwrap_or("en")
            .to_ascii_lowercase();
        let host = header("Host").unwrap_or("127.0.0.1");
        let base = format!("http://{host}{}", def.root);
        let ctx = Ctx {
            base: &base,
            lang: &lang,
        };

        if def.flavor == Flavor::S4 {
            let client = q("sap-client")
                .or_else(|| header("sap-client"))
                .unwrap_or(S4_CLIENT);
            if client != S4_CLIENT {
                return odata::error(
                    def,
                    &lang,
                    401,
                    &format!("Logon failed: client {client} does not exist in system RT1"),
                );
            }
            let expected = format!("Basic {}", client::base64(&format!("{S4_USER}:{S4_PASS}")));
            if header("Authorization") != Some(expected.as_str()) {
                let mut reply = plain(401, "Logon failed: user or password wrong");
                reply.headers.push((
                    "WWW-Authenticate".into(),
                    "Basic realm=\"SAP NetWeaver Application Server [RT1/100]\"".into(),
                ));
                return reply;
            }
        }

        let cookie_name = match def.flavor {
            Flavor::S4 => S4_SESSION_COOKIE,
            Flavor::Cap => CAP_SESSION_COOKIE,
        };
        let session = header("Cookie").and_then(|c| {
            c.split(';').find_map(|kv| {
                let (n, v) = kv.trim().split_once('=')?;
                (n == cookie_name).then(|| v.to_owned())
            })
        });
        let token_of_session = session.as_ref().and_then(|s| self.sessions.get(s)).cloned();

        let mut extra = Vec::new();
        let raw_method = method.to_ascii_uppercase();
        if matches!(raw_method.as_str(), "GET" | "HEAD")
            && header("X-CSRF-Token").is_some_and(|t| t.eq_ignore_ascii_case("fetch"))
        {
            let token = match token_of_session.clone() {
                Some(t) => t,
                None => {
                    let sid = self.token();
                    let token = self.token();
                    self.sessions.insert(sid.clone(), token.clone());
                    extra.push((
                        "Set-Cookie".to_owned(),
                        format!("{cookie_name}={sid}; path=/; HttpOnly"),
                    ));
                    token
                }
            };
            extra.push(("X-CSRF-Token".to_owned(), token));
        }
        let modifying = matches!(
            raw_method.as_str(),
            "POST" | "PUT" | "PATCH" | "MERGE" | "DELETE"
        );
        if modifying {
            let sent = header("X-CSRF-Token");
            if sent.is_none() || sent != token_of_session.as_deref() {
                let mut reply = plain(403, "CSRF token validation failed");
                reply
                    .headers
                    .push(("X-CSRF-Token".into(), "Required".into()));
                return reply;
            }
        }

        let resource = percent_decode(
            raw_path
                .strip_prefix(def.root)
                .unwrap_or_default()
                .trim_end_matches('/'),
        );
        let state = &mut self.services[index];
        let mut reply = match (raw_method.as_str(), resource.as_str()) {
            ("GET" | "HEAD", "") => odata::service_document(state, &ctx),
            ("GET" | "HEAD", "$metadata") => Reply {
                status: 200,
                headers: vec![
                    ("Content-Type".into(), "application/xml".into()),
                    odata::version_header(def.version),
                ],
                body: def.edmx.as_bytes().to_vec(),
            },
            ("POST", "$batch") => {
                self.counter += 1;
                batch::handle(
                    state,
                    &ctx,
                    header("Content-Type").unwrap_or_default(),
                    body,
                    self.counter,
                )
            }
            (_, "") | (_, "$metadata") | (_, "$batch") => {
                odata::error(def, &lang, 405, "Method not allowed on this resource")
            }
            _ => {
                let method = match (raw_method.as_str(), header("X-HTTP-Method")) {
                    ("POST", Some(tunnel)) => tunnel.to_ascii_uppercase(),
                    _ => raw_method.clone(),
                };
                let op = Op {
                    method,
                    path: resource,
                    query,
                    headers: headers.to_vec(),
                    body: body.to_vec(),
                };
                odata::execute(state, &ctx, &op)
            }
        };
        reply.headers.extend(extra);
        reply
    }
}

/// Decodes `%XX` escapes (invalid ones stay as they are).
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a=1&b=x%20y` → pairs, decoded (`+` is a space).
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (n, v) = p.split_once('=').unwrap_or((p, ""));
            (
                percent_decode(&n.replace('+', " ")),
                percent_decode(&v.replace('+', " ")),
            )
        })
        .collect()
}

/// Reason phrase of a status code.
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        412 => "Precondition Failed",
        428 => "Precondition Required",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("Books(%27a%27)%2"), "Books('a')%2");
        assert_eq!(
            parse_query("$filter=stock+gt%20100&$top=2"),
            vec![
                ("$filter".to_owned(), "stock gt 100".to_owned()),
                ("$top".to_owned(), "2".to_owned())
            ]
        );
    }
}
