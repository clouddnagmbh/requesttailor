// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17 interface: exchange observers and the snapshot operator. Strand A and strand B
//! extend this scaffold (masking, delivery from `send_with`, comparison).

use std::sync::{Arc, Mutex};

use rt_core::assert::{MATCHES_SNAPSHOT, SnapshotStore, evaluate, evaluate_with};
use rt_core::guard::Caller;
use rt_core::model::Assertion;
use rt_core::observe::{
    EXCHANGE_BODY_LIMIT, Exchange, ExchangeObserver, ObserverHandle, ObserverSet,
};
use rt_core::script::ScriptResponse;
use rt_core::send::{REPORT_VERSION, SendReport};
use rt_core::services::Services;
use rt_core::vars::Scope;

struct Collect(Mutex<Vec<String>>);

impl ExchangeObserver for Collect {
    fn observe(&self, exchange: &Exchange) {
        self.0
            .lock()
            .unwrap()
            .push(exchange.request_path.clone().unwrap_or_default());
    }
}

fn exchange(path: &str) -> Exchange {
    Exchange {
        caller: Caller::Agent,
        collection: None,
        request_path: Some(path.into()),
        environment: None,
        at: 1,
        report: SendReport {
            version: REPORT_VERSION,
            request_path: path.into(),
            environment: None,
            request: None,
            response: None,
            variables: Vec::new(),
            unresolved: Vec::new(),
            error: None,
            notes: Vec::new(),
            tests: Vec::new(),
            assertions: Vec::new(),
            logs: Vec::new(),
            var_changes: Vec::new(),
            flow: None,
        },
        truncated: false,
    }
}

#[test]
fn an_observer_set_delivers_in_order_and_serializes_the_exchange() {
    let a = Arc::new(Collect(Mutex::new(Vec::new())));
    let handle: ObserverHandle = ObserverSet::new().with(a.clone());
    handle.notify(&exchange("a.yml"));
    handle.notify(&exchange("b.yml"));
    assert_eq!(*a.0.lock().unwrap(), vec!["a.yml", "b.yml"]);
    let json = serde_json::to_value(exchange("a.yml")).unwrap();
    assert_eq!(json["caller"], "agent");
    assert_eq!(json["truncated"], false);
    assert_eq!(EXCHANGE_BODY_LIMIT, 1024 * 1024);
}

#[test]
fn no_observers_is_the_default() {
    let services = Services::none();
    assert!(services.observers.is_none());
    assert!(services.snapshots.is_none());
}

fn response() -> ScriptResponse {
    ScriptResponse {
        status: 200,
        headers: Vec::new(),
        body: b"{}".to_vec(),
        time_ms: 1,
        url: "http://127.0.0.1/".into(),
    }
}

fn snapshot_assertion() -> Assertion {
    Assertion {
        expression: "res".into(),
        operator: MATCHES_SNAPSHOT.into(),
        value: "get.snap.json".into(),
        description: None,
        enabled: true,
    }
}

struct Files;

impl SnapshotStore for Files {
    fn read(&self, _: &str, _: &str) -> Option<String> {
        None
    }
}

#[test]
fn a_snapshot_assertion_never_passes_silently_before_strand_b() {
    let a = [snapshot_assertion()];
    for results in [
        evaluate(&a, &response(), &Scope::default()),
        evaluate_with(
            &a,
            &response(),
            &Scope::default(),
            Some(rt_core::assert::SnapshotContext {
                store: &Files,
                request_path: "get.yml",
            }),
        ),
    ] {
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].error.is_some());
    }
}
