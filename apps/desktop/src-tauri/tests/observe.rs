// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17: a distribution's exchange observers (`Core::with_observers`, managed as
//! `rt_core::observe::ObserverHandle` on the builder) get one masked exchange from every send
//! of the app; a secret variable's value never reaches them.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use rt_app::{AppError, SecretKey, SecretStore, SendInput};
use rt_core::guard::Caller;
use rt_core::observe::{Exchange, ExchangeObserver, ObserverHandle};
use rt_desktop::Core;

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
        self.0.lock().unwrap().insert(key.clone(), value.to_owned());
        Ok(())
    }
    fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Keeps every exchange as JSON.
#[derive(Default)]
struct Keep(Mutex<Vec<Exchange>>);

impl ExchangeObserver for Keep {
    fn observe(&self, exchange: &Exchange) {
        self.0.lock().unwrap().push(exchange.clone());
    }
}

const CANARY: &str = "RT-CANARY-71d0aa";

#[tokio::test(flavor = "multi_thread")]
async fn the_app_hands_every_send_to_the_observers_masked() {
    let addr = rt_testserver::spawn_local().await;
    let dir = std::env::temp_dir().join(format!("rt-desktop-observe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Watched\n",
    )
    .unwrap();
    let keep = Arc::new(Keep::default());
    let core = Core::new(MemorySecrets::default())
        .unwrap()
        .with_observers(ObserverHandle::new().with(keep.clone()));
    core.open(&dir).unwrap();
    let path = core.create_request("", "Ping").unwrap();
    let mut spec = core.get_request(&path).unwrap();
    spec.url = format!("http://{addr}/echo/ok?key={{{{key}}}}");
    core.update_request(&path, spec).unwrap();
    core.update_environment(rt_core::model::Environment {
        name: "dev".into(),
        variables: vec![rt_core::model::Variable {
            name: "key".into(),
            value: None,
            enabled: true,
            secret: true,
        }],
        client_certificates: Vec::new(),
        external_secrets: None,
    })
    .unwrap();
    core.secret_set("dev", "key", CANARY).unwrap();
    let input = SendInput {
        environment: Some("dev".into()),
        vars: Vec::new(),
        secrets: Vec::new(),
    };
    let stored = core.send(&path, input).await.unwrap();
    assert_eq!(stored.report.response.as_ref().map(|r| r.status), Some(200));
    let got = keep.0.lock().unwrap();
    assert_eq!(got.len(), 1, "one exchange per send");
    assert_eq!(got[0].caller, Caller::App);
    assert_eq!(got[0].collection.as_deref(), Some("Watched"));
    assert_eq!(got[0].report.response.as_ref().map(|r| r.status), Some(200));
    let json = serde_json::to_string(&got[0]).unwrap();
    assert!(!json.contains(CANARY), "{json}");
    assert!(json.contains("key=***"), "the secret was in play: {json}");
    // The echo server repeats the query, so the body held the canary before masking.
    assert!(got[0].report.response.as_ref().is_some_and(
        |r| matches!(&r.body, rt_core::send::ReportBody::Utf8(t) if t.contains("key=***"))
    ));
    let _ = std::fs::remove_dir_all(&dir);
}
