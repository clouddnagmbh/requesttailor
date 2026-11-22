// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand D, in the desktop shell without a window: the history of a request across
//! a rename, blame per field, an old version, and the test results recorded per commit in the
//! data folder (only for a run of exactly `HEAD`'s files; the repository stays clean).

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::git::GitFileChange;
use rt_app::timeline::results_file;
use rt_app::{AppError, SecretKey, SecretStore};
use rt_core::runner::{RunEvent, RunStep};
use rt_desktop::timeline::{self, RunRecorder};
use rt_desktop::{Core, git};

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
    let dir = std::env::temp_dir().join(format!("rt-desktop-tl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(rt_git::canonical(&dir).unwrap())
}

fn set_user(work: &Path, name: &str) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(work.join(".git/config"))
        .unwrap();
    write!(f, "[user]\n\tname = {name}\n\temail = {name}@example.com\n").unwrap();
}

fn request(url: &str, header: &str) -> String {
    format!(
        "info:\n  name: Users\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{url}\"\n  headers:\n    - name: X-Env\n      value: {header}\n  auth: inherit\n"
    )
}

fn commit(core: &Core<MemorySecrets>, message: &str, paths: &[&str]) -> String {
    let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
    git::commit(core, message, &paths).unwrap().oid
}

/// A finished request as the runner emits it.
fn step(request_path: &str, passed: bool) -> RunEvent {
    let step: RunStep = serde_json::from_value(serde_json::json!({
        "iteration": 0,
        "request_path": request_path,
        "request_name": "Users",
        "report": {
            "version": 1,
            "request_path": request_path,
            "environment": "Local",
            "request": null,
            "response": null,
            "variables": [],
            "unresolved": [],
            "error": null,
            "tests": [{ "name": "status is 200", "passed": passed, "origin": "request" }],
        },
        "passed": passed,
        "skipped": false,
    }))
    .unwrap();
    RunEvent::Step {
        step: Box::new(step),
    }
}

fn finished() -> RunEvent {
    serde_json::from_value(serde_json::json!({
        "kind": "finished",
        "summary": {
            "requests": 1, "passed": 1, "failed": 0, "skipped": 0,
            "tests_passed": 1, "tests_failed": 0,
            "assertions_passed": 0, "assertions_failed": 0,
            "duration_ms": 1, "stopped": false,
        }
    }))
    .unwrap()
}

fn record_run(core: &Core<MemorySecrets>, request_path: &str, passed: bool) {
    let recorder = RunRecorder::for_run(core);
    recorder.observe(&step(request_path, passed));
    recorder.observe(&finished());
}

struct Fixture {
    work: TempDir,
    data: TempDir,
    core: Core<MemorySecrets>,
    commits: Vec<String>,
}

/// Five commits touching `users/get.yml`, which is renamed to `api/list.yml` in the fourth.
fn fixture(name: &str) -> Fixture {
    let work = temp(&format!("{name}-work"));
    let data = temp(&format!("{name}-data"));
    let root = &work.0;
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: History\n",
    )
    .unwrap();
    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.0.clone())).unwrap();
    core.open(root).unwrap();
    git::init(&core).unwrap();
    set_user(root, "Ada");
    let put = |rel: &str, text: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let mut commits = Vec::new();
    put("users/get.yml", &request("{{baseUrl}}/users", "one"));
    commits.push(commit(
        &core,
        "add",
        &["opencollection.yml", "users/get.yml"],
    ));
    put("users/get.yml", &request("{{baseUrl}}/users?page=1", "one"));
    commits.push(commit(&core, "url", &["users/get.yml"]));
    put("users/get.yml", &request("{{baseUrl}}/users?page=1", "two"));
    commits.push(commit(&core, "header", &["users/get.yml"]));
    std::fs::remove_file(root.join("users/get.yml")).unwrap();
    put("api/list.yml", &request("{{baseUrl}}/users?page=1", "two"));
    commits.push(commit(&core, "move", &["users/get.yml", "api/list.yml"]));
    put(
        "api/list.yml",
        &request("{{baseUrl}}/v2/users?page=1", "two"),
    );
    commits.push(commit(&core, "v2", &["api/list.yml"]));
    core.reload().unwrap();
    Fixture {
        work,
        data,
        core,
        commits,
    }
}

#[test]
fn history_follows_a_rename_and_blame_names_the_commit_per_field() {
    let f = fixture("history");
    let t = timeline::history(&f.core, "api/list.yml", 50).unwrap();
    assert_eq!(t.path, "api/list.yml");
    let seen: Vec<(&str, GitFileChange, &str)> = t
        .entries
        .iter()
        .map(|e| {
            (
                e.commit.commit.oid.as_str(),
                e.commit.change,
                e.commit.path.as_str(),
            )
        })
        .collect();
    let c = &f.commits;
    assert_eq!(
        seen,
        vec![
            (c[4].as_str(), GitFileChange::Modified, "api/list.yml"),
            (c[3].as_str(), GitFileChange::Renamed, "api/list.yml"),
            (c[2].as_str(), GitFileChange::Modified, "users/get.yml"),
            (c[1].as_str(), GitFileChange::Modified, "users/get.yml"),
            (c[0].as_str(), GitFileChange::Added, "users/get.yml"),
        ]
    );
    assert_eq!(t.entries[1].commit.from.as_deref(), Some("users/get.yml"));
    // The rename changed no field; the header change is named.
    assert!(t.entries[1].fields.is_empty());
    assert_eq!(
        t.entries[2]
            .fields
            .iter()
            .map(|f| f.path.join("/"))
            .collect::<Vec<_>>(),
        ["headers/X-Env"]
    );

    let blame = timeline::blame(&f.core, "api/list.yml").unwrap();
    let by = |field: &[&str]| {
        blame
            .iter()
            .find(|b| b.field == field)
            .unwrap_or_else(|| panic!("no blame for {field:?}"))
            .commit
            .summary
            .clone()
    };
    assert_eq!(by(&["url"]), "v2");
    assert_eq!(by(&["headers", "X-Env"]), "header");
    assert_eq!(by(&["name"]), "add");

    // An old version, under the path it had then.
    let old = timeline::version(&f.core, &f.commits[0], "users/get.yml").unwrap();
    assert_eq!(old.url, "{{baseUrl}}/users");
    assert!(timeline::version(&f.core, &f.commits[0], "api/list.yml").is_err());
}

#[test]
fn only_a_run_of_head_is_recorded_and_the_repository_stays_clean() {
    let f = fixture("record");
    let head = &f.commits[4];
    let file = f.data.0.join(results_file(head));

    record_run(&f.core, "api/list.yml", true);
    assert!(file.is_file(), "{file:?} was not written");
    assert!(
        git::status(&f.core).unwrap().files.is_empty(),
        "the repository stays clean"
    );
    let results = timeline::results(&f.core, "HEAD").unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].passed && results[0].environment.as_deref() == Some("Local"));
    assert_eq!(results[0].tests[0].name, "status is 200");

    // The history shows it on the HEAD entry only.
    let t = timeline::history(&f.core, "api/list.yml", 50).unwrap();
    assert_eq!(t.entries[0].tests.len(), 1);
    assert!(t.entries[1..].iter().all(|e| e.tests.is_empty()));

    // At most 20 per request and commit; the oldest go.
    for _ in 0..25 {
        record_run(&f.core, "api/list.yml", false);
    }
    assert_eq!(timeline::results(&f.core, "HEAD").unwrap().len(), 20);

    // Unsaved changes: nothing is recorded.
    let before = std::fs::read_to_string(&file).unwrap();
    let mut spec = f.core.get_request("api/list.yml").unwrap();
    spec.url = "{{baseUrl}}/edited".into();
    f.core.update_request("api/list.yml", spec).unwrap();
    record_run(&f.core, "api/list.yml", true);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);

    // Saved but not committed: nothing is recorded either.
    f.core.save().unwrap();
    record_run(&f.core, "api/list.yml", true);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    // A new commit is a new key.
    let new = commit(&f.core, "edited", &["api/list.yml"]);
    record_run(&f.core, "api/list.yml", true);
    assert_eq!(timeline::results(&f.core, &new).unwrap().len(), 1);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    assert!(f.work.0.join(".git").is_dir());
}
