// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The two-user access check (wave 19, strand D): a fake orders API with owners answers as user
//! A (the owner) and user B (somebody else), and the check must say `ok`, `broken-object-access`,
//! `broken-function-access`, `inconclusive` or `skipped`. Requests are counted at the transport,
//! so what the tests say about reads, writes, production and enumeration is what went out.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::access::{
    AccessCheck, AccessEnv, AccessOptions, AccessProfile, AccessVerdict, EVIDENCE_LIMIT, check,
    check_with,
};
use rt_app::agents::{AuditEntry, AuditOutcome};
use rt_app::envcompare::EnvRun;
use rt_app::runner::RunSnapshot;
use rt_core::guard::{GuardVerdict, SendCheck, SendGuard};
use rt_core::model::{
    Collection, Environment, Item, KeyValue, RequestEntry, RequestSpec, Variable,
};
use rt_core::ratelimit::RateTracker;
use rt_core::send::SendOptions;
use rt_core::services::{LocalBoxFuture, Services};
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::FileSet;

const CANARY_A: &str = "RT-CANARY-access-secret-A-91c4";
const CANARY_B: &str = "RT-CANARY-access-secret-B-77e0";
const PERSONAL: &str = "alice.canary@example.test";

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

/// What the API does with somebody else's object.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Other {
    /// The bug: it answers.
    Leak,
    Forbid,
    Hide,
    Crash,
    /// A success answer that is not the object.
    Decoy,
}

type Rule = Box<dyn Fn(&HttpRequest, usize) -> (u16, Vec<(String, String)>, String) + Send + Sync>;

struct Server {
    rule: Rule,
    seen: Mutex<Vec<HttpRequest>>,
}

impl Server {
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
    fn header(&self, call: usize, name: &str) -> Option<String> {
        header(&self.seen.lock().unwrap()[call], name)
    }
}

impl Transport for Server {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let n = {
            let mut seen = self.seen.lock().unwrap();
            seen.push(request.clone());
            seen.len()
        };
        let (status, mut headers, body) = (self.rule)(&request, n);
        headers.push(("Content-Type".into(), "application/json".into()));
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(body))));
        Ok(Response {
            head: ResponseHead {
                status,
                version: HttpVersion::Http11,
                headers,
                url: request.url.clone(),
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

fn header(r: &HttpRequest, name: &str) -> Option<String> {
    r.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

fn path_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.find('/').map_or("", |i| &rest[i..]);
    path.split('?').next().unwrap_or("").to_owned()
}

/// Orders 1 (alice) and 2 (bob); `/admin/orders` is for `admin`; `/health` for everybody.
fn api(other: Other) -> Server {
    Server {
        seen: Mutex::new(Vec::new()),
        rule: Box::new(move |r, _| {
            let user = header(r, "x-user").unwrap_or_default();
            let path = path_of(&r.url);
            let json = |status: u16, body: String| (status, Vec::new(), body);
            if path == "/health" {
                return json(200, "{\"ok\":true}".into());
            }
            if path == "/admin/orders" {
                return if user == "admin" || other == Other::Leak {
                    json(200, "{\"cleared\":true}".into())
                } else {
                    json(403, "{\"error\":\"forbidden\"}".into())
                };
            }
            if let Some(id) = path.strip_prefix("/orders/") {
                let owner = match id {
                    "1" => "alice",
                    "2" => "bob",
                    _ => return json(404, "{\"error\":\"not found\"}".into()),
                };
                let order = format!(
                    "{{\"id\":{id},\"owner\":\"{owner}\",\"email\":\"{PERSONAL}\",\"item\":\"book\",\"echo\":\"{}\"}}",
                    header(r, "authorization").unwrap_or_default()
                );
                if owner == user {
                    return json(200, order);
                }
                return match other {
                    Other::Leak => json(200, order),
                    Other::Forbid => json(403, "{\"error\":\"forbidden\"}".into()),
                    Other::Hide => json(404, "{\"error\":\"not found\"}".into()),
                    Other::Crash => json(500, "{\"error\":\"boom\"}".into()),
                    Other::Decoy => json(200, "{\"message\":\"welcome\"}".into()),
                };
            }
            json(404, "{\"error\":\"not found\"}".into())
        }),
    }
}

fn spec(method: &str, url: &str) -> RequestSpec {
    RequestSpec {
        name: "r".into(),
        method: method.into(),
        url: url.into(),
        headers: vec![
            KeyValue::new("x-user", "{{user}}"),
            KeyValue::new("Authorization", "Bearer {{token}}"),
        ],
        ..RequestSpec::default()
    }
}

fn environment(name: &str, user: &str, order: &str, token: &str) -> Environment {
    Environment {
        name: name.into(),
        variables: vec![
            Variable::new("base", "http://orders.test"),
            Variable::new("user", user),
            Variable::new("orderId", order),
            Variable {
                name: "token".into(),
                value: Some(token.into()),
                enabled: true,
                secret: true,
            },
        ],
        client_certificates: vec![],
        external_secrets: None,
    }
}

fn collection(spec: RequestSpec) -> Collection {
    Collection {
        name: "orders".into(),
        items: vec![Item::Request(RequestEntry {
            path: "r.yml".into(),
            spec,
        })],
        environments: vec![
            environment("A", "alice", "1", CANARY_A),
            environment("B", "bob", "2", CANARY_B),
            environment("PRD", "bob", "2", CANARY_B),
            environment("admin", "admin", "1", CANARY_A),
        ],
        ..Collection::default()
    }
}

fn profile(a: &str, b: &str) -> AccessProfile {
    AccessProfile {
        a: a.into(),
        b: b.into(),
    }
}

fn options() -> AccessOptions {
    AccessOptions {
        rate: 100_000,
        ..AccessOptions::default()
    }
}

struct Ran {
    check: AccessCheck,
    audit: Vec<AuditEntry>,
}

fn run_with(
    spec: RequestSpec,
    server: &Server,
    profile: &AccessProfile,
    options: &AccessOptions,
    services: &Services<'_>,
) -> Result<Ran, rt_app::AppError> {
    let c = collection(spec);
    let snap = |env: &str| RunSnapshot {
        collection: c.clone(),
        environment: Some(env.into()),
        vars: Vec::new(),
        files: FileSet::default(),
    };
    let a = EnvRun {
        snapshot: snap(&profile.a),
        services: Services { ..*services },
    };
    let b = EnvRun {
        snapshot: snap(&profile.b),
        services: Services { ..*services },
    };
    let audit: Mutex<Vec<AuditEntry>> = Mutex::new(Vec::new());
    let sink = |e: &AuditEntry| audit.lock().unwrap().push(e.clone());
    let env = AccessEnv {
        a: &a,
        b: &b,
        transport: server,
        options: &SendOptions::default(),
        caller: "test",
        audit: &sink,
        cancelled: &|| false,
    };
    let check = block_on(check(&env, "r.yml", profile, options))?;
    Ok(Ran {
        check,
        audit: audit.into_inner().unwrap(),
    })
}

fn run(spec: RequestSpec, server: &Server, opts: &AccessOptions) -> Ran {
    run_with(spec, server, &profile("A", "B"), opts, &Services::none()).unwrap()
}

fn read() -> RequestSpec {
    spec("GET", "{{base}}/orders/{{orderId}}")
}

// ---------------------------------------------------------------------------------------------
// Verdicts

#[test]
fn b_reading_a_s_object_is_broken_object_access() {
    let server = api(Other::Leak);
    let ran = run(read(), &server, &options());
    assert_eq!(ran.check.verdict, AccessVerdict::BrokenObjectAccess);
    assert!(ran.check.verdict.is_broken());
    assert_eq!(
        (ran.check.status_a, ran.check.status_b),
        (Some(200), Some(200))
    );
    // A's id went into B's run, B's own identity stayed.
    assert_eq!(server.calls(), 2);
    assert_eq!(server.urls()[0], "http://orders.test/orders/1");
    assert_eq!(server.urls()[1], "http://orders.test/orders/1");
    assert_eq!(server.header(1, "x-user").as_deref(), Some("bob"));
    assert_eq!(
        server.header(1, "authorization").as_deref(),
        Some(format!("Bearer {CANARY_B}").as_str())
    );
    let finding = &ran.check.findings[0];
    assert_eq!(finding.verdict, AccessVerdict::BrokenObjectAccess);
    assert_eq!(
        finding.message,
        "B received object 1 that A's request returned"
    );
    let profiles: Vec<&str> = finding
        .evidence
        .iter()
        .map(|e| e.profile.as_str())
        .collect();
    assert_eq!(profiles, ["A", "B"]);
}

#[test]
fn b_being_denied_is_ok() {
    for (other, status) in [(Other::Forbid, 403), (Other::Hide, 404)] {
        let server = api(other);
        let ran = run(read(), &server, &options());
        assert_eq!(ran.check.verdict, AccessVerdict::Ok, "{status}");
        assert_eq!(ran.check.status_b, Some(status));
        assert!(!ran.check.verdict.is_broken());
    }
}

#[test]
fn what_says_neither_yes_nor_no_is_inconclusive() {
    // B crashes, B gets a success answer that is not the object.
    for other in [Other::Crash, Other::Decoy] {
        let server = api(other);
        let ran = run(read(), &server, &options());
        assert_eq!(ran.check.verdict, AccessVerdict::Inconclusive);
        assert!(!ran.check.verdict.is_broken());
        assert_eq!(server.calls(), 2);
    }
}

#[test]
fn without_a_reference_answer_from_a_nothing_is_compared() {
    // A's order does not exist (id 9): A gets 404, B is not sent.
    let mut s = read();
    s.url = "{{base}}/orders/9".into();
    let server = api(Other::Leak);
    let ran = run(s, &server, &options());
    assert_eq!(ran.check.verdict, AccessVerdict::Inconclusive);
    assert_eq!(ran.check.status_a, Some(404));
    assert_eq!(ran.check.status_b, None);
    assert_eq!(server.calls(), 1);
}

#[test]
fn no_object_reference_found_is_said_and_not_guessed() {
    let server = api(Other::Leak);
    let ran = run(spec("GET", "{{base}}/health"), &server, &options());
    assert_eq!(
        ran.check.verdict,
        AccessVerdict::Skipped {
            reason: "no object reference found".into()
        }
    );
    assert_eq!(server.calls(), 1, "B is not sent without an object");
}

#[test]
fn an_id_in_the_url_that_the_answer_names_is_a_reference() {
    let server = api(Other::Leak);
    let ran = run(spec("GET", "{{base}}/orders/1"), &server, &options());
    assert_eq!(ran.check.verdict, AccessVerdict::BrokenObjectAccess);
    assert_eq!(server.calls(), 2);
}

#[test]
fn a_function_that_b_must_be_denied_is_checked_without_ids() {
    let opts = AccessOptions {
        b_must_be_denied: true,
        ..options()
    };
    let mut s = spec("GET", "{{base}}/admin/orders");
    s.name = "admin".into();
    // The bug: B may call it.
    let ran = run_with(
        s.clone(),
        &api(Other::Leak),
        &profile("admin", "B"),
        &opts,
        &Services::none(),
    )
    .unwrap();
    assert_eq!(ran.check.verdict, AccessVerdict::BrokenFunctionAccess);
    assert_eq!(
        ran.check.findings[0].verdict,
        AccessVerdict::BrokenFunctionAccess
    );
    // Fixed: B gets 403.
    let ran = run_with(
        s.clone(),
        &api(Other::Forbid),
        &profile("admin", "B"),
        &opts,
        &Services::none(),
    )
    .unwrap();
    assert_eq!(ran.check.verdict, AccessVerdict::Ok);
    // Without the switch there is no object and nothing is compared.
    let ran = run_with(
        s,
        &api(Other::Leak),
        &profile("admin", "B"),
        &options(),
        &Services::none(),
    )
    .unwrap();
    assert!(matches!(ran.check.verdict, AccessVerdict::Skipped { .. }));
}

// ---------------------------------------------------------------------------------------------
// Writes, production, guard

fn put() -> RequestSpec {
    spec("PUT", "{{base}}/orders/{{orderId}}")
}

#[test]
fn a_writing_request_only_runs_with_permission() {
    let server = api(Other::Leak);
    let ran = run(put(), &server, &options());
    assert_eq!(
        ran.check.verdict,
        AccessVerdict::Skipped {
            reason: "skipped (writes)".into()
        }
    );
    assert_eq!(server.calls(), 0, "nothing was sent");

    let allowed = AccessOptions {
        allow_writes: true,
        ..options()
    };
    let server = api(Other::Leak);
    // The fake API answers every method on the order; the unknown PUT is a read to it.
    let ran = run(put(), &server, &allowed);
    assert_eq!(ran.check.verdict, AccessVerdict::BrokenObjectAccess);
    assert_eq!(server.calls(), 2);
    assert_eq!(ran.audit[0].approved_by.as_deref(), Some("test"));
}

#[test]
fn a_writing_request_never_runs_against_production() {
    let allowed = AccessOptions {
        allow_writes: true,
        ..options()
    };
    for p in [profile("A", "PRD"), profile("PRD", "B")] {
        let server = api(Other::Leak);
        let ran = run_with(put(), &server, &p, &allowed, &Services::none()).unwrap();
        assert_eq!(
            ran.check.verdict,
            AccessVerdict::Skipped {
                reason: "skipped (production)".into()
            }
        );
        assert_eq!(server.calls(), 0);
    }
    // A read may.
    let server = api(Other::Leak);
    let ran = run_with(
        read(),
        &server,
        &profile("A", "PRD"),
        &options(),
        &Services::none(),
    )
    .unwrap();
    assert_eq!(ran.check.verdict, AccessVerdict::BrokenObjectAccess);
}

struct Refuse;

impl SendGuard for Refuse {
    fn check(&self, _: &SendCheck) -> GuardVerdict {
        GuardVerdict::Refuse {
            rule: "no-access".into(),
            reason: "not today".into(),
        }
    }
}

#[test]
fn the_send_guard_answers_for_every_request() {
    let server = api(Other::Leak);
    let guard = Refuse;
    let services = Services {
        guard: Some(&guard),
        ..Services::none()
    };
    let ran = run_with(read(), &server, &profile("A", "B"), &options(), &services).unwrap();
    assert!(matches!(ran.check.verdict, AccessVerdict::Skipped { .. }));
    assert!(ran.check.findings[0].message.contains("no-access"));
    assert_eq!(server.calls(), 0, "nothing went out");
    assert_eq!(ran.audit[0].outcome, AuditOutcome::Refused);
    assert_eq!(ran.audit[0].rule.as_deref(), Some("no-access"));
}

/// The guard lets A through and refuses B.
struct RefuseUser(&'static str);

impl SendGuard for RefuseUser {
    fn check(&self, c: &SendCheck) -> GuardVerdict {
        if c.environment.as_deref() == Some(self.0) {
            GuardVerdict::Refuse {
                rule: "no-b".into(),
                reason: "no".into(),
            }
        } else {
            GuardVerdict::Allow
        }
    }
}

#[test]
fn a_refusal_for_b_leaves_a_skipped_check() {
    let server = api(Other::Leak);
    let guard = RefuseUser("B");
    let services = Services {
        guard: Some(&guard),
        ..Services::none()
    };
    let ran = run_with(read(), &server, &profile("A", "B"), &options(), &services).unwrap();
    assert!(matches!(ran.check.verdict, AccessVerdict::Skipped { .. }));
    assert_eq!(server.calls(), 1, "A went out, B did not");
}

#[test]
fn the_profiles_must_be_two_known_environments() {
    let server = api(Other::Leak);
    for p in [profile("A", "A"), profile("A", "nope")] {
        assert!(run_with(read(), &server, &p, &options(), &Services::none()).is_err());
    }
    assert_eq!(server.calls(), 0);
}

// ---------------------------------------------------------------------------------------------
// No secrets, no personal data, no enumeration

#[test]
fn neither_secret_nor_personal_data_appears_in_any_output() {
    for other in [Other::Leak, Other::Forbid, Other::Decoy] {
        let server = api(other);
        let ran = run(read(), &server, &options());
        let all = format!(
            "{}\n{}",
            serde_json::to_string(&ran.check).unwrap(),
            serde_json::to_string(&ran.audit).unwrap()
        );
        for canary in [CANARY_A, CANARY_B, PERSONAL, "alice@"] {
            assert!(!all.contains(canary), "{canary} leaked: {all}");
        }
    }
    // The evidence shows paths and types, and an id that reads as one.
    let ran = run(read(), &api(Other::Leak), &options());
    let b = &ran.check.findings[0].evidence[1];
    assert!(
        b.values.contains(&("$.id".into(), "1".into())),
        "{:?}",
        b.values
    );
    assert!(b.values.contains(&("$.email".into(), "string".into())));
    assert!(b.header_names.contains(&"content-type".to_owned()));
}

#[test]
fn evidence_is_bounded() {
    // A body with a huge number of fields and a long id: capped, id not shown.
    let long = "x".repeat(2000);
    let server = Server {
        seen: Mutex::new(Vec::new()),
        rule: Box::new(move |_, _| {
            let mut body = format!("{{\"id\":\"{long}\"");
            for i in 0..300 {
                body.push_str(&format!(",\"field{i}\":\"{long}\""));
            }
            body.push('}');
            (200, Vec::new(), body)
        }),
    };
    let ran = run(read(), &server, &options());
    for e in ran.check.findings.iter().flat_map(|f| &f.evidence) {
        assert!(e.values.len() <= 24);
        for (path, value) in &e.values {
            assert!(path.len() <= EVIDENCE_LIMIT && value.len() <= EVIDENCE_LIMIT);
            assert!(!value.contains("xxxx"), "a long value is not shown");
        }
    }
}

#[test]
fn only_the_ids_a_saw_are_used_and_a_check_sends_at_most_two_requests() {
    let server = api(Other::Hide);
    let ran = run(read(), &server, &options());
    assert_eq!(ran.check.verdict, AccessVerdict::Ok);
    assert_eq!(server.calls(), 2);
    assert_eq!(server.urls(), ["http://orders.test/orders/1"; 2]);
}

#[test]
fn one_audit_entry_per_check() {
    let ran = run(read(), &api(Other::Leak), &options());
    assert_eq!(ran.audit.len(), 1);
    let e = &ran.audit[0];
    assert_eq!(e.tool, "access-check");
    assert_eq!(e.request_path.as_deref(), Some("r.yml"));
    assert_eq!(e.environment.as_deref(), Some("A vs B"));
    assert!(e.detail.contains("broken-object-access"));
    assert_eq!(e.approved_by, None, "a read needs no approval");
}

// ---------------------------------------------------------------------------------------------
// Rate

fn fake_tracker() -> (RateTracker, Arc<AtomicU64>) {
    let clock = Arc::new(AtomicU64::new(1_000));
    let c = clock.clone();
    (
        RateTracker::with_clock(Arc::new(move || c.load(Ordering::SeqCst))),
        clock,
    )
}

fn timed_check(
    server: &Server,
    opts: &AccessOptions,
    clock: &Arc<AtomicU64>,
    tracker: &RateTracker,
) -> (AccessCheck, u64) {
    let c = collection(read());
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
        transport: server,
        options: &SendOptions::default(),
        caller: "test",
        audit: &|_| {},
        cancelled: &|| false,
    };
    let slept = std::cell::Cell::new(0u64);
    let sleep = |ms: u64| -> LocalBoxFuture<'static, ()> {
        slept.set(slept.get() + ms);
        clock.fetch_add(ms, Ordering::SeqCst);
        Box::pin(async {})
    };
    let check = block_on(check_with(
        &env,
        "r.yml",
        &profile("A", "B"),
        opts,
        tracker,
        &sleep,
    ))
    .unwrap();
    (check, slept.get())
}

#[test]
fn the_two_requests_keep_the_rate() {
    let (tracker, clock) = fake_tracker();
    let (check, slept) = timed_check(
        &api(Other::Leak),
        &AccessOptions::default(),
        &clock,
        &tracker,
    );
    assert_eq!(check.verdict, AccessVerdict::BrokenObjectAccess);
    // 5 requests per second: 200 ms between A and B.
    assert_eq!(slept, 200);
}

#[test]
fn a_host_that_ran_out_is_waited_for() {
    let (tracker, clock) = fake_tracker();
    let server = Server {
        seen: Mutex::new(Vec::new()),
        rule: Box::new(|r, n| {
            let body = format!("{{\"id\":1,\"call\":{n},\"url\":\"{}\"}}", r.url.len());
            let headers = if n == 1 {
                vec![
                    ("RateLimit-Remaining".to_owned(), "0".to_owned()),
                    ("RateLimit-Reset".to_owned(), "3".to_owned()),
                ]
            } else {
                Vec::new()
            };
            (200, headers, body)
        }),
    };
    let (check, slept) = timed_check(&server, &options(), &clock, &tracker);
    assert_eq!(server.calls(), 2);
    assert!(slept >= 2_900, "B waited for the host: {slept} ms");
    assert!(matches!(
        check.verdict,
        AccessVerdict::BrokenObjectAccess | AccessVerdict::Inconclusive
    ));
}
