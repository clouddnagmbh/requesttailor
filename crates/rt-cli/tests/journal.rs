// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand C: `rt journal`. The entries are device state in the data folder (here
//! written the way the app writes them); `revert` sends the request you name with the entry's
//! values filled in, against a local server.

#[path = "support/watch_server.rs"]
mod server;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use rt_app::journal::{FileStorage, JournalEntry, JournalStorage};

const SECRET: &str = "RT-CANARY-journal-cli-8c40";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-journal-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(root: &Path, port: u16) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Shop\n",
    );
    for env in ["dev", "prod"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
            ),
        );
    }
    write(
        &root.join("remove.yml"),
        "info:\n  name: Remove\n  type: http\n  seq: 1\n\nhttp:\n  method: DELETE\n  url: \"{{base}}/orders/{{journal.id}}?key={{token}}\"\n",
    );
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn entry(id: u64, environment: &str) -> JournalEntry {
    JournalEntry {
        id,
        session: "s-test".into(),
        at: now(),
        environment: Some(environment.into()),
        request_path: "create.yml".into(),
        method: "POST".into(),
        url: "http://127.0.0.1/orders?key=***".to_string(),
        status: 201,
        location: None,
        etag: None,
        resource_id: Some(id.to_string()),
        reverted_at: None,
    }
}

fn seed(dir: &Path, root: &Path, entries: &[JournalEntry]) {
    // The id `rt` computes (no `\\?\` prefix on Windows).
    let id = rt_cli::oauth::collection_id(root);
    let storage = FileStorage::new(&dir.join("data"), &id);
    for e in entries {
        storage.append(e);
    }
    storage.flush();
    let _: Arc<dyn JournalStorage> = Arc::new(storage);
}

fn rt(dir: &Path, root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("journal")
        .args(args)
        .arg("-C")
        .arg(root)
        .args(["--var", &format!("token={SECRET}")])
        .env("RT_DATA_DIR", dir.join("data"))
        .output()
        .unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn list_shows_the_entries_and_an_empty_journal_says_so() {
    let dir = tmp("list");
    let root = dir.join("c");
    collection(&root, 1);
    let o = rt(&dir, &root, &["list"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("journal is empty"), "{}", out(&o));
    seed(&dir, &root, &[entry(1, "dev"), entry(2, "dev")]);
    let o = rt(&dir, &root, &["list"]);
    let text = out(&o);
    assert!(
        text.contains("#2") && text.contains("POST") && text.contains("[dev]"),
        "{text}"
    );
    assert!(
        text.find("#2").unwrap() < text.find("#1").unwrap(),
        "newest first: {text}"
    );
    let o = rt(&dir, &root, &["list", "--json"]);
    let page: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(page["total"], 2);
    assert!(!out(&o).contains(SECRET) && !err(&o).contains(SECRET));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn revert_sends_the_named_request_audits_and_marks_the_entry() {
    let server = server::spawn();
    let dir = tmp("revert");
    let root = dir.join("c");
    collection(&root, server.port);
    seed(&dir, &root, &[entry(7, "dev")]);
    let o = rt(&dir, &root, &["revert", "7", "--with", "remove.yml"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    // The plan was shown first, masked.
    assert!(
        err(&o).contains("DELETE") && err(&o).contains("/orders/7?key=***"),
        "{}",
        err(&o)
    );
    assert!(out(&o).contains("#7: Reverted"), "{}", out(&o));
    let seen: Vec<String> = server
        .seen
        .lock()
        .unwrap()
        .iter()
        .map(|(_, l)| l.clone())
        .collect();
    assert_eq!(seen, [format!("DELETE /orders/7?key={SECRET}")]);
    // Marked in the journal and written to the audit log, without the secret.
    let list = out(&rt(&dir, &root, &["list"]));
    assert!(list.contains("(reverted)"), "{list}");
    let audit = std::fs::read_to_string(dir.join("data/agents/audit.jsonl")).unwrap();
    assert!(
        audit.contains("journal-revert") && audit.contains("\"agent\":\"cli\""),
        "{audit}"
    );
    for text in [out(&o), err(&o), audit] {
        assert!(
            !text.contains(SECRET),
            "a secret value was printed or logged"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn production_needs_yes_production_and_nothing_is_sent_without_it() {
    let server = server::spawn();
    let dir = tmp("prod");
    let root = dir.join("c");
    collection(&root, server.port);
    seed(&dir, &root, &[entry(3, "prod")]);
    let o = rt(&dir, &root, &["revert", "3", "--with", "remove.yml"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        err(&o).contains("PRODUCTION") && err(&o).contains("--yes-production"),
        "{}",
        err(&o)
    );
    assert!(server.seen.lock().unwrap().is_empty());
    let o = rt(
        &dir,
        &root,
        &["revert", "3", "--with", "remove.yml", "--yes-production"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(server.seen.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clear_and_usage_errors() {
    let dir = tmp("clear");
    let root = dir.join("c");
    collection(&root, 1);
    seed(&dir, &root, &[entry(1, "dev")]);
    let o = rt(&dir, &root, &["revert", "99", "--with", "remove.yml"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("no journal entry #99"));
    let o = rt(&dir, &root, &["revert", "1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("--with"));
    assert_eq!(rt(&dir, &root, &["clear"]).status.code(), Some(0));
    assert!(out(&rt(&dir, &root, &["list"])).contains("journal is empty"));
    let _ = std::fs::remove_dir_all(&dir);
}
