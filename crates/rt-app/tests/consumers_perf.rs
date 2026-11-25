// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Performance budget `consumers_verify` (wave 19, strand F): the own time of verifying 20
//! consumers of 50 requests each (reading the collections, running them through the runner
//! against a transport that answers at once, the text report and the JSON), median of 5
//! runs, at most 1000 ms (`perf/budgets.toml`). `scripts/perf/consumers.mjs` runs this in a
//! release build with `RT_PERF_OUT=<dir>` and reads `<dir>/consumers.json`.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::consumers::{
    Budget, ConsumerRef, ConsumerReport, ConsumerVerifyRequest, Loaded, run_consumer,
};
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};
use web_time::Instant;

const CONSUMERS: usize = 20;
const REQUESTS: usize = 50;
const RUNS: usize = 5;

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

/// Answers every request with the same small object.
struct Fake;

impl Transport for Fake {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let body: BodyStream = Box::pin(Once(Some(Bytes::from_static(
            b"{\"count\":3,\"items\":[{\"price\":1.5,\"name\":\"a\"}]}",
        ))));
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

fn consumer_files() -> FileSet {
    let mut files = FileSet::default();
    let mut put = |path: String, text: String| {
        files.files.insert(path, text.into_bytes());
    };
    put(
        COLLECTION_FILE.to_owned(),
        "opencollection: 1.0.0\ninfo:\n  name: perf consumer\n".into(),
    );
    put(
        "environments/ci.yml".into(),
        "name: ci\nvariables:\n  - name: baseUrl\n    value: http://localhost:1\n".into(),
    );
    for n in 0..REQUESTS {
        put(
            format!("r{n:02}.yml"),
            format!(
                "info:\n  name: r{n}\n  type: http\n  seq: {n}\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/things/{n}\"\n\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n    - expression: res.body.count\n      operator: isNumber\n    - expression: res.body.items[0].price\n      operator: isNumber\n"
            ),
        );
    }
    files
}

#[test]
fn consumers_verify_is_measured() {
    let files = consumer_files();
    let refs: Vec<ConsumerRef> = (0..CONSUMERS)
        .map(|n| ConsumerRef {
            name: format!("consumer-{n}"),
            source: format!("../c{n}"),
            git_ref: None,
            path: None,
            environment: "ci".into(),
            vars: Vec::new(),
        })
        .collect();
    let request = ConsumerVerifyRequest {
        base_url: "http://provider.test".into(),
        only: Vec::new(),
        fetch: false,
        keep_going: true,
        trust_scripts: false,
        allow_writes: false,
        max_minutes: None,
    };
    let mut times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        let mut budget = Budget::new(None);
        let mut report = ConsumerReport::default();
        for consumer in &refs {
            let collection = read_collection(&files).unwrap().model().clone();
            let loaded = Loaded {
                collection,
                files: files.clone(),
                commit: None,
            };
            report.consumers.push(block_on(run_consumer(
                consumer,
                loaded,
                &request,
                &mut budget,
                &Fake,
                &SendOptions::default(),
                &Services::none(),
                &|| false,
            )));
        }
        let text = rt_app::consumers::text(&report);
        let json = serde_json::to_string_pretty(&report).unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        assert!(!report.broken(), "{text}");
        assert_eq!(
            report
                .consumers
                .iter()
                .map(|c| c.requests as usize)
                .sum::<usize>(),
            CONSUMERS * REQUESTS
        );
        assert!(json.len() > 100);
    }
    times.sort_by(f64::total_cmp);
    let median = times[RUNS / 2];
    println!("consumers_verify: median {median:.1} ms over {RUNS} runs");
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({ "consumers_verify": median, "worst": times[RUNS - 1] });
        std::fs::write(
            std::path::Path::new(&dir).join("consumers.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
    if !cfg!(debug_assertions) {
        assert!(median < 1000.0, "{median} ms");
    }
}
