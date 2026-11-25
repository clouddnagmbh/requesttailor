// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Performance budget `access_check` (wave 19, strand D): the own time of one two-user access
//! check (two profiles, a fake transport that answers at once, the comparison and the evidence
//! of 2 KB JSON bodies), median of 200 checks, at most 10 ms (`perf/budgets.toml`).
//! `scripts/perf/access.mjs` runs this in a release build with `RT_PERF_OUT=<dir>` and reads
//! `<dir>/access.json`.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::access::{AccessEnv, AccessOptions, AccessProfile, AccessVerdict, check};
use rt_app::envcompare::EnvRun;
use rt_app::runner::RunSnapshot;
use rt_core::model::{
    Collection, Environment, Item, KeyValue, RequestEntry, RequestSpec, Variable,
};
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::FileSet;
use web_time::Instant;

const CHECKS: usize = 200;

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

/// Answers every request with the same 2 KB object.
struct Fake(String);

impl Transport for Fake {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(self.0.clone()))));
        Ok(Response {
            head: ResponseHead {
                status: 200,
                version: HttpVersion::Http11,
                headers: vec![("Content-Type".into(), "application/json".into())],
                url: request.url.clone(),
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

fn collection() -> Collection {
    let env = |name: &str| Environment {
        name: name.into(),
        variables: vec![
            Variable::new("base", "http://perf.test"),
            Variable::new("orderId", "7"),
        ],
        client_certificates: vec![],
        external_secrets: None,
    };
    Collection {
        name: "perf".into(),
        items: vec![Item::Request(RequestEntry {
            path: "r.yml".into(),
            spec: RequestSpec {
                name: "r".into(),
                method: "GET".into(),
                url: "{{base}}/orders/{{orderId}}".into(),
                headers: vec![KeyValue::new("x-user", "u")],
                ..RequestSpec::default()
            },
        })],
        environments: vec![env("A"), env("B")],
        ..Collection::default()
    }
}

#[test]
fn access_check_is_measured() {
    let items: Vec<String> = (0..20)
        .map(|n| format!("{{\"id\":{n},\"name\":\"item number {n}\",\"qty\":{n},\"tags\":[\"a\",\"b\"],\"note\":\"{}\"}}", "n".repeat(40)))
        .collect();
    let body = format!("{{\"id\":7,\"items\":[{}]}}", items.join(","));
    let server = Fake(body);
    let c = collection();
    let snap = |env: &str| RunSnapshot {
        collection: c.clone(),
        environment: Some(env.into()),
        vars: Vec::new(),
        files: FileSet::default(),
    };
    let a = EnvRun {
        snapshot: snap("A"),
        services: Services::none(),
    };
    let b = EnvRun {
        snapshot: snap("B"),
        services: Services::none(),
    };
    let env = AccessEnv {
        a: &a,
        b: &b,
        transport: &server,
        options: &SendOptions::default(),
        caller: "perf",
        audit: &|_| {},
        cancelled: &|| false,
    };
    let profile = AccessProfile {
        a: "A".into(),
        b: "B".into(),
    };
    let options = AccessOptions {
        rate: 1_000_000,
        ..AccessOptions::default()
    };
    let mut times = Vec::with_capacity(CHECKS);
    for _ in 0..CHECKS {
        let start = Instant::now();
        let result = block_on(check(&env, "r.yml", &profile, &options)).unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(result.verdict, AccessVerdict::BrokenObjectAccess);
    }
    times.sort_by(f64::total_cmp);
    let median = times[CHECKS / 2];
    println!("access_check: median {median:.3} ms over {CHECKS} checks");
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({ "access_check": median, "p95": times[CHECKS * 95 / 100] });
        std::fs::write(
            std::path::Path::new(&dir).join("access.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
    // The own-time budget is a release-build number.
    if !cfg!(debug_assertions) {
        assert!(median < 10.0, "{median} ms");
    }
}
