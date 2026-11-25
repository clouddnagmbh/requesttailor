// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance, against the fake tenant on loopback (ports 48818 to 48820): the imported
//! collection triggers an iFlow with OAuth client credentials from its folder, keeps the log id
//! and checks the message processing log with an assertion that is green or red. The run is
//! the core's own runner; the plugin is not part of it (its hook is called on the answers
//! afterwards).

mod common;

use std::cell::RefCell;
use std::collections::HashMap;

use common::{API_PORT, API_SECRET, RUNTIME_PORT, RUNTIME_SECRET, TOKEN_PORT};
use rt_core::model::{Body, Collection, Item};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::runner::{RunEvent, RunPlan, RunStep, RunSummary, plan, run};
use rt_core::send::{ReportBody, SendOptions};
use rt_core::services::{LocalBoxFuture, Services, StoredToken, TokenStore};
use rt_core::transport::native::NativeTransport;
use rt_plugin_sap_integration::Integration;
use rt_plugin_sdk::api::{Call, NotesReply, PluginBody, PluginRequest, PluginResponse};
use rt_plugin_sdk::testing;

struct NoFiles;

impl FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("{path}: not found"))
    }
}

#[derive(Default)]
struct Tokens(RefCell<HashMap<String, StoredToken>>);

impl TokenStore for Tokens {
    fn get<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, Option<StoredToken>> {
        Box::pin(async move { self.0.borrow().get(key).cloned() })
    }
    fn put<'a>(&'a self, key: &'a str, token: &'a StoredToken) -> LocalBoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.borrow_mut().insert(key.to_owned(), token.clone());
        })
    }
    fn remove<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.borrow_mut().remove(key);
        })
    }
}

/// The collection the importer writes for the two fixture keys, with the payload of the
/// trigger replaced.
fn collection(payload: &str) -> Collection {
    let reply = common::import(vec![
        common::fixture("service-key-api.json"),
        common::fixture("service-key-runtime.json"),
    ])
    .unwrap();
    let mut collection = common::read(&reply);
    fn set(items: &mut [Item], payload: &str) {
        for item in items {
            match item {
                Item::Request(r) if r.spec.name == "Trigger iFlow" => {
                    r.spec.body = Body::Raw {
                        kind: rt_core::model::RawKind::Json,
                        data: payload.into(),
                    };
                }
                Item::Folder(f) => set(&mut f.items, payload),
                _ => {}
            }
        }
    }
    set(&mut collection.items, payload);
    collection
}

fn path_of(collection: &Collection, name: &str) -> String {
    common::request(collection, name).0
}

/// What the app does when sending: the variables of the tenant on loopback, and the secrets
/// the key store would hold.
fn input() -> PrepareInput<'static> {
    let local = |port: u16| format!("http://127.0.0.1:{port}");
    PrepareInput {
        environment: Some("rt-test"),
        globals: Vec::new(),
        runtime: vec![
            ("apiUrl".into(), local(API_PORT)),
            ("runtimeUrl".into(), local(RUNTIME_PORT)),
            (
                "tokenUrl".into(),
                format!("{}/oauth/token", local(TOKEN_PORT)),
            ),
            (
                "runtimeTokenUrl".into(),
                format!("{}/oauth/token", local(TOKEN_PORT)),
            ),
            ("clientId".into(), common::API_CLIENT.into()),
            ("runtimeClientId".into(), common::RUNTIME_CLIENT.into()),
            ("clientSecret".into(), API_SECRET.into()),
            ("runtimeClientSecret".into(), RUNTIME_SECRET.into()),
            ("iflowPath".into(), "orders".into()),
            ("iflowName".into(), "Orders_Flow".into()),
        ],
    }
}

async fn run_paths(collection: &Collection, paths: Vec<String>) -> (RunSummary, Vec<RunStep>) {
    common::start();
    let transport = NativeTransport::new().unwrap();
    let tokens = Tokens::default();
    let services = Services {
        tokens: Some(&tokens),
        ..Services::none()
    };
    let plan = RunPlan {
        requests: paths,
        ..RunPlan::default()
    };
    let mut steps = Vec::new();
    let summary = run(
        collection,
        &plan,
        &input(),
        &NoFiles,
        &transport,
        &SendOptions::default(),
        &services,
        &mut |event| {
            if let RunEvent::Step { step } = event {
                steps.push(*step);
            }
        },
        &|| false,
    )
    .await;
    (summary, steps)
}

fn notes_of(step: &RunStep) -> NotesReply {
    let response = step.report.response.as_ref().expect("a response");
    let text = match &response.body {
        ReportBody::Utf8(t) => t.clone(),
        ReportBody::Base64(_) => String::new(),
    };
    testing::call(
        &Integration,
        &Call::Response {
            request: PluginRequest {
                method: "GET".into(),
                url: step.report.request.as_ref().unwrap().url.clone(),
                headers: Vec::new(),
                body: None,
            },
            response: PluginResponse {
                status: response.status,
                headers: response.headers.clone(),
                url: response.url.clone(),
                body: PluginBody {
                    content_type: None,
                    text: Some(text),
                    base64: None,
                },
                truncated: false,
            },
            context: testing::context(&step.request_path),
        },
    )
    .unwrap()
}

fn order(collection: &Collection) -> Vec<String> {
    plan(collection, None)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_that_completes_makes_every_assertion_green() {
    let collection = collection("{\"order\":\"ok\"}");
    let paths = vec![
        path_of(&collection, "Trigger iFlow"),
        path_of(&collection, "Log of the last trigger"),
    ];
    let (summary, steps) = run_paths(&collection, paths).await;
    assert_eq!(summary.failed, 0, "{steps:#?}");
    assert_eq!(summary.passed, 2);
    assert_eq!(summary.assertions_failed, 0);
    assert!(summary.assertions_passed >= 4);
    // The log request asked for the id the trigger answered with.
    let url = steps[1].report.request.as_ref().unwrap().url.clone();
    assert!(url.contains("MessageProcessingLogs('AGvDm3x"), "{url}");
    // The status assertion saw COMPLETED.
    let status = steps[1]
        .report
        .assertions
        .iter()
        .find(|a| a.expression == "res.body.d.Status")
        .unwrap();
    assert!(status.passed);
    assert_eq!(status.actual.as_deref(), Some("COMPLETED"));
    // The plugin's hook speaks plainly about both answers.
    let notes = notes_of(&steps[0]);
    assert!(
        notes.notes[0]
            .text
            .contains("Message processing log ID AGvDm3x")
    );
    let notes = notes_of(&steps[1]);
    assert!(notes.notes[0].text.contains("COMPLETED"), "{notes:?}");
    assert!(notes.notes[0].text.contains("2.5 s"), "{notes:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_that_fails_turns_the_status_assertion_red() {
    let collection = collection("{\"order\":\"fail\"}");
    let paths = vec![
        path_of(&collection, "Trigger iFlow"),
        path_of(&collection, "Log of the last trigger"),
        path_of(&collection, "Error text of the last trigger"),
    ];
    let (summary, steps) = run_paths(&collection, paths).await;
    // The runtime answered 500, so the trigger's own assertion is red too ...
    assert!(!steps[0].passed);
    // ... and the log says FAILED, which the status assertion refuses.
    assert!(!steps[1].passed);
    let status = steps[1]
        .report
        .assertions
        .iter()
        .find(|a| a.expression == "res.body.d.Status")
        .unwrap();
    assert!(!status.passed);
    assert_eq!(status.actual.as_deref(), Some("FAILED"));
    assert!(summary.assertions_failed >= 2);
    // The error text request reads what the flow said.
    let ReportBody::Utf8(text) = &steps[2].report.response.as_ref().unwrap().body else {
        panic!("text expected");
    };
    assert!(text.contains("Order 4711 rejected"), "{text}");
    // The plugin's note points to the error text.
    let notes = notes_of(&steps[1]);
    assert!(
        notes.notes[0].text.contains("FAILED") && notes.notes[0].text.contains("ErrorInformation")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_log_that_is_still_processing_is_red_until_the_message_is_done() {
    let collection = collection("{\"order\":\"slow\"}");
    let log = path_of(&collection, "Log of the last trigger");
    let paths = vec![path_of(&collection, "Trigger iFlow"), log.clone(), log];
    let (_, steps) = run_paths(&collection, paths).await;
    assert!(steps[0].passed);
    assert!(!steps[1].passed, "the first read still says PROCESSING");
    let notes = notes_of(&steps[1]);
    assert!(
        notes.notes[0].text.contains("still PROCESSING"),
        "{notes:?}"
    );
    assert!(steps[2].passed, "{:#?}", steps[2].report.assertions);
}

#[tokio::test(flavor = "multi_thread")]
async fn each_folder_gets_its_own_token_and_no_secret_reaches_the_plugin() {
    let tenant = common::start();
    let collection = collection("{\"order\":\"ok\"}");
    let (_, steps) = run_paths(&collection, order(&collection)).await;
    // Everything planned ran: the trigger, the log, the failed list, the error text.
    assert_eq!(steps.len(), 4);
    let seen = tenant.seen();
    let runtime_calls: Vec<_> = seen.iter().filter(|s| s.port == RUNTIME_PORT).collect();
    let api_calls: Vec<_> = seen.iter().filter(|s| s.port == API_PORT).collect();
    assert!(
        runtime_calls
            .iter()
            .all(|s| s.header("Authorization") == Some("Bearer rt-token"))
    );
    assert!(
        api_calls
            .iter()
            .all(|s| s.header("Authorization") == Some("Bearer api-token"))
    );
    // Everything the plugin returns for these answers is free of the secrets.
    let mut output = String::new();
    for step in &steps {
        output.push_str(&serde_json::to_string(&notes_of(step)).unwrap());
    }
    for secret in [API_SECRET, RUNTIME_SECRET] {
        assert!(!output.contains(secret));
    }
    // The failed-messages list is read as a table and offers the error text of failures.
    let list = &steps[2];
    assert_eq!(list.report.response.as_ref().unwrap().status, 200);
    assert!(!notes_of(list).notes.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_secret_is_refused_by_the_token_host_and_the_run_says_so() {
    common::start();
    let collection = collection("{\"order\":\"ok\"}");
    let transport = NativeTransport::new().unwrap();
    let tokens = Tokens::default();
    let services = Services {
        tokens: Some(&tokens),
        ..Services::none()
    };
    let mut bad = input();
    bad.runtime.retain(|(n, _)| n != "runtimeClientSecret");
    bad.runtime
        .push(("runtimeClientSecret".into(), "wrong".into()));
    let plan = RunPlan {
        requests: vec![path_of(&collection, "Trigger iFlow")],
        ..RunPlan::default()
    };
    let mut steps = Vec::new();
    run(
        &collection,
        &plan,
        &bad,
        &NoFiles,
        &transport,
        &SendOptions::default(),
        &services,
        &mut |e| {
            if let RunEvent::Step { step } = e {
                steps.push(*step);
            }
        },
        &|| false,
    )
    .await;
    assert!(!steps[0].passed);
    assert!(steps[0].report.error.is_some(), "{:?}", steps[0].report);
}
