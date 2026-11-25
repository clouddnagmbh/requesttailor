// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand B: egress for extensions in the desktop app. A host that is not on the
//! device's list is refused, the list survives a restart (`egress/hosts.json` in the data
//! folder), a secret header is resolved from the keychain and masked in the answer (canary),
//! the builder's send guard is asked with `Purpose::Egress`, and the call leaves nothing in the
//! history and nothing for the exchange observers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rt_app::egress::{EgressError, EgressMethod, EgressRequest};
use rt_app::{AppError, SecretKey, SecretStore};
use rt_core::guard::{GuardHandle, GuardVerdict, Purpose, SendCheck, SendGuard};
use rt_core::model::{Environment, Variable};
use rt_core::observe::{Exchange, ExchangeObserver, ObserverHandle};
use rt_desktop::Core;
use rt_desktop::egress;

const CANARY: &str = "RT-CANARY-desktop-egress-2b7c";

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

#[derive(Default)]
struct Keep(Mutex<Vec<Exchange>>);

impl ExchangeObserver for Keep {
    fn observe(&self, exchange: &Exchange) {
        self.0.lock().unwrap().push(exchange.clone());
    }
}

/// Records every question; refuses `Purpose::Egress` to hosts named `blocked`.
#[derive(Default)]
struct Guard(Mutex<Vec<SendCheck>>);

impl SendGuard for Guard {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        self.0.lock().unwrap().push(check.clone());
        if check.purpose == Purpose::Egress && check.url.contains("blocked.") {
            GuardVerdict::Refuse {
                rule: "air-gap".into(),
                reason: "no outbound calls".into(),
            }
        } else {
            GuardVerdict::Allow
        }
    }
}

fn request(url: &str, headers: Vec<(String, String)>) -> EgressRequest {
    EgressRequest {
        method: EgressMethod::Get,
        url: url.into(),
        headers,
        body: None,
        extension: "traces".into(),
        purpose: "Server traces".into(),
        collection: None,
        environment: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_needs_an_allowed_host_resolves_secrets_and_leaves_no_trace() {
    let addr = rt_testserver::spawn_local().await;
    let root = std::env::temp_dir().join(format!("rt-desktop-egress-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (coll, data) = (root.join("coll"), root.join("data"));
    std::fs::create_dir_all(&coll).unwrap();
    std::fs::write(
        coll.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Egress\n",
    )
    .unwrap();
    let guard = Arc::new(Guard::default());
    let keep = Arc::new(Keep::default());
    let build = || {
        Core::with_data_dir(MemorySecrets::default(), Some(data.clone()))
            .unwrap()
            .with_guard(GuardHandle::new(guard.clone()))
            .with_observers(ObserverHandle::new().with(keep.clone()))
    };
    let core = build();
    core.open(&coll).unwrap();
    core.update_environment(Environment {
        name: "dev".into(),
        variables: vec![Variable {
            name: "trace_token".into(),
            value: None,
            enabled: true,
            secret: true,
        }],
        client_certificates: Vec::new(),
        external_secrets: None,
    })
    .unwrap();
    core.secret_set("dev", "trace_token", CANARY).unwrap();

    let url = format!("http://{addr}/echo/trace");
    let with_secret = || vec![("X-Api-Key".to_owned(), "{{secret:trace_token}}".to_owned())];

    // Not allowed yet: refused. The guard is asked first (a refusing guard means the user is
    // never asked about the host); this one allows, so the host list answers.
    let err = egress::request(&core, request(&url, with_secret()))
        .await
        .unwrap_err();
    assert!(matches!(err, EgressError::NotAllowed { .. }), "{err:?}");
    assert_eq!(guard.0.lock().unwrap().len(), 1);

    // Allowed once (plain http on its port): through, the secret masked in the answer.
    egress::allow(
        &core,
        "127.0.0.1",
        Some(addr.port()),
        "traces",
        "Server traces",
    )
    .unwrap();
    let answer = egress::request(&core, request(&url, with_secret()))
        .await
        .unwrap();
    assert_eq!(answer.status, 200);
    assert!(answer.body.contains("x-api-key"), "{}", answer.body);
    assert!(!serde_json::to_string(&answer).unwrap().contains(CANARY));
    {
        let checks = guard.0.lock().unwrap();
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[1].purpose, Purpose::Egress);
        assert_eq!(checks[1].collection, "Egress");
    }

    // The guard wins: a host it refuses is never reached.
    egress::allow(
        &core,
        "blocked.localhost",
        Some(addr.port()),
        "traces",
        "Server traces",
    )
    .unwrap();
    let err = egress::request(
        &core,
        request(
            &format!("http://blocked.localhost:{}/x", addr.port()),
            Vec::new(),
        ),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, EgressError::Blocked { rule, .. } if rule == "air-gap"),
        "{err:?}"
    );

    // No history entry and nothing for the observers.
    assert!(core.history.list("", "", 10).is_empty());
    assert!(
        keep.0.lock().unwrap().is_empty(),
        "an egress call is not an exchange"
    );

    // The list is per device: a restarted app still has it; removing takes it away.
    drop(core);
    let core = build();
    assert_eq!(egress::hosts(&core).unwrap().len(), 2);
    egress::remove(&core, "127.0.0.1", Some(addr.port())).unwrap();
    assert!(matches!(
        egress::request(&core, request(&url, Vec::new())).await,
        Err(EgressError::NotAllowed { .. })
    ));
    let _ = std::fs::remove_dir_all(&root);
}
