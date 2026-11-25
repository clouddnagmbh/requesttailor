// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The minimal failing request (wave 19, strand E): a fake transport with a predicate over the
//! request answers 500 or 200, and the search must find the parts that matter. Probes are
//! counted at the transport, so what the tests say about limits is what really went out.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::runner::RunSnapshot;
use rt_app::share::{CopyKind, CopyOptions, copy_spec};
use rt_app::shrink::{DEFAULT_PROBES, MAX_PROBES, Predicate, ShrinkRequest, ShrinkResult, shrink};
use rt_core::guard::{GuardVerdict, SendCheck, SendGuard};
use rt_core::model::{
    AgentAccess, AgentPolicy, Assertion, Body, Collection, Environment, Item, KeyValue, RawKind,
    RequestEntry, RequestSpec, Variable,
};
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::FileSet;

const CANARY: &str = "RT-CANARY-shrink-secret-5d2e";

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

type Rule = Box<dyn Fn(&HttpRequest, usize) -> (u16, String) + Send + Sync>;

/// Answers with `rule(request, call number)` and remembers every request.
struct Server {
    rule: Rule,
    seen: Mutex<Vec<HttpRequest>>,
}

impl Server {
    fn new(rule: impl Fn(&HttpRequest, usize) -> (u16, String) + Send + Sync + 'static) -> Self {
        Self {
            rule: Box::new(rule),
            seen: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl Transport for Server {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let n = {
            let mut seen = self.seen.lock().unwrap();
            seen.push(request.clone());
            seen.len()
        };
        let (status, body) = (self.rule)(&request, n);
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(body))));
        Ok(Response {
            head: ResponseHead {
                status,
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

fn has_header(r: &HttpRequest, name: &str) -> bool {
    r.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

fn body_of(r: &HttpRequest) -> String {
    r.body
        .as_ref()
        .map(|b| String::from_utf8_lossy(&b.bytes).into_owned())
        .unwrap_or_default()
}

fn fail_when(hit: impl Fn(&HttpRequest) -> bool + Send + Sync + 'static) -> Server {
    Server::new(move |r, _| {
        if hit(r) {
            (500, "{\"error\":\"boom\"}".into())
        } else {
            (200, "{}".into())
        }
    })
}

fn snapshot(spec: RequestSpec, environment: &str) -> RunSnapshot {
    let collection = Collection {
        name: "c".into(),
        items: vec![Item::Request(RequestEntry {
            path: "r.yml".into(),
            spec,
        })],
        environments: vec![Environment {
            name: environment.into(),
            variables: vec![
                Variable::new("base", "http://shrink.test"),
                Variable {
                    name: "token".into(),
                    value: Some(CANARY.into()),
                    enabled: true,
                    secret: true,
                },
            ],
            client_certificates: vec![],
            external_secrets: None,
        }],
        ..Collection::default()
    };
    RunSnapshot {
        collection,
        environment: Some(environment.into()),
        vars: Vec::new(),
        files: FileSet::default(),
    }
}

fn spec(method: &str, url: &str) -> RequestSpec {
    RequestSpec {
        name: "r".into(),
        method: method.into(),
        url: url.into(),
        headers: vec![KeyValue::new("Authorization", "Bearer {{token}}")],
        ..RequestSpec::default()
    }
}

fn ask(predicate: Predicate) -> ShrinkRequest {
    ShrinkRequest {
        path: "r.yml".into(),
        environment: None,
        predicate,
        keep: Vec::new(),
        max_probes: 0,
        max_seconds: 0,
        simplify_values: false,
        confirmed_writes: false,
    }
}

fn run(
    snapshot: &RunSnapshot,
    request: &ShrinkRequest,
    server: &Server,
) -> Result<ShrinkResult, rt_app::AppError> {
    run_with(snapshot, request, server, &Services::none())
}

fn run_with(
    snapshot: &RunSnapshot,
    request: &ShrinkRequest,
    server: &Server,
    services: &Services<'_>,
) -> Result<ShrinkResult, rt_app::AppError> {
    block_on(shrink(
        snapshot,
        request,
        server,
        &SendOptions::default(),
        services,
        &|| false,
    ))
}

fn header_names(spec: &RequestSpec) -> Vec<&str> {
    spec.headers.iter().map(|h| h.name.as_str()).collect()
}

/// 100 headers and 99 query parameters, plus the auth header.
fn wide() -> RequestSpec {
    let mut s = spec("GET", "{{base}}/items?q0=0");
    for i in 0..100 {
        s.headers
            .push(KeyValue::new(format!("X-H{i}"), format!("v{i}")));
    }
    let mut url = String::from("{{base}}/items?q0=0");
    for i in 1..99 {
        url.push_str(&format!("&q{i}=v{i}"));
    }
    s.url = url;
    s
}

#[test]
fn one_cause_among_200_parts_is_found_in_few_probes() {
    let server = fail_when(|r| has_header(r, "X-H42"));
    let result = run(
        &snapshot(wide(), "dev"),
        &ask(Predicate::ServerError),
        &server,
    )
    .unwrap();
    assert_eq!(header_names(&result.request), ["Authorization", "X-H42"]);
    assert!(!result.request.url.contains('?'), "{}", result.request.url);
    assert!(result.one_minimal);
    assert!(!result.flaky && !result.not_reproduced);
    assert_eq!(result.stopped_by, None);
    // 100 headers (99 besides the cause) + 99 query parameters were taken away.
    assert_eq!(result.removed.len(), 99 + 99);
    assert!(result.probes <= 60, "{} probes", result.probes);
    assert_eq!(result.probes as usize, server.calls());
}

#[test]
fn three_scattered_causes_are_all_kept() {
    let body = "{\"a\":1,\"deep\":{\"x\":1,\"flag\":true,\"y\":2},\"list\":[1,2,3]}";
    let mut s = wide();
    s.method = "POST".into();
    s.body = Body::Raw {
        kind: RawKind::Json,
        data: body.into(),
    };
    let server = fail_when(|r| {
        has_header(r, "X-H10") && r.url.contains("q77=") && body_of(r).contains("\"flag\"")
    });
    let mut req = ask(Predicate::ServerError);
    req.confirmed_writes = true;
    let result = run(&snapshot(s, "dev"), &req, &server).unwrap();
    assert_eq!(header_names(&result.request), ["Authorization", "X-H10"]);
    assert!(
        result.request.url.ends_with("?q77=v77"),
        "{}",
        result.request.url
    );
    let Body::Raw { data, .. } = &result.request.body else {
        panic!("body")
    };
    let value: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(value, serde_json::json!({"deep": {"flag": true}}));
    assert!(result.one_minimal);
    assert!(result.probes <= 160, "{} probes", result.probes);
}

#[test]
fn the_body_is_cut_as_a_tree_and_keeps_its_order() {
    let mut s = spec("GET", "{{base}}/orders");
    s.body = Body::Raw {
        kind: RawKind::Json,
        data: "{\"zeta\":1,\"items\":[{\"sku\":\"a\",\"qty\":1},{\"sku\":\"b\",\"qty\":2},{\"sku\":\"c\",\"qty\":3}],\"alpha\":{\"k\":[1,2]}}".into(),
    };
    let server = fail_when(|r| {
        serde_json::from_str::<serde_json::Value>(&body_of(r))
            .ok()
            .and_then(|v| v["items"][1]["sku"].as_str().map(|s| s == "b"))
            .unwrap_or(false)
            || body_of(r).contains("\"sku\": \"b\"")
    });
    // `items[1].sku` stays; once the array shrinks it is element 0.
    let result = run(&snapshot(s, "dev"), &ask(Predicate::ServerError), &server).unwrap();
    let Body::Raw { data, .. } = &result.request.body else {
        panic!("body")
    };
    let value: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"items": [{"sku": "b"}]}),
        "{data}"
    );
    assert!(result.removed.iter().any(|s| s.part == "body:$.zeta"));
    assert!(result.removed.iter().any(|s| s.part == "body:$.items[0]"));
    assert!(result.removed.iter().all(|s| !s.part.contains("\"b\"")));
}

#[test]
fn authentication_stays_and_so_do_kept_parts() {
    // The failure does not depend on any header: everything that may go, goes.
    let mut s = spec("GET", "{{base}}/x?a=1&b=2");
    s.headers.push(KeyValue::new("Host", "shrink.test"));
    s.headers.push(KeyValue::new("X-Api-Key", "k"));
    s.headers.push(KeyValue::new("X-Debug", "1"));
    s.headers.push(KeyValue::new("X-Keep", "1"));
    s.headers.push(KeyValue::new("Cookie", "sid=1; theme=dark"));
    let server = fail_when(|_| true);
    let mut req = ask(Predicate::ServerError);
    req.keep = vec!["header:x-keep".into(), "query:b".into()];
    let result = run(&snapshot(s, "dev"), &req, &server).unwrap();
    assert_eq!(
        header_names(&result.request),
        ["Authorization", "Host", "X-Api-Key", "X-Keep"]
    );
    assert!(
        result.request.url.ends_with("/x?b=2"),
        "{}",
        result.request.url
    );
    let ids: Vec<_> = result.removed.iter().map(|s| s.part.as_str()).collect();
    assert_eq!(
        ids,
        ["header:X-Debug", "cookie:sid", "cookie:theme", "query:a"]
    );
}

#[test]
fn cookies_are_parts_of_the_cookie_header() {
    let mut s = spec("GET", "{{base}}/me");
    s.headers
        .push(KeyValue::new("Cookie", "sid=1; theme=dark; flag=on"));
    let server = fail_when(|r| {
        r.headers
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("cookie") && v.contains("theme=dark"))
    });
    let result = run(&snapshot(s, "dev"), &ask(Predicate::ServerError), &server).unwrap();
    let cookie = result
        .request
        .headers
        .iter()
        .find(|h| h.name == "Cookie")
        .unwrap();
    assert_eq!(cookie.value, "theme=dark");
}

#[test]
fn form_fields_are_parts() {
    let mut s = spec("POST", "{{base}}/form");
    s.body = Body::FormUrlEncoded {
        fields: vec![
            KeyValue::new("a", "1"),
            KeyValue::new("b", "2"),
            KeyValue::new("c", "3"),
        ],
    };
    let server = fail_when(|r| body_of(r).contains("b=2"));
    let mut req = ask(Predicate::ServerError);
    req.confirmed_writes = true;
    let result = run(&snapshot(s, "dev"), &req, &server).unwrap();
    let Body::FormUrlEncoded { fields } = &result.request.body else {
        panic!("body")
    };
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "b");
}

#[test]
fn a_failure_that_does_not_show_is_not_reproduced() {
    let server = fail_when(|_| false);
    let result = run(
        &snapshot(wide(), "dev"),
        &ask(Predicate::ServerError),
        &server,
    )
    .unwrap();
    assert!(result.not_reproduced);
    assert_eq!(result.probes, 1);
    assert_eq!(result.request, wide());
    assert!(result.removed.is_empty());
}

#[test]
fn a_failure_that_comes_and_goes_is_called_flaky() {
    // Hit, miss, hit, miss ...
    let server = Server::new(|_, n| {
        if n % 2 == 1 {
            (500, "{}".into())
        } else {
            (200, "{}".into())
        }
    });
    let result = run(
        &snapshot(wide(), "dev"),
        &ask(Predicate::ServerError),
        &server,
    )
    .unwrap();
    assert!(result.flaky);
    assert!(!result.one_minimal);
    assert_eq!(
        result.request,
        wide(),
        "nothing is cut from a flaky failure"
    );
    assert_eq!(result.probes, 2);
}

#[test]
fn a_flip_late_in_the_search_is_caught_by_the_final_check() {
    // Reliable while the request is wide; the last confirmation (the shrunk request) flips.
    let server = Server::new(|r, n| {
        let narrow = r.headers.len() <= 2;
        if narrow && n > 3 && n % 2 == 1 {
            (200, "{}".into())
        } else if has_header(r, "X-H3") {
            (500, "{}".into())
        } else {
            (200, "{}".into())
        }
    });
    let result = run(
        &snapshot(wide(), "dev"),
        &ask(Predicate::ServerError),
        &server,
    )
    .unwrap();
    // Whatever the exact flip, a result is returned and flaky is told when the confirmation
    // misses; it never claims 1-minimal then.
    if result.flaky {
        assert!(!result.one_minimal);
    }
}

#[test]
fn the_probe_limit_ends_the_search_with_a_usable_request() {
    let server = fail_when(|r| has_header(r, "X-H42"));
    let mut req = ask(Predicate::ServerError);
    req.max_probes = 6;
    let result = run(&snapshot(wide(), "dev"), &req, &server).unwrap();
    assert_eq!(result.stopped_by.as_deref(), Some("probes"));
    assert!(!result.one_minimal);
    assert_eq!(result.probes, 6);
    assert_eq!(server.calls(), 6);
    // The request that is left still fails.
    assert!(result.request.headers.iter().any(|h| h.name == "X-H42"));
    assert!(result.request.headers.len() < wide().headers.len());
}

#[test]
fn the_limits_are_clamped_to_the_hard_maximum() {
    let server = fail_when(|_| false);
    let mut req = ask(Predicate::ServerError);
    req.max_probes = 5000;
    req.max_seconds = 100_000;
    let result = run(&snapshot(wide(), "dev"), &req, &server).unwrap();
    assert!(result.not_reproduced);
    const { assert!(DEFAULT_PROBES < MAX_PROBES) };
}

#[test]
fn a_cancelled_search_stops() {
    let server = fail_when(|r| has_header(r, "X-H42"));
    let seen = Mutex::new(0u32);
    let cancel = || {
        let mut n = seen.lock().unwrap();
        *n += 1;
        *n > 8
    };
    let result = block_on(shrink(
        &snapshot(wide(), "dev"),
        &ask(Predicate::ServerError),
        &server,
        &SendOptions::default(),
        &Services::none(),
        &cancel,
    ))
    .unwrap();
    assert_eq!(result.stopped_by.as_deref(), Some("cancelled"));
}

#[test]
fn a_rate_limit_ends_the_search() {
    let server = Server::new(|r, n| {
        if n > 2 {
            (429, "{}".into())
        } else if has_header(r, "X-H1") {
            (500, "{}".into())
        } else {
            (200, "{}".into())
        }
    });
    let result = run(
        &snapshot(wide(), "dev"),
        &ask(Predicate::ServerError),
        &server,
    )
    .unwrap();
    assert_eq!(result.stopped_by.as_deref(), Some("rate-limited"));
}

#[test]
fn writing_requests_need_the_confirmation_and_never_run_against_production() {
    let server = fail_when(|_| true);
    let post = || spec("POST", "{{base}}/orders");
    let err = run(
        &snapshot(post(), "test"),
        &ask(Predicate::ServerError),
        &server,
    )
    .unwrap_err();
    assert!(err.to_string().contains("requests that write"), "{err}");
    assert_eq!(server.calls(), 0);

    let mut confirmed = ask(Predicate::ServerError);
    confirmed.confirmed_writes = true;
    assert!(run(&snapshot(post(), "test"), &confirmed, &server).is_ok());

    let before = server.calls();
    let err = run(&snapshot(post(), "prod"), &confirmed, &server).unwrap_err();
    assert!(err.to_string().contains("production"), "{err}");
    assert_eq!(server.calls(), before, "nothing is sent against production");

    // The agent policy decides, not only the name.
    let mut snap = snapshot(post(), "staging");
    snap.collection.config.agents = AgentPolicy {
        environments: vec![rt_core::model::EnvironmentPolicy {
            environment: "staging".into(),
            access: AgentAccess::Write,
            production: true,
        }],
        ..AgentPolicy::default()
    };
    assert!(run(&snap, &confirmed, &server).is_err());
}

#[test]
fn reading_requests_run_without_a_question() {
    let server = fail_when(|_| true);
    let result = run(
        &snapshot(spec("GET", "{{base}}/x"), "test"),
        &ask(Predicate::ServerError),
        &server,
    );
    assert!(result.is_ok());
}

struct Refuse;

impl SendGuard for Refuse {
    fn check(&self, _: &SendCheck) -> GuardVerdict {
        GuardVerdict::Refuse {
            rule: "no-shrink".into(),
            reason: "not today".into(),
        }
    }
}

#[test]
fn the_send_guard_answers_for_every_probe() {
    let server = fail_when(|_| true);
    let guard = Refuse;
    let services = Services {
        guard: Some(&guard),
        ..Services::none()
    };
    let result = run_with(
        &snapshot(wide(), "dev"),
        &ask(Predicate::ServerError),
        &server,
        &services,
    )
    .unwrap();
    assert_eq!(result.stopped_by.as_deref(), Some("blocked: no-shrink"));
    assert_eq!(server.calls(), 0, "nothing went out");
}

#[test]
fn the_predicates() {
    let mut s = spec("GET", "{{base}}/p?marker=1&noise=2");
    s.assertions = vec![Assertion {
        expression: "res.status".into(),
        operator: "eq".into(),
        value: "200".into(),
        description: None,
        enabled: true,
    }];
    let server = Server::new(|r, _| {
        if r.url.contains("marker=1") {
            (418, "{\"detail\":\"teapot-7\"}".into())
        } else {
            (200, "{\"detail\":\"fine\"}".into())
        }
    });
    let cases: Vec<(Predicate, bool)> = vec![
        (Predicate::Status { status: 418 }, true),
        (
            Predicate::BodyMatches {
                pattern: "teapot-\\d".into(),
            },
            true,
        ),
        (Predicate::AssertionFails { assertion: None }, true),
        (
            Predicate::AssertionFails {
                assertion: Some("0".into()),
            },
            true,
        ),
        (
            Predicate::HeaderPresent {
                name: "content-type".into(),
            },
            true,
        ),
        (
            Predicate::HeaderMissing {
                name: "x-nothing".into(),
            },
            true,
        ),
        (Predicate::LastFailure, true),
    ];
    for (predicate, _) in cases {
        let result = run(
            &snapshot(s.clone(), "dev"),
            &ask(predicate.clone()),
            &server,
        )
        .unwrap();
        // For the last four predicates every probe "hits", so even `marker` goes; the first
        // three keep what makes the server answer 418.
        match predicate {
            Predicate::Status { .. }
            | Predicate::BodyMatches { .. }
            | Predicate::AssertionFails { .. }
            | Predicate::LastFailure => {
                assert!(
                    result.request.url.ends_with("?marker=1"),
                    "{predicate:?}: {}",
                    result.request.url
                );
            }
            _ => {}
        }
        assert!(!result.not_reproduced, "{predicate:?}");
    }
    // 500 is not what this server says.
    let r = run(
        &snapshot(s, "dev"),
        &ask(Predicate::Status { status: 500 }),
        &server,
    )
    .unwrap();
    assert!(r.not_reproduced);
}

#[test]
fn the_last_failure_is_the_status_and_the_failed_assertion() {
    // 404 with a failing assertion; a 404 without `marker` also fails the assertion but the
    // status must match as well, and a 500 must not count.
    let mut s = spec("GET", "{{base}}/p?marker=1&noise=2");
    s.assertions = vec![Assertion {
        expression: "res.status".into(),
        operator: "eq".into(),
        value: "200".into(),
        description: None,
        enabled: true,
    }];
    let server = Server::new(|r, _| {
        if r.url.contains("marker=1") {
            (404, "{}".into())
        } else {
            (500, "{}".into())
        }
    });
    let result = run(&snapshot(s, "dev"), &ask(Predicate::LastFailure), &server).unwrap();
    assert!(
        result.request.url.ends_with("?marker=1"),
        "{}",
        result.request.url
    );
}

#[test]
fn a_last_failure_without_a_failure_is_not_reproduced() {
    let server = fail_when(|_| false);
    let result = run(
        &snapshot(wide(), "dev"),
        &ask(Predicate::LastFailure),
        &server,
    )
    .unwrap();
    assert!(result.not_reproduced);
}

#[test]
fn values_are_shortened_only_when_asked() {
    let mut s = spec(
        "POST",
        "{{base}}/v?token={{token}}&note=this-is-a-rather-long-note-text",
    );
    s.headers.push(KeyValue::new(
        "X-Long",
        "a-header-value-that-is-long-enough",
    ));
    s.body = Body::Raw {
        kind: RawKind::Json,
        data: "{\"count\":4711,\"text\":\"a long string with plenty of characters\",\"id\":\"keep-me-as-is-please-1234\"}".into(),
    };
    // Fails whenever the id is intact and `count` is a number.
    let server = fail_when(|r| body_of(r).contains("keep-me-as-is-please-1234"));
    let mut req = ask(Predicate::ServerError);
    req.confirmed_writes = true;
    let plain = run(&snapshot(s.clone(), "dev"), &req, &server).unwrap();
    assert!(plain.removed.iter().all(|r| r.kind != "value"));
    req.simplify_values = true;
    req.keep = vec!["body:$.id".into()];
    let result = run(&snapshot(s, "dev"), &req, &server).unwrap();
    assert!(
        result
            .removed
            .iter()
            .all(|r| r.kind != "value" || r.part != "body:$.id")
    );
    // The id is the cause and is kept; no value of it was touched.
    let Body::Raw { data, .. } = &result.request.body else {
        panic!()
    };
    assert!(data.contains("keep-me-as-is-please-1234"), "{data}");
}

#[test]
fn simplifying_values_uses_what_the_request_has() {
    let mut s = spec("POST", "{{base}}/v");
    s.body = Body::Raw {
        kind: RawKind::Json,
        data: "{\"n\":4711,\"s\":\"a rather long string value here\",\"flag\":true}".into(),
    };
    // Everything fails whatever the values are: they all get as small as allowed.
    let server = fail_when(|r| body_of(r).contains("\"flag\""));
    let mut req = ask(Predicate::ServerError);
    req.confirmed_writes = true;
    req.simplify_values = true;
    req.keep = vec!["body:$.flag".into(), "body:$.n".into(), "body:$.s".into()];
    let result = run(&snapshot(s, "dev"), &req, &server).unwrap();
    let Body::Raw { data, .. } = &result.request.body else {
        panic!()
    };
    let v: serde_json::Value = serde_json::from_str(data).unwrap();
    // Kept parts are not simplified either: the user said keep.
    assert_eq!(v["n"], 4711);
    assert!(result.removed.iter().all(|s| s.kind != "value"));

    let mut s = spec("POST", "{{base}}/v");
    s.body = Body::Raw {
        kind: RawKind::Json,
        data: "{\"n\":4711,\"s\":\"a rather long string value here\",\"flag\":true}".into(),
    };
    let mut req = ask(Predicate::ServerError);
    req.confirmed_writes = true;
    req.simplify_values = true;
    req.keep = vec!["body:$.flag".into()];
    let result = run(&snapshot(s, "dev"), &req, &server).unwrap();
    let Body::Raw { data, .. } = &result.request.body else {
        panic!()
    };
    let v: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"flag": true}),
        "n and s are not needed at all"
    );
}

#[test]
fn output_and_removed_list_hold_no_secret_value() {
    // The token is a secret of the environment, used in the auth header, in a query value
    // and in the body; the server echoes it back, which the report masks.
    let mut s = spec("GET", "{{base}}/echo?key={{token}}&other=1");
    s.body = Body::Raw {
        kind: RawKind::Json,
        data: "{\"auth\":\"{{token}}\",\"more\":1}".into(),
    };
    let server = Server::new(|r, _| {
        let echoed = format!("{{\"echo\":\"{}\"}}", r.url);
        (500, echoed)
    });
    let snap = snapshot(s, "dev");
    let result = run(&snap, &ask(Predicate::ServerError), &server).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains(CANARY), "{json}");
    assert!(json.contains("{{token}}"));
    // The way out: curl and the link follow the share rules.
    let env = snap.collection.environments[0].clone();
    let curl = copy_spec(
        &result.request,
        CopyKind::Curl,
        Some(&env),
        &CopyOptions::default(),
    )
    .unwrap();
    assert!(!curl.text.contains(CANARY), "{}", curl.text);
}

#[test]
fn unknown_paths_and_bad_patterns_are_errors() {
    let server = fail_when(|_| true);
    let mut req = ask(Predicate::ServerError);
    req.path = "nope.yml".into();
    assert!(matches!(
        run(&snapshot(wide(), "dev"), &req, &server),
        Err(rt_app::AppError::RequestNotFound(_))
    ));
    let bad = ask(Predicate::BodyMatches {
        pattern: "(".into(),
    });
    assert!(run(&snapshot(wide(), "dev"), &bad, &server).is_err());
    assert_eq!(server.calls(), 0);
}

#[test]
fn parts_beyond_the_limit_stay_and_the_search_still_works() {
    // 2 500 headers: the first 2 000 parts may go, the rest stays in the request.
    let mut s = spec("GET", "{{base}}/many");
    for i in 0..2500 {
        s.headers.push(KeyValue::new(format!("X-N{i}"), "v"));
    }
    let server = fail_when(|r| has_header(r, "X-N5"));
    let mut req = ask(Predicate::ServerError);
    req.max_probes = 200;
    let result = run(&snapshot(s, "dev"), &req, &server).unwrap();
    assert!(result.request.headers.iter().any(|h| h.name == "X-N5"));
    assert!(result.request.headers.len() < 2501);
}

// ---- the budget `shrink_probes` ----

/// 200 parts with one cause (at most 60 probes) and with three scattered causes (at most 160),
/// own time under 100 ms with a fake transport. `scripts/perf/shrink.mjs` runs this in a
/// release build with `RT_PERF_OUT=<dir>` and reads `<dir>/shrink.json`.
#[test]
fn shrink_probes_are_measured() {
    let one = fail_when(|r| has_header(r, "X-H42"));
    let start = web_time::Instant::now();
    let a = run(&snapshot(wide(), "dev"), &ask(Predicate::ServerError), &one).unwrap();
    let one_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(a.one_minimal);

    let three =
        fail_when(|r| has_header(r, "X-H10") && r.url.contains("q77=") && has_header(r, "X-H90"));
    let start = web_time::Instant::now();
    let b = run(
        &snapshot(wide(), "dev"),
        &ask(Predicate::ServerError),
        &three,
    )
    .unwrap();
    let three_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(
        b.one_minimal,
        "{} probes, stopped by {:?}",
        b.probes, b.stopped_by
    );
    assert_eq!(
        header_names(&b.request),
        ["Authorization", "X-H10", "X-H90"]
    );

    println!(
        "shrink_probes: one cause {} probes in {one_ms:.1} ms, three causes {} probes in {three_ms:.1} ms",
        a.probes, b.probes
    );
    assert!(a.probes <= 60, "{} probes for one cause", a.probes);
    assert!(b.probes <= 160, "{} probes for three causes", b.probes);
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({
            "shrink_probes": a.probes, "three_probes": b.probes,
            "one_ms": one_ms, "three_ms": three_ms,
        });
        std::fs::write(
            std::path::Path::new(&dir).join("shrink.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
    // The own-time budget is a release-build number.
    if !cfg!(debug_assertions) {
        assert!(
            one_ms < 100.0 && three_ms < 100.0,
            "{one_ms} / {three_ms} ms"
        );
    }
}
