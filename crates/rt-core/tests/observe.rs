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
                secrets: &[],
            }),
        ),
    ] {
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].error.is_some());
    }
}

/// Delivery from `send_with` (strand A): one masked exchange per send.
mod delivery {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::mpsc;
    use std::task::{Context, Poll};
    use std::time::{Duration, Instant};

    use bytes::Bytes;
    use rt_core::guard::{Caller, GuardVerdict, SendCheck, SendGuard};
    use rt_core::model::{
        Collection, Environment, Item, ObserveCollectionConfig, RequestEntry, RequestSpec, Variable,
    };
    use rt_core::observe::{EXCHANGE_BODY_LIMIT, Exchange, ExchangeObserver, ObserverSet};
    use rt_core::prepare::{FileSource, PrepareInput};
    use rt_core::send::{ReportBody, SendError, SendOptions, SendReport, send_with};
    use rt_core::services::Services;
    use rt_core::transport::{
        BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport,
        TransportError,
    };

    const CANARY: &str = "RT-CANARY-9f3a71c2";

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

    /// Answers with a body and headers that echo the canary secret.
    struct Echo {
        body: Vec<u8>,
        fail: bool,
    }

    impl Transport for Echo {
        async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
            if self.fail {
                return Err(TransportError::Other(format!(
                    "connect to {} failed",
                    request.url
                )));
            }
            let body: BodyStream = Box::pin(Once(Some(Bytes::from(self.body.clone()))));
            Ok(Response {
                head: ResponseHead {
                    status: 200,
                    version: HttpVersion::Http11,
                    headers: vec![
                        ("Content-Type".into(), "application/json".into()),
                        ("X-Echo".into(), format!("value {CANARY}")),
                        ("X-Api-Key".into(), "an-unlisted-key-value".into()),
                        ("Authorization".into(), "Bearer abc".into()),
                        ("Set-Cookie".into(), "sid=sessioncookie; Path=/".into()),
                    ],
                    url: request.url,
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

    fn collection(observe: bool) -> Collection {
        let mut c = Collection {
            name: "Shop".into(),
            items: vec![Item::Request(RequestEntry {
                path: "get.yml".into(),
                spec: RequestSpec {
                    name: "get".into(),
                    method: "GET".into(),
                    url: "http://shop.example/items?key={{key}}".into(),
                    headers: vec![rt_core::model::KeyValue::new("X-Custom-Token", "{{key}}")],
                    ..Default::default()
                },
            })],
            environments: vec![Environment {
                name: "dev".into(),
                variables: vec![Variable {
                    name: "key".into(),
                    value: Some(CANARY.into()),
                    enabled: true,
                    secret: true,
                }],
                client_certificates: Vec::new(),
                external_secrets: None,
            }],
            ..Default::default()
        };
        c.config.observe = ObserveCollectionConfig { enabled: observe };
        c
    }

    fn input() -> PrepareInput<'static> {
        PrepareInput {
            environment: Some("dev"),
            globals: Vec::new(),
            runtime: Vec::new(),
        }
    }

    #[derive(Default)]
    struct Collect(std::sync::Mutex<Vec<Exchange>>);

    impl ExchangeObserver for Collect {
        fn observe(&self, exchange: &Exchange) {
            self.0.lock().unwrap().push(exchange.clone());
        }
    }

    fn run(
        collection: &Collection,
        wire: &Echo,
        observers: &ObserverSet,
        caller: Caller,
    ) -> SendReport {
        block_on(send_with(
            collection,
            "get.yml",
            &input(),
            &NoFiles,
            wire,
            &SendOptions::default(),
            &Services {
                observers: Some(observers),
                caller,
                ..Services::none()
            },
        ))
    }

    fn echo(body: &str) -> Echo {
        Echo {
            body: body.as_bytes().to_vec(),
            fail: false,
        }
    }

    #[test]
    fn one_masked_exchange_per_send() {
        let seen = std::sync::Arc::new(Collect::default());
        let set = ObserverSet::new().with(seen.clone());
        let body = format!(r#"{{"token":"{CANARY}","n":1}}"#);
        let report = run(&collection(true), &echo(&body), &set, Caller::Cli);
        assert_eq!(report.error, None);
        let got = seen.0.lock().unwrap();
        assert_eq!(got.len(), 1, "exactly one exchange");
        let x = &got[0];
        assert_eq!(x.caller, Caller::Cli);
        assert_eq!(x.collection.as_deref(), Some("Shop"));
        assert_eq!(x.request_path.as_deref(), Some("get.yml"));
        assert_eq!(x.environment.as_deref(), Some("dev"));
        assert!(x.at > 1_700_000_000_000);
        assert!(!x.truncated);
        let json = serde_json::to_string(x).unwrap();
        assert!(!json.contains(CANARY), "canary leaked: {json}");
        assert!(!json.contains("sessioncookie"), "{json}");
        assert!(!json.contains("an-unlisted-key-value"), "{json}");
        assert!(!json.contains("Bearer abc"), "{json}");
        let response = x.report.response.as_ref().unwrap();
        match &response.body {
            ReportBody::Utf8(text) => assert_eq!(text, r#"{"token":"***","n":1}"#),
            other => panic!("{other:?}"),
        }
        // The original report is untouched (the caller still sees the response as received).
        match &report.response.as_ref().unwrap().body {
            ReportBody::Utf8(text) => assert!(text.contains(CANARY)),
            other => panic!("{other:?}"),
        }
        // Non-secret headers survive.
        assert!(
            response
                .headers
                .iter()
                .any(|(n, v)| n == "Content-Type" && v == "application/json")
        );
        let request = x.report.request.as_ref().unwrap();
        assert!(
            request
                .headers
                .iter()
                .filter(|(n, _)| n == "X-Custom-Token")
                .all(|(_, v)| v == "***")
        );
    }

    #[test]
    fn a_base64_body_is_masked_too() {
        let seen = std::sync::Arc::new(Collect::default());
        let set = ObserverSet::new().with(seen.clone());
        let mut body = vec![0xff, 0xfe, 0x00];
        body.extend_from_slice(CANARY.as_bytes());
        body.push(0x80);
        let wire = Echo { body, fail: false };
        run(&collection(true), &wire, &set, Caller::App);
        let got = seen.0.lock().unwrap();
        let x = &got[0];
        let ReportBody::Base64(b64) = &x.report.response.as_ref().unwrap().body else {
            panic!("expected base64");
        };
        let bytes = base64_decode(b64);
        assert!(!bytes.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()));
        assert!(bytes.windows(3).any(|w| w == b"***"));
    }

    fn base64_decode(text: &str) -> Vec<u8> {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .unwrap()
    }

    #[test]
    fn long_bodies_are_cut_at_the_limit() {
        let seen = std::sync::Arc::new(Collect::default());
        let set = ObserverSet::new().with(seen.clone());
        let body = "a".repeat(EXCHANGE_BODY_LIMIT + 10);
        run(&collection(true), &echo(&body), &set, Caller::App);
        let got = seen.0.lock().unwrap();
        let x = &got[0];
        assert!(x.truncated);
        let ReportBody::Utf8(text) = &x.report.response.as_ref().unwrap().body else {
            panic!()
        };
        assert_eq!(text.len(), EXCHANGE_BODY_LIMIT);
        // `size` still tells how much arrived.
        assert_eq!(
            x.report.response.as_ref().unwrap().size as usize,
            EXCHANGE_BODY_LIMIT + 10
        );
    }

    #[test]
    fn a_secret_cut_by_the_limit_does_not_leak_a_prefix() {
        let seen = std::sync::Arc::new(Collect::default());
        let set = ObserverSet::new().with(seen.clone());
        // The canary straddles the limit: masking comes before cutting.
        let mut body = "a".repeat(EXCHANGE_BODY_LIMIT - 5);
        body.push_str(CANARY);
        run(&collection(true), &echo(&body), &set, Caller::App);
        let got = seen.0.lock().unwrap();
        let ReportBody::Utf8(text) = &got[0].report.response.as_ref().unwrap().body else {
            panic!()
        };
        assert!(!text.contains("RT-CANA"), "a prefix of the secret leaked");
    }

    #[test]
    fn observe_enabled_false_stops_delivery() {
        let seen = std::sync::Arc::new(Collect::default());
        let set = ObserverSet::new().with(seen.clone());
        run(&collection(false), &echo("{}"), &set, Caller::App);
        assert!(seen.0.lock().unwrap().is_empty());
    }

    #[test]
    fn a_transport_error_is_an_exchange_without_response_and_masked() {
        let seen = std::sync::Arc::new(Collect::default());
        let set = ObserverSet::new().with(seen.clone());
        let wire = Echo {
            body: Vec::new(),
            fail: true,
        };
        let report = run(&collection(true), &wire, &set, Caller::App);
        assert!(matches!(report.error, Some(SendError::Transport(_))));
        let got = seen.0.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].report.response.is_none());
        assert!(!serde_json::to_string(&got[0]).unwrap().contains(CANARY));
    }

    struct Refuse;

    impl SendGuard for Refuse {
        fn check(&self, _: &SendCheck) -> GuardVerdict {
            GuardVerdict::Refuse {
                rule: "r".into(),
                reason: "no".into(),
            }
        }
    }

    #[test]
    fn nothing_is_delivered_when_nothing_was_sent() {
        let seen = std::sync::Arc::new(Collect::default());
        let set = ObserverSet::new().with(seen.clone());
        let report = block_on(send_with(
            &collection(true),
            "get.yml",
            &input(),
            &NoFiles,
            &echo("{}"),
            &SendOptions::default(),
            &Services {
                observers: Some(&set),
                guard: Some(&Refuse),
                ..Services::none()
            },
        ));
        assert!(matches!(report.error, Some(SendError::Blocked { .. })));
        let missing = block_on(send_with(
            &collection(true),
            "nope.yml",
            &input(),
            &NoFiles,
            &echo("{}"),
            &SendOptions::default(),
            &Services {
                observers: Some(&set),
                ..Services::none()
            },
        ));
        assert!(matches!(missing.error, Some(SendError::NotFound(_))));
        assert!(seen.0.lock().unwrap().is_empty());
    }

    /// Hands over to a channel and returns; its consumer is slow.
    struct Channel(std::sync::Mutex<mpsc::Sender<Exchange>>);

    impl ExchangeObserver for Channel {
        fn observe(&self, exchange: &Exchange) {
            let _ = self.0.lock().unwrap().send(exchange.clone());
        }
    }

    #[test]
    fn a_channel_observer_with_a_slow_consumer_does_not_slow_the_send() {
        let (tx, rx) = mpsc::channel::<Exchange>();
        let consumer = std::thread::spawn(move || {
            let mut n = 0;
            while rx.recv().is_ok() {
                std::thread::sleep(Duration::from_millis(300));
                n += 1;
            }
            n
        });
        let set = ObserverSet::new().with(std::sync::Arc::new(Channel(std::sync::Mutex::new(tx))));
        let started = Instant::now();
        for _ in 0..3 {
            run(&collection(true), &echo("{}"), &set, Caller::App);
        }
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "sending waited for the consumer: {:?}",
            started.elapsed()
        );
        drop(set);
        assert_eq!(consumer.join().unwrap(), 3);
    }

    #[test]
    fn without_observers_the_report_is_what_it_was() {
        let a = run(
            &collection(true),
            &echo("{}"),
            &ObserverSet::new(),
            Caller::App,
        );
        let wire = echo("{}");
        let b = block_on(send_with(
            &collection(true),
            "get.yml",
            &input(),
            &NoFiles,
            &wire,
            &SendOptions::default(),
            &Services::none(),
        ));
        assert_eq!(a.request, b.request);
        assert_eq!(a.error, b.error);
    }
}
