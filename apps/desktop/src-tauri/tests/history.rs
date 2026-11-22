// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! History and search of the desktop shell, through `Core` without a window (wave 4,
//! strand E): every send is recorded in `history.jsonl` in the data folder, survives a
//! restart, tolerates a broken last line, is cleared per collection, is pruned, and never
//! contains a secret value. Search sees unsaved changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::history::HISTORY_LIMIT;
use rt_app::{AppError, HistoryEntry, SearchField, SecretKey, SecretStore, SendInput};
use rt_core::model::{ApiKeyPlacement, Auth, KeyValue, RequestSpec};
use rt_core::send::SendReport;
use rt_desktop::Core;
use rt_desktop::history::{DesktopHistory, HISTORY_FILE};

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
            std::env::temp_dir().join(format!("rt-desktop-history-{tag}-{}", std::process::id()));
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

const SECRET: &str = "s3cr3tDesktopHist";

/// A collection in `dir/collection` whose `Local` environment points at `base_url` and has
/// the secret variables `token` and `bad`.
fn collection(dir: &Path, base_url: &str) -> PathBuf {
    let root = dir.join("collection");
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: History\n",
    )
    .unwrap();
    std::fs::write(
        root.join("environments/Local.yml"),
        format!(
            "name: Local\nvariables:\n  - name: base\n    value: {base_url}\n  - secret: true\n    name: token\n  - secret: true\n    name: bad\n"
        ),
    )
    .unwrap();
    root
}

fn local() -> SendInput {
    SendInput {
        environment: Some("Local".into()),
        vars: Vec::new(),
        // Never trusted from the webview: the core replaces them with the keychain's.
        secrets: Vec::new(),
    }
}

fn history_file(data: &Path) -> String {
    std::fs::read_to_string(data.join(HISTORY_FILE)).unwrap_or_default()
}

fn assert_clean(what: &str, text: &str) {
    for form in [SECRET, "s3cr3t"] {
        assert!(!text.contains(form), "{what} contains {form:?}: {text}");
    }
}

/// Creates a request named `name` with `spec`'s method, URL, headers and auth.
fn add(core: &Core<MemorySecrets>, name: &str, spec: RequestSpec) -> String {
    let path = core.create_request("", name).unwrap();
    let current = core.get_request(&path).unwrap();
    core.update_request(
        &path,
        RequestSpec {
            name: name.into(),
            seq: current.seq,
            ..spec
        },
    )
    .unwrap();
    path
}

fn get(url: &str) -> RequestSpec {
    RequestSpec {
        method: "GET".into(),
        url: url.into(),
        ..RequestSpec::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_send_is_recorded_persisted_and_free_of_secrets() {
    let addr = rt_testserver::spawn_local().await;
    let tmp = TempDir::new("send");
    let data = tmp.0.join("data");
    let root = collection(&tmp.0, &format!("http://{addr}"));

    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.clone())).unwrap();
    let info = core.open(&root).unwrap();
    core.secret_set("Local", "token", SECRET).unwrap();
    core.secret_set("Local", "bad", &format!("{SECRET}\nbroken"))
        .unwrap();

    let cases: Vec<(&str, RequestSpec, Option<u16>)> = vec![
        ("Query", get("{{base}}/echo/q?key={{token}}"), Some(200)),
        (
            "Path and bearer",
            RequestSpec {
                auth: Auth::Bearer {
                    token: "{{token}}".into(),
                },
                ..get("{{base}}/bearer/{{token}}")
            },
            Some(200),
        ),
        (
            "Header",
            RequestSpec {
                headers: vec![KeyValue::new("X-Token", "{{token}}")],
                ..get("{{base}}/echo/h")
            },
            Some(200),
        ),
        (
            "Basic",
            RequestSpec {
                auth: Auth::Basic {
                    username: "ada".into(),
                    password: "{{token}}".into(),
                },
                ..get("{{base}}/basic-auth/ada/{{token}}")
            },
            Some(200),
        ),
        (
            "API key",
            RequestSpec {
                auth: Auth::ApiKey {
                    key: "api_key".into(),
                    value: "{{token}}".into(),
                    placement: ApiKeyPlacement::Query,
                },
                ..get("{{base}}/status/500")
            },
            Some(500),
        ),
        ("Bad scheme", get("ftp://example.test/{{token}}"), None),
        (
            "Bad header",
            RequestSpec {
                headers: vec![KeyValue::new("X-Bad", "{{bad}}")],
                ..get("{{base}}/echo/b")
            },
            None,
        ),
    ];
    for (name, spec, status) in &cases {
        let path = add(&core, name, spec.clone());
        let sent = core.send(&path, local()).await.unwrap();
        let got = sent.report.response.as_ref().map(|r| r.status);
        assert_eq!(got, *status, "{name}: {:?}", sent.report);
    }

    // Newest first, all of them, with what was sent.
    let listed = core.history.list(&info.id, "", 100);
    let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Bad header",
            "Bad scheme",
            "API key",
            "Basic",
            "Header",
            "Path and bearer",
            "Query"
        ]
    );
    assert!(listed.iter().all(|e| e.collection == info.id));
    assert!(
        listed
            .iter()
            .all(|e| e.environment.as_deref() == Some("Local"))
    );
    assert_eq!(listed[2].status, Some(500));
    assert!(listed[0].error.is_some() && listed[1].error.is_some());
    assert_eq!(
        listed[6].url.as_deref(),
        Some(format!("http://{addr}/echo/q?key=***").as_str())
    );
    assert!(listed[6].duration_ms.is_some_and(|ms| ms >= 0.0));
    assert_eq!(
        names_of(&core.history.list(&info.id, "5xx", 100)),
        ["API key"]
    );

    // Nothing secret in the file or the list output.
    let file = history_file(&data);
    assert_eq!(file.lines().count(), cases.len());
    assert_clean("history.jsonl", &file);
    assert_clean("list", &serde_json::to_string(&listed).unwrap());

    // A restart reads the history back.
    drop(core);
    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.clone())).unwrap();
    core.open(&root).unwrap();
    assert_eq!(core.history.list(&info.id, "", 100), listed);

    // Clearing empties the file for this collection.
    core.history.clear(&info.id).unwrap();
    assert!(core.history.list(&info.id, "", 100).is_empty());
    assert_eq!(history_file(&data), "");
}

fn names_of(entries: &[HistoryEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

fn report(path: &str) -> SendReport {
    serde_json::from_value(serde_json::json!({
        "version": 1, "request_path": path, "environment": null,
        "request": { "method": "GET", "url": "http://h/x", "headers": [], "body_size": 0 },
        "response": null, "variables": [], "unresolved": [],
        "error": { "kind": "transport", "message": { "kind": "timeout" } }
    }))
    .unwrap()
}

#[test]
fn a_broken_last_line_is_skipped_and_repaired() {
    let tmp = TempDir::new("broken");
    let h = DesktopHistory::new(Some(&tmp.0));
    h.record("c", "One", "GET", &report("one.yml"));
    h.record("c", "Two", "GET", &report("two.yml"));
    drop(h);
    // A crash in the middle of appending the third line.
    let path = tmp.0.join(HISTORY_FILE);
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("{\"id\":3,\"at\":1,\"coll");
    std::fs::write(&path, &text).unwrap();

    let h = DesktopHistory::new(Some(&tmp.0));
    assert_eq!(names_of(&h.list("c", "", 10)), ["Two", "One"]);
    h.record("c", "Three", "GET", &report("three.yml"));
    drop(h);
    let h = DesktopHistory::new(Some(&tmp.0));
    assert_eq!(names_of(&h.list("c", "", 10)), ["Three", "Two", "One"]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 3);
    for line in text.lines() {
        serde_json::from_str::<HistoryEntry>(line).unwrap();
    }
}

#[test]
fn garbage_lines_and_a_missing_folder_are_tolerated() {
    let tmp = TempDir::new("garbage");
    std::fs::write(tmp.0.join(HISTORY_FILE), "not json\n\n{}\n").unwrap();
    let h = DesktopHistory::new(Some(&tmp.0));
    assert!(h.list("c", "", 10).is_empty());
    // The data folder does not exist yet: it is created on the first write.
    let h = DesktopHistory::new(Some(&tmp.0.join("not/yet")));
    h.record("c", "One", "GET", &report("one.yml"));
    assert_eq!(
        std::fs::read_to_string(tmp.0.join("not/yet").join(HISTORY_FILE))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn clear_keeps_other_collections_and_prune_rewrites_the_file() {
    let tmp = TempDir::new("prune");
    let h = DesktopHistory::new(Some(&tmp.0));
    h.record("other", "Keep", "GET", &report("k.yml"));
    for i in 0..HISTORY_LIMIT {
        h.record("c", &format!("R{i}"), "GET", &report("r.yml"));
    }
    let path = tmp.0.join(HISTORY_FILE);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), HISTORY_LIMIT);
    // The oldest ("Keep") was dropped from the file, not just from memory.
    assert!(!text.contains("\"Keep\""));
    assert_eq!(
        h.list("c", "", 1)[0].name,
        format!("R{}", HISTORY_LIMIT - 1)
    );

    h.record("other", "Other", "GET", &report("o.yml"));
    h.clear("c").unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 1);
    let h = DesktopHistory::new(Some(&tmp.0));
    assert_eq!(names_of(&h.list("other", "", 10)), ["Other"]);
}

#[test]
fn without_a_data_folder_nothing_is_written() {
    let h = DesktopHistory::new(None);
    h.record("c", "One", "GET", &report("one.yml"));
    assert_eq!(names_of(&h.list("c", "", 10)), ["One"]);
    h.clear("c").unwrap();
    assert!(h.list("c", "", 10).is_empty());
}

#[test]
fn search_through_core_sees_unsaved_changes() {
    let tmp = TempDir::new("search");
    let root = collection(&tmp.0, "http://127.0.0.1:1");
    let core = Core::new(MemorySecrets::default()).unwrap();
    assert!(matches!(core.search("x", 10), Err(AppError::Shell(_))));
    core.open(&root).unwrap();
    let path = add(&core, "Find me", get("{{base}}/needle"));
    let hits = core.search("NEEDLE", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, path);
    assert_eq!(hits[0].field, SearchField::Url);
    assert_eq!(hits[0].ranges, [(9, 15)]);
    assert_eq!(core.search("find", 10).unwrap()[0].field, SearchField::Name);
}
