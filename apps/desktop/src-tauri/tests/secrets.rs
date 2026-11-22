// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 5, strand E in the desktop shell, without a window: a commit with a credential typed
//! into a request file is refused with `AppError::SecretsFound` (`git::scan` lists it with a
//! hint), and external secrets are read before a send (fake runner) and joined with the
//! keychain values. Fake credentials are assembled at run time (gitleaks stays clean).

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rt_app::secret_sources::{CommandRunner, RunError, SecretCommand, SecretResolver};
use rt_app::{AppError, SecretKey, SecretStore, SendInput};
use rt_core::model::{
    Environment, ExternalSecret, ExternalSecrets, KeyValue, SecretProvider, Variable,
};
use rt_desktop::Core;
use rt_desktop::git;

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

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir =
        std::env::temp_dir().join(format!("rt-desktop-secrets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(rt_git::canonical(&dir).unwrap())
}

fn token() -> String {
    ["9c1e2f3a4b5c", "6d7e8f90a1b2"].concat()
}

fn set_user(work: &Path) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(work.join(".git/config"))
        .unwrap();
    f.write_all(b"[user]\n\tname = Ada\n\temail = ada@example.com\n")
        .unwrap();
}

fn bearer(value: &str) -> KeyValue {
    KeyValue {
        name: "Authorization".into(),
        value: format!("Bearer {value}"),
        enabled: true,
    }
}

#[test]
fn a_commit_with_a_plaintext_token_is_refused_with_a_hint() {
    let work = temp("commit");
    std::fs::write(
        work.0.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Scan\n",
    )
    .unwrap();
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&work.0).unwrap();
    git::init(&core).unwrap();
    set_user(&work.0);

    let path = core.create_request("", "Me").unwrap();
    let mut spec = core.get_request(&path).unwrap();
    spec.url = "https://api.example.com/me".into();
    spec.headers.push(bearer(&token()));
    core.update_request(&path, spec.clone()).unwrap();
    core.save().unwrap();
    let paths = vec!["opencollection.yml".to_owned(), path.clone()];

    // The dialog's list: file, line, kind, masked snippet, hint.
    let findings = git::scan(&core, &paths).unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let f = &findings[0];
    assert_eq!(f.path, path);
    let text = std::fs::read_to_string(work.0.join(&path)).unwrap();
    let line = text.lines().nth(f.line as usize - 1).unwrap();
    assert!(line.contains(&token()), "{line}");
    assert!(f.snippet.contains("***") && !f.snippet.contains(&token()));
    assert!(f.hint.contains("secret"), "{}", f.hint);

    // The commit is refused, nothing is committed, and the message does not show the value.
    let err = git::commit(&core, "Add Me", &paths).unwrap_err();
    match &err {
        AppError::SecretsFound(message) => {
            assert!(message.contains(&path), "{message}");
            assert!(!message.contains(&token()), "{message}");
        }
        other => panic!("expected SecretsFound, got {other:?}"),
    }
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["kind"], "secrets-found");
    assert!(git::log(&core, 10).unwrap().is_empty());

    // With a variable instead, the commit goes through.
    spec.headers = vec![bearer("{{token}}")];
    core.update_request(&path, spec).unwrap();
    core.save().unwrap();
    assert!(git::scan(&core, &paths).unwrap().is_empty());
    git::commit(&core, "Add Me", &paths).unwrap();
    assert_eq!(git::log(&core, 10).unwrap().len(), 1);

    // A deleted file has nothing to scan; paths outside the collection are refused.
    assert!(
        git::scan(&core, &["gone.yml".to_owned()])
            .unwrap()
            .is_empty()
    );
    assert!(git::scan(&core, &["../x.yml".to_owned()]).is_err());
}

#[test]
fn a_marked_line_can_be_committed() {
    let work = temp("allow");
    std::fs::write(
        work.0.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Scan\n",
    )
    .unwrap();
    // A documented example value, marked on its line.
    std::fs::write(
        work.0.join("example.yml"),
        format!(
            "info:\n  name: Example\n  type: http\n\nhttp:\n  method: GET\n  url: https://api.example.com/me\n  headers:\n    - name: X-Api-Key\n      value: {} # rt-scan:allow\n",
            token()
        ),
    )
    .unwrap();
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&work.0).unwrap();
    git::init(&core).unwrap();
    set_user(&work.0);
    let paths = vec!["opencollection.yml".to_owned(), "example.yml".to_owned()];
    assert!(git::scan(&core, &paths).unwrap().is_empty());
    git::commit(&core, "Example", &paths).unwrap();
}

// ---------------------------------------------------------------------------------------------
// External secrets

/// Answers `op read --no-newline op://Dev/api/<field>` with `<field>-value`, except `locked`
/// (not signed in); counts calls.
#[derive(Default)]
struct FakeOp(Mutex<Vec<String>>);

impl CommandRunner for FakeOp {
    fn run(&self, command: &SecretCommand, _: Duration) -> Result<Vec<u8>, RunError> {
        let reference = command.args.last().cloned().unwrap_or_default();
        self.0.lock().unwrap().push(reference.clone());
        match reference.rsplit('/').next() {
            Some("locked") => Err(RunError::Failed {
                code: Some(1),
                stderr: "[ERROR] You are not currently signed in.".into(),
            }),
            Some(field) => Ok(format!("{field}-{}", token()).into_bytes()),
            None => unreachable!(),
        }
    }
}

fn collection_with_external(dir: &Path, base: &str) {
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: External\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("environments")).unwrap();
    std::fs::write(
        dir.join("environments/Dev.yml"),
        format!(
            "name: Dev\nvariables:\n  - name: baseUrl\n    value: {base}\n  - name: shared\n    value: \"\"\n    secret: true\nexternalSecrets:\n  type: one-password\n  variables:\n    - name: token\n      secretReference: op://Dev/api/token\n    - name: shared\n      secretReference: op://Dev/api/shared\n"
        ),
    )
    .unwrap();
}

fn dev() -> SendInput {
    SendInput {
        environment: Some("Dev".into()),
        vars: Vec::new(),
        secrets: Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn external_secrets_are_read_before_a_send_and_the_keychain_wins() {
    let addr = rt_testserver::spawn_local().await;
    let dir = temp("external");
    collection_with_external(&dir.0, &format!("http://{addr}"));
    let mut core = Core::new(MemorySecrets::default()).unwrap();
    let fake = Arc::new(FakeOp::default());
    core.external_secrets = Arc::new(SecretResolver::new(fake.clone()));
    core.open(&dir.0).unwrap();
    // `shared` has a keychain value: that one is sent, and the CLI is not asked for it.
    let keychain = ["from-keychain-", &token()].concat();
    core.secret_set("Dev", "shared", &keychain).unwrap();

    let path = core.create_request("", "Echo").unwrap();
    let mut spec = core.get_request(&path).unwrap();
    spec.url = "{{baseUrl}}/echo/x".into();
    spec.headers = vec![
        bearer("{{token}}"),
        KeyValue {
            name: "X-Shared".into(),
            value: "{{shared}}".into(),
            enabled: true,
        },
    ];
    core.update_request(&path, spec).unwrap();

    // Preview never runs a CLI: before the first send `token` is unresolved.
    let preview = core.preview(&path, dev()).unwrap();
    assert_eq!(preview.unresolved, ["token"]);
    assert!(fake.0.lock().unwrap().is_empty());

    let stored = core.send(&path, dev()).await.unwrap();
    let response = stored.report.response.as_ref().expect("response");
    let body = match &response.body {
        rt_core::send::ReportBody::Utf8(text) => text.clone(),
        other => panic!("unexpected body {other:?}"),
    };
    assert!(body.contains(&format!("token-{}", token())), "{body}");
    assert!(body.contains(&keychain), "{body}");
    assert_eq!(*fake.0.lock().unwrap(), ["op://Dev/api/token"]);
    // Masked in what the UI sees.
    let request = serde_json::to_string(&stored.report.request).unwrap();
    assert!(!request.contains(&token()), "{request}");
    let token_var = stored
        .report
        .variables
        .iter()
        .find(|v| v.name == "token")
        .unwrap();
    assert!(token_var.secret);

    // A second send uses the session cache; preview now resolves (masked).
    core.send(&path, dev()).await.unwrap();
    assert_eq!(fake.0.lock().unwrap().len(), 1);
    let preview = core.preview(&path, dev()).unwrap();
    assert!(preview.unresolved.is_empty(), "{:?}", preview.unresolved);
    assert!(
        preview
            .headers
            .iter()
            .any(|(n, v)| n == "Authorization" && v == "Bearer ***")
    );

    // Nothing of it reaches the files.
    for change in core.save().unwrap() {
        assert!(!change.text.contains(&token()), "{}", change.path);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_that_cannot_be_read_stops_the_send_with_the_reason() {
    let dir = temp("locked");
    collection_with_external(&dir.0, "http://127.0.0.1:9");
    let mut core = Core::new(MemorySecrets::default()).unwrap();
    core.external_secrets = Arc::new(SecretResolver::new(Arc::new(FakeOp::default())));
    core.open(&dir.0).unwrap();
    let mut env: Environment = core
        .environments()
        .unwrap()
        .into_iter()
        .find(|e| e.name == "Dev")
        .unwrap();
    env.external_secrets = Some(ExternalSecrets {
        provider: SecretProvider::OnePassword,
        variables: vec![ExternalSecret {
            name: "token".into(),
            reference: "op://Dev/api/locked".into(),
            enabled: true,
        }],
    });
    env.variables.retain(|v: &Variable| v.name == "baseUrl");
    core.update_environment(env).unwrap();
    let path = core.create_request("", "Echo").unwrap();
    let err = core.send(&path, dev()).await.unwrap_err();
    let message = err.to_string();
    assert!(message.contains("1Password: not signed in"), "{message}");
    assert!(message.contains("op signin"), "{message}");
}
