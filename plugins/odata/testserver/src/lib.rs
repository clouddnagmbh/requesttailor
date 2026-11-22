// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OData V2/V4 test server for the OData plugin (wave 6, strand C). Serves the synthetic
//! services of `plugins/odata/testdata` (see `SOURCES.md` there) with in-memory data and the
//! behaviour the plugin must handle:
//! - `$metadata`, entity sets, keys, navigation, `$filter`/`$select`/`$expand`/`$orderby`/
//!   `$top`/`$skip`/`$count` (subset), V2 JSON (`d`/`results`, `__metadata`) and V4 JSON;
//! - CSRF: modifying requests need `X-CSRF-Token` fetched with `X-CSRF-Token: Fetch`, bound
//!   to a session cookie (`SAP_SESSIONID_RT1_100` style); otherwise `403` with
//!   `X-CSRF-Token: Required`;
//! - ETags: `ETag` / `__metadata.etag` / `@odata.etag`; writes need a matching `If-Match`
//!   (`412` otherwise, `428` without one on the S/4 services);
//! - `$batch`: multipart/mixed with changesets (all-or-nothing), V4 JSON batch;
//! - drafts (V4 RAP and CAP): edit, prepare, activate, discard;
//! - `sap-client` (`100` default; others `401`) and `sap-language` on the S/4 services;
//!   Basic auth `RT_USER` / `RT_PASS` on the S/4 services, none on CAP.
//!
//! Interface commit: only the skeleton; the services answer `501`.

use std::convert::Infallible;
use std::net::SocketAddr;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use tokio::net::TcpListener;

/// Default port of `rt-odata-testserver` (rt-testserver uses 47811).
pub const DEFAULT_PORT: u16 = 47830;

/// Basic auth of the S/4-style services (test credentials only).
pub const S4_USER: &str = "RT_USER";
pub const S4_PASS: &str = "RT_PASS";

/// Service roots (paths) and their EDMX, as in `testdata/SOURCES.md`.
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
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(async move {
                let service = hyper::service::service_fn(handle);
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

async fn handle(request: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let _ = request;
    Ok(Response::builder()
        .status(501)
        .body(Full::new(Bytes::from_static(b"not implemented yet")))
        .expect("static response"))
}
