// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Comparing environments (wave 18, strand A): the same reading requests in two or more
//! environments, answers compared per request. A fake transport answers by host, so the two
//! "servers" differ the way a dev and a test system do.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::envcompare::{EnvCompareRequest, EnvRun, compare_environments};
use rt_app::runner::RunSnapshot;
use rt_core::send::SendOptions;
use rt_core::services::{CookieJar, Services};
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

const CANARY_DEV: &str = "RT-CANARY-dev-secret-7f3a";
const CANARY_TEST: &str = "RT-CANARY-test-secret-91bc";

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

/// `dev.test` and `qa.test`. `/items` differs in status-independent body and a header, `/same`
/// is the same, `/echo` echoes the query (a secret comes back), `/where` names its host in the
/// `Location` header and the body, `/login` sets a cookie named after the host, `/me` echoes
/// the cookie header it got. Every URL that went out is recorded.
#[derive(Default)]
struct Wire {
    urls: Mutex<Vec<String>>,
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        self.urls.lock().unwrap().push(request.url.clone());
        let url = request.url.clone();
        let host = url.split('/').nth(2).unwrap_or("").to_owned();
        let path = url.split(&host).nth(1).unwrap_or("").to_owned();
        let dev = host.starts_with("dev");
        let mut headers = vec![("Content-Type".to_owned(), "application/json".to_owned())];
        let body = if path.starts_with("/items") {
            headers.push(("X-Stage".into(), host.clone()));
            if dev {
                r#"{"count":2,"flag":false}"#.to_owned()
            } else {
                r#"{"count":3,"flag":false,"extra":1}"#.to_owned()
            }
        } else if path.starts_with("/echo") {
            let echoed = path.split("t=").nth(1).unwrap_or("").to_owned();
            headers.push(("X-Echo".into(), echoed.clone()));
            format!(r#"{{"echo":"{echoed}"}}"#)
        } else if path.starts_with("/where") {
            headers.push(("Location".into(), format!("http://{host}/next")));
            format!(r#"{{"self":"http://{host}","n":1}}"#)
        } else if path.starts_with("/who") {
            format!(r#"{{"served_by":"{host}"}}"#)
        } else if path.starts_with("/login") {
            headers.push(("Set-Cookie".into(), format!("sid=from-{host}; Path=/")));
            "{}".to_owned()
        } else if path.starts_with("/me") {
            let cookie = request
                .headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("cookie"))
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            format!(r#"{{"cookie":"{cookie}"}}"#)
        } else {
            r#"{"same":true}"#.to_owned()
        };
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(body))));
        Ok(Response {
            head: ResponseHead {
                status: 200,
                version: HttpVersion::Http11,
                headers,
                url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

/// One cookie jar for everything, keyed by the scope `send_with` passes (the environment).
#[derive(Default)]
struct Jar(Mutex<HashMap<String, Vec<String>>>);

impl CookieJar for Jar {
    fn cookie_header<'a>(
        &'a self,
        scope: &'a str,
        _url: &'a str,
    ) -> futures_util_local::LocalBoxFuture<'a, Option<String>> {
        let found = self.0.lock().unwrap().get(scope).map(|c| c.join("; "));
        Box::pin(async move { found })
    }
    fn store<'a>(
        &'a self,
        scope: &'a str,
        _url: &'a str,
        set_cookie: &'a [String],
    ) -> futures_util_local::LocalBoxFuture<'a, ()> {
        let mut map = self.0.lock().unwrap();
        for c in set_cookie {
            let pair = c.split(';').next().unwrap_or("").to_owned();
            map.entry(scope.to_owned()).or_default().push(pair);
        }
        Box::pin(async {})
    }
}

mod futures_util_local {
    pub use rt_core::services::LocalBoxFuture;
}

fn request_file(name: &str, seq: u32, method: &str, path: &str) -> (String, Vec<u8>) {
    (
        format!("{name}.yml"),
        format!(
            "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {method}\n  url: \"{{{{base}}}}{path}\"\n"
        )
        .into_bytes(),
    )
}

fn collection(extra_env: &str, requests: &[(&str, &str, &str)]) -> FileSet {
    let mut files = vec![
        (
            COLLECTION_FILE.to_owned(),
            b"opencollection: \"1.0.0\"\ninfo:\n  name: c\n".to_vec(),
        ),
        (
            "environments/dev.yml".to_owned(),
            format!(
                "name: dev\nvariables:\n  - name: base\n    value: http://dev.test\n  - name: token\n    secret: true\n{extra_env}"
            )
            .into_bytes(),
        ),
        (
            "environments/qa.yml".to_owned(),
            "name: qa\nvariables:\n  - name: base\n    value: http://qa.test\n  - name: token\n    secret: true\n"
                .as_bytes()
                .to_vec(),
        ),
        (
            "environments/prod.yml".to_owned(),
            "name: prod\nvariables:\n  - name: base\n    value: http://prod.test\n".as_bytes().to_vec(),
        ),
    ];
    for (i, (name, method, path)) in requests.iter().enumerate() {
        files.push(request_file(name, i as u32 + 1, method, path));
    }
    FileSet {
        files: files.into_iter().collect(),
    }
}

fn snapshot(files: &FileSet, environment: &str, secret: Option<&str>) -> RunSnapshot {
    let mut collection = read_collection(files).unwrap().model().clone();
    if let Some(secret) = secret {
        let env = collection
            .environments
            .iter_mut()
            .find(|e| e.name == environment);
        for v in env.into_iter().flat_map(|e| &mut e.variables) {
            if v.secret {
                v.value = Some(secret.to_owned());
            }
        }
    }
    RunSnapshot {
        collection,
        environment: Some(environment.to_owned()),
        vars: Vec::new(),
        files: FileSet::default(),
    }
}

fn ask(environments: &[&str]) -> EnvCompareRequest {
    EnvCompareRequest {
        environments: environments.iter().map(|s| (*s).to_owned()).collect(),
        folder: None,
        paths: None,
        ignore_env_values: false,
        production_confirmed: false,
    }
}

fn run(
    files: &FileSet,
    wire: &Wire,
    jar: Option<&Jar>,
    request: &EnvCompareRequest,
) -> Result<rt_app::envcompare::EnvComparison, rt_app::AppError> {
    let secrets = [Some(CANARY_DEV), Some(CANARY_TEST), None];
    let runs: Vec<EnvRun> = request
        .environments
        .iter()
        .enumerate()
        .map(|(i, name)| EnvRun {
            snapshot: snapshot(files, name, secrets[i.min(2)]),
            services: Services {
                cookies: jar.map(|j| j as &dyn CookieJar),
                ..Services::none()
            },
        })
        .collect();
    block_on(compare_environments(
        &runs,
        request,
        wire,
        &SendOptions::default(),
        &|| false,
    ))
}

#[test]
fn differences_in_headers_and_json_are_found_and_same_is_same() {
    let files = collection("", &[("items", "GET", "/items"), ("same", "GET", "/same")]);
    let wire = Wire::default();
    let cmp = run(&files, &wire, None, &ask(&["dev", "qa"])).unwrap();
    assert_eq!(cmp.reference, "dev");
    assert_eq!(cmp.pairs.len(), 1);
    let pair = &cmp.pairs[0];
    assert_eq!(pair.environment, "qa");
    let items = &pair.requests[0];
    assert!(!items.same);
    assert!(items.headers.iter().any(|h| h.name == "X-Stage"));
    let pointers: Vec<_> = items.body.iter().map(|b| b.pointer.as_str()).collect();
    assert_eq!(pointers, ["/count", "/extra"]);
    // The answers of both environments are on the sides, the reference on the old side.
    let sides = items.sides.as_ref().unwrap();
    assert!(sides.old.as_ref().unwrap().body.as_ref().unwrap().contains("\"count\":2"));
    assert!(sides.new.as_ref().unwrap().body.as_ref().unwrap().contains("\"count\":3"));
    assert!(pair.requests[1].same);
    assert_eq!(pair.reference_summary.requests, 2);
    assert_eq!(pair.summary.requests, 2);
    assert!(cmp.skipped.is_empty());
}

#[test]
fn requests_alternate_between_the_environments() {
    let files = collection("", &[("items", "GET", "/items"), ("same", "GET", "/same")]);
    let wire = Wire::default();
    run(&files, &wire, None, &ask(&["dev", "qa"])).unwrap();
    let urls = wire.urls.lock().unwrap().clone();
    assert_eq!(
        urls,
        [
            "http://dev.test/items",
            "http://qa.test/items",
            "http://dev.test/same",
            "http://qa.test/same"
        ]
    );
}

#[test]
fn with_three_environments_the_first_is_the_reference() {
    let files = collection("", &[("items", "GET", "/items")]);
    let wire = Wire::default();
    let cmp = run(&files, &wire, None, &ask(&["dev", "qa", "prod"]).confirmed()).unwrap();
    let names: Vec<_> = cmp.pairs.iter().map(|p| p.environment.as_str()).collect();
    assert_eq!(names, ["qa", "prod"]);
    assert_eq!(cmp.reference, "dev");
}

trait Confirmed {
    fn confirmed(self) -> Self;
}

impl Confirmed for EnvCompareRequest {
    fn confirmed(mut self) -> Self {
        self.production_confirmed = true;
        self
    }
}

#[test]
fn writing_requests_are_skipped_and_named() {
    let files = collection(
        "",
        &[
            ("items", "GET", "/items"),
            ("create", "POST", "/items"),
            ("remove", "DELETE", "/items"),
        ],
    );
    let wire = Wire::default();
    let cmp = run(&files, &wire, None, &ask(&["dev", "qa"])).unwrap();
    let skipped: Vec<_> = cmp
        .skipped
        .iter()
        .map(|s| (s.name.as_str(), s.reason.as_str()))
        .collect();
    assert_eq!(
        skipped,
        [("create", "skipped (writes)"), ("remove", "skipped (writes)")]
    );
    // Only the reading request went out, to both.
    assert_eq!(wire.urls.lock().unwrap().len(), 2);
    assert_eq!(cmp.pairs[0].requests.len(), 1);
}

#[test]
fn production_needs_confirmation_before_anything_is_sent() {
    let files = collection("", &[("items", "GET", "/items")]);
    let wire = Wire::default();
    let err = run(&files, &wire, None, &ask(&["dev", "prod"])).unwrap_err();
    assert!(err.to_string().contains("prod"), "{err}");
    assert!(wire.urls.lock().unwrap().is_empty());
    let cmp = run(&files, &wire, None, &ask(&["dev", "prod"]).confirmed()).unwrap();
    assert_eq!(cmp.pairs[0].environment, "prod");
    assert_eq!(wire.urls.lock().unwrap().len(), 2);
}

#[test]
fn bad_environments_are_refused() {
    let files = collection("", &[("items", "GET", "/items")]);
    let wire = Wire::default();
    assert!(run(&files, &wire, None, &ask(&["dev", "dev"])).is_err());
    let missing = run(&files, &wire, None, &ask(&["dev", "nowhere"]));
    assert!(matches!(missing, Err(rt_app::AppError::EnvironmentNotFound(_))));
}

#[test]
fn each_environment_sends_its_own_secret_and_none_is_shown() {
    let files = collection("", &[("echo", "GET", "/echo?t={{token}}")]);
    let wire = Wire::default();
    let cmp = run(&files, &wire, None, &ask(&["dev", "qa"])).unwrap();
    let urls = wire.urls.lock().unwrap().clone();
    assert_eq!(urls[0], format!("http://dev.test/echo?t={CANARY_DEV}"));
    assert_eq!(urls[1], format!("http://qa.test/echo?t={CANARY_TEST}"));
    // The answers echoed the secrets back; nothing of the result carries either value.
    let json = serde_json::to_string(&cmp).unwrap();
    assert!(!json.contains(CANARY_DEV) && !json.contains(CANARY_TEST), "{json}");
    assert!(!json.contains("RT-CANARY"), "{json}");
}

#[test]
fn cookies_stay_with_their_environment() {
    let files = collection("", &[("login", "GET", "/login"), ("me", "GET", "/me")]);
    let wire = Wire::default();
    let jar = Jar::default();
    let cmp = run(&files, &wire, Some(&jar), &ask(&["dev", "qa"])).unwrap();
    let me = &cmp.pairs[0].requests[1];
    let sides = me.sides.as_ref().unwrap();
    let body = |s: &Option<rt_app::compare::ResponseSnapshot>| s.as_ref().unwrap().body.clone().unwrap();
    assert!(body(&sides.old).contains("sid=from-dev.test"), "{}", body(&sides.old));
    assert!(body(&sides.new).contains("sid=from-qa.test"), "{}", body(&sides.new));
    assert!(!body(&sides.old).contains("qa.test") && !body(&sides.new).contains("dev.test"));
}

#[test]
fn ignoring_environment_values_hides_the_hosts() {
    let files = collection("", &[("where", "GET", "/where")]);
    let wire = Wire::default();
    let plain = run(&files, &wire, None, &ask(&["dev", "qa"])).unwrap();
    let r = &plain.pairs[0].requests[0];
    assert!(!r.same && r.headers.iter().any(|h| h.name == "Location"));
    assert!(r.body.iter().any(|b| b.pointer == "/self"));
    let mut quiet = ask(&["dev", "qa"]);
    quiet.ignore_env_values = true;
    let cmp = run(&files, &wire, None, &quiet).unwrap();
    assert!(cmp.pairs[0].requests[0].same, "{:?}", cmp.pairs[0].requests[0]);
}

#[test]
fn folder_and_paths_limit_the_run() {
    let files = collection("", &[("items", "GET", "/items"), ("same", "GET", "/same")]);
    let wire = Wire::default();
    let mut request = ask(&["dev", "qa"]);
    request.paths = Some(vec!["same.yml".into()]);
    let cmp = run(&files, &wire, None, &request).unwrap();
    assert_eq!(cmp.pairs[0].requests.len(), 1);
    assert_eq!(cmp.pairs[0].requests[0].path, "same.yml");
}

// ---------------------------------------------------------------------------------------
// Budget `env_compare`: 100 reading requests in two environments with a fake transport, every
// answer different; the run, `compare_steps` and the list of results, under 500 ms
// (median of 9). `scripts/perf/envcompare.mjs` runs this in a release build with
// `RT_PERF_OUT=<dir>` and reads `<dir>/envcompare.json`.

const PERF_REQUESTS: usize = 100;
const PERF_RUNS: usize = 9;
const PERF_LIMIT_MS: f64 = 500.0;

/// About 2 KB of JSON per answer; the `stage` field names the host, so every request differs.
struct Fat;

impl Transport for Fat {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let stage = if request.url.contains("dev.test") { "dev" } else { "qa" };
        let items: Vec<String> = (0..24)
            .map(|n| format!("{{\"id\":{n},\"name\":\"item number {n}\",\"tags\":[\"a\",\"b\"]}}"))
            .collect();
        let body = format!("{{\"stage\":\"{stage}\",\"items\":[{}]}}", items.join(","));
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(body))));
        Ok(Response {
            head: ResponseHead {
                status: 200,
                version: HttpVersion::Http11,
                headers: vec![("Content-Type".into(), "application/json".into())],
                url: request.url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

#[test]
fn env_compare_of_100_requests_is_measured() {
    let requests: Vec<(String, String, String)> = (0..PERF_REQUESTS)
        .map(|i| (format!("r{i:03}"), "GET".to_owned(), format!("/items/{i}")))
        .collect();
    let refs: Vec<(&str, &str, &str)> = requests
        .iter()
        .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
        .collect();
    let files = collection("", &refs);
    let request = ask(&["dev", "qa"]);
    let mut times = Vec::new();
    for _ in 0..PERF_RUNS {
        let started = web_time::Instant::now();
        let runs: Vec<EnvRun> = request
            .environments
            .iter()
            .map(|name| EnvRun {
                snapshot: snapshot(&files, name, None),
                services: Services::none(),
            })
            .collect();
        let cmp = block_on(compare_environments(
            &runs,
            &request,
            &Fat,
            &SendOptions::default(),
            &|| false,
        ))
        .unwrap();
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(cmp.pairs[0].requests.len(), PERF_REQUESTS);
        assert!(cmp.pairs[0].requests.iter().all(|r| !r.same), "100 differences");
    }
    times.sort_by(f64::total_cmp);
    let median = times[PERF_RUNS / 2];
    if let Ok(dir) = std::env::var("RT_PERF_OUT") {
        let json = format!("{{\"env_compare\":{median:.3},\"runs\":{PERF_RUNS}}}");
        std::fs::write(std::path::Path::new(&dir).join("envcompare.json"), json).unwrap();
    }
    // Debug builds are slower; the budget is for the release build the script measures.
    if !cfg!(debug_assertions) {
        assert!(median < PERF_LIMIT_MS, "env_compare {median:.1} ms");
    }
}

#[test]
fn a_bare_host_in_the_body_is_an_environment_value_too() {
    let files = collection("", &[("who", "GET", "/who")]);
    let wire = Wire::default();
    let plain = run(&files, &wire, None, &ask(&["dev", "qa"])).unwrap();
    assert!(!plain.pairs[0].requests[0].same);
    let mut quiet = ask(&["dev", "qa"]);
    quiet.ignore_env_values = true;
    let cmp = run(&files, &wire, None, &quiet).unwrap();
    assert!(cmp.pairs[0].requests[0].same, "{:?}", cmp.pairs[0].requests[0]);
    let side = cmp.pairs[0].requests[0].sides.as_ref().unwrap();
    assert!(side.old.as_ref().unwrap().body.as_ref().unwrap().contains("{{base.host}}"));
}
