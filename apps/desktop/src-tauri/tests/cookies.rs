// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Cookie jar of the desktop shell, through `Core` without a window (wave 5, strand D):
//! cookies set by a response are stored per collection and environment, sent on the next
//! send, survive a restart, can be deleted and cleared, and never reach the collection
//! folder or the history.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::{AppError, SecretKey, SecretStore, SendInput};
use rt_core::model::RequestSpec;
use rt_desktop::Core;
use rt_desktop::history::HISTORY_FILE;

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
            std::env::temp_dir().join(format!("rt-desktop-cookies-{tag}-{}", std::process::id()));
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

fn collection(dir: &Path, base_url: &str) -> PathBuf {
    let root = dir.join("collection");
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Cookies\n",
    )
    .unwrap();
    for env in ["Local", "Prod"] {
        std::fs::write(
            root.join(format!("environments/{env}.yml")),
            format!("name: {env}\nvariables:\n  - name: base\n    value: {base_url}\n"),
        )
        .unwrap();
    }
    root
}

fn env(name: &str) -> SendInput {
    SendInput {
        environment: Some(name.into()),
        vars: Vec::new(),
        secrets: Vec::new(),
    }
}

fn add(core: &Core<MemorySecrets>, name: &str, url: &str) -> String {
    let path = core.create_request("", name).unwrap();
    let current = core.get_request(&path).unwrap();
    core.update_request(
        &path,
        RequestSpec {
            name: name.into(),
            seq: current.seq,
            method: "GET".into(),
            url: url.into(),
            ..RequestSpec::default()
        },
    )
    .unwrap();
    path
}

/// The `cookie` header the test server saw (`GET /cookies` echoes it).
async fn echoed(core: &Core<MemorySecrets>, path: &str, environment: &str) -> Option<String> {
    let sent = core.send(path, env(environment)).await.unwrap();
    let id = sent.response_id.expect("a response");
    let body = core.response_bytes(id, 0, 64 * 1024).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["cookie"].as_str().map(str::to_owned)
}

fn all_files(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            all_files(&path, out);
        } else {
            let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
            out.push((path, text));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cookies_are_stored_per_environment_sent_and_persisted() {
    let addr = rt_testserver::spawn_local().await;
    let tmp = TempDir::new("flow");
    let data = tmp.0.join("data");
    let root = collection(&tmp.0, &format!("http://{addr}"));

    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.clone())).unwrap();
    let info = core.open(&root).unwrap();
    let login = add(
        &core,
        "Login",
        "{{base}}/cookies/set?session=abc123&theme=dark",
    );
    let whoami = add(&core, "Who am I", "{{base}}/cookies");
    core.save().unwrap();

    // Nothing yet; the login sets two cookies in Local.
    assert_eq!(echoed(&core, &whoami, "Local").await, None);
    let sent = core.send(&login, env("Local")).await.unwrap();
    let set_cookie: Vec<&str> = sent
        .report
        .response
        .as_ref()
        .unwrap()
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("set-cookie"))
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(set_cookie, ["session=***; Path=/", "theme=***; Path=/"]);

    let listed = core.services.cookies.list(&info.id, "Local");
    let pairs: Vec<(&str, &str)> = listed
        .iter()
        .map(|c| (c.name.as_str(), c.value.as_str()))
        .collect();
    assert_eq!(pairs, [("session", "abc123"), ("theme", "dark")]);
    assert_eq!(listed[0].domain, "127.0.0.1");
    assert_eq!(listed[0].path, "/");

    // Sent on the next send in Local, not in Prod.
    assert_eq!(
        echoed(&core, &whoami, "Local").await.as_deref(),
        Some("session=abc123; theme=dark")
    );
    assert_eq!(echoed(&core, &whoami, "Prod").await, None);
    assert!(core.services.cookies.list(&info.id, "Prod").is_empty());

    // Survives a restart.
    drop(core);
    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.clone())).unwrap();
    core.open(&root).unwrap();
    assert_eq!(
        echoed(&core, &whoami, "Local").await.as_deref(),
        Some("session=abc123; theme=dark")
    );

    // Delete one, then clear.
    core.services
        .cookies
        .delete(&info.id, "Local", "127.0.0.1", "/", "session")
        .unwrap();
    assert_eq!(
        echoed(&core, &whoami, "Local").await.as_deref(),
        Some("theme=dark")
    );
    core.services.cookies.clear(&info.id, "Local").unwrap();
    assert_eq!(echoed(&core, &whoami, "Local").await, None);
    drop(core);
    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.clone())).unwrap();
    assert!(core.services.cookies.list(&info.id, "Local").is_empty());

    // The value was in the jar file only: not in the collection folder, not in the history.
    let mut files = Vec::new();
    all_files(&root, &mut files);
    assert!(files.len() >= 4, "{files:?}");
    // The login request's URL carries the value the test server sets; skip that file.
    for (path, text) in files.iter().filter(|(p, _)| !p.ends_with(&login)) {
        assert!(
            !text.contains("abc123"),
            "{} holds the cookie",
            path.display()
        );
    }
    let history = std::fs::read_to_string(data.join(HISTORY_FILE)).unwrap();
    assert!(history.contains("Login"), "{history}");
    // Only the login's own URL carries the value; the sends that carried the cookie do not.
    for line in history.lines().filter(|l| !l.contains("\"login.yml\"")) {
        assert!(!line.contains("abc123"), "{line}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn jars_are_per_collection_and_private() {
    let addr = rt_testserver::spawn_local().await;
    let tmp = TempDir::new("collections");
    let data = tmp.0.join("data");
    let a = collection(&tmp.0.join("a"), &format!("http://{addr}"));
    let b = collection(&tmp.0.join("b"), &format!("http://{addr}"));

    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.clone())).unwrap();
    let info_a = core.open(&a).unwrap();
    let login = add(&core, "Login", "{{base}}/cookies/set?sid=one");
    core.send(&login, env("Local")).await.unwrap();

    let info_b = core.open(&b).unwrap();
    let whoami = add(&core, "Who am I", "{{base}}/cookies");
    assert_eq!(echoed(&core, &whoami, "Local").await, None);
    assert_eq!(core.services.cookies.list(&info_a.id, "Local").len(), 1);
    assert!(core.services.cookies.list(&info_b.id, "Local").is_empty());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(data.join("cookies.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "cookie file readable by others: {mode:o}");
    }
}
