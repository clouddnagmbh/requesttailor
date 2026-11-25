// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `GET /v1/inbox?token=…` (wave 18, strand E): the webhook inbox for the browser. A page cannot
//! listen on a port, so the companion does it (`rt_app::inbox::Inbox`) and relays what arrives:
//!
//! 1. The upgrade passes the checks of every route: loopback `Host`, an allowlisted `Origin`
//!    (both checked by `server.rs` before we are called) and, as for `/v1/stream`, a valid pairing
//!    token for that origin in the query.
//! 2. The browser sends [`InboxClientMessage`]s (`start` with settings, `stop`, `clear`,
//!    `status`); we answer with `status` and push every inbox event as `event`. A connecting
//!    browser first gets the status and the events held, so a reload shows what arrived.
//! 3. The listener itself is the inbox of `rt_app::inbox`: loopback only, fixed answer, hard
//!    limits, memory only. It starts on `start` and runs until `stop` or until the companion ends,
//!    also when the page goes away (so a reload does not lose the webhook sender's target).
//!
//! The companion keeps nothing on disk and logs only the port.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use hyper::header::{self, HeaderValue};
use hyper::upgrade::OnUpgrade;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rt_app::inbox::Inbox;
use rt_core::transport::companion::{InboxClientMessage, InboxServerMessage};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;

use super::server::{Agent, log};

/// How often new events are looked for (the inbox is polled, a few milliseconds are enough).
const TICK: Duration = Duration::from_millis(10);

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
            "rejected GET /v1/inbox from {origin}: missing or invalid token"
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
    let inbox = agent.inbox().clone();
    tokio::task::spawn_local(async move {
        let Ok(upgraded) = on_upgrade.await else {
            return;
        };
        let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None).await;
        relay(ws, inbox).await;
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

async fn relay(mut ws: Socket, inbox: Inbox) {
    let mut since = 0;
    if !send(&mut ws, &InboxServerMessage::Status { status: inbox.status() }).await {
        return;
    }
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            message = next_client(&mut ws) => {
                let Some(message) = message else { break };
                let reply = match message {
                    InboxClientMessage::Start { settings } => {
                        let port = settings.port;
                        match inbox.set_settings(settings).and_then(|()| inbox.start()) {
                            Ok(status) => {
                                log(format_args!("inbox: listening on port {}", status.port.unwrap_or(port)));
                                InboxServerMessage::Status { status }
                            }
                            Err(e) => InboxServerMessage::Failed { message: e.to_string() },
                        }
                    }
                    InboxClientMessage::Stop => InboxServerMessage::Status { status: inbox.stop() },
                    InboxClientMessage::Clear => {
                        inbox.clear();
                        InboxServerMessage::Status { status: inbox.status() }
                    }
                    InboxClientMessage::Status => InboxServerMessage::Status { status: inbox.status() },
                };
                if !send(&mut ws, &reply).await {
                    break;
                }
            }
            _ = tick.tick() => {}
        }
        let page = inbox.events(since);
        since = page.next;
        for event in page.events {
            if !send(&mut ws, &InboxServerMessage::Event { event }).await {
                return;
            }
        }
        // Status changes (start, stop, a request held or dropped) ride along with events.
    }
    let _ = ws.close(None).await;
}

async fn next_client(ws: &mut Socket) -> Option<InboxClientMessage> {
    loop {
        match ws.next().await? {
            Ok(Message::Text(text)) => return serde_json::from_str(text.as_str()).ok(),
            Ok(Message::Binary(bytes)) => return serde_json::from_slice(&bytes).ok(),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => continue,
        }
    }
}

async fn send(ws: &mut Socket, message: &InboxServerMessage) -> bool {
    let Ok(text) = serde_json::to_string(message) else {
        return false;
    };
    ws.send(Message::text(text)).await.is_ok()
}
