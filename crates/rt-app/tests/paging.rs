// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Following pages and merging them (wave 18, strand C): the three rules against a fake
//! transport and against `rt-testserver` with 100 pages, merging, every limit with its reason,
//! the loop guard, the origin rule, the send guard per page, reads only, plugin rules and the
//! companion file. A canary secret rides in the auth header of every request and must not
//! reach a stored report, a note or the next page.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::paging::rules::{self, MatchContext};
use rt_app::paging::{
    CursorParam, CursorPlace, EffectivePaging, HARD_MAX_BYTES, HARD_MAX_PAGES, HARD_MAX_SECONDS,
    PagedRun, PagingLimits, PagingMode, PagingOrigin, PagingRule, PagingRuleScope, PagingRun,
    PagingSpec, PagingStop, PluginPaging, previous_page, run_on_snapshot,
};
use rt_app::runner::RunSnapshot;
use rt_app::{ResponseStore, SendInput, Workspace};
use rt_core::guard::{GuardVerdict, SendCheck, SendGuard};
use rt_core::model::{
    Body, Collection, Environment, Item, KeyValue, RawKind, RequestEntry, RequestSpec, Variable,
};
use rt_core::send::{ReportBody, SendOptions, SendReport};
use rt_core::services::Services;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::FileSet;
use rt_plugin::api::{PagingContribution, PagingMatch};

const CANARY: &str = "RT-CANARY-paging-secret-77c1";

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

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn ok(body: impl Into<String>) -> Reply {
    Reply {
        status: 200,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: body.into(),
    }
}

fn with_header(mut r: Reply, name: &str, value: &str) -> Reply {
    r.headers.push((name.into(), value.into()));
    r
}

type Rule = Box<dyn Fn(&HttpRequest, usize) -> Reply + Send + Sync>;

/// Answers with `rule(request, call number)` and remembers every request.
struct Server {
    rule: Rule,
    seen: Mutex<Vec<HttpRequest>>,
}

impl Server {
    fn new(rule: impl Fn(&HttpRequest, usize) -> Reply + Send + Sync + 'static) -> Self {
        Self {
            rule: Box::new(rule),
            seen: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
    fn urls(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.url.clone())
            .collect()
    }
    fn request(&self, n: usize) -> HttpRequest {
        self.seen.lock().unwrap()[n].clone()
    }
}

impl Transport for Server {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let n = {
            let mut seen = self.seen.lock().unwrap();
            seen.push(request.clone());
            seen.len()
        };
        let reply = (self.rule)(&request, n);
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(reply.body))));
        Ok(Response {
            head: ResponseHead {
                status: reply.status,
                version: HttpVersion::Http11,
                headers: reply.headers,
                url: request.url.clone(),
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

fn page_of(url: &str) -> usize {
    url.split("page=")
        .nth(1)
        .and_then(|p| p.split('&').next())
        .and_then(|p| p.parse().ok())
        .unwrap_or(1)
}

fn items(page: usize, per: usize) -> String {
    let first = (page - 1) * per + 1;
    let list: Vec<String> = (first..first + per)
        .map(|i| format!("{{\"id\":{i}}}"))
        .collect();
    format!("[{}]", list.join(","))
}

/// `pages` pages of two items, the next one named by a `Link` header.
fn link_server(pages: usize) -> Server {
    Server::new(move |r, _| {
        let page = page_of(&r.url);
        let body = format!(
            "{{\"total\":{},\"page\":{page},\"items\":{}}}",
            pages * 2,
            items(page, 2)
        );
        let reply = ok(body);
        if page < pages {
            with_header(
                reply,
                "Link",
                &format!("<http://pages.test/list?page={}>; rel=\"next\"", page + 1),
            )
        } else {
            reply
        }
    })
}

/// The next URL in the body (`next`), absolute or relative.
fn url_server(pages: usize, relative: bool) -> Server {
    Server::new(move |r, _| {
        let page = page_of(&r.url);
        let next = if page < pages {
            let path = format!("/list?page={}", page + 1);
            if relative {
                format!("\"{path}\"")
            } else {
                format!("\"http://pages.test{path}\"")
            }
        } else {
            "null".to_owned()
        };
        ok(format!("{{\"next\":{next},\"items\":{}}}", items(page, 2)))
    })
}

/// A cursor `c<k>` in `meta.cursor`; the request sends it as `after` (query, header or body).
fn cursor_server(pages: usize) -> Server {
    Server::new(move |r, _| {
        let sent = r
            .url
            .split("after=")
            .nth(1)
            .map(str::to_owned)
            .or_else(|| {
                r.headers
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case("x-after"))
                    .map(|(_, v)| v.clone())
            })
            .or_else(|| {
                r.body.as_ref().and_then(|b| {
                    let v = serde_json::from_slice::<serde_json::Value>(&b.bytes).ok()?;
                    let found = v
                        .get("after")
                        .or_else(|| v.get("variables")?.get("after"))?;
                    found.as_str().map(str::to_owned)
                })
            });
        let page = sent
            .as_deref()
            .and_then(|c| c.strip_prefix('c'))
            .and_then(|c| c.parse::<usize>().ok())
            .unwrap_or(1);
        let next = if page < pages {
            format!("\"c{}\"", page + 1)
        } else {
            "null".to_owned()
        };
        ok(format!(
            "{{\"meta\":{{\"cursor\":{next}}},\"items\":{}}}",
            items(page, 2)
        ))
    })
}

fn snapshot(spec: RequestSpec) -> RunSnapshot {
    let collection = Collection {
        name: "c".into(),
        items: vec![Item::Request(RequestEntry {
            path: "r.yml".into(),
            spec,
        })],
        environments: vec![Environment {
            name: "dev".into(),
            variables: vec![
                Variable::new("base", "http://pages.test"),
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
        environment: Some("dev".into()),
        vars: Vec::new(),
        files: FileSet::default(),
    }
}

fn get(url: &str) -> RequestSpec {
    RequestSpec {
        name: "r".into(),
        method: "GET".into(),
        url: url.into(),
        headers: vec![KeyValue::new("Authorization", "Bearer {{token}}")],
        ..RequestSpec::default()
    }
}

fn link() -> PagingSpec {
    PagingSpec {
        rule: PagingRule::LinkHeader,
        merge: Some("$.items".into()),
    }
}

fn json_url() -> PagingSpec {
    PagingSpec {
        rule: PagingRule::JsonUrl {
            next: "$.next".into(),
        },
        merge: Some("$.items".into()),
    }
}

fn cursor(place: CursorPlace, name: &str) -> PagingSpec {
    PagingSpec {
        rule: PagingRule::Cursor {
            next: "$.meta.cursor".into(),
            param: CursorParam {
                place,
                name: name.into(),
            },
        },
        merge: Some("$.items".into()),
    }
}

/// One page through the whole send path, as the app sends it.
fn send_page(snap: &RunSnapshot, page: RequestSpec, server: &Server) -> SendReport {
    let mut work = snap.clone();
    let Item::Request(entry) = &mut work.collection.items[0] else {
        unreachable!()
    };
    entry.spec = page;
    let paths = ["r.yml".to_owned()];
    let outcome = block_on(rt_app::compare::run_snapshot(
        &work,
        None,
        Some(&paths),
        server,
        &SendOptions::default(),
        &Services::none(),
        &mut |_| {},
        &|| false,
    ));
    outcome.steps.into_iter().next().unwrap().report
}

fn run_request(mode: PagingMode, limits: Option<PagingLimits>) -> PagingRun {
    PagingRun {
        path: "r.yml".into(),
        input: SendInput {
            environment: Some("dev".into()),
            vars: Vec::new(),
            secrets: Vec::new(),
        },
        mode,
        response_id: None,
        limits,
    }
}

fn run(
    snapshot: &RunSnapshot,
    rule: &PagingSpec,
    mode: PagingMode,
    limits: Option<PagingLimits>,
    server: &Server,
    services: &Services<'_>,
) -> Result<PagedRun, rt_app::AppError> {
    block_on(run_on_snapshot(
        snapshot,
        &run_request(mode, limits),
        rule,
        None,
        server,
        &SendOptions::default(),
        services,
        &|| false,
    ))
}

fn run_all(snapshot: &RunSnapshot, rule: &PagingSpec, server: &Server) -> PagedRun {
    run(
        snapshot,
        rule,
        PagingMode::All,
        None,
        server,
        &Services::none(),
    )
    .unwrap()
}

fn body(run: &PagedRun) -> serde_json::Value {
    let ReportBody::Utf8(text) = &run.report.response.as_ref().unwrap().body else {
        panic!("binary body")
    };
    serde_json::from_str(text).unwrap()
}

fn ids(run: &PagedRun) -> Vec<u64> {
    body(run)["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_u64().unwrap())
        .collect()
}

fn notes(run: &PagedRun) -> String {
    run.report
        .notes
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn no_canary(run: &PagedRun) {
    let all = serde_json::to_string(&run.report).unwrap();
    assert!(
        !all.contains(CANARY),
        "the canary reached the report: {all}"
    );
}

#[test]
fn link_header_pages_are_followed_and_merged() {
    let server = link_server(5);
    let run = run_all(&snapshot(get("{{base}}/list")), &link(), &server);
    assert_eq!(run.pages, 5);
    assert_eq!(run.stopped, PagingStop::Done);
    assert!(!run.more);
    assert_eq!(ids(&run), (1..=10).collect::<Vec<_>>());
    // The other fields are the first page's.
    assert_eq!(body(&run)["page"], 1);
    assert_eq!(body(&run)["total"], 10);
    assert_eq!(server.calls(), 5);
    assert!(notes(&run).contains("5 pages"), "{}", notes(&run));
    assert_eq!(
        run.report.response.as_ref().unwrap().size as usize,
        serde_json::to_string(&body(&run)).unwrap().len()
    );
    no_canary(&run);
}

#[test]
fn json_url_pages_absolute_and_relative() {
    for relative in [false, true] {
        let server = url_server(4, relative);
        let run = run_all(&snapshot(get("{{base}}/list")), &json_url(), &server);
        assert_eq!(run.pages, 4, "relative={relative}");
        assert_eq!(ids(&run), (1..=8).collect::<Vec<_>>());
        assert_eq!(server.urls()[3], "http://pages.test/list?page=4");
    }
}

#[test]
fn cursor_goes_in_the_query_a_header_or_the_body() {
    // Query.
    let server = cursor_server(4);
    let run = run_all(
        &snapshot(get("{{base}}/list?limit=2")),
        &cursor(CursorPlace::Query, "after"),
        &server,
    );
    assert_eq!(ids(&run), (1..=8).collect::<Vec<_>>());
    assert_eq!(server.urls()[0], "http://pages.test/list?limit=2");
    assert_eq!(server.urls()[1], "http://pages.test/list?limit=2&after=c2");
    // Header.
    let server = cursor_server(3);
    let run = run_all(
        &snapshot(get("{{base}}/list")),
        &cursor(CursorPlace::Header, "X-After"),
        &server,
    );
    assert_eq!(ids(&run), (1..=6).collect::<Vec<_>>());
    // Body (a JSON POST that reads).
    let server = cursor_server(3);
    let mut post = get("{{base}}/search");
    post.method = "POST".into();
    post.body = Body::Raw {
        kind: RawKind::Json,
        data: "{\"q\":\"x\"}".into(),
    };
    let search = PagingSpec {
        merge: Some("$.items".into()),
        ..cursor(CursorPlace::Body, "after")
    };
    let result = block_on(run_on_snapshot(
        &snapshot(post.clone()),
        &run_request(PagingMode::All, None),
        &search,
        None,
        &server,
        &SendOptions::default(),
        &Services::none(),
        &|| false,
    ));
    // A POST of plain JSON is a write: pages are not followed.
    let err = result.unwrap_err().to_string();
    assert!(err.contains("writes"), "{err}");
    assert_eq!(server.calls(), 0, "nothing was sent");
}

#[test]
fn graphql_queries_follow_but_mutations_do_not() {
    let server = cursor_server(3);
    let mut query = get("{{base}}/graphql");
    query.method = "POST".into();
    query.body = Body::GraphQl {
        query: "query { items { id } }".into(),
        variables: "{\"first\": 2}".into(),
    };
    let rule = cursor(CursorPlace::Body, "after");
    let run = block_on(run_on_snapshot(
        &snapshot(query.clone()),
        &run_request(PagingMode::All, None),
        &rule,
        None,
        &server,
        &SendOptions::default(),
        &Services::none(),
        &|| false,
    ))
    .unwrap();
    assert_eq!(run.pages, 3);
    let second = server.request(1);
    let sent: serde_json::Value = serde_json::from_slice(&second.body.unwrap().bytes).unwrap();
    assert_eq!(sent["variables"]["after"], "c2");
    assert_eq!(sent["variables"]["first"], 2);

    query.body = Body::GraphQl {
        query: "mutation { addItem { id } }".into(),
        variables: String::new(),
    };
    let server = cursor_server(3);
    let err = block_on(run_on_snapshot(
        &snapshot(query),
        &run_request(PagingMode::All, None),
        &rule,
        None,
        &server,
        &SendOptions::default(),
        &Services::none(),
        &|| false,
    ))
    .unwrap_err();
    assert!(err.to_string().contains("writes"));
    assert_eq!(server.calls(), 0);
}

#[test]
fn writing_requests_are_never_followed() {
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let server = link_server(3);
        let mut spec = get("{{base}}/list");
        spec.method = method.into();
        let err = run(
            &snapshot(spec),
            &link(),
            PagingMode::All,
            None,
            &server,
            &Services::none(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("writes"), "{method}: {err}");
        assert_eq!(server.calls(), 0, "{method} sent nothing");
    }
}

#[test]
fn without_a_merge_path_the_last_page_is_the_result_and_every_page_is_seen() {
    let server = link_server(3);
    let rule = PagingSpec {
        rule: PagingRule::LinkHeader,
        merge: None,
    };
    let snap = snapshot(get("{{base}}/list"));
    let spec = get("{{base}}/list");
    let mut seen = Vec::new();
    let run = block_on(rt_app::paging::follow(
        rt_app::paging::Follow {
            spec: &spec,
            rule: &rule,
            mode: PagingMode::All,
            previous: None,
            limits: PagingLimits::default(),
            first: None,
        },
        async |page: RequestSpec| Ok(send_page(&snap, page, &server)),
        &|| false,
        &mut |report: &SendReport| seen.push(report.response.as_ref().unwrap().url.clone()),
    ))
    .unwrap();
    assert_eq!(run.pages, 3);
    assert_eq!(seen.len(), 3);
    assert_eq!(body(&run)["page"], 3, "the last page");
}

// ----------------------------------------------------------------------------------------
// Limits

#[test]
fn the_page_limit_stops_with_a_reason_and_says_more_pages_exist() {
    let server = link_server(50);
    let limits = PagingLimits {
        max_pages: 7,
        ..PagingLimits::default()
    };
    let run = run(
        &snapshot(get("{{base}}/list")),
        &link(),
        PagingMode::All,
        Some(limits),
        &server,
        &Services::none(),
    )
    .unwrap();
    assert_eq!(run.pages, 7);
    assert_eq!(run.stopped, PagingStop::MaxPages);
    assert!(run.more);
    assert_eq!(server.calls(), 7);
    assert_eq!(ids(&run).len(), 14);
    assert!(
        notes(&run).contains("stopped after 7 pages: limit reached; more pages exist"),
        "{}",
        notes(&run)
    );
}

#[test]
fn the_default_is_twenty_pages() {
    let server = link_server(100);
    let run = run_all(&snapshot(get("{{base}}/list")), &link(), &server);
    assert_eq!(run.pages, 20);
    assert_eq!(run.stopped, PagingStop::MaxPages);
    assert!(notes(&run).contains("stopped after 20 pages"));
}

#[test]
fn limits_are_cut_to_the_hard_maxima() {
    let cut = PagingLimits {
        max_pages: 1_000_000,
        max_bytes: u64::MAX,
        max_seconds: u64::MAX,
    }
    .clamped();
    assert_eq!(cut.max_pages, HARD_MAX_PAGES);
    assert_eq!(cut.max_bytes, HARD_MAX_BYTES);
    assert_eq!(cut.max_seconds, HARD_MAX_SECONDS);
    assert_eq!(HARD_MAX_PAGES, 1_000);
    assert_eq!(HARD_MAX_BYTES, 100 * 1024 * 1024);
    assert_eq!(HARD_MAX_SECONDS, 600);
    let zero = PagingLimits {
        max_pages: 0,
        max_bytes: 0,
        max_seconds: 0,
    }
    .clamped();
    assert_eq!(
        (zero.max_pages, zero.max_bytes, zero.max_seconds),
        (1, 1, 1)
    );
    // A run with 5000 pages asked for goes no further than the hard maximum would allow.
    let server = link_server(1_200);
    let run = run(
        &snapshot(get("{{base}}/list")),
        &link(),
        PagingMode::All,
        Some(PagingLimits {
            max_pages: 5_000,
            max_bytes: u64::MAX,
            max_seconds: u64::MAX,
        }),
        &server,
        &Services::none(),
    )
    .unwrap();
    assert_eq!(run.pages, 1_000);
    assert_eq!(run.stopped, PagingStop::MaxPages);
}

#[test]
fn the_byte_limit_stops_before_the_page_that_does_not_fit() {
    let server = link_server(50);
    let limits = PagingLimits {
        max_bytes: 300,
        ..PagingLimits::default()
    };
    let run = run(
        &snapshot(get("{{base}}/list")),
        &link(),
        PagingMode::All,
        Some(limits),
        &server,
        &Services::none(),
    )
    .unwrap();
    assert_eq!(run.stopped, PagingStop::MaxBytes);
    assert!(run.more);
    let size: usize = serde_json::to_string(&body(&run)).unwrap().len();
    assert!(size <= 300 + 60, "{size}");
    assert!(
        notes(&run).contains("size limit reached"),
        "{}",
        notes(&run)
    );
}

#[test]
fn the_time_limit_counts_a_wait_the_server_asked_for() {
    // A response that says "wait 30 s" with a 2 s budget stops at once, without sleeping.
    let spec = get("{{base}}/list");
    let snap = snapshot(spec.clone());
    let server = link_server(10);
    let started = std::time::Instant::now();
    let run = block_on(rt_app::paging::follow(
        rt_app::paging::Follow {
            spec: &spec,
            rule: &link(),
            mode: PagingMode::All,
            previous: None,
            limits: PagingLimits {
                max_seconds: 2,
                ..PagingLimits::default()
            },
            first: None,
        },
        async |page: RequestSpec| {
            let mut report = send_page(&snap, page, &server);
            report.rate_limit = Some(rt_core::ratelimit::RateLimits {
                retry_after_ms: Some(30_000),
                ..Default::default()
            });
            Ok(report)
        },
        &|| false,
        &mut |_| {},
    ))
    .unwrap();
    assert!(started.elapsed().as_secs() < 2);
    assert_eq!(run.stopped, PagingStop::MaxSeconds);
    assert_eq!(run.pages, 1);
    assert!(run.more);
}

#[test]
fn a_repeated_cursor_or_url_is_a_loop() {
    // The cursor never advances.
    let server = Server::new(|_, _| ok("{\"meta\":{\"cursor\":\"same\"},\"items\":[{\"id\":1}]}"));
    let run = run_all(
        &snapshot(get("{{base}}/list")),
        &cursor(CursorPlace::Query, "after"),
        &server,
    );
    assert_eq!(run.stopped, PagingStop::Loop);
    assert_eq!(
        run.pages, 2,
        "the second page brought the same cursor again"
    );
    assert!(notes(&run).contains("already loaded"), "{}", notes(&run));
    // The next URL is the page itself.
    let server = Server::new(|r, _| {
        ok(format!(
            "{{\"next\":\"{}\",\"items\":[{{\"id\":1}}]}}",
            r.url
        ))
    });
    let run = run_all(&snapshot(get("{{base}}/list")), &json_url(), &server);
    assert_eq!(run.stopped, PagingStop::Loop);
    assert_eq!(server.calls(), 1);
}

#[test]
fn another_origin_stops_the_run_with_a_note() {
    let server = Server::new(|r, _| {
        let page = page_of(&r.url);
        if page == 1 {
            ok(format!(
                "{{\"next\":\"http://elsewhere.test/list?page=2\",\"items\":{}}}",
                items(1, 2)
            ))
        } else {
            ok("{\"next\":null,\"items\":[]}")
        }
    });
    let run = run_all(&snapshot(get("{{base}}/list")), &json_url(), &server);
    assert!(
        matches!(run.stopped, PagingStop::OtherOrigin(_)),
        "{:?}",
        run.stopped
    );
    assert_eq!(server.calls(), 1);
    assert!(run.more);
    assert_eq!(ids(&run), vec![1, 2]);
    let text = notes(&run);
    assert!(text.contains("elsewhere.test"), "{text}");
    // Same host, other port, other scheme: also other origins.
    for next in ["http://pages.test:8080/x", "https://pages.test/x"] {
        let target = next.to_owned();
        let server = Server::new(move |_, _| ok(format!("{{\"next\":\"{target}\",\"items\":[]}}")));
        let run = run_all(&snapshot(get("{{base}}/list")), &json_url(), &server);
        assert!(matches!(run.stopped, PagingStop::OtherOrigin(_)), "{next}");
    }
    // The default port written out is the same origin.
    let server = Server::new(|r, _| {
        if page_of(&r.url) == 1 && !r.url.contains(":80/") {
            ok("{\"next\":\"http://pages.test:80/list?page=2\",\"items\":[]}")
        } else {
            ok("{\"next\":null,\"items\":[]}")
        }
    });
    let run = run_all(&snapshot(get("{{base}}/list")), &json_url(), &server);
    assert_eq!(run.stopped, PagingStop::Done);
}

#[test]
fn a_template_in_a_next_url_is_never_resolved() {
    let server = Server::new(|r, _| {
        if page_of(&r.url) == 1 && !r.url.contains("%7B") {
            ok("{\"next\":\"http://pages.test/list?page=2&t={{token}}\",\"items\":[]}")
        } else {
            ok("{\"next\":null,\"items\":[]}")
        }
    });
    let run = run_all(&snapshot(get("{{base}}/list")), &json_url(), &server);
    assert_eq!(run.pages, 2);
    let url = &server.urls()[1];
    assert!(!url.contains(CANARY), "{url}");
    assert!(url.contains("%7B%7Btoken%7D%7D"), "{url}");
    no_canary(&run);
}

#[test]
fn an_error_status_ends_the_run_with_what_was_loaded() {
    let server = Server::new(|r, _| {
        let page = page_of(&r.url);
        if page == 3 {
            Reply {
                status: 503,
                headers: vec![],
                body: "{\"error\":\"down\"}".into(),
            }
        } else {
            with_header(
                ok(format!("{{\"items\":{}}}", items(page, 2))),
                "Link",
                &format!("<http://pages.test/list?page={}>; rel=\"next\"", page + 1),
            )
        }
    });
    let run = run_all(&snapshot(get("{{base}}/list")), &link(), &server);
    assert_eq!(run.stopped, PagingStop::ErrorStatus(503));
    assert_eq!(run.pages, 2, "the failed page is not counted");
    assert_eq!(ids(&run), vec![1, 2, 3, 4]);
    assert!(run.more);
    assert!(notes(&run).contains("answered 503"), "{}", notes(&run));
    // The first page failing gives that page.
    let server = Server::new(|_, _| Reply {
        status: 404,
        headers: vec![],
        body: "nope".into(),
    });
    let run = run_all(&snapshot(get("{{base}}/list")), &link(), &server);
    assert_eq!(run.stopped, PagingStop::ErrorStatus(404));
    assert_eq!(run.report.response.as_ref().unwrap().status, 404);
}

struct RefuseFrom(usize, Mutex<usize>);

impl SendGuard for RefuseFrom {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        let mut n = self.1.lock().unwrap();
        *n += 1;
        assert_eq!(check.effect, rt_core::guard::Effect::Read);
        if *n >= self.0 {
            GuardVerdict::Refuse {
                rule: "no-paging".into(),
                reason: "enough".into(),
            }
        } else {
            GuardVerdict::Allow
        }
    }
}

#[test]
fn the_send_guard_answers_for_every_page_and_a_no_ends_the_run() {
    let server = link_server(10);
    let guard = RefuseFrom(3, Mutex::new(0));
    let services = Services {
        guard: Some(&guard),
        ..Services::none()
    };
    let run = run(
        &snapshot(get("{{base}}/list")),
        &link(),
        PagingMode::All,
        None,
        &server,
        &services,
    )
    .unwrap();
    assert_eq!(
        *guard.1.lock().unwrap(),
        3,
        "asked for the first three pages"
    );
    assert_eq!(server.calls(), 2, "the third never went out");
    assert_eq!(run.pages, 2);
    assert!(
        matches!(&run.stopped, PagingStop::Refused(m) if m.contains("no-paging")),
        "{:?}",
        run.stopped
    );
    assert!(run.more);
    assert_eq!(ids(&run), vec![1, 2, 3, 4], "what was loaded stays");
    assert!(notes(&run).contains("refused"), "{}", notes(&run));
}

#[test]
fn cancelling_stops_between_pages() {
    let server = link_server(10);
    let spec = get("{{base}}/list");
    let snap = snapshot(spec.clone());
    let count = std::cell::Cell::new(0);
    let run = block_on(rt_app::paging::follow(
        rt_app::paging::Follow {
            spec: &spec,
            rule: &link(),
            mode: PagingMode::All,
            previous: None,
            limits: PagingLimits::default(),
            first: None,
        },
        async |page: RequestSpec| {
            count.set(count.get() + 1);
            Ok(send_page(&snap, page, &server))
        },
        &|| count.get() >= 3,
        &mut |_| {},
    ))
    .unwrap();
    assert_eq!(run.pages, 3);
    assert!(matches!(&run.stopped, PagingStop::Refused(m) if m == "cancelled"));
}

// ----------------------------------------------------------------------------------------
// Next, the store, rules

#[test]
fn next_continues_from_a_kept_page_and_returns_that_page_alone() {
    let server = url_server(3, false);
    let snap = snapshot(get("{{base}}/list"));
    let first = run(
        &snap,
        &PagingSpec {
            rule: json_url().rule,
            merge: None,
        },
        PagingMode::All,
        Some(PagingLimits {
            max_pages: 1,
            ..PagingLimits::default()
        }),
        &server,
        &Services::none(),
    )
    .unwrap();
    assert_eq!(first.pages, 1);
    assert_eq!(first.stopped, PagingStop::MaxPages);
    let mut store = ResponseStore::new();
    let stored = first.into_result(&mut store);
    let id = stored.response.response_id.unwrap();
    let previous = previous_page(&store, id).unwrap();
    let second = block_on(run_on_snapshot(
        &snap,
        &run_request(PagingMode::Next, None),
        &PagingSpec {
            rule: json_url().rule,
            merge: None,
        },
        Some(previous),
        &server,
        &SendOptions::default(),
        &Services::none(),
        &|| false,
    ))
    .unwrap();
    assert_eq!(second.pages, 1);
    assert!(second.more, "a third page exists");
    assert_eq!(ids(&second), vec![3, 4]);
    assert_eq!(server.urls()[1], "http://pages.test/list?page=2");
    // Next on the last page: nothing more.
    let last = run(
        &snap,
        &PagingSpec {
            rule: json_url().rule,
            merge: None,
        },
        PagingMode::All,
        Some(PagingLimits {
            max_pages: 3,
            ..PagingLimits::default()
        }),
        &server,
        &Services::none(),
    )
    .unwrap();
    assert_eq!(last.stopped, PagingStop::Done);
}

fn workspace() -> Workspace {
    let files = FileSet {
        files: [
            (
                "opencollection.yml".to_owned(),
                b"opencollection: 1.0.0\ninfo:\n  name: C\n".to_vec(),
            ),
            (
                "list.yml".to_owned(),
                b"info:\n  name: list\n  type: http\nhttp:\n  method: GET\n  url: http://pages.test/list\n".to_vec(),
            ),
        ]
        .into(),
    };
    Workspace::open(files).unwrap()
}

fn plugin_rule(host: &str) -> PluginPaging {
    PluginPaging {
        plugin: "acme".into(),
        contribution: PagingContribution {
            id: "next-link".into(),
            matches: PagingMatch {
                hosts: vec![host.into()],
                content_types: vec!["application/json".into()],
                url_pattern: None,
            },
            rule: PagingSpec {
                rule: PagingRule::JsonUrl {
                    next: "$.next".into(),
                },
                merge: Some("$.value".into()),
            },
        },
    }
}

#[test]
fn the_request_beats_the_collection_beats_the_plugin() {
    let mut ws = workspace();
    let plugins = [plugin_rule("pages.test")];
    let context = MatchContext {
        url: "http://pages.test/list".into(),
        content_type: Some("application/json; charset=utf-8".into()),
    };
    // Only the plugin knows a rule: offered with its name.
    let found = rules::effective(&ws, "list.yml", &plugins, Some(&context))
        .unwrap()
        .unwrap();
    assert_eq!(
        found.origin,
        PagingOrigin::Plugin {
            plugin: "acme".into(),
            id: "next-link".into()
        }
    );
    // Without a match context a plugin's rule does not apply.
    assert!(
        rules::effective(&ws, "list.yml", &plugins, None)
            .unwrap()
            .is_none()
    );
    // A host that is not the plugin's.
    let other = MatchContext {
        url: "http://other.test/".into(),
        ..context.clone()
    };
    assert!(
        rules::effective(&ws, "list.yml", &plugins, Some(&other))
            .unwrap()
            .is_none()
    );
    // The collection default wins over the plugin.
    rules::set(
        &mut ws,
        "list.yml",
        Some(&cursor(CursorPlace::Query, "after")),
        PagingRuleScope::Collection,
    )
    .unwrap();
    let found = rules::effective(&ws, "list.yml", &plugins, Some(&context))
        .unwrap()
        .unwrap();
    assert_eq!(found.origin, PagingOrigin::Collection);
    // The request wins over both.
    rules::set(&mut ws, "list.yml", Some(&link()), PagingRuleScope::Request).unwrap();
    let found: EffectivePaging = rules::effective(&ws, "list.yml", &plugins, Some(&context))
        .unwrap()
        .unwrap();
    assert_eq!(found.origin, PagingOrigin::Request);
    assert_eq!(found.spec, link());
    // Removing the request's rule gives the collection's back.
    rules::set(&mut ws, "list.yml", None, PagingRuleScope::Request).unwrap();
    let found = rules::effective(&ws, "list.yml", &plugins, Some(&context))
        .unwrap()
        .unwrap();
    assert_eq!(found.origin, PagingOrigin::Collection);
}

#[test]
fn rules_are_saved_as_files_and_nothing_else_changes() {
    let mut ws = workspace();
    rules::set(
        &mut ws,
        "list.yml",
        Some(&json_url()),
        PagingRuleScope::Request,
    )
    .unwrap();
    rules::set(
        &mut ws,
        "list.yml",
        Some(&link()),
        PagingRuleScope::Collection,
    )
    .unwrap();
    let changes = ws.save().unwrap();
    let paths: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
    assert!(paths.contains(&"list.paging.json"), "{paths:?}");
    assert!(paths.contains(&"opencollection.yml"), "{paths:?}");
    assert!(
        !paths.contains(&"list.yml"),
        "the request file is untouched: {paths:?}"
    );
    let companion = changes
        .iter()
        .find(|c| c.path == "list.paging.json")
        .unwrap();
    assert_eq!(
        companion.text,
        rt_format::paging_config::write_companion(Some(&json_url()))
    );
    let collection = changes
        .iter()
        .find(|c| c.path == "opencollection.yml")
        .unwrap();
    assert!(
        collection.text.contains("rule: link-header"),
        "{}",
        collection.text
    );
}

#[test]
fn a_bad_rule_is_refused_when_it_is_saved() {
    let mut ws = workspace();
    for bad in [
        PagingSpec {
            rule: PagingRule::JsonUrl {
                next: "next".into(),
            },
            merge: None,
        },
        PagingSpec {
            rule: PagingRule::LinkHeader,
            merge: Some("$.[".into()),
        },
        PagingSpec {
            rule: PagingRule::Cursor {
                next: "$.c".into(),
                param: CursorParam {
                    place: CursorPlace::Query,
                    name: " ".into(),
                },
            },
            merge: None,
        },
    ] {
        assert!(
            rules::set(&mut ws, "list.yml", Some(&bad), PagingRuleScope::Request).is_err(),
            "{bad:?}"
        );
        assert!(rules::set(&mut ws, "list.yml", Some(&bad), PagingRuleScope::Collection).is_err());
    }
    assert!(rules::set(&mut ws, "nope.yml", Some(&link()), PagingRuleScope::Request).is_err());
    assert!(ws.save().unwrap().is_empty(), "nothing was staged");
}

#[test]
fn a_damaged_companion_is_an_error_not_a_guess() {
    let mut ws = workspace();
    ws.stage_companion("list.paging.json", b"{ nonsense".to_vec())
        .unwrap();
    assert!(rules::effective(&ws, "list.yml", &[], None).is_err());
}

// ----------------------------------------------------------------------------------------
// The real server, 100 pages

/// The test server on a thread of its own (a current-thread runtime serves only while it runs).
fn serve() -> ((), std::net::SocketAddr) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let addr = rt_testserver::spawn(([127, 0, 0, 1], 0).into())
                .await
                .unwrap();
            tx.send(addr).unwrap();
            std::future::pending::<()>().await;
        });
    });
    ((), rx.recv().unwrap())
}

fn real(
    url: &str,
    rule: &PagingSpec,
    limits: PagingLimits,
    addr: std::net::SocketAddr,
) -> PagedRun {
    let mut spec = get(url);
    spec.url = url.replace("{{host}}", &addr.to_string());
    let snap = snapshot(spec);
    let (runtime, _) = (
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
        (),
    );
    let transport = rt_core::transport::native::NativeTransport::new().unwrap();
    runtime
        .block_on(run_on_snapshot(
            &snap,
            &run_request(PagingMode::All, Some(limits)),
            rule,
            None,
            &transport,
            &SendOptions::default(),
            &Services::none(),
            &|| false,
        ))
        .unwrap()
}

#[test]
fn a_hundred_pages_of_each_kind_against_the_test_server() {
    let (_server, addr) = serve();
    let limits = PagingLimits {
        max_pages: 1_000,
        ..PagingLimits::default()
    };
    let all: Vec<u64> = (1..=1_000).collect();
    for (kind, rule) in [
        ("link", link()),
        ("url", json_url()),
        ("cursor", cursor(CursorPlace::Query, "cursor")),
    ] {
        let run = real(
            &format!("http://{{{{host}}}}/pages/{kind}/100?size=10"),
            &rule,
            limits.clone(),
            addr,
        );
        assert_eq!(run.pages, 100, "{kind}");
        assert_eq!(run.stopped, PagingStop::Done, "{kind}");
        assert_eq!(ids(&run), all, "{kind}");
        no_canary(&run);
    }
}

#[test]
fn page_101_is_not_loaded_when_the_limit_is_100() {
    let (_server, addr) = serve();
    let run = real(
        "http://{{host}}/pages/link/101?size=2",
        &link(),
        PagingLimits {
            max_pages: 100,
            ..PagingLimits::default()
        },
        addr,
    );
    assert_eq!(run.pages, 100);
    assert_eq!(run.stopped, PagingStop::MaxPages);
    assert!(run.more);
    assert_eq!(ids(&run).len(), 200);
}

#[test]
fn the_server_side_failure_and_the_foreign_next_url() {
    let (_server, addr) = serve();
    let run = real(
        "http://{{host}}/pages/link/10?size=2&fail_at=4",
        &link(),
        PagingLimits::default(),
        addr,
    );
    assert_eq!(run.stopped, PagingStop::ErrorStatus(500));
    assert_eq!(ids(&run).len(), 6);
    let run = real(
        "http://{{host}}/pages/url/10?size=2&foreign=1",
        &json_url(),
        PagingLimits::default(),
        addr,
    );
    assert!(
        matches!(run.stopped, PagingStop::OtherOrigin(_)),
        "{:?}",
        run.stopped
    );
    assert_eq!(run.pages, 1);
    let run = real(
        "http://{{host}}/pages/cursor/10?size=2&loop=1",
        &cursor(CursorPlace::Query, "cursor"),
        PagingLimits::default(),
        addr,
    );
    assert_eq!(run.stopped, PagingStop::Loop);
}

// ---------------------------------------------------------------------------------------------
// Budget `paging_merge` (perf/budgets.toml): 100 pages of 100 items followed and merged into one
// stored response. `scripts/perf/paging.mjs` runs this in a release build with `RT_PERF_OUT=<dir>`
// and reads `<dir>/paging.json`. Without `RT_PERF_OUT` it only checks the result.
// ---------------------------------------------------------------------------------------------

/// Resident memory of this process in KiB (`ps`, no unsafe), 0 where `ps` is missing.
fn rss_kib() -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

#[test]
fn paging_merge_of_100_pages_is_measured() {
    const PAGES: usize = 100;
    const PER: usize = 100;
    let pad = "x".repeat(1000);
    let server = {
        let pad = pad.clone();
        Server::new(move |r, _| {
            let page = page_of(&r.url);
            let first = (page - 1) * PER + 1;
            let list: Vec<String> = (first..first + PER)
                .map(|i| format!("{{\"id\":{i},\"pad\":\"{pad}\"}}"))
                .collect();
            let reply = ok(format!(
                "{{\"page\":{page},\"items\":[{}]}}",
                list.join(",")
            ));
            if page < PAGES {
                with_header(
                    reply,
                    "Link",
                    &format!("<http://pages.test/list?page={}>; rel=\"next\"", page + 1),
                )
            } else {
                reply
            }
        })
    };
    let snap = snapshot(get("{{base}}/list?page=1"));
    let limits = Some(PagingLimits {
        max_pages: PAGES as u32,
        ..PagingLimits::default()
    });

    let mut times = Vec::new();
    let mut size = 0;
    let mut peak_growth = 0;
    for _ in 0..9 {
        let before = rss_kib();
        let sampler_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sampler = {
            let stop = sampler_stop.clone();
            std::thread::spawn(move || {
                let mut peak = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    peak = peak.max(rss_kib());
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                peak
            })
        };
        let start = web_time::Instant::now();
        let done = run(
            &snap,
            &link(),
            PagingMode::All,
            limits.clone(),
            &server,
            &Services::none(),
        )
        .unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        sampler_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let peak = sampler.join().unwrap();
        let merged = body(&done);
        assert_eq!(merged["items"].as_array().unwrap().len(), PAGES * PER);
        assert_eq!(done.pages as usize, PAGES);
        size = serde_json::to_vec(&merged).unwrap().len();
        peak_growth = peak_growth.max(peak.saturating_sub(before) * 1024);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    eprintln!("paging_merge median {median:.1} ms, result {size} B, peak growth {peak_growth} B");
    if let Ok(dir) = std::env::var("RT_PERF_OUT") {
        let json = format!(
            "{{\"paging_merge\":{median},\"runs\":{},\"result_bytes\":{size},\"peak_growth_bytes\":{peak_growth}}}",
            times.len()
        );
        std::fs::write(std::path::Path::new(&dir).join("paging.json"), json).unwrap();
    }
}
