// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Rate limits (wave 18, strand D): the header parser, the per-host tracker and the runner's
//! throttling (`RunPlan.throttle`). Time is a fake clock that the fake sleep advances, so a
//! wait of a minute costs nothing and its error can be measured exactly. Budget
//! `ratelimit_wait` (at most 50 ms from the announced wait): the fake clock proves the logic,
//! `ratelimit_wait_is_measured` (with `RT_PERF_OUT`) the real sleep.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_core::model::{Collection, Item, RequestEntry, RequestSpec};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::ratelimit::{
    self, DEFAULT_MAX_WAIT_MS, MAX_WAIT_HARD_MS, RateLimits, RateTracker, RateWindow, Throttle,
    ThrottleMode, host_of, parse_at,
};
use rt_core::runner::{RunEvent, RunPlan, RunSummary, run_with_tracker};
use rt_core::send::{SendOptions, SendReport, send_with};
use rt_core::services::{LocalBoxFuture, Services};
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

/// 2023-11-14 22:13:20 GMT, a modern date (epoch seconds 1.7e9, so "Unix seconds" is
/// recognised from the value alone).
const DATE: &str = "Tue, 14 Nov 2023 22:13:20 GMT";
const DATE_MS: i64 = 1_700_000_000_000;
/// The example date of RFC 9110.
const RFC_DATE: &str = "Sun, 06 Nov 1994 08:49:37 GMT";
const RFC_DATE_MS: i64 = 784_111_777_000;

// ---------------------------------------------------------------------------------------------
// The parser

#[test]
fn the_ietf_draft_headers_are_read() {
    let got = parse_at(
        &h(&[
            ("RateLimit-Limit", "60"),
            ("RateLimit-Remaining", "47"),
            ("RateLimit-Reset", "12"),
            ("RateLimit-Policy", "60;w=60"),
        ]),
        0,
    )
    .unwrap();
    assert_eq!(got.limit, Some(60));
    assert_eq!(got.remaining, Some(47));
    assert_eq!(got.reset_ms, Some(12_000));
    assert_eq!(got.retry_after_ms, None);
    assert_eq!(
        got.windows,
        vec![RateWindow {
            limit: 60,
            seconds: 60
        }]
    );
}

#[test]
fn the_combined_header_is_read_as_dictionary_and_as_items() {
    let dict = parse_at(&h(&[("ratelimit", "limit=100, remaining=7, reset=30")]), 0).unwrap();
    assert_eq!(
        (dict.limit, dict.remaining, dict.reset_ms),
        (Some(100), Some(7), Some(30_000))
    );

    let items = parse_at(
        &h(&[
            ("RateLimit", "\"default\";r=5;t=20"),
            (
                "RateLimit-Policy",
                "\"default\";q=100;w=60, \"burst\";q=10;w=1",
            ),
        ]),
        0,
    )
    .unwrap();
    assert_eq!(
        (items.limit, items.remaining, items.reset_ms),
        (Some(100), Some(5), Some(20_000))
    );
    assert_eq!(items.windows.len(), 2);
    assert_eq!(items.windows[1].seconds, 1);
}

#[test]
fn vendor_prefixes_are_read_and_the_standard_ones_win() {
    let x = parse_at(
        &h(&[
            ("X-RateLimit-Limit", "5000"),
            ("X-RateLimit-Remaining", "4999"),
            ("X-RateLimit-Reset", "20"),
        ]),
        0,
    )
    .unwrap();
    assert_eq!(
        (x.limit, x.remaining, x.reset_ms),
        (Some(5000), Some(4999), Some(20_000))
    );
    let dashed = parse_at(
        &h(&[
            ("x-rate-limit-limit", "15"),
            ("x-rate-limit-remaining", "0"),
            ("x-rate-limit-reset", "3"),
        ]),
        0,
    )
    .unwrap();
    assert_eq!(
        (dashed.limit, dashed.remaining, dashed.reset_ms),
        (Some(15), Some(0), Some(3000))
    );
    let both = parse_at(
        &h(&[("X-RateLimit-Remaining", "9"), ("RateLimit-Remaining", "4")]),
        0,
    )
    .unwrap();
    assert_eq!(both.remaining, Some(4));
}

#[test]
fn the_unit_of_a_reset_is_recognised_not_guessed() {
    let now = 1_700_000_000_000_i64; // Unix ms
    // Seconds until the reset.
    let secs = parse_at(&h(&[("X-RateLimit-Reset", "45")]), now).unwrap();
    assert_eq!(secs.reset_ms, Some(45_000));
    // Unix seconds (from 10^9): the difference to now, 45 s ahead.
    let epoch = parse_at(&h(&[("X-RateLimit-Reset", "1700000045")]), now).unwrap();
    assert_eq!(epoch.reset_ms, Some(45_000));
    // Unix milliseconds (from 10^12).
    let epoch_ms = parse_at(&h(&[("X-RateLimit-Reset", "1700000045000")]), now).unwrap();
    assert_eq!(epoch_ms.reset_ms, Some(45_000));
    // An epoch in the past is "now", never negative.
    let past = parse_at(&h(&[("X-RateLimit-Reset", "1699999990")]), now).unwrap();
    assert_eq!(past.reset_ms, Some(0));
    // The server's own `Date` is the reference when there is one.
    let dated = parse_at(
        &h(&[
            ("Date", DATE),
            ("X-RateLimit-Reset", &format!("{}", DATE_MS / 1000 + 90)),
        ]),
        now,
    )
    .unwrap();
    assert_eq!(dated.reset_ms, Some(90_000));
    // An HTTP date as reset value.
    let http = parse_at(
        &h(&[
            ("Date", DATE),
            ("RateLimit-Reset", "Tue, 14 Nov 2023 22:14:20 GMT"),
        ]),
        now,
    )
    .unwrap();
    assert_eq!(http.reset_ms, Some(60_000));
}

#[test]
fn retry_after_is_seconds_or_an_http_date() {
    let secs = parse_at(&h(&[("Retry-After", "2")]), 0).unwrap();
    assert_eq!(secs.retry_after_ms, Some(2000));
    let date = parse_at(
        &h(&[
            ("Date", DATE),
            ("Retry-After", "Tue, 14 Nov 2023 22:15:20 GMT"),
        ]),
        0,
    )
    .unwrap();
    assert_eq!(date.retry_after_ms, Some(120_000));
    // Without a `Date` header the local clock is the reference.
    let local = parse_at(&h(&[("Retry-After", RFC_DATE)]), RFC_DATE_MS - 3000).unwrap();
    assert_eq!(local.retry_after_ms, Some(3000));
    // A date in the past is "right now".
    let past = parse_at(&h(&[("Retry-After", RFC_DATE)]), RFC_DATE_MS + 5000).unwrap();
    assert_eq!(past.retry_after_ms, Some(0));
}

#[test]
fn nothing_or_nonsense_is_nothing() {
    assert!(parse_at(&[], 0).is_none());
    assert!(parse_at(&h(&[("Content-Type", "text/plain")]), 0).is_none());
    assert!(
        parse_at(
            &h(&[("RateLimit-Limit", "many"), ("Retry-After", "soon")]),
            0
        )
        .is_none()
    );
    assert!(parse_at(&h(&[("Retry-After", "Mon, 99 Foo 2020 99:99:99 GMT")]), 0).is_none());
    // Header names are case-insensitive, values may carry blanks and a legacy suffix.
    let odd = parse_at(&h(&[("rAtElImIt-LiMiT", " 100, 100;w=60 ")]), 0).unwrap();
    assert_eq!(odd.limit, Some(100));
}

#[test]
fn the_parser_costs_microseconds() {
    let headers = h(&[
        ("Content-Type", "application/json"),
        ("Date", DATE),
        ("RateLimit-Limit", "60"),
        ("RateLimit-Remaining", "47"),
        ("RateLimit-Reset", "12"),
        ("RateLimit-Policy", "60;w=60"),
        ("Retry-After", "5"),
    ]);
    let started = std::time::Instant::now();
    for _ in 0..2000 {
        std::hint::black_box(ratelimit::parse(std::hint::black_box(&headers)));
    }
    let each = started.elapsed().as_secs_f64() / 2000.0 * 1e6;
    // 50 us in the release build (the budget); debug builds get a lot more room.
    let limit = if cfg!(debug_assertions) { 1000.0 } else { 50.0 };
    assert!(each < limit, "{each} us per response");
}

// ---------------------------------------------------------------------------------------------
// observe, through a real send

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

/// One scripted answer.
#[derive(Clone)]
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
}

fn answer(status: u16, headers: &[(&str, &str)]) -> Answer {
    Answer {
        status,
        headers: h(headers),
    }
}

/// Answers per host from a script (the last answer repeats; 200 when a host has none) and
/// records what was sent when (on the fake clock).
struct Wire {
    script: RefCell<Vec<(String, Vec<Answer>)>>,
    clock: Arc<AtomicU64>,
    sent: RefCell<Vec<(String, String, u64)>>, // method, url, clock
}

impl Wire {
    fn new(clock: &Arc<AtomicU64>) -> Self {
        Self {
            script: RefCell::default(),
            clock: clock.clone(),
            sent: RefCell::default(),
        }
    }

    fn host(self, host: &str, answers: Vec<Answer>) -> Self {
        self.script.borrow_mut().push((host.to_owned(), answers));
        self
    }

    fn sent(&self) -> Vec<(String, String, u64)> {
        self.sent.borrow().clone()
    }

    fn times(&self) -> Vec<u64> {
        self.sent.borrow().iter().map(|s| s.2).collect()
    }
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let url = request.url.clone();
        self.sent.borrow_mut().push((
            request.method.clone(),
            url.clone(),
            self.clock.load(Ordering::SeqCst),
        ));
        let host = host_of(&url).unwrap_or_default();
        let answer = {
            let mut script = self.script.borrow_mut();
            match script.iter_mut().find(|(h, _)| *h == host) {
                Some((_, answers)) if answers.len() > 1 => answers.remove(0),
                Some((_, answers)) if answers.len() == 1 => answers[0].clone(),
                _ => answer(200, &[]),
            }
        };
        let body: BodyStream = Box::pin(Once(Some(Bytes::from_static(b"{}"))));
        Ok(Response {
            head: ResponseHead {
                status: answer.status,
                version: HttpVersion::Http11,
                headers: answer.headers,
                url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

struct NoFiles;

impl FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("{path}: not found"))
    }
}

fn entry(path: &str, method: &str, url: &str) -> Item {
    Item::Request(RequestEntry {
        path: path.into(),
        spec: RequestSpec {
            name: path.into(),
            method: method.into(),
            url: url.into(),
            ..Default::default()
        },
    })
}

fn collection() -> Collection {
    Collection {
        name: "c".into(),
        items: vec![
            entry("a1.yml", "GET", "http://a.test/one"),
            entry("a2.yml", "GET", "http://a.test/two"),
            entry("a3.yml", "GET", "http://a.test/three"),
            entry("b1.yml", "GET", "http://b.test/one"),
            entry("post.yml", "POST", "http://a.test/create"),
            entry("var.yml", "GET", "http://{{host}}/v"),
        ],
        ..Default::default()
    }
}

fn input() -> PrepareInput<'static> {
    PrepareInput::default()
}

fn send_one(status: u16, headers: &[(&str, &str)]) -> SendReport {
    let clock = Arc::new(AtomicU64::new(0));
    let wire = Wire::new(&clock).host("a.test", vec![answer(status, headers)]);
    block_on(send_with(
        &collection(),
        "a1.yml",
        &input(),
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &Services::none(),
    ))
}

#[test]
fn a_send_reports_what_the_response_said() {
    let report = send_one(
        200,
        &[
            ("RateLimit-Limit", "60"),
            ("RateLimit-Remaining", "47"),
            ("RateLimit-Reset", "12"),
        ],
    );
    let limits = report.rate_limit.expect("limits");
    assert_eq!(
        (limits.limit, limits.remaining, limits.reset_ms),
        (Some(60), Some(47), Some(12_000))
    );
    // No headers, nothing in the report (and nothing in its JSON).
    let plain = send_one(200, &[]);
    assert!(plain.rate_limit.is_none());
    assert!(
        serde_json::to_value(&plain)
            .unwrap()
            .get("rate_limit")
            .is_none()
    );
}

#[test]
fn retry_after_counts_on_429_and_503_only() {
    for status in [429, 503] {
        let r = send_one(status, &[("Retry-After", "7")])
            .rate_limit
            .unwrap();
        assert_eq!(r.retry_after_ms, Some(7000), "status {status}");
    }
    // A redirect or a created resource may carry Retry-After for other reasons.
    assert!(send_one(201, &[("Retry-After", "7")]).rate_limit.is_none());
    let with_limits = send_one(301, &[("Retry-After", "7"), ("RateLimit-Remaining", "3")])
        .rate_limit
        .unwrap();
    assert_eq!(
        (with_limits.remaining, with_limits.retry_after_ms),
        (Some(3), None)
    );
}

#[test]
fn a_bare_429_still_says_limited_and_a_bare_503_does_not() {
    assert_eq!(send_one(429, &[]).rate_limit, Some(RateLimits::default()));
    assert!(send_one(503, &[]).rate_limit.is_none());
}

// ---------------------------------------------------------------------------------------------
// The tracker

fn fake_tracker() -> (RateTracker, Arc<AtomicU64>) {
    let clock = Arc::new(AtomicU64::new(1_000));
    let c = clock.clone();
    (
        RateTracker::with_clock(Arc::new(move || c.load(Ordering::SeqCst))),
        clock,
    )
}

#[test]
fn the_tracker_waits_only_for_hosts_that_ran_out() {
    let (tracker, clock) = fake_tracker();
    tracker.record(
        "a.test",
        RateLimits {
            remaining: Some(0),
            reset_ms: Some(12_000),
            ..Default::default()
        },
    );
    tracker.record(
        "b.test",
        RateLimits {
            remaining: Some(5),
            reset_ms: Some(12_000),
            ..Default::default()
        },
    );
    tracker.record(
        "c.test",
        RateLimits {
            retry_after_ms: Some(3000),
            ..Default::default()
        },
    );
    let a = tracker.wait("a.test").unwrap();
    assert_eq!(a.ms, 12_000);
    assert_eq!(a.reason, "rate limit: 0 left, resets in 12 s");
    assert!(tracker.wait("b.test").is_none(), "5 left: no wait");
    assert!(tracker.wait("unknown.test").is_none());
    assert_eq!(
        tracker.wait("c.test").unwrap().reason,
        "rate limit: the server asks to retry after 3 s"
    );

    // Time passes: the wait shrinks and then ends, without a new response.
    clock.fetch_add(10_500, Ordering::SeqCst);
    assert_eq!(tracker.wait("a.test").unwrap().ms, 1500);
    clock.fetch_add(1500, Ordering::SeqCst);
    assert!(tracker.wait("a.test").is_none());

    // The longer of Retry-After and the reset wins; a new response replaces the old one.
    tracker.record(
        "a.test",
        RateLimits {
            remaining: Some(0),
            reset_ms: Some(2000),
            retry_after_ms: Some(9000),
            ..Default::default()
        },
    );
    assert_eq!(tracker.wait("a.test").unwrap().ms, 9000);
    tracker.record(
        "a.test",
        RateLimits {
            remaining: Some(8),
            ..Default::default()
        },
    );
    assert!(tracker.wait("a.test").is_none());
    tracker.forget("c.test");
    assert!(tracker.get("c.test").is_none());
}

#[test]
fn clones_share_the_state_and_hosts_are_told_apart_by_port() {
    let (tracker, _) = fake_tracker();
    let clone = tracker.clone();
    assert!(clone.record_url(
        "http://Example.test:8080/x?y=1",
        RateLimits {
            retry_after_ms: Some(1000),
            ..Default::default()
        }
    ));
    assert!(tracker.wait("example.test:8080").is_some());
    assert!(tracker.wait("example.test").is_none());
    assert_eq!(
        host_of("https://API.example.test/v1"),
        Some("api.example.test".into())
    );
    assert!(!tracker.record_url("not a url", RateLimits::default()));
}

// ---------------------------------------------------------------------------------------------
// The runner

struct Ran {
    events: Vec<RunEvent>,
    summary: RunSummary,
    /// Everything the fake sleep was asked to wait.
    slept: u64,
}

impl Ran {
    fn waits(&self) -> Vec<(String, String)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                RunEvent::Waiting {
                    request_path,
                    reason,
                    ..
                } => Some((request_path.clone(), reason.clone())),
                _ => None,
            })
            .collect()
    }

    fn steps(&self) -> Vec<&rt_core::runner::RunStep> {
        self.events
            .iter()
            .filter_map(|e| match e {
                RunEvent::Step { step } => Some(&**step),
                _ => None,
            })
            .collect()
    }
}

fn throttle(max_wait_ms: u64, retries: u32) -> Throttle {
    Throttle {
        mode: ThrottleMode::Respect,
        max_wait_ms,
        retries,
    }
}

fn plan(requests: &[&str], throttle: Throttle) -> RunPlan {
    RunPlan {
        requests: requests.iter().map(|s| (*s).to_owned()).collect(),
        iterations: 1,
        throttle,
        ..Default::default()
    }
}

/// Runs on the fake clock: every sleep advances it.
fn run(
    c: &Collection,
    plan: &RunPlan,
    wire: &Wire,
    clock: &Arc<AtomicU64>,
    tracker: &RateTracker,
    cancelled: &dyn Fn() -> bool,
) -> Ran {
    let slept = Cell::new(0u64);
    let sleep = |ms: u64| -> LocalBoxFuture<'static, ()> {
        slept.set(slept.get() + ms);
        clock.fetch_add(ms, Ordering::SeqCst);
        Box::pin(async {})
    };
    let mut events = Vec::new();
    let summary = block_on(run_with_tracker(
        c,
        plan,
        &input(),
        &NoFiles,
        wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |e| events.push(e),
        cancelled,
        &sleep,
        tracker,
    ));
    Ran {
        events,
        summary,
        slept: slept.get(),
    }
}

fn never() -> bool {
    false
}

/// Distance of the second send from the first on the fake clock, minus `expected`.
fn deviation(wire: &Wire, expected: u64) -> i64 {
    let t = wire.times();
    (t[1] - t[0]) as i64 - expected as i64
}

#[test]
fn the_default_is_off_nothing_waits() {
    let clock = Arc::new(AtomicU64::new(0));
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(
            200,
            &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "30")],
        )],
    );
    let (tracker, _) = fake_tracker();
    let ran = run(
        &collection(),
        &plan(&["a1.yml", "a2.yml"], Throttle::default()),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert_eq!(ran.summary.requests, 2);
    assert_eq!(ran.slept, 0);
    assert!(ran.waits().is_empty());
    assert_eq!(wire.sent().len(), 2);
}

#[test]
fn it_waits_until_the_reset_when_nothing_is_left() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![
            answer(
                200,
                &[
                    ("RateLimit-Limit", "2"),
                    ("RateLimit-Remaining", "0"),
                    ("RateLimit-Reset", "12"),
                ],
            ),
            answer(
                200,
                &[("RateLimit-Remaining", "1"), ("RateLimit-Reset", "60")],
            ),
        ],
    );
    let ran = run(
        &collection(),
        &plan(&["a1.yml", "a2.yml"], throttle(DEFAULT_MAX_WAIT_MS, 2)),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert_eq!(ran.summary.requests, 2);
    assert_eq!(
        ran.waits(),
        vec![("a2.yml".into(), "rate limit: 0 left, resets in 12 s".into())]
    );
    assert!(deviation(&wire, 12_000).abs() <= 50, "{:?}", wire.times());
    // The waiting event comes before the request it holds back.
    let kinds: Vec<&str> = ran
        .events
        .iter()
        .map(|e| match e {
            RunEvent::Started { .. } => "started",
            RunEvent::Request { .. } => "request",
            RunEvent::Step { .. } => "step",
            RunEvent::Waiting { .. } => "waiting",
            RunEvent::Finished { .. } => "finished",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "started", "request", "step", "waiting", "request", "step", "finished"
        ]
    );
    let RunEvent::Waiting { until, .. } = &ran.events[3] else {
        unreachable!()
    };
    assert!((until - ratelimit::unix_now_ms() - 12_000).abs() < 5_000);
}

#[test]
fn it_waits_for_retry_after_in_seconds_and_as_a_date_and_for_an_epoch_reset() {
    type Case = (&'static str, Vec<(&'static str, String)>, u64);
    let cases: [Case; 3] = [
        ("seconds", vec![("Retry-After", "2".into())], 2000),
        (
            "http date",
            vec![
                ("Date", DATE.into()),
                ("Retry-After", "Tue, 14 Nov 2023 22:13:23 GMT".into()),
            ],
            3000,
        ),
        (
            "epoch",
            vec![
                ("Date", DATE.into()),
                ("X-RateLimit-Remaining", "0".into()),
                ("X-RateLimit-Reset", format!("{}", DATE_MS / 1000 + 5)),
            ],
            5000,
        ),
    ];
    for (name, headers, expected) in cases {
        let (tracker, clock) = fake_tracker();
        let borrowed: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
        // 429 for the retry-after cases, 200 for the epoch one.
        let status = if name == "epoch" { 200 } else { 429 };
        let wire =
            Wire::new(&clock).host("a.test", vec![answer(status, &borrowed), answer(200, &[])]);
        let p = plan(&["a1.yml", "a2.yml"], throttle(DEFAULT_MAX_WAIT_MS, 0));
        let ran = run(&collection(), &p, &wire, &clock, &tracker, &never);
        assert_eq!(ran.summary.requests, 2, "{name}");
        assert!(
            deviation(&wire, expected).abs() <= 50,
            "{name}: {:?}",
            wire.times()
        );
    }
}

#[test]
fn a_429_is_retried_for_reads_up_to_the_retry_limit() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![
            answer(429, &[("Retry-After", "2")]),
            answer(429, &[("Retry-After", "3")]),
            answer(200, &[]),
        ],
    );
    let ran = run(
        &collection(),
        &plan(&["a1.yml"], throttle(DEFAULT_MAX_WAIT_MS, 2)),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    let times = wire.times();
    assert_eq!(times.len(), 3);
    assert!(((times[1] - times[0]) as i64 - 2000).abs() <= 50);
    assert!(((times[2] - times[1]) as i64 - 3000).abs() <= 50);
    // One step, the final one, with a note about the retries.
    let steps = ran.steps();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].report.response.as_ref().unwrap().status, 200);
    assert!(
        steps[0]
            .report
            .notes
            .iter()
            .any(|n| n.source == "rate-limit" && n.text.contains("2 times"))
    );
    assert_eq!(ran.waits().len(), 2);

    // Out of retries: the 429 is the result.
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host("a.test", vec![answer(429, &[("Retry-After", "1")])]);
    let ran = run(
        &collection(),
        &plan(&["a1.yml"], throttle(DEFAULT_MAX_WAIT_MS, 1)),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert_eq!(wire.sent().len(), 2);
    assert_eq!(ran.steps()[0].report.response.as_ref().unwrap().status, 429);
}

#[test]
fn writes_are_never_repeated_automatically() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(429, &[("Retry-After", "1")]), answer(200, &[])],
    );
    let ran = run(
        &collection(),
        &plan(&["post.yml"], throttle(DEFAULT_MAX_WAIT_MS, 5)),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert_eq!(wire.sent().len(), 1, "a POST goes out once");
    assert_eq!(ran.steps()[0].report.response.as_ref().unwrap().status, 429);
    assert_eq!(ran.slept, 0);
    // Writes do wait before their first attempt when the host said so earlier.
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(
            200,
            &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "4")],
        )],
    );
    run(
        &collection(),
        &plan(&["a1.yml", "post.yml"], throttle(DEFAULT_MAX_WAIT_MS, 0)),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert!(deviation(&wire, 4000).abs() <= 50);
    assert_eq!(wire.sent()[1].0, "POST");
}

#[test]
fn a_wait_longer_than_allowed_ends_the_run_with_the_reason() {
    // Before the next request: nothing is sent, a failed step says why.
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(
            200,
            &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "120")],
        )],
    );
    let ran = run(
        &collection(),
        &plan(&["a1.yml", "a2.yml", "a3.yml"], throttle(60_000, 2)),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert_eq!(wire.sent().len(), 1);
    assert_eq!(
        ran.slept, 0,
        "it does not hang, it does not even start waiting"
    );
    assert!(ran.summary.stopped);
    assert_eq!(
        (ran.summary.requests, ran.summary.passed, ran.summary.failed),
        (2, 1, 1)
    );
    let last = ran.steps().pop().unwrap().clone();
    assert!(!last.passed);
    assert_eq!(last.request_path, "a2.yml");
    let message = format!("{:?}", last.report.error);
    assert!(
        message.contains("resets in 120 s")
            && message.contains("60 s")
            && message.contains("--max-wait"),
        "{message}"
    );
    assert!(last.report.request.is_none() && last.report.response.is_none());
    assert!(matches!(ran.events.last(), Some(RunEvent::Finished { .. })));

    // After a 429: the response is the step, failed, with a note, and the run stops.
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host("a.test", vec![answer(429, &[("Retry-After", "300")])]);
    let ran = run(
        &collection(),
        &plan(&["a1.yml", "a2.yml"], throttle(60_000, 2)),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert_eq!(wire.sent().len(), 1);
    assert!(ran.summary.stopped);
    let step = &ran.steps()[0].clone();
    assert!(!step.passed);
    assert!(
        step.report
            .notes
            .iter()
            .any(|n| n.text.contains("--max-wait") && n.text.contains("the run stops"))
    );
}

#[test]
fn the_wait_is_capped_at_ten_minutes_whatever_the_plan_says() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(
            200,
            &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "601")],
        )],
    );
    let p = plan(&["a1.yml", "a2.yml"], throttle(u64::MAX, 2));
    let ran = run(&collection(), &p, &wire, &clock, &tracker, &never);
    assert!(ran.summary.stopped);
    assert_eq!(wire.sent().len(), 1);
    assert_eq!(MAX_WAIT_HARD_MS, 600_000);
    // Just inside the cap it waits.
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![
            answer(
                200,
                &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "600")],
            ),
            answer(200, &[]),
        ],
    );
    let ran = run(&collection(), &p, &wire, &clock, &tracker, &never);
    assert!(!ran.summary.stopped);
    assert!(deviation(&wire, 600_000).abs() <= 50);
}

#[test]
fn cancelling_while_waiting_is_immediate() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(
            200,
            &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "30")],
        )],
    );
    let polls = Cell::new(0u32);
    let cancelled = || {
        // The user cancels during the third slice of the wait.
        polls.set(polls.get() + 1);
        polls.get() > 4
    };
    let ran = run(
        &collection(),
        &plan(&["a1.yml", "a2.yml"], throttle(DEFAULT_MAX_WAIT_MS, 2)),
        &wire,
        &clock,
        &tracker,
        &cancelled,
    );
    assert!(ran.summary.stopped);
    assert_eq!(wire.sent().len(), 1, "the held-back request never goes out");
    assert!(
        ran.slept <= 200,
        "{} ms slept before the cancel took effect",
        ran.slept
    );
    assert!(matches!(ran.events.last(), Some(RunEvent::Finished { .. })));
}

#[test]
fn cancelling_while_waiting_to_retry_stops_with_the_429_as_the_last_step() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host("a.test", vec![answer(429, &[("Retry-After", "30")])]);
    let polls = Cell::new(0u32);
    let cancelled = || {
        polls.set(polls.get() + 1);
        polls.get() > 2
    };
    let ran = run(
        &collection(),
        &plan(&["a1.yml", "a2.yml"], throttle(DEFAULT_MAX_WAIT_MS, 2)),
        &wire,
        &clock,
        &tracker,
        &never_after(&cancelled),
    );
    assert!(ran.summary.stopped);
    assert_eq!(wire.sent().len(), 1);
    assert_eq!(ran.steps().len(), 1);
}

fn never_after<'a>(f: &'a dyn Fn() -> bool) -> impl Fn() -> bool + 'a {
    move || f()
}

#[test]
fn the_tracker_is_per_host_and_the_host_is_resolved_with_variables() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock)
        .host(
            "a.test",
            vec![answer(
                200,
                &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "9")],
            )],
        )
        .host("b.test", vec![answer(200, &[])]);
    // b.test has no limit: no wait between a1 and b1; the next a-request waits.
    let p = plan(
        &["a1.yml", "b1.yml", "a2.yml"],
        throttle(DEFAULT_MAX_WAIT_MS, 0),
    );
    let ran = run(&collection(), &p, &wire, &clock, &tracker, &never);
    assert_eq!(ran.waits().len(), 1);
    assert_eq!(ran.waits()[0].0, "a2.yml");
    let t = wire.times();
    assert_eq!(t[1] - t[0], 0, "another host is not held back");
    assert!(((t[2] - t[0]) as i64 - 9000).abs() <= 50, "{t:?}");
    assert!(tracker.get("a.test").is_some() && tracker.get("b.test").is_none());

    // A host that comes from a variable is resolved before sending.
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(
            200,
            &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "5")],
        )],
    );
    let mut input = input();
    input.runtime.push(("host".into(), "a.test".into()));
    let slept = Cell::new(0u64);
    let sleep = |ms: u64| -> LocalBoxFuture<'static, ()> {
        slept.set(slept.get() + ms);
        clock.fetch_add(ms, Ordering::SeqCst);
        Box::pin(async {})
    };
    block_on(run_with_tracker(
        &collection(),
        &plan(&["a1.yml", "var.yml"], throttle(DEFAULT_MAX_WAIT_MS, 0)),
        &input,
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |_| {},
        &never,
        &sleep,
        &tracker,
    ));
    assert!(slept.get() >= 5000);
}

#[test]
fn the_delay_and_the_rate_limit_do_not_add_up() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(
            200,
            &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "10")],
        )],
    );
    let mut p = plan(&["a1.yml", "a2.yml"], throttle(DEFAULT_MAX_WAIT_MS, 0));
    p.delay_ms = 4000;
    let ran = run(&collection(), &p, &wire, &clock, &tracker, &never);
    assert_eq!(ran.summary.requests, 2);
    // The longer one counts: 10 s since the response, not 4 + 10.
    assert!(deviation(&wire, 10_000).abs() <= 50, "{:?}", wire.times());
    // A delay longer than the reset needs no extra wait.
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(
            200,
            &[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "3")],
        )],
    );
    p.delay_ms = 4000;
    let ran = run(&collection(), &p, &wire, &clock, &tracker, &never);
    assert!(ran.waits().is_empty());
    assert!(deviation(&wire, 4000).abs() <= 50);
}

#[test]
fn a_run_without_responses_that_state_limits_never_waits() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock);
    let ran = run(
        &collection(),
        &plan(
            &["a1.yml", "a2.yml", "a3.yml"],
            throttle(DEFAULT_MAX_WAIT_MS, 2),
        ),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert_eq!(ran.slept, 0);
    assert_eq!(ran.summary.passed, 3);
}

#[test]
fn the_throttle_in_a_plan_round_trips_and_the_default_stays_out_of_the_json() {
    let p = plan(&["a1.yml"], Throttle::default());
    assert!(serde_json::to_value(&p).unwrap().get("throttle").is_none());
    let on = plan(&["a1.yml"], throttle(5000, 1));
    let json = serde_json::to_value(&on).unwrap();
    assert_eq!(json["throttle"]["mode"], "respect");
    assert_eq!(serde_json::from_value::<RunPlan>(json).unwrap(), on);
}

// ---------------------------------------------------------------------------------------------
// Budget `ratelimit_wait`

/// With the fake clock the wait is exact; with `RT_PERF_OUT` the real sleep is measured too:
/// a response says `Retry-After: 1`, the next send must follow 1 s later (median error of 9,
/// at most 50 ms; `node scripts/perf/run.mjs --only ratelimit`).
#[test]
fn ratelimit_wait_is_measured() {
    let (tracker, clock) = fake_tracker();
    let wire = Wire::new(&clock).host(
        "a.test",
        vec![answer(429, &[("Retry-After", "2")]), answer(200, &[])],
    );
    run(
        &collection(),
        &plan(&["a1.yml"], throttle(DEFAULT_MAX_WAIT_MS, 1)),
        &wire,
        &clock,
        &tracker,
        &never,
    );
    assert_eq!(deviation(&wire, 2000), 0);

    let Some(dir) = std::env::var_os("RT_PERF_OUT") else {
        return;
    };
    let mut errors = Vec::new();
    for _ in 0..9 {
        let start = std::time::Instant::now();
        let stamps = RefCell::new(Vec::new());
        struct Timed<'a>(&'a RefCell<Vec<f64>>, std::time::Instant, Wire);
        impl Transport for Timed<'_> {
            async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
                self.0
                    .borrow_mut()
                    .push(self.1.elapsed().as_secs_f64() * 1000.0);
                self.2.send(request).await
            }
        }
        let c = Arc::new(AtomicU64::new(0));
        let wire = Timed(
            &stamps,
            start,
            Wire::new(&c).host(
                "a.test",
                vec![answer(429, &[("Retry-After", "1")]), answer(200, &[])],
            ),
        );
        let p = plan(&["a1.yml"], throttle(DEFAULT_MAX_WAIT_MS, 1));
        let sleep = rt_core::runner::platform_sleep;
        block_on(run_with_tracker(
            &collection(),
            &p,
            &input(),
            &NoFiles,
            &wire,
            &SendOptions::default(),
            &Services::none(),
            &mut |_| {},
            &never,
            &sleep,
            &RateTracker::new(),
        ));
        let t = stamps.borrow();
        errors.push(((t[1] - t[0]) - 1000.0).abs());
    }
    errors.sort_by(f64::total_cmp);
    let median = errors[errors.len() / 2];
    println!("ratelimit_wait: median error {median:.2} ms");
    assert!(median <= 50.0, "{errors:?}");
    let json = serde_json::json!({ "ratelimit_wait": median, "errors_ms": errors });
    std::fs::write(
        std::path::Path::new(&dir).join("ratelimit.json"),
        serde_json::to_string_pretty(&json).unwrap(),
    )
    .unwrap();
}
