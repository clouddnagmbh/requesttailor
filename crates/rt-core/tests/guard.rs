// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 10: the send guard a distribution adds. It sees the final request (secrets masked)
//! before the wire, also for requests built by scripts; a refusal sends nothing.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_core::guard::{Caller, Effect, GuardVerdict, Purpose, SendCheck, SendGuard};
use rt_core::model::{
    AgentAccess, AgentPolicy, Collection, Environment, EnvironmentPolicy, Item, RequestEntry,
    RequestSpec, Script, ScriptPhase, Variable,
};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::script::{ScriptCall, ScriptOutcome, ScriptRequest};
use rt_core::send::{SendError, SendOptions, send_with};
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
    seen: RefCell<Vec<String>>,
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let url = request.url.clone();
        self.seen
            .borrow_mut()
            .push(format!("{} {}", request.method, request.url));
        let body: BodyStream = Box::pin(Once(Some(Bytes::from_static(b"{}"))));
        Ok(Response {
            head: ResponseHead {
                status: 200,
                version: HttpVersion::Http11,
                headers: Vec::new(),
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

/// Refuses writing requests of agents in production; records what it was asked.
#[derive(Default)]
struct NoAgentWritesInProduction {
    asked: RefCell<Vec<SendCheck>>,
}

impl SendGuard for NoAgentWritesInProduction {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        self.asked.borrow_mut().push(check.clone());
        if check.caller == Caller::Agent && check.production && check.effect == Effect::Write {
            GuardVerdict::Refuse {
                rule: "agents.production-writes".into(),
                reason: "agents may not write to production".into(),
            }
        } else {
            GuardVerdict::Allow
        }
    }
}

/// Refuses every request a script builds.
struct NoScriptRequests;

impl SendGuard for NoScriptRequests {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        match check.purpose {
            Purpose::Script => GuardVerdict::Refuse {
                rule: "scripts.requests".into(),
                reason: "scripts may not send".into(),
            },
            Purpose::Main => GuardVerdict::Allow,
            Purpose::Egress | Purpose::Audit => GuardVerdict::Refuse {
                rule: "egress".into(),
                reason: "no egress".into(),
            },
        }
    }
}

/// A before-request script that calls `rt.sendRequest` and keeps the result.
#[derive(Default)]
struct SendingScript {
    result: RefCell<Option<Result<u16, String>>>,
}

impl ScriptRunner for SendingScript {
    fn run<'a>(&'a self, call: ScriptCall<'a>) -> LocalBoxFuture<'a, ScriptOutcome> {
        Box::pin(async move {
            if call.phase == ScriptPhase::BeforeRequest
                && let Some(sender) = call.sender
            {
                let r = sender
                    .send(ScriptRequest {
                        method: "DELETE".into(),
                        url: "http://prd.example/all".into(),
                        headers: Vec::new(),
                        body: None,
                    })
                    .await
                    .map(|r| r.status);
                *self.result.borrow_mut() = Some(r);
            }
            ScriptOutcome::default()
        })
    }
}

fn collection() -> Collection {
    let mut c = Collection {
        name: "Shop".into(),
        items: vec![Item::Request(RequestEntry {
            path: "orders/create.yml".into(),
            spec: RequestSpec {
                name: "create".into(),
                method: "POST".into(),
                url: "http://prd.example/orders?key={{key}}".into(),
                scripts: vec![Script {
                    phase: ScriptPhase::BeforeRequest,
                    code: "/* fake */".into(),
                }],
                ..Default::default()
            },
        })],
        environments: vec![Environment {
            name: "PRD".into(),
            variables: vec![Variable {
                name: "key".into(),
                value: Some("s3cr3t".into()),
                enabled: true,
                secret: true,
            }],
            client_certificates: Vec::new(),
            external_secrets: None,
        }],
        ..Default::default()
    };
    c.config.agents = AgentPolicy {
        access: AgentAccess::Read,
        environments: vec![EnvironmentPolicy {
            environment: "PRD".into(),
            access: AgentAccess::Write,
            production: true,
        }],
    };
    c
}

fn input() -> PrepareInput<'static> {
    PrepareInput {
        environment: Some("PRD"),
        globals: Vec::new(),
        runtime: Vec::new(),
    }
}

fn send(services: &Services<'_>, wire: &Wire) -> rt_core::send::SendReport {
    block_on(send_with(
        &collection(),
        "orders/create.yml",
        &input(),
        &NoFiles,
        wire,
        &SendOptions::default(),
        services,
    ))
}

#[test]
fn a_refused_request_is_not_sent_and_the_guard_never_sees_a_secret() {
    let guard = NoAgentWritesInProduction::default();
    let wire = Wire::default();
    let report = send(
        &Services {
            guard: Some(&guard),
            caller: Caller::Agent,
            ..Services::none()
        },
        &wire,
    );
    assert_eq!(
        report.error,
        Some(SendError::Blocked {
            rule: "agents.production-writes".into(),
            reason: "agents may not write to production".into(),
        })
    );
    assert!(wire.seen.borrow().is_empty(), "nothing on the wire");
    let asked = guard.asked.borrow();
    assert_eq!(asked.len(), 1);
    let check = &asked[0];
    assert_eq!(check.caller, Caller::Agent);
    assert_eq!(check.purpose, Purpose::Main);
    assert_eq!(check.collection, "Shop");
    assert_eq!(check.request_path, "orders/create.yml");
    assert_eq!(check.environment.as_deref(), Some("PRD"));
    assert_eq!(check.method, "POST");
    assert_eq!(check.effect, Effect::Write);
    assert!(check.production);
    assert!(!check.url.contains("s3cr3t"), "{}", check.url);
    assert!(check.url.contains("key=***"), "{}", check.url);
    // The report shows what would have gone out, masked.
    let shown = report.request.expect("request in the report");
    assert!(!shown.url.contains("s3cr3t"));
}

#[test]
fn the_same_request_from_the_command_line_goes_out() {
    let guard = NoAgentWritesInProduction::default();
    let wire = Wire::default();
    let report = send(
        &Services {
            guard: Some(&guard),
            caller: Caller::Cli,
            ..Services::none()
        },
        &wire,
    );
    assert_eq!(report.error, None);
    assert_eq!(wire.seen.borrow().len(), 1);
    assert_eq!(guard.asked.borrow()[0].caller, Caller::Cli);
}

#[test]
fn requests_of_scripts_are_asked_too() {
    let script = SendingScript::default();
    let wire = Wire::default();
    let report = send(
        &Services {
            scripts: Some(&script),
            guard: Some(&NoScriptRequests),
            caller: Caller::App,
            ..Services::none()
        },
        &wire,
    );
    let result = script.result.borrow().clone().expect("script ran");
    assert_eq!(result, Err("not sent: scripts may not send".into()));
    // Only the main request went out.
    assert_eq!(
        wire.seen.borrow().as_slice(),
        ["POST http://prd.example/orders?key=s3cr3t"]
    );
    assert_eq!(report.error, None);
}

#[test]
fn without_a_guard_nothing_changes() {
    let script = SendingScript::default();
    let wire = Wire::default();
    let report = send(
        &Services {
            scripts: Some(&script),
            ..Services::none()
        },
        &wire,
    );
    assert_eq!(report.error, None);
    assert_eq!(wire.seen.borrow().len(), 2);
    assert_eq!(*script.result.borrow(), Some(Ok(200)));
}
