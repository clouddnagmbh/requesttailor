// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The test-data journal (wave 19, strand C): what is recorded, what is not, the ring, masking
//! and where it lives. The hook is global state, so the tests take turns.

mod journal_support;

use std::sync::{Arc, Mutex, MutexGuard};

use journal_support::{CANARY, NoFiles, TempDir, Wire, answer, block_on, collection, snapshot};
use rt_app::journal::{
    FileStorage, Journal, JournalEntry, JournalQuery, JournalSettings, QueueStorage, RING_ENTRIES,
    StorageOp, forget, journal_for, refresh,
};
use rt_core::prepare::PrepareInput;
use rt_core::send::{SendOptions, send_with};
use rt_core::services::Services;
use rt_format::FileSet;

static TURN: Mutex<()> = Mutex::new(());

fn turn() -> MutexGuard<'static, ()> {
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

fn send(files: &FileSet, name: &str, environment: &str, path: &str, wire: &Wire) {
    let snap = snapshot(files, environment);
    let input = PrepareInput {
        environment: Some(environment),
        globals: Vec::new(),
        runtime: Vec::new(),
    };
    let _ = name;
    block_on(send_with(
        &snap.collection,
        path,
        &input,
        &NoFiles,
        wire,
        &SendOptions::default(),
        &Services::none(),
    ));
}

fn on(name: &str, storage: Arc<dyn rt_app::journal::JournalStorage>) -> Arc<Journal> {
    let journal = journal_for(name, || storage);
    journal
        .set_settings(JournalSettings {
            enabled: true,
            id_path: Some("$.id".into()),
        })
        .unwrap();
    refresh();
    journal
}

fn all(journal: &Journal) -> Vec<JournalEntry> {
    let mut page = journal.entries(&JournalQuery {
        limit: 10_000,
        ..Default::default()
    });
    page.entries.reverse();
    page.entries
}

fn shop() -> (&'static str, FileSet) {
    let name = "journal-shop";
    (
        name,
        collection(
            name,
            &[
                ("create", "POST", "{{base}}/orders?key={{token}}"),
                ("read", "GET", "{{base}}/orders/1"),
                ("update", "PATCH", "{{base}}/orders/1"),
                ("fail", "POST", "{{base}}/fail"),
            ],
        ),
    )
}

fn server() -> Wire {
    Wire::new(|r| {
        if r.url.contains("/fail") {
            return answer(500, &[], "{\"error\":\"boom\"}");
        }
        match r.method.as_str() {
            "POST" => answer(
                201,
                &[
                    (
                        "Location",
                        &format!("{}/42?key={CANARY}", r.url.split('?').next().unwrap()),
                    ),
                    ("ETag", "\"v1\""),
                    ("X-Private", "private-header-value"),
                ],
                "{\"id\":42,\"note\":\"private-body-text\"}",
            ),
            _ => answer(
                200,
                &[("ETag", "\"v2\"")],
                "{\"id\":1,\"note\":\"private-body-text\"}",
            ),
        }
    })
}

#[test]
fn only_successful_writing_requests_are_kept() {
    let _t = turn();
    let (name, files) = shop();
    let journal = on(name, Arc::new(QueueStorage::default()));
    let wire = server();
    for path in ["create.yml", "read.yml", "update.yml", "fail.yml"] {
        send(&files, name, "dev", path, &wire);
    }
    let entries = all(&journal);
    let seen: Vec<_> = entries
        .iter()
        .map(|e| (e.method.as_str(), e.status))
        .collect();
    assert_eq!(seen, [("POST", 201), ("PATCH", 200)]);
    assert!(entries[0].id < entries[1].id);
    assert_eq!(entries[0].request_path, "create.yml");
    assert_eq!(entries[0].environment.as_deref(), Some("dev"));
    assert_eq!(entries[0].resource_id.as_deref(), Some("42"));
    assert_eq!(entries[0].etag.as_deref(), Some("\"v1\""));
    forget(name);
}

#[test]
fn no_bodies_no_header_values_and_nothing_secret() {
    let _t = turn();
    let (name, files) = shop();
    let dir = TempDir::new("masking");
    let journal = on(name, Arc::new(FileStorage::new(&dir.0, "/c/shop")));
    send(&files, name, "dev", "create.yml", &server());
    journal.flush();
    let entry = &all(&journal)[0];
    // The Location is a URL and is masked; so is the URL that went out.
    assert!(entry.location.as_deref().unwrap().contains("key=***"));
    assert!(entry.url.contains("key=***"));
    // What reaches the disk: no body, no header value but Location and ETag, no secret.
    let mut text = String::new();
    for file in std::fs::read_dir(dir.0.join("journal")).unwrap() {
        text.push_str(&std::fs::read_to_string(file.unwrap().path()).unwrap());
    }
    assert!(!text.is_empty());
    for forbidden in [
        CANARY,
        "private-body-text",
        "private-header-value",
        "X-Private",
    ] {
        assert!(
            !text.contains(forbidden),
            "{forbidden} reached the journal file"
        );
    }
    let json = serde_json::to_string(&journal.entries(&JournalQuery::default())).unwrap();
    assert!(!json.contains(CANARY) && !json.contains("private-body-text"));
    forget(name);
}

#[test]
fn the_ring_keeps_at_most_5000_entries() {
    let _t = turn();
    let (name, files) = shop();
    let journal = on(name, Arc::new(QueueStorage::default()));
    let wire = server();
    for _ in 0..RING_ENTRIES + 25 {
        send(&files, name, "dev", "update.yml", &wire);
    }
    let page = journal.entries(&JournalQuery::default());
    assert_eq!(page.total, RING_ENTRIES as u64);
    assert_eq!(page.entries.len(), 200);
    // The oldest were dropped, the newest stay; ids keep increasing.
    let newest = page.entries[0].id;
    assert_eq!(newest, (RING_ENTRIES + 25) as u64);
    let rest = journal.entries(&JournalQuery {
        limit: 10_000,
        before: Some(100),
        ..Default::default()
    });
    assert!(rest.entries.is_empty() || rest.entries.iter().all(|e| e.id < 100));
    assert_eq!(all(&journal)[0].id, 26);
    forget(name);
}

fn old_entry(id: u64, days: i64) -> JournalEntry {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    JournalEntry {
        id,
        session: "s-old".into(),
        at: now - days * 24 * 60 * 60 * 1000,
        environment: None,
        request_path: "x.yml".into(),
        method: "POST".into(),
        url: "http://x.test/".into(),
        status: 201,
        location: None,
        etag: None,
        resource_id: None,
        reverted_at: None,
    }
}

#[test]
fn entries_older_than_30_days_are_dropped_when_it_opens() {
    let _t = turn();
    let storage = Arc::new(QueueStorage::new(
        vec![
            old_entry(1, 40),
            old_entry(2, 31),
            old_entry(3, 29),
            old_entry(4, 1),
        ],
        JournalSettings::default(),
    ));
    let journal = Journal::open("journal-age", storage.clone());
    let ids: Vec<_> = all(&journal).iter().map(|e| e.id).collect();
    assert_eq!(ids, [3, 4]);
    // The next entry does not reuse an id.
    assert!(
        storage
            .drain()
            .iter()
            .any(|op| matches!(op, StorageOp::Rewrite(list) if list.len() == 2))
    );
}

#[test]
fn off_by_default_records_nothing_and_costs_no_sink() {
    let _t = turn();
    let (name, files) = shop();
    let journal = journal_for(name, || Arc::new(QueueStorage::default()));
    refresh();
    assert!(!journal.enabled());
    assert!(!rt_core::journal::enabled());
    send(&files, name, "dev", "create.yml", &server());
    assert!(all(&journal).is_empty());
    // Switching it on installs the hook, off removes it.
    journal
        .set_settings(JournalSettings {
            enabled: true,
            id_path: None,
        })
        .unwrap();
    refresh();
    assert!(rt_core::journal::enabled());
    journal.set_settings(JournalSettings::default()).unwrap();
    refresh();
    assert!(!rt_core::journal::enabled());
    forget(name);
}

#[test]
fn entries_survive_a_restart_but_the_switch_does_not() {
    let _t = turn();
    let (name, files) = shop();
    let dir = TempDir::new("restart");
    {
        let journal = on(name, Arc::new(FileStorage::new(&dir.0, "/c/restart")));
        send(&files, name, "dev", "create.yml", &server());
        journal.flush();
        forget(name);
    }
    let again = Journal::open(name, Arc::new(FileStorage::new(&dir.0, "/c/restart")));
    assert_eq!(all(&again).len(), 1);
    assert!(!again.enabled(), "tracking is on for the session only");
    assert_eq!(again.settings().id_path.as_deref(), Some("$.id"));
    // Later entries continue the numbering.
    assert!(all(&again)[0].id < again.entries(&JournalQuery::default()).entries[0].id + 1);
}

#[test]
fn the_id_can_come_from_a_variable_and_a_bad_path_is_refused() {
    let _t = turn();
    let (name, _files) = shop();
    let journal = journal_for(name, || Arc::new(QueueStorage::default()));
    assert!(
        journal
            .set_settings(JournalSettings {
                enabled: true,
                id_path: Some("$[".into())
            })
            .is_err()
    );
    assert!(
        journal
            .set_settings(JournalSettings {
                enabled: false,
                id_path: Some("var:orderId".into())
            })
            .is_ok()
    );
    forget(name);
}

#[test]
fn marking_reverted_and_clearing_work_and_states_stay_in_memory() {
    let _t = turn();
    let (name, files) = shop();
    let storage = Arc::new(QueueStorage::default());
    let journal = on(name, storage.clone());
    send(&files, name, "dev", "create.yml", &server());
    let id = all(&journal)[0].id;
    // What the write answered is kept in memory (masked), for compensators.
    let states = journal.states(&[id]);
    let after = states[0].after.as_ref().unwrap();
    assert_eq!(after.status, 201);
    assert!(after.body.as_deref().unwrap().contains("private-body-text"));
    // ... and never saved.
    let saved = format!("{:?}", storage.drain());
    assert!(!saved.contains("private-body-text"));
    assert!(journal.mark_reverted(id));
    assert!(all(&journal)[0].reverted_at.is_some());
    assert!(
        journal
            .entries(&JournalQuery {
                only_open: true,
                ..Default::default()
            })
            .entries
            .is_empty()
    );
    journal.clear();
    assert!(all(&journal).is_empty());
    assert!(journal.states(&[id])[0].after.is_none());
    forget(name);
}
