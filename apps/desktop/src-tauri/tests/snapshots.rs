// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests of the desktop shell (wave 17, strand B), through `Core` without a window:
//! freeze a kept response (the keychain secret of its environment becomes `***`), compare,
//! ignore, accept; nothing is on disk until `save`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rt_app::snapshots::SnapshotState;
use rt_app::{AppError, SecretKey, SecretStore, SendInput};
use rt_desktop::{Core, snapshots};

const SECRET: &str = "RT-CANARY-desktop-snapshot-1";

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

fn collection(dir: &std::path::Path, base: &str) -> PathBuf {
    let root = dir.join("collection");
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Snaps\n",
    )
    .unwrap();
    std::fs::write(
        root.join("environments/Local.yml"),
        format!("name: Local\nvariables:\n  - name: base\n    value: {base}\n  - name: n\n    value: \"1\"\n  - secret: true\n    name: token\n"),
    )
    .unwrap();
    std::fs::write(
        root.join("get.yml"),
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/echo/x?n={{n}}\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{token}}\"\n",
    )
    .unwrap();
    root
}

fn input(n: &str) -> SendInput {
    SendInput {
        environment: Some("Local".into()),
        vars: vec![("n".into(), n.into())],
        secrets: Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn freeze_check_ignore_and_save() {
    let addr = rt_testserver::spawn_local().await;
    let dir = std::env::temp_dir().join(format!("rt-desktop-snap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = collection(&dir, &format!("http://{addr}"));
    let core = Core::with_data_dir(MemorySecrets::default(), None).unwrap();
    core.open(&root).unwrap();
    core.secret_set("Local", "token", SECRET).unwrap();

    let first = core
        .send("get.yml", input("1"))
        .await
        .unwrap()
        .response_id
        .unwrap();
    assert_eq!(
        snapshots::check(&core, "get.yml", first).unwrap().state,
        SnapshotState::None
    );
    assert!(snapshots::read(&core, "get.yml").unwrap().is_none());

    let frozen = snapshots::freeze(&core, "get.yml", first, &Default::default()).unwrap();
    assert!(frozen.assertion_added);
    assert_eq!(frozen.snapshot, "get.snap.json");
    let text = serde_json::to_string(&frozen.content).unwrap();
    assert!(!text.contains(SECRET), "{text}");
    assert!(text.contains("Bearer ***"), "{text}");
    // Staged, not on disk.
    assert!(!root.join("get.snap.json").exists());
    assert_eq!(snapshots::list(&core).unwrap().len(), 1);
    assert_eq!(
        snapshots::check(&core, "get.yml", first).unwrap().state,
        SnapshotState::Matches
    );

    // A send now carries the assertion with the staged file; another query differs.
    let second = core.send("get.yml", input("2")).await.unwrap();
    assert!(
        !second.report.assertions[0].passed,
        "{:?}",
        second.report.assertions
    );
    let check = snapshots::check(&core, "get.yml", second.response_id.unwrap()).unwrap();
    assert_eq!(check.state, SnapshotState::Differs);
    assert_eq!(check.differences.len(), 1);
    assert_eq!(check.differences[0].path, "body.query");
    snapshots::set_ignore(&core, "get.yml", vec!["$.query".into()]).unwrap();
    let third = core.send("get.yml", input("3")).await.unwrap();
    assert!(
        third.report.assertions[0].passed,
        "{:?}",
        third.report.assertions
    );

    // Save writes both files, without the secret.
    let saved = core.save().unwrap();
    assert!(saved.iter().any(|c| c.path == "get.snap.json"));
    for change in &saved {
        assert!(!change.text.contains(SECRET), "{}", change.path);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
