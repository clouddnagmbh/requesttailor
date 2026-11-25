// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance tests of the guided network access for extensions (wave 19, strand B): a host
//! that is not allowed is refused, an allowed one is remembered per device, a secret in a
//! header is resolved and masked everywhere (canary), a redirect is returned and not followed,
//! the limits hold (body, response, time), the send guard answers for every call with
//! `Purpose::Egress`, and nothing is observed or recorded. The wire is a fake transport (what
//! went out is what the tests say), and the real native transport against rt-testserver for
//! redirects, size and time.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use bytes::Bytes;
use rt_app::AppError;
use rt_app::egress::{
    Context, EgressError, EgressMethod, EgressRequest, HostList, MAX_BODY, MAX_RESPONSE,
    lookup_secrets, request, secret_names,
};
use rt_core::guard::{Caller, Effect, GuardVerdict, Purpose, SendCheck, SendGuard};
use rt_core::model::{Environment, Variable};
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};

const CANARY: &str = "RT-CANARY-egress-secret-8f31";

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

type Rule = Box<dyn Fn(&HttpRequest) -> (u16, Vec<(String, String)>, Vec<u8>) + Send + Sync>;

/// Answers with `rule(request)` and remembers every request that reached the wire.
struct Wire {
    rule: Rule,
    seen: Mutex<Vec<HttpRequest>>,
}

impl Wire {
    fn new(
        rule: impl Fn(&HttpRequest) -> (u16, Vec<(String, String)>, Vec<u8>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            rule: Box::new(rule),
            seen: Mutex::new(Vec::new()),
        }
    }
    fn ok(body: &str) -> Self {
        let body = body.to_owned();
        Self::new(move |_| (200, Vec::new(), body.clone().into_bytes()))
    }
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
    fn last(&self) -> HttpRequest {
        self.seen
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a request")
    }
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push(request.clone());
        let (status, headers, body) = (self.rule)(&request);
        let stream: BodyStream = Box::pin(Once(Some(Bytes::from(body))));
        Ok(Response {
            head: ResponseHead {
                status,
                version: HttpVersion::Http11,
                headers,
                url: request.url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body: stream,
        })
    }
}

fn allowed(hosts: &[(&str, Option<u16>)]) -> HostList {
    let mut list = HostList::new();
    for (host, port) in hosts {
        list.allow(host, *port, "traces", "Server traces", 1)
            .unwrap();
    }
    list
}

fn get(url: &str) -> EgressRequest {
    EgressRequest {
        method: EgressMethod::Get,
        url: url.into(),
        headers: Vec::new(),
        body: None,
        extension: "traces".into(),
        purpose: "Server traces".into(),
        collection: None,
        environment: None,
    }
}

fn with_header(mut r: EgressRequest, name: &str, value: &str) -> EgressRequest {
    r.headers.push((name.into(), value.into()));
    r
}

fn run(
    wire: &Wire,
    hosts: &HostList,
    guard: Option<&dyn SendGuard>,
    secrets: &[(String, String)],
    req: &EgressRequest,
) -> Result<rt_app::egress::EgressResponse, EgressError> {
    let ctx = Context {
        hosts,
        guard,
        caller: Caller::App,
        collection: "Orders",
        environment: Some("prod-like"),
        secrets,
    };
    block_on(request(wire, &ctx, req))
}

// ---- hosts: refused until allowed, allowed once per device ----

#[test]
fn a_host_that_is_not_allowed_is_refused_and_nothing_is_sent() {
    let wire = Wire::ok("{}");
    let err = run(
        &wire,
        &HostList::new(),
        None,
        &[],
        &get("https://traces.example/api/t/1"),
    )
    .unwrap_err();
    assert_eq!(
        err,
        EgressError::NotAllowed {
            host: "traces.example".into(),
            port: None
        }
    );
    assert_eq!(wire.calls(), 0);
    // Another host, another port, another scheme's default port: all refused.
    let hosts = allowed(&[("traces.example", None)]);
    for url in [
        "https://other.example/x",
        "https://traces.example:8443/x",
        "http://traces.example/x",
    ] {
        assert!(
            matches!(
                run(&wire, &hosts, None, &[], &get(url)),
                Err(EgressError::NotAllowed { .. })
            ),
            "{url}"
        );
    }
    assert_eq!(wire.calls(), 0);
}

#[test]
fn allowing_a_host_lets_calls_through_and_survives_a_restart() {
    let wire = Wire::ok("{\"ok\":true}");
    let mut hosts = HostList::new();
    let entry = hosts
        .allow(
            "Traces.Example.",
            None,
            "traces",
            "Server traces",
            1_700_000_000_000,
        )
        .unwrap();
    assert_eq!(entry.host, "traces.example");
    assert_eq!(entry.purpose, "Server traces");
    // The list a restarted app reads back.
    let hosts = HostList::from_json(&hosts.to_json()).unwrap();
    assert_eq!(hosts.list().len(), 1);
    let answer = run(
        &wire,
        &hosts,
        None,
        &[],
        &get("https://TRACES.example/api/t/1"),
    )
    .unwrap();
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, "{\"ok\":true}");
    assert_eq!(wire.calls(), 1);
    // Removing the entry takes the access away again.
    let mut hosts = hosts;
    assert!(hosts.remove("traces.example", None));
    assert!(!hosts.remove("traces.example", None));
    assert!(matches!(
        run(&wire, &hosts, None, &[], &get("https://traces.example/x")),
        Err(EgressError::NotAllowed { .. })
    ));
}

#[test]
fn a_port_is_part_of_the_permission_and_allowing_twice_keeps_one_entry() {
    let wire = Wire::ok("x");
    let mut hosts = allowed(&[("tempo.example", Some(3200))]);
    hosts
        .allow("tempo.example", Some(3200), "other", "Other", 2)
        .unwrap();
    assert_eq!(hosts.list().len(), 1);
    assert_eq!(hosts.list()[0].extension, "traces");
    assert!(
        run(
            &wire,
            &hosts,
            None,
            &[],
            &get("http://tempo.example:3200/x")
        )
        .is_ok()
    );
    assert!(
        run(
            &wire,
            &hosts,
            None,
            &[],
            &get("http://tempo.example:3201/x")
        )
        .is_err()
    );
    assert!(run(&wire, &hosts, None, &[], &get("http://tempo.example/x")).is_err());
    // An entry without a port is the https default, also written out; plain http needs 80.
    let hosts = allowed(&[("a.example", None), ("b.example", Some(80))]);
    assert!(run(&wire, &hosts, None, &[], &get("https://a.example:443/x")).is_ok());
    assert!(run(&wire, &hosts, None, &[], &get("http://b.example/x")).is_ok());
    assert!(run(&wire, &hosts, None, &[], &get("https://b.example/x")).is_err());
}

#[test]
fn the_host_file_is_per_device_and_a_broken_file_allows_nothing() {
    let dir = std::env::temp_dir().join(format!("rt-egress-hosts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(rt_app::egress::file::load(&dir).unwrap().list().is_empty());
    rt_app::egress::file::update(&dir, |l| {
        l.allow("traces.example", None, "traces", "Server traces", 5)
    })
    .unwrap();
    let again = rt_app::egress::file::load(&dir).unwrap();
    assert_eq!(again.list()[0].allowed_at, 5);
    assert!(dir.join("egress").join("hosts.json").is_file());
    // Not a list: an error, and `update` does not write over it.
    std::fs::write(dir.join("egress").join("hosts.json"), "{ not json").unwrap();
    assert!(rt_app::egress::file::load(&dir).is_err());
    assert!(
        rt_app::egress::file::update(&dir, |l| l.allow("a.example", None, "x", "y", 1)).is_err()
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("egress").join("hosts.json")).unwrap(),
        "{ not json"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hosts_that_are_not_host_names_are_not_added() {
    let mut hosts = HostList::new();
    for bad in [
        "",
        "https://a.example",
        "a.example/path",
        "a b",
        "user@a.example",
        "a.example:80",
    ] {
        assert!(hosts.allow(bad, None, "x", "y", 1).is_err(), "{bad:?}");
    }
    assert!(hosts.allow("[::1]", None, "x", "y", 1).is_ok());
    assert!(hosts.allow("127.0.0.1", Some(48706), "x", "y", 1).is_ok());
}

// ---- secrets: resolved in Rust, masked everywhere (canary) ----

fn secret_env() -> Vec<Environment> {
    let var = |name: &str, secret: bool| Variable {
        name: name.into(),
        value: if secret { None } else { Some("plain".into()) },
        secret,
        enabled: true,
    };
    vec![
        Environment {
            name: "dev".into(),
            variables: vec![var("trace_token", true), var("plain_one", false)],
            ..Environment::default()
        },
        Environment {
            name: "prod-like".into(),
            variables: vec![var("trace_token", true)],
            ..Environment::default()
        },
    ]
}

#[test]
fn a_secret_in_a_header_is_resolved_on_the_wire_and_masked_in_every_output() {
    // The server echoes the header back in the body, a header and an error-like text.
    let wire = Wire::new(|r| {
        let auth = r
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        (
            200,
            vec![("X-Echo".into(), auth.clone())],
            format!("{{\"you_sent\":\"{auth}\"}}").into_bytes(),
        )
    });
    let hosts = allowed(&[("traces.example", None)]);
    let req = with_header(
        get("https://traces.example/api/t/1"),
        "Authorization",
        "Bearer {{secret:trace_token}}",
    );
    assert_eq!(secret_names(&req), vec!["trace_token".to_string()]);
    let secrets = lookup_secrets(&secret_names(&req), &secret_env(), None, |env, var| {
        Ok((env == "dev" && var == "trace_token").then(|| CANARY.to_owned()))
    })
    .unwrap();
    assert_eq!(secrets, vec![("trace_token".into(), CANARY.into())]);
    let answer = run(&wire, &hosts, None, &secrets, &req).unwrap();
    // The value reached the wire ...
    let sent = wire.last();
    assert!(
        sent.headers
            .iter()
            .any(|(_, v)| v == &format!("Bearer {CANARY}"))
    );
    // ... and nowhere else: not in the answer, however it is written down.
    let json = serde_json::to_string(&answer).unwrap();
    assert!(!json.contains(CANARY), "{json}");
    assert!(answer.body.contains("Bearer ") && !answer.body.contains(CANARY));
    assert!(answer.headers.iter().all(|(_, v)| !v.contains(CANARY)));
    // The request as given still holds the reference, never the value.
    assert!(!serde_json::to_string(&req).unwrap().contains(CANARY));
}

#[test]
fn a_secret_is_masked_in_errors_and_in_what_the_guard_sees() {
    struct Failing;
    impl Transport for Failing {
        async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
            Err(TransportError::Connect(format!(
                "cannot reach {}",
                request.url
            )))
        }
    }
    let seen = Arc::new(Mutex::new(Vec::<SendCheck>::new()));
    struct Spy(Arc<Mutex<Vec<SendCheck>>>);
    impl SendGuard for Spy {
        fn check(&self, c: &SendCheck) -> GuardVerdict {
            self.0.lock().unwrap().push(c.clone());
            GuardVerdict::Allow
        }
    }
    let hosts = allowed(&[("traces.example", None)]);
    // A secret that also appears in the URL (a user's mistake) is masked in the error.
    let url = format!("https://traces.example/api?k={CANARY}");
    let req = with_header(get(&url), "X-Key", "{{secret:trace_token}}");
    let secrets = vec![("trace_token".to_owned(), CANARY.to_owned())];
    let spy = Spy(seen.clone());
    let ctx = Context {
        hosts: &hosts,
        guard: Some(&spy),
        caller: Caller::App,
        collection: "Orders",
        environment: None,
        secrets: &secrets,
    };
    let err = block_on(request(&Failing, &ctx, &req)).unwrap_err();
    let text = format!("{err} {err:?} {}", serde_json::to_string(&err).unwrap());
    assert!(!text.contains(CANARY), "{text}");
    assert!(matches!(err, EgressError::Transport { .. }));
    let checks = seen.lock().unwrap();
    assert_eq!(checks.len(), 1);
    assert!(!serde_json::to_string(&checks[0]).unwrap().contains(CANARY));
}

#[test]
fn a_secret_without_a_value_or_over_plain_http_is_refused_before_anything_is_sent() {
    let wire = Wire::ok("x");
    let hosts = allowed(&[("traces.example", None), ("localhost", Some(48706))]);
    let req = with_header(
        get("https://traces.example/x"),
        "Authorization",
        "{{secret:nope}}",
    );
    let err = run(&wire, &hosts, None, &[], &req).unwrap_err();
    assert!(matches!(&err, EgressError::Limit { message } if message.contains("nope")));
    let secrets = vec![("t".to_owned(), CANARY.to_owned())];
    let req = with_header(
        get("http://traces.example/x"),
        "Authorization",
        "{{secret:t}}",
    );
    let hosts_http = allowed(&[("traces.example", Some(80))]);
    let err = run(&wire, &hosts_http, None, &secrets, &req).unwrap_err();
    assert!(matches!(&err, EgressError::Limit { message } if message.contains("plain http")));
    assert!(!format!("{err:?}").contains(CANARY));
    // Loopback is fine, also over http.
    let req = with_header(
        get("http://localhost:48706/x"),
        "Authorization",
        "{{secret:t}}",
    );
    assert!(run(&wire, &hosts, None, &secrets, &req).is_ok());
    assert_eq!(wire.calls(), 1);
}

#[test]
fn secrets_are_found_per_environment_and_never_for_plain_variables() {
    let names = vec![
        "trace_token".to_owned(),
        "plain_one".to_owned(),
        "missing".to_owned(),
    ];
    let reads = Mutex::new(Vec::<String>::new());
    let get = |env: &str, var: &str| -> Result<Option<String>, AppError> {
        reads.lock().unwrap().push(format!("{env}/{var}"));
        Ok(Some(format!("v-{env}-{var}")))
    };
    // An environment asked for: only its values.
    let found = lookup_secrets(&names, &secret_env(), Some("prod-like"), get).unwrap();
    assert_eq!(
        found,
        vec![(
            "trace_token".to_owned(),
            "v-prod-like-trace_token".to_owned()
        )]
    );
    // None asked for: the first environment with a value.
    reads.lock().unwrap().clear();
    let found = lookup_secrets(&names, &secret_env(), None, get).unwrap();
    assert_eq!(
        found,
        vec![("trace_token".to_owned(), "v-dev-trace_token".to_owned())]
    );
    // The plain variable and the unknown name were never read from the store.
    assert!(
        reads
            .lock()
            .unwrap()
            .iter()
            .all(|r| r.ends_with("trace_token"))
    );
}

// ---- limits ----

#[test]
fn only_get_and_post_with_a_small_body_and_a_plain_http_url() {
    let wire = Wire::ok("x");
    let hosts = allowed(&[("traces.example", None)]);
    let post = |body: String| EgressRequest {
        method: EgressMethod::Post,
        body: Some(body),
        ..get("https://traces.example/q")
    };
    assert!(run(&wire, &hosts, None, &[], &post("x".repeat(MAX_BODY))).is_ok());
    let err = run(&wire, &hosts, None, &[], &post("x".repeat(MAX_BODY + 1))).unwrap_err();
    assert!(matches!(err, EgressError::Limit { .. }));
    // A GET with a body, other schemes, credentials in the URL, a header that smuggles a line.
    let mut with_body = get("https://traces.example/q");
    with_body.body = Some("x".into());
    assert!(matches!(
        run(&wire, &hosts, None, &[], &with_body),
        Err(EgressError::Limit { .. })
    ));
    for url in [
        "ftp://traces.example/x",
        "file:///etc/passwd",
        "https://u:p@traces.example/x",
        "not a url",
    ] {
        assert!(
            matches!(
                run(&wire, &hosts, None, &[], &get(url)),
                Err(EgressError::Limit { .. })
            ),
            "{url}"
        );
    }
    let smuggle = with_header(get("https://traces.example/q"), "X-A", "1\r\nX-B: 2");
    assert!(matches!(
        run(&wire, &hosts, None, &[], &smuggle),
        Err(EgressError::Limit { .. })
    ));
    // The JSON of the request only knows GET and POST.
    assert!(
        serde_json::from_str::<EgressRequest>(
            r#"{"method":"DELETE","url":"https://a/","extension":"e","purpose":"p"}"#
        )
        .is_err()
    );
    assert_eq!(wire.calls(), 1);
}

#[test]
fn the_call_goes_out_without_following_redirects_and_with_the_time_limit() {
    let wire = Wire::new(|_| {
        (
            302,
            vec![("Location".into(), "https://evil.example/".into())],
            Vec::new(),
        )
    });
    let hosts = allowed(&[("traces.example", None)]);
    let answer = run(&wire, &hosts, None, &[], &get("https://traces.example/old")).unwrap();
    // The 3xx comes back as it is.
    assert_eq!(answer.status, 302);
    assert!(
        answer
            .headers
            .iter()
            .any(|(n, v)| n == "Location" && v.contains("evil.example"))
    );
    let sent = wire.last();
    assert!(!sent.options.follow_redirects);
    assert_eq!(sent.options.timeout, Some(Duration::from_secs(30)));
    assert!(sent.options.client_identity.is_none());
    assert_eq!(wire.calls(), 1);
}

#[test]
fn a_response_over_the_limit_is_cut_and_says_so() {
    let big = vec![b'a'; MAX_RESPONSE + 10_000];
    let wire = Wire::new(move |_| (200, Vec::new(), big.clone()));
    let hosts = allowed(&[("traces.example", None)]);
    let answer = run(&wire, &hosts, None, &[], &get("https://traces.example/big")).unwrap();
    assert_eq!(answer.body.len(), MAX_RESPONSE);
    assert!(answer.truncated);
    let small = Wire::ok("short");
    assert!(
        !run(&small, &hosts, None, &[], &get("https://traces.example/s"))
            .unwrap()
            .truncated
    );
}

#[test]
fn a_secret_cut_by_the_size_limit_is_still_masked() {
    // The secret straddles the limit: half of it before, half after.
    let mut body = vec![b'a'; MAX_RESPONSE - 10];
    body.extend_from_slice(CANARY.as_bytes());
    body.extend_from_slice(&[b'b'; 100]);
    let wire = Wire::new(move |_| (200, Vec::new(), body.clone()));
    let hosts = allowed(&[("traces.example", None)]);
    let secrets = vec![("t".to_owned(), CANARY.to_owned())];
    let answer = run(
        &wire,
        &hosts,
        None,
        &secrets,
        &get("https://traces.example/x"),
    )
    .unwrap();
    assert!(
        !answer.body.contains(&CANARY[..8]),
        "a prefix of the secret is left"
    );
    assert!(answer.truncated);
}

// ---- the guard ----

struct Recorder(Mutex<Vec<SendCheck>>);

impl SendGuard for Recorder {
    fn check(&self, c: &SendCheck) -> GuardVerdict {
        self.0.lock().unwrap().push(c.clone());
        GuardVerdict::Allow
    }
}

struct AirGap;

impl SendGuard for AirGap {
    fn check(&self, c: &SendCheck) -> GuardVerdict {
        match c.purpose {
            Purpose::Egress => GuardVerdict::Refuse {
                rule: "air-gap".into(),
                reason: "no outbound calls in this mode".into(),
            },
            Purpose::Main | Purpose::Script | Purpose::Audit => GuardVerdict::Allow,
        }
    }
}

#[test]
fn every_call_goes_through_the_guard_with_the_egress_purpose() {
    let wire = Wire::ok("x");
    let hosts = allowed(&[("traces.example", None)]);
    let spy = Recorder(Mutex::new(Vec::new()));
    for _ in 0..3 {
        run(
            &wire,
            &hosts,
            Some(&spy),
            &[],
            &get("https://traces.example/a?b=1"),
        )
        .unwrap();
    }
    let post = EgressRequest {
        method: EgressMethod::Post,
        body: Some("{}".into()),
        ..get("https://traces.example/q")
    };
    run(&wire, &hosts, Some(&spy), &[], &post).unwrap();
    let checks = spy.0.lock().unwrap();
    assert_eq!(checks.len(), 4, "one question per call");
    assert!(checks.iter().all(|c| c.purpose == Purpose::Egress));
    assert_eq!(checks[0].method, "GET");
    assert_eq!(checks[0].effect, Effect::Read);
    assert_eq!(checks[0].url, "https://traces.example/a?b=1");
    assert_eq!(checks[0].collection, "Orders");
    assert_eq!(checks[0].request_path, "traces");
    assert_eq!(checks[0].caller, Caller::App);
    assert!(!checks[0].production);
    assert_eq!(
        (checks[3].method.as_str(), checks[3].effect),
        ("POST", Effect::Write)
    );
}

#[test]
fn a_guard_that_refuses_wins_and_nothing_goes_out() {
    let wire = Wire::ok("x");
    let hosts = allowed(&[("traces.example", None)]);
    let err = run(
        &wire,
        &hosts,
        Some(&AirGap),
        &[],
        &get("https://traces.example/a"),
    )
    .unwrap_err();
    assert_eq!(
        err,
        EgressError::Blocked {
            rule: "air-gap".into(),
            reason: "no outbound calls in this mode".into()
        }
    );
    assert_eq!(wire.calls(), 0);
    // A host that is not allowed is refused before the guard is asked: the guard only sees
    // calls that would go out.
    let spy = Recorder(Mutex::new(Vec::new()));
    let _ = run(
        &wire,
        &HostList::new(),
        Some(&spy),
        &[],
        &get("https://other.example/"),
    );
    assert!(spy.0.lock().unwrap().is_empty());
}

// ---- the real transport: redirects, size and time (rt-testserver) ----

mod native {
    use super::*;
    use rt_core::transport::native::NativeTransport;

    fn on_server<F: Future<Output = ()>>(f: impl FnOnce(String) -> F) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let port = rt_testserver::spawn_local().await.port();
            f(format!("http://127.0.0.1:{port}")).await;
        });
    }

    async fn call(
        base: &str,
        path: &str,
        headers: Vec<(String, String)>,
        secrets: &[(String, String)],
    ) -> Result<rt_app::egress::EgressResponse, EgressError> {
        let host = base.trim_start_matches("http://");
        let (h, p) = host.split_once(':').unwrap();
        let hosts = allowed(&[(h, Some(p.parse().unwrap()))]);
        let ctx = Context {
            hosts: &hosts,
            guard: None,
            caller: Caller::App,
            collection: "",
            environment: None,
            secrets,
        };
        let mut req = get(&format!("{base}{path}"));
        req.headers = headers;
        request(&NativeTransport::new().unwrap(), &ctx, &req).await
    }

    #[test]
    fn a_redirect_comes_back_and_is_not_followed() {
        on_server(|base| async move {
            let answer = call(&base, "/redirect/3", Vec::new(), &[]).await.unwrap();
            assert_eq!(answer.status, 302);
            assert!(
                answer
                    .headers
                    .iter()
                    .any(|(n, v)| n.eq_ignore_ascii_case("location") && v == "/redirect/2")
            );
        });
    }

    #[test]
    fn the_header_secret_reaches_the_server_and_is_masked_in_what_comes_back() {
        on_server(|base| async move {
            let secrets = vec![("t".to_owned(), CANARY.to_owned())];
            let answer = call(
                &base,
                "/echo/x",
                vec![("X-Api-Key".into(), "{{secret:t}}".into())],
                &secrets,
            )
            .await
            .unwrap();
            // The echo lists the request headers: the value was sent and is masked here.
            assert!(answer.body.contains("x-api-key"), "{}", answer.body);
            assert!(!answer.body.contains(CANARY));
        });
    }

    #[test]
    fn a_big_response_is_cut_at_four_mebibytes() {
        on_server(|base| async move {
            let answer = call(
                &base,
                &format!("/bytes/{}", MAX_RESPONSE * 2),
                Vec::new(),
                &[],
            )
            .await
            .unwrap();
            assert!(answer.truncated);
            // Octet bytes become replacement characters; the cut is on a character.
            assert!(answer.body.len() <= MAX_RESPONSE && answer.body.len() > MAX_RESPONSE - 4);
        });
    }
}
