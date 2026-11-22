// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `GET /v1/stream?token=…` (wave 8, strand A): the browser's sessions (WebSocket, SSE,
//! gRPC) through the companion. The browser opens a WebSocket to us; we open the session
//! with the native `StreamTransport` and relay (`rt_core::transport::companion`):
//!
//! 1. The upgrade passes the same checks as `/v1/send`: loopback `Host`, an allowlisted
//!    `Origin` (checked by `server.rs` before we are called), and a valid pairing token for
//!    that origin, here in the query because browsers cannot set headers on a WebSocket.
//! 2. The first message must be `open` (within [`OPEN_TIMEOUT`]); a target on the companion
//!    itself is refused, as for `/v1/send`.
//! 3. Then every event of the native session goes out as `event`; `send`, `finish` and
//!    `close` from the browser go to the session. A failed open or send is answered with
//!    `failed`, and the WebSocket closes. When the browser goes away, the session is closed.
//!
//! The companion sees what the browser sends (handshake headers, messages), as for
//! `/v1/send`; it keeps nothing and logs only the kind and the target's host and port.

use std::future::Future as _;
use std::task::Poll;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use hyper::header::{self, HeaderValue};
use hyper::upgrade::OnUpgrade;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rt_core::stream::{StreamOpen, StreamTransport};
use rt_core::transport::TransportError;
use rt_core::transport::companion::{StreamClientMessage, StreamServerMessage, WireStreamOpen};
use rt_core::transport::native::NativeTransport;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;

use super::server::{Agent, log, log_target};

/// The browser must send `open` within this time after the upgrade.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// Answers the upgrade request (`101`, or an error status with a short text) and, on
/// success, starts the relay once hyper hands over the connection.
pub(super) fn upgrade(
    agent: &Agent,
    req: &Request<()>,
    on_upgrade: Option<OnUpgrade>,
    origin: &str,
) -> Result<Response<()>, (StatusCode, &'static str)> {
    let token = req.uri().query().and_then(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .find(|(k, _)| k == "token")
            .map(|(_, v)| v.into_owned())
    });
    if !token.is_some_and(|t| agent.token_valid(origin, &t)) {
        log(format_args!(
            "rejected GET /v1/stream from {origin}: missing or invalid token"
        ));
        return Err((StatusCode::UNAUTHORIZED, "pairing required"));
    }
    let headers = req.headers();
    let is_upgrade = headers
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let key = headers.get(header::SEC_WEBSOCKET_KEY);
    let (Some(key), true, Some(on_upgrade)) = (key, is_upgrade, on_upgrade) else {
        return Err((StatusCode::BAD_REQUEST, "WebSocket upgrade required"));
    };
    let accept = HeaderValue::from_str(&derive_accept_key(key.as_bytes()))
        .map_err(|_| (StatusCode::BAD_REQUEST, "bad Sec-WebSocket-Key"))?;
    let transport = agent.transport().clone();
    let port = agent.port();
    let origin = origin.to_owned();
    tokio::task::spawn_local(async move {
        let Ok(upgraded) = on_upgrade.await else {
            return;
        };
        let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None).await;
        relay(ws, &transport, port, &origin).await;
    });
    let mut response = Response::new(());
    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    let h = response.headers_mut();
    h.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    h.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    h.insert(header::SEC_WEBSOCKET_ACCEPT, accept);
    Ok(response)
}

type Socket = WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>;

async fn relay(mut ws: Socket, transport: &NativeTransport, port: u16, origin: &str) {
    let open = match tokio::time::timeout(OPEN_TIMEOUT, next_client(&mut ws)).await {
        Ok(Some(StreamClientMessage::Open { open })) => open,
        Ok(Some(_)) => {
            fail(&mut ws, other("the first message must be `open`")).await;
            return;
        }
        Ok(None) => return,
        Err(_) => {
            fail(&mut ws, other("no `open` message in time")).await;
            return;
        }
    };
    let open = match stream_open(open) {
        Ok(open) => open,
        Err(e) => {
            fail(&mut ws, other(&e)).await;
            return;
        }
    };
    let (kind, url) = describe(&open);
    let target = log_target(url);
    if is_self(url, port) {
        log(format_args!(
            "rejected {kind} {target}: the companion itself"
        ));
        fail(
            &mut ws,
            TransportError::Blocked("the companion does not open sessions to itself".into()),
        )
        .await;
        return;
    }
    let mut session = match transport.open(open).await {
        Ok(session) => session,
        Err(e) => {
            log(format_args!("{kind} {target} for {origin} -> {e}"));
            fail(&mut ws, e).await;
            return;
        }
    };
    log(format_args!("{kind} {target} for {origin}: open"));
    let mut client_open = true;
    loop {
        enum Step {
            Event(Option<rt_core::stream::StreamEvent>),
            Client(Option<StreamClientMessage>),
        }
        let step = {
            let mut next = session.next();
            let mut client = std::pin::pin!(next_client(&mut ws));
            std::future::poll_fn(|cx| {
                if let Poll::Ready(event) = next.as_mut().poll(cx) {
                    return Poll::Ready(Step::Event(event));
                }
                if client_open && let Poll::Ready(message) = client.as_mut().poll(cx) {
                    return Poll::Ready(Step::Client(message));
                }
                Poll::Pending
            })
            .await
        };
        match step {
            Step::Event(Some(event)) => {
                if !send(&mut ws, &StreamServerMessage::Event { event }).await {
                    session.close().await;
                    client_open = false;
                }
            }
            Step::Event(None) => break,
            Step::Client(Some(StreamClientMessage::Send { frame })) => {
                if let Err(e) = session.send(frame).await {
                    fail(&mut ws, e).await;
                    session.close().await;
                    return;
                }
            }
            Step::Client(Some(StreamClientMessage::Finish)) => {
                if let Err(e) = session.finish().await {
                    fail(&mut ws, e).await;
                    session.close().await;
                    return;
                }
            }
            Step::Client(Some(StreamClientMessage::Close)) => session.close().await,
            Step::Client(Some(StreamClientMessage::Open { .. })) => {
                fail(&mut ws, other("one session per WebSocket")).await;
                session.close().await;
                return;
            }
            // The browser went away (or sent garbage): end the session.
            Step::Client(None) => {
                client_open = false;
                session.close().await;
            }
        }
    }
    log(format_args!("{kind} {target} for {origin}: closed"));
    let _ = ws.close(None).await;
}

/// The next message from the browser; `None` when it closed or sent something unreadable.
async fn next_client(ws: &mut Socket) -> Option<StreamClientMessage> {
    loop {
        match ws.next().await? {
            Ok(Message::Text(text)) => return serde_json::from_str(text.as_str()).ok(),
            Ok(Message::Binary(bytes)) => return serde_json::from_slice(&bytes).ok(),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => continue,
        }
    }
}

/// Sends one message; `false` if the browser is gone.
async fn send(ws: &mut Socket, message: &StreamServerMessage) -> bool {
    let Ok(text) = serde_json::to_string(message) else {
        return false;
    };
    ws.send(Message::text(text)).await.is_ok()
}

async fn fail(ws: &mut Socket, error: TransportError) {
    let _ = send(ws, &StreamServerMessage::Failed { error }).await;
    let _ = ws.close(None).await;
}

fn other(message: &str) -> TransportError {
    TransportError::Other(message.to_owned())
}

fn stream_open(open: WireStreamOpen) -> Result<StreamOpen, String> {
    Ok(match open {
        WireStreamOpen::WebSocket {
            url,
            headers,
            protocols,
            timeout_ms,
            keep_alive_ms,
        } => StreamOpen::WebSocket {
            url,
            headers,
            protocols,
            timeout_ms,
            keep_alive_ms,
        },
        WireStreamOpen::Sse { request } => StreamOpen::Sse {
            request: request.try_into()?,
        },
        WireStreamOpen::Grpc {
            url,
            path,
            metadata,
            timeout_ms,
        } => StreamOpen::Grpc {
            url,
            path,
            metadata,
            timeout_ms,
        },
    })
}

fn describe(open: &StreamOpen) -> (&'static str, &str) {
    match open {
        StreamOpen::WebSocket { url, .. } => ("WebSocket", url),
        StreamOpen::Sse { request } => ("SSE", &request.url),
        StreamOpen::Grpc { url, .. } => ("gRPC", url),
    }
}

/// Whether `url` (`ws`, `http` or `grpc` scheme) points at this companion.
fn is_self(url: &str, port: u16) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let default = match parsed.scheme() {
        "ws" | "http" | "grpc" => 80,
        "wss" | "https" | "grpcs" => 443,
        _ => 0,
    };
    let local = match parsed.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Domain(d)) => {
            d.eq_ignore_ascii_case("localhost") || d.to_ascii_lowercase().ends_with(".localhost")
        }
        None => false,
    };
    local && parsed.port().unwrap_or(default) == port
}

#[cfg(test)]
mod tests {
    use super::is_self;

    #[test]
    fn sessions_to_the_companion_itself_are_recognised() {
        assert!(is_self("ws://127.0.0.1:47812/v1/stream", 47812));
        assert!(is_self("http://localhost:47812/", 47812));
        assert!(is_self("ws://[::1]:47812/", 47812));
        assert!(!is_self("ws://127.0.0.1:47813/", 47812));
        assert!(!is_self("wss://example.com:47812/", 47812));
        assert!(is_self("ws://127.0.0.1/", 80));
    }
}
