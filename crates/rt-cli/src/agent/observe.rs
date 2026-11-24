// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Exchange delivery for `/v1/send` (wave 17). The companion streams the response to the
//! browser unchanged; [`Tap`] copies up to [`EXCHANGE_BODY_LIMIT`] bytes on the way and, when
//! the stream ends, hands one masked [`Exchange`] to the observers. Nothing is stored and
//! nothing waits: the observers get the exchange after the last chunk went to the client.
//!
//! The companion sees only the final request the web app built, so it cannot know which
//! values are secret. It masks what it can recognize (`rt_core::observe::guess_secrets`:
//! values of headers and query parameters named `authorization`, `cookie`, `*key*`,
//! `*token*`, `*secret*`), also where they echo in the response, and the header rules of
//! `rt_core::observe`.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::Stream;
use rt_core::guard::Caller;
use rt_core::observe::{EXCHANGE_BODY_LIMIT, ObserverSet, exchange_of, guess_secrets};
use rt_core::send::{REPORT_VERSION, ReceivedResponse, ReportBody, SendReport, SentRequest};
use rt_core::transport::{BodyStream, ResponseHead, TransportError};
use web_time::Instant;

/// What was sent and answered, minus the body.
pub(super) struct Wire {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body_size: u64,
    pub head: ResponseHead,
    pub started: Instant,
}

/// The response body stream that reports the exchange when it ends.
pub(super) struct Tap {
    inner: BodyStream,
    observers: ObserverSet,
    wire: Option<Wire>,
    copy: Vec<u8>,
    size: u64,
}

impl Tap {
    pub(super) fn new(observers: ObserverSet, wire: Wire, inner: BodyStream) -> Self {
        Self {
            inner,
            observers,
            wire: Some(wire),
            copy: Vec::new(),
            size: 0,
        }
    }

    fn deliver(&mut self) {
        let Some(wire) = self.wire.take() else { return };
        let secrets = guess_secrets(&wire.url, &wire.headers);
        let truncated = self.size as usize > self.copy.len();
        let body = match std::str::from_utf8(&self.copy) {
            Ok(text) => ReportBody::Utf8(text.to_owned()),
            Err(e) if truncated && e.error_len().is_none() => ReportBody::Utf8(
                String::from_utf8_lossy(&self.copy[..e.valid_up_to()]).into_owned(),
            ),
            Err(_) => {
                use base64::Engine as _;
                ReportBody::Base64(base64::engine::general_purpose::STANDARD.encode(&self.copy))
            }
        };
        let report = SendReport {
            version: REPORT_VERSION,
            request_path: String::new(),
            environment: None,
            request: Some(SentRequest {
                method: wire.method,
                url: wire.url,
                headers: wire.headers,
                body_size: wire.body_size,
            }),
            response: Some(ReceivedResponse {
                tls: None,
                status: wire.head.status,
                version: wire.head.version,
                headers: wire.head.headers,
                url: wire.head.url,
                redirects: wire.head.redirects,
                body,
                size: self.size,
                truncated,
                timing: wire.head.timing,
                total: wire.started.elapsed(),
            }),
            variables: Vec::new(),
            unresolved: Vec::new(),
            error: None,
            notes: Vec::new(),
            tests: Vec::new(),
            assertions: Vec::new(),
            logs: Vec::new(),
            var_changes: Vec::new(),
            flow: None,
            checks: Vec::new(),
            rate_limit: None,
            trace: None,
            segments: Vec::new(),
        };
        let mut exchange = exchange_of(&report, &secrets, Caller::App, None);
        exchange.request_path = None;
        self.observers.notify(&exchange);
    }
}

impl Stream for Tap {
    type Item = Result<Bytes, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let item = self.inner.as_mut().poll_next(cx);
        match &item {
            Poll::Ready(Some(Ok(chunk))) => {
                self.size += chunk.len() as u64;
                // A little more than the limit: masking cuts after it (see `exchange_of`).
                let room = (EXCHANGE_BODY_LIMIT * 2).saturating_sub(self.copy.len());
                let take = room.min(chunk.len());
                self.copy.extend_from_slice(&chunk[..take]);
            }
            Poll::Ready(None) => self.deliver(),
            _ => {}
        }
        item
    }
}
