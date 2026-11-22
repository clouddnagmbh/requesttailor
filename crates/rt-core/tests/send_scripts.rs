// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 7 interface: where `send_with` calls the script runner and what it does with the
//! outcome. A fake runner stands in for `rt-script`.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_core::model::{
    Collection, Environment, Item, RequestDefaults, RequestEntry, RequestSpec, Script, ScriptPhase,
    VarScope, Variable,
};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::script::{
    Flow, LogLevel, ScriptCall, ScriptLog, ScriptOutcome, TestResult, VarChange,
};
use rt_core::send::{MASK, SendError, SendOptions, send_with};
use rt_core::services::{LocalBoxFuture, ScriptRunner, Services};
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

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

#[derive(Default)]
struct Wire {
    seen: RefCell<Vec<HttpRequest>>,
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let url = request.url.clone();
        self.seen.borrow_mut().push(request);
        let body: BodyStream = Box::pin(Once(Some(Bytes::from_static(b"{\"id\":7}"))));
        Ok(Response {
            head: ResponseHead {
                status: 201,
                version: HttpVersion::Http11,
                headers: vec![("content-type".into(), "application/json".into())],
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

/// One recorded call: phase, script origins, response status.
type Call = (ScriptPhase, Vec<String>, Option<u16>);

/// Records every call; answers with a fixed outcome per phase.
#[derive(Default)]
struct Fake {
    calls: RefCell<Vec<Call>>,
    before: RefCell<ScriptOutcome>,
}

impl ScriptRunner for Fake {
    fn run<'a>(&'a self, call: ScriptCall<'a>) -> LocalBoxFuture<'a, ScriptOutcome> {
        Box::pin(async move {
            self.calls.borrow_mut().push((
                call.phase,
                call.scripts.iter().map(|s| s.origin.clone()).collect(),
                call.response.as_ref().map(|r| r.status),
            ));
            match call.phase {
                ScriptPhase::BeforeRequest => self.before.borrow().clone(),
                ScriptPhase::AfterResponse => ScriptOutcome {
                    var_changes: vec![VarChange {
                        name: "id".into(),
                        value: Some("7".into()),
                        scope: VarScope::Runtime,
                    }],
                    ..Default::default()
                },
                ScriptPhase::Tests => {
                    // The after-response change is visible to tests.
                    let seen = call
                        .variables
                        .iter()
                        .any(|v| v.name == "id" && v.value == "7");
                    ScriptOutcome {
                        tests: vec![TestResult {
                            name: "sees id".into(),
                            passed: seen,
                            error: None,
                            origin: "request".into(),
                        }],
                        logs: vec![ScriptLog {
                            level: LogLevel::Log,
                            text: "key is s3cr3t".into(),
                            origin: "request".into(),
                        }],
                        ..Default::default()
                    }
                }
            }
        })
    }
}

fn script(phase: ScriptPhase) -> Script {
    Script {
        phase,
        code: "/* fake */".into(),
    }
}

fn collection() -> Collection {
    Collection {
        name: "c".into(),
        defaults: RequestDefaults {
            scripts: vec![script(ScriptPhase::BeforeRequest)],
            ..Default::default()
        },
        items: vec![Item::Request(RequestEntry {
            path: "r.yml".into(),
            spec: RequestSpec {
                name: "r".into(),
                method: "GET".into(),
                url: "http://h/{{path}}".into(),
                scripts: vec![
                    script(ScriptPhase::BeforeRequest),
                    script(ScriptPhase::AfterResponse),
                    script(ScriptPhase::Tests),
                ],
                ..Default::default()
            },
        })],
        environments: vec![Environment {
            name: "E".into(),
            variables: vec![Variable {
                name: "key".into(),
                value: Some("s3cr3t".into()),
                enabled: true,
                secret: true,
            }],
            client_certificates: Vec::new(),
            external_secrets: None,
        }],
        config: Default::default(),
    }
}

fn input() -> PrepareInput<'static> {
    PrepareInput {
        environment: Some("E"),
        globals: Vec::new(),
        runtime: Vec::new(),
    }
}

fn services(fake: &Fake) -> Services<'_> {
    Services {
        scripts: Some(fake),
        ..Services::none()
    }
}

#[test]
fn phases_in_order_with_changes_applied_and_masked() {
    let fake = Fake::default();
    *fake.before.borrow_mut() = ScriptOutcome {
        request: Some(rt_core::script::ScriptRequest {
            method: "POST".into(),
            url: "http://h/{{path}}".into(),
            headers: vec![("X-Key".into(), "{{key}}".into())],
            body: Some("{\"a\":1}".into()),
        }),
        var_changes: vec![VarChange {
            name: "path".into(),
            value: Some("items".into()),
            scope: VarScope::Runtime,
        }],
        ..Default::default()
    };
    let wire = Wire::default();
    let c = collection();
    let report = block_on(send_with(
        &c,
        "r.yml",
        &input(),
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &services(&fake),
    ));
    assert_eq!(report.error, None);
    let calls = fake.calls.borrow();
    assert_eq!(
        *calls,
        vec![
            (
                ScriptPhase::BeforeRequest,
                vec!["collection".to_owned(), "request".to_owned()],
                None
            ),
            (
                ScriptPhase::AfterResponse,
                vec!["request".to_owned()],
                Some(201)
            ),
            (ScriptPhase::Tests, vec!["request".to_owned()], Some(201)),
        ]
    );
    // The script's request went out, resolved with the variable it set.
    let seen = wire.seen.borrow();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].url, "http://h/items");
    assert!(seen[0].headers.contains(&("X-Key".into(), "s3cr3t".into())));
    // Report: tests, masked logs and headers, variable changes in order.
    assert_eq!(report.tests.len(), 1);
    assert!(report.tests[0].passed);
    assert_eq!(report.logs[0].text, format!("key is {MASK}"));
    let sent = report.request.unwrap();
    assert!(sent.headers.contains(&("X-Key".into(), MASK.into())));
    let names: Vec<_> = report.var_changes.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["path", "id"]);
}

#[test]
fn a_failing_before_request_script_stops_the_send() {
    let fake = Fake::default();
    *fake.before.borrow_mut() = ScriptOutcome {
        error: Some("Error: boom with s3cr3t".into()),
        ..Default::default()
    };
    let wire = Wire::default();
    let c = collection();
    let report = block_on(send_with(
        &c,
        "r.yml",
        &input(),
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &services(&fake),
    ));
    assert_eq!(
        report.error,
        Some(SendError::Script(format!("Error: boom with {MASK}")))
    );
    assert!(wire.seen.borrow().is_empty());
    assert!(report.request.is_none());
}

#[test]
fn skip_sends_nothing_and_without_a_runner_scripts_do_not_run() {
    let fake = Fake::default();
    *fake.before.borrow_mut() = ScriptOutcome {
        flow: Flow::Skip,
        ..Default::default()
    };
    let wire = Wire::default();
    let c = collection();
    let report = block_on(send_with(
        &c,
        "r.yml",
        &input(),
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &services(&fake),
    ));
    assert_eq!(report.flow, Some(Flow::Skip));
    assert!(wire.seen.borrow().is_empty());

    let report = block_on(send_with(
        &c,
        "r.yml",
        &input(),
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &Services::none(),
    ));
    assert_eq!(report.error, None);
    assert!(report.tests.is_empty());
    assert_eq!(wire.seen.borrow().len(), 1);
}
