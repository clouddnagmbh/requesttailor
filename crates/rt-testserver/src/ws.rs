// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! WebSocket routes (wave 8, strand A): `GET /ws/echo` and `GET /ws/headers`.
//!
//! The upgrade is hyper's (`serve_connection_with_upgrades`); framing is `tungstenite`'s.
//! A requested `Sec-WebSocket-Protocol` is answered with the first offered protocol.

use futures_util::{SinkExt as _, StreamExt as _};
use hyper::body::Incoming;
use hyper::header::{self, HeaderValue};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role};

use super::{Body, full, json, query_param};

/// What a WebSocket route does once the connection is upgraded.
enum Route {
    /// Echo text and binary messages; with `close`, close with that code after `after`
    /// messages (default 1; 0: right away).
    Echo { close: Option<u16>, after: usize },
    /// Send the handshake headers as JSON, then close (1000).
    Headers(serde_json::Value),
}

pub(super) fn echo(req: Request<Incoming>, query: &str) -> Response<Body> {
    let close = query_param(query, "close").and_then(|c| c.parse().ok());
    let after = query_param(query, "after")
        .and_then(|a| a.parse().ok())
        .unwrap_or(1);
    upgrade(req, Route::Echo { close, after })
}

pub(super) fn headers(req: Request<Incoming>) -> Response<Body> {
    let headers: Vec<serde_json::Value> = req
        .headers()
        .iter()
        .map(|(k, v)| serde_json::json!([k.as_str(), String::from_utf8_lossy(v.as_bytes())]))
        .collect();
    upgrade(req, Route::Headers(serde_json::Value::Array(headers)))
}

fn upgrade(mut req: Request<Incoming>, route: Route) -> Response<Body> {
    let is_upgrade = req
        .headers()
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let Some(key) = req.headers().get(header::SEC_WEBSOCKET_KEY).cloned() else {
        return json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "not a WebSocket handshake" }),
        );
    };
    if !is_upgrade {
        return json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "not a WebSocket handshake" }),
        );
    }
    let protocol = req
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').map(str::trim).find(|p| !p.is_empty()))
        .and_then(|p| HeaderValue::from_str(p).ok());
    let on_upgrade = hyper::upgrade::on(&mut req);
    tokio::spawn(async move {
        let Ok(upgraded) = on_upgrade.await else {
            return;
        };
        let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None).await;
        run(ws, route).await;
    });
    let mut r = full(StatusCode::SWITCHING_PROTOCOLS, "text/plain", "");
    let h = r.headers_mut();
    h.remove(header::CONTENT_TYPE);
    h.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    h.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    if let Ok(accept) = HeaderValue::from_str(&derive_accept_key(key.as_bytes())) {
        h.insert(header::SEC_WEBSOCKET_ACCEPT, accept);
    }
    if let Some(protocol) = protocol {
        h.insert(header::SEC_WEBSOCKET_PROTOCOL, protocol);
    }
    r
}

async fn run<S>(mut ws: WebSocketStream<S>, route: Route)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let close = |code: u16, reason: &str| {
        Some(CloseFrame {
            code: CloseCode::from(code),
            reason: reason.to_owned().into(),
        })
    };
    match route {
        Route::Headers(headers) => {
            let _ = ws.send(Message::text(headers.to_string())).await;
            let _ = ws.close(close(1000, "done")).await;
            drain(&mut ws).await;
        }
        Route::Echo {
            close: Some(code),
            after: 0,
        } => {
            let _ = ws.close(close(code, "closed by request")).await;
            drain(&mut ws).await;
        }
        Route::Echo {
            close: close_code,
            after,
        } => {
            let mut echoed = 0;
            while let Some(Ok(message)) = ws.next().await {
                let reply = match message {
                    Message::Text(_) | Message::Binary(_) => message,
                    // tungstenite answers the close frame on the next read.
                    Message::Close(_) => continue,
                    _ => continue,
                };
                if ws.send(reply).await.is_err() {
                    return;
                }
                echoed += 1;
                if let Some(code) = close_code
                    && echoed >= after
                {
                    let _ = ws.close(close(code, "closed by request")).await;
                    drain(&mut ws).await;
                    return;
                }
            }
        }
    }
}

/// Reads until the peer's close frame (or the end), so the close handshake completes.
async fn drain<S>(ws: &mut WebSocketStream<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    while let Some(Ok(_)) = ws.next().await {}
}
