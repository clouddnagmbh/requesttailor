// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Server-Sent Events routes (wave 8, strand A): `GET /sse/count/{n}` and `GET /sse/named`.
//! The events are written one by one with a pause between them (`?interval=<ms>`, default
//! 10), then the stream ends.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::BodyExt as _;
use hyper::body::Frame;
use hyper::header::{self, HeaderValue};
use hyper::{Response, StatusCode};
use tokio::sync::mpsc;

use super::{Body, json, query_param};

/// The events of `/sse/named`: names, ids, multi-line data and a comment.
const NAMED: &[&str] = &[
    ": a comment line, ignored by clients\n",
    "event: greeting\nid: 1\ndata: hello\n\n",
    "event: update\nid: 2\ndata: {\"n\":2}\n\n",
    "event: update\nid: 3\ndata: line one\ndata: line two\n\n",
    "id: 4\ndata: default type\n\n",
];

pub(super) fn count(n: &str, query: &str) -> Response<Body> {
    let Ok(n) = n.parse::<u32>() else {
        return json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "bad count" }),
        );
    };
    let events = (1..=n.min(100_000))
        .map(|i| format!("id: {i}\ndata: {i}\n\n"))
        .collect();
    stream(events, interval(query))
}

pub(super) fn named(query: &str) -> Response<Body> {
    stream(
        NAMED.iter().map(|s| (*s).to_owned()).collect(),
        interval(query),
    )
}

fn interval(query: &str) -> Duration {
    Duration::from_millis(
        query_param(query, "interval")
            .and_then(|ms| ms.parse().ok())
            .unwrap_or(10u64)
            .min(10_000),
    )
}

fn stream(events: Vec<String>, interval: Duration) -> Response<Body> {
    let (tx, rx) = mpsc::channel::<Bytes>(16);
    tokio::spawn(async move {
        for event in events {
            if tx.send(Bytes::from(event)).await.is_err() {
                return;
            }
            tokio::time::sleep(interval).await;
        }
    });
    let mut r = Response::new(Channel(rx).boxed());
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

/// A body fed by a channel; ends when the sender is dropped.
struct Channel(mpsc::Receiver<Bytes>);

impl hyper::body::Body for Channel {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.0.poll_recv(cx).map(|b| b.map(|b| Ok(Frame::data(b))))
    }
}
