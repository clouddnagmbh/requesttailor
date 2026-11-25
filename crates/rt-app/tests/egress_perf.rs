// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget `egress_overhead` (wave 19, strand B): one egress call adds at most 1 ms to the
//! transport it goes through (host list of 100 entries, a secret header, the guard asked).
//! The measured test runs in a release build with `RT_PERF_OUT=<dir>` and writes
//! `<dir>/egress.json` (`{"egress_overhead": <ms>, ...}`); `node scripts/perf/run.mjs --only
//! egress` drives it. Without the variable it only checks the limit.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use std::time::Instant;

use bytes::Bytes;
use rt_app::egress::{Context, EgressMethod, EgressRequest, HostList, request};
use rt_core::guard::{Caller, GuardVerdict, SendCheck, SendGuard};
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport,
    TransportError, collect,
};

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = TaskContext::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

/// A transport that answers a small JSON body at once.
struct Fake;

impl Transport for Fake {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let body: BodyStream = Box::pin(Once(Some(Bytes::from_static(
            b"{\"spans\":[{\"name\":\"GET /orders\",\"duration\":12}]}",
        ))));
        Ok(Response {
            head: ResponseHead {
                status: 200,
                version: HttpVersion::Http11,
                headers: vec![("content-type".into(), "application/json".into())],
                url: request.url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

struct Allow;

impl SendGuard for Allow {
    fn check(&self, _: &SendCheck) -> GuardVerdict {
        GuardVerdict::Allow
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

#[test]
fn egress_overhead_is_measured() {
    let mut hosts = HostList::new();
    for i in 0..100 {
        hosts
            .allow(
                &format!("host-{i}.example"),
                None,
                "traces",
                "Server traces",
                1,
            )
            .unwrap();
    }
    let secrets = vec![("t".to_owned(), "perf-secret-value".to_owned())];
    let req = EgressRequest {
        method: EgressMethod::Get,
        url: "https://host-99.example/api/traces/4bf92f3577b34da6a3ce929d0e0e4736".into(),
        headers: vec![("Authorization".into(), "Bearer {{secret:t}}".into())],
        body: None,
        extension: "traces".into(),
        purpose: "Server traces".into(),
        collection: None,
        environment: None,
    };
    let ctx = Context {
        hosts: &hosts,
        guard: Some(&Allow),
        caller: Caller::App,
        collection: "Orders",
        environment: None,
        secrets: &secrets,
    };
    let wire = HttpRequest {
        method: "GET".into(),
        url: req.url.clone(),
        headers: vec![("Authorization".into(), "Bearer perf-secret-value".into())],
        body: None,
        options: Default::default(),
    };
    // Both are timed call by call, interleaved, so a noisy moment hits both alike.
    let (mut with, mut without) = (Vec::new(), Vec::new());
    for _ in 0..1000 {
        let t = Instant::now();
        let r = block_on(request(&Fake, &ctx, &req)).unwrap();
        with.push(t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(r.status, 200);
        let t = Instant::now();
        block_on(async {
            let response = Fake.send(wire.clone()).await.unwrap();
            collect(response, 4 << 20, web_time::Instant::now())
                .await
                .unwrap()
        });
        without.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let overhead = (median(with.clone()) - median(without.clone())).max(0.0);
    assert!(overhead <= 1.0, "an egress call adds {overhead:.3} ms");
    if let Ok(out) = std::env::var("RT_PERF_OUT") {
        let json = serde_json::json!({
            "egress_overhead": overhead,
            "calls": 1000,
            "hosts": 100,
            "median_call_ms": median(with),
            "median_transport_ms": median(without),
        });
        std::fs::write(
            std::path::Path::new(&out).join("egress.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}
