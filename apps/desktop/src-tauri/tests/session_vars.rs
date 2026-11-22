// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Session variables of the desktop shell (wave 7, strand C), through `Core` without a
//! window: a declarative action takes a value from one response, the next send uses it
//! (chaining), preview shows it as a runtime variable, each environment has its own, the
//! user's own `vars` win, and nothing is written to the collection folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::{AppError, SecretKey, SecretStore, SendInput};
use rt_core::model::VarScope;
use rt_core::script::VarChange;
use rt_core::vars::VarSource;
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

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rt-desktop-session-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const LOGIN: &str = r#"info:
  name: Login
  type: http
  seq: 1

http:
  method: GET
  url: "{{base}}/cookies/set?token=tok-abc"

runtime:
  assertions:
    - expression: res.status
      operator: eq
      value: "200"
    - expression: res.body.token
      operator: startsWith
      value: tok-
  actions:
    - type: set-variable
      phase: after-response
      selector:
        expression: res.body.token
        method: jsonq
      variable:
        name: token
        scope: runtime
"#;

const WHOAMI: &str = r#"info:
  name: Who am I
  type: http
  seq: 2

http:
  method: GET
  url: "{{base}}/echo/whoami"
  headers:
    - name: X-Token
      value: "{{token}}"
"#;

fn collection(dir: &Path, base_url: &str) -> PathBuf {
    let root = dir.join("collection");
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Chaining\n",
    )
    .unwrap();
    for env in ["Local", "Prod"] {
        std::fs::write(
            root.join(format!("environments/{env}.yml")),
            format!("name: {env}\nvariables:\n  - name: base\n    value: {base_url}\n"),
        )
        .unwrap();
    }
    std::fs::write(root.join("login.yml"), LOGIN).unwrap();
    std::fs::write(root.join("whoami.yml"), WHOAMI).unwrap();
    root
}

fn env(name: &str) -> SendInput {
    SendInput {
        environment: Some(name.into()),
        vars: Vec::new(),
        secrets: Vec::new(),
    }
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(snapshot(&path));
        } else {
            out.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
    out.sort();
    out
}

/// The `x-token` header the test server saw, or `None`.
async fn echoed_token(core: &Core<MemorySecrets>, input: SendInput) -> Option<String> {
    let sent = core.send("whoami.yml", input).await.unwrap();
    let id = sent.response_id.expect("a response");
    let body = core.response_bytes(id, 0, 64 * 1024).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h[0] == "x-token")
        .and_then(|h| h[1].as_str().map(str::to_owned))
}

#[tokio::test(flavor = "multi_thread")]
async fn an_action_chains_a_value_into_the_next_send_for_the_session() {
    let addr = rt_testserver::spawn_local().await;
    let tmp = TempDir::new("chain");
    let root = collection(&tmp.0, &format!("http://{addr}"));
    let core = Core::with_data_dir(MemorySecrets::default(), None).unwrap();
    core.open(&root).unwrap();
    let before = snapshot(&root);

    // Nothing in the session yet: the placeholder stays.
    assert_eq!(
        echoed_token(&core, env("Local")).await.as_deref(),
        Some("{{token}}")
    );

    // The login's assertions pass and its action takes the token.
    let login = core.send("login.yml", env("Local")).await.unwrap();
    assert_eq!(login.report.assertions.len(), 2);
    assert!(login.report.assertions.iter().all(|a| a.passed), "{:?}", login.report.assertions);
    assert_eq!(
        login.report.var_changes,
        vec![VarChange {
            name: "token".into(),
            value: Some("tok-abc".into()),
            scope: VarScope::Runtime,
        }]
    );
    assert_eq!(core.session_vars("Local"), login.report.var_changes);

    // Chained into the next send, and shown as a runtime variable in the preview.
    assert_eq!(echoed_token(&core, env("Local")).await.as_deref(), Some("tok-abc"));
    let preview = core.preview("whoami.yml", env("Local")).unwrap();
    let token = preview.variables.iter().find(|v| v.name == "token").unwrap();
    assert_eq!(token.source, VarSource::Runtime);
    assert_eq!(token.value.as_deref(), Some("tok-abc"));

    // The user's own runtime vars win over the session's.
    let mut own = env("Local");
    own.vars.push(("token".into(), "typed".into()));
    assert_eq!(echoed_token(&core, own).await.as_deref(), Some("typed"));

    // Another environment has its own session.
    assert!(core.session_vars("Prod").is_empty());
    assert_eq!(
        echoed_token(&core, env("Prod")).await.as_deref(),
        Some("{{token}}")
    );
    core.set_session_var(
        "Prod",
        &VarChange {
            name: "token".into(),
            value: Some("set-by-hand".into()),
            scope: VarScope::Environment,
        },
    );
    assert_eq!(
        echoed_token(&core, env("Prod")).await.as_deref(),
        Some("set-by-hand")
    );
    assert_eq!(echoed_token(&core, env("Local")).await.as_deref(), Some("tok-abc"));

    // Unset one, clear all.
    core.set_session_var(
        "Prod",
        &VarChange {
            name: "token".into(),
            value: None,
            scope: VarScope::Runtime,
        },
    );
    assert!(core.session_vars("Prod").is_empty());
    core.clear_session_vars("Local");
    assert!(core.session_vars("Local").is_empty());
    assert_eq!(
        echoed_token(&core, env("Local")).await.as_deref(),
        Some("{{token}}")
    );

    // Files are never written, nothing is dirty.
    assert!(core.dirty().unwrap().is_empty());
    assert_eq!(snapshot(&root), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_collection_the_session_is_empty() {
    let core = Core::with_data_dir(MemorySecrets::default(), None).unwrap();
    core.set_session_var(
        "Local",
        &VarChange {
            name: "a".into(),
            value: Some("1".into()),
            scope: VarScope::Runtime,
        },
    );
    assert!(core.session_vars("Local").is_empty());
    core.clear_session_vars("Local");
}
