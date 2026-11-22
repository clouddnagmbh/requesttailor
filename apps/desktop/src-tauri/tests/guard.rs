// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 10: a distribution's send guard (`Core::with_guard`, managed as
//! `rt_core::guard::GuardHandle` on the builder) is asked before the app sends; a refused
//! request never reaches the server.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use rt_app::{AppError, SecretKey, SecretStore, SendInput};
use rt_core::guard::{Caller, GuardHandle, GuardVerdict, SendCheck, SendGuard};
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

/// Refuses every path ending in `blocked`; records the caller.
#[derive(Default)]
struct Guard(Mutex<Vec<Caller>>);

impl SendGuard for Guard {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        self.0.lock().unwrap().push(check.caller);
        if check.url.ends_with("/blocked") {
            GuardVerdict::Refuse {
                rule: "hosts.blocked".into(),
                reason: "this URL is blocked".into(),
            }
        } else {
            GuardVerdict::Allow
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_app_asks_the_guard_before_sending() {
    let addr = rt_testserver::spawn_local().await;
    let dir = std::env::temp_dir().join(format!("rt-desktop-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Guarded\n",
    )
    .unwrap();
    let guard = Arc::new(Guard::default());
    let core = Core::new(MemorySecrets::default())
        .unwrap()
        .with_guard(GuardHandle::new(guard.clone()));
    core.open(&dir).unwrap();
    let path = core.create_request("", "Ping").unwrap();
    let input = || SendInput {
        environment: None,
        vars: Vec::new(),
        secrets: Vec::new(),
    };

    let mut spec = core.get_request(&path).unwrap();
    spec.url = format!("http://{addr}/echo/blocked");
    core.update_request(&path, spec.clone()).unwrap();
    let stored = core.send(&path, input()).await.unwrap();
    assert!(stored.report.response.is_none());
    assert!(
        matches!(
            &stored.report.error,
            Some(rt_core::send::SendError::Blocked { rule, .. }) if rule == "hosts.blocked"
        ),
        "{:?}",
        stored.report.error
    );

    spec.url = format!("http://{addr}/echo/ok");
    core.update_request(&path, spec).unwrap();
    let stored = core.send(&path, input()).await.unwrap();
    assert_eq!(stored.report.response.map(|r| r.status), Some(200));
    assert_eq!(*guard.0.lock().unwrap(), [Caller::App, Caller::App]);
    let _ = std::fs::remove_dir_all(&dir);
}
