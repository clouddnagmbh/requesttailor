// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Recent collections of the desktop app (strand C of wave 4): the list survives a restart,
//! the last opened comes first, and `open_recent` opens only listed folders that still exist.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::{AppError, SecretKey, SecretStore};
use rt_desktop::recent::{FILE_NAME, MAX_RECENT, Recent, open_recent_in};
use rt_desktop::{CollectionInfo, Core};

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
        let dir = std::env::temp_dir().join(format!(
            "rt-desktop-recent-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn info(id: &str) -> CollectionInfo {
    CollectionInfo {
        id: id.into(),
        name: format!("Name of {id}"),
        location: id.into(),
    }
}

fn ids(recent: &Recent) -> Vec<String> {
    recent.list().into_iter().map(|r| r.info.id).collect()
}

/// A collection folder named `name` in `parent`.
fn collection(parent: &Path, name: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        format!("opencollection: \"1.0.0\"\ninfo:\n  name: {name}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("health.yml"),
        "info:\n  name: Health\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://127.0.0.1:9/\"\n",
    )
    .unwrap();
    dir
}

#[test]
fn the_last_opened_comes_first_and_the_list_survives_a_restart() {
    let data = TempDir::new("order");
    let recent = Recent::in_dir(Some(data.path()));
    assert!(recent.list().is_empty());
    recent.opened(&info("/a"));
    recent.opened(&info("/b"));
    recent.opened(&info("/a"));
    assert_eq!(ids(&recent), ["/a", "/b"]);
    let first = &recent.list()[0];
    assert_eq!(first.info.name, "Name of /a");
    assert!(first.opened_at > 1_700_000_000_000, "unix milliseconds");

    let restarted = Recent::in_dir(Some(data.path()));
    assert_eq!(ids(&restarted), ["/a", "/b"]);
    assert_eq!(restarted.list(), recent.list());
}

#[test]
fn the_list_is_bounded() {
    let data = TempDir::new("bounded");
    let recent = Recent::in_dir(Some(data.path()));
    for i in 0..MAX_RECENT + 5 {
        recent.opened(&info(&format!("/c{i}")));
    }
    let list = ids(&recent);
    assert_eq!(list.len(), MAX_RECENT);
    assert_eq!(list[0], format!("/c{}", MAX_RECENT + 4));
    assert_eq!(ids(&Recent::in_dir(Some(data.path()))), list);
}

#[test]
fn an_unreadable_file_is_an_empty_list_and_gets_replaced() {
    let data = TempDir::new("corrupt");
    std::fs::write(data.path().join(FILE_NAME), "{ not json").unwrap();
    let recent = Recent::in_dir(Some(data.path()));
    assert!(recent.list().is_empty());
    recent.opened(&info("/a"));
    assert_eq!(ids(&Recent::in_dir(Some(data.path()))), ["/a"]);
}

#[test]
fn without_a_data_folder_nothing_is_persisted() {
    let recent = Recent::in_dir(None);
    recent.opened(&info("/a"));
    assert_eq!(ids(&recent), ["/a"]);
    assert!(Recent::in_dir(None).list().is_empty());
}

#[test]
fn serializes_like_the_ts_recent_collection() {
    let recent = Recent::in_dir(None);
    recent.opened(&info("/a"));
    let json = serde_json::to_value(&recent.list()[0]).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.len(), 4);
    for k in ["id", "name", "location", "openedAt"] {
        assert!(keys.contains(&k), "{k} missing in {json}");
    }
}

#[test]
fn opening_a_collection_records_it_and_open_recent_reopens_it_after_a_restart() {
    let data = TempDir::new("reopen-data");
    let folders = TempDir::new("reopen-folders");
    let root = collection(folders.path(), "Reopened");

    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.path().into())).unwrap();
    let opened = core.open(&root).unwrap();
    assert_eq!(core.recent.list()[0].info, opened);
    drop(core);

    // "Restart": a new core over the same data folder.
    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.path().into())).unwrap();
    let last = core.recent.list()[0].info.clone();
    let again = open_recent_in(&core, &last.id).unwrap().expect("reopened");
    assert_eq!(again, opened);
    assert_eq!(again.name, "Reopened");
    assert!(core.tree().is_ok());
}

#[test]
fn open_recent_opens_only_listed_folders() {
    let folders = TempDir::new("unlisted");
    let root = collection(folders.path(), "Unlisted");
    let core = Core::new(MemorySecrets::default()).unwrap();
    let id = std::fs::canonicalize(&root).unwrap();
    assert_eq!(open_recent_in(&core, &id.to_string_lossy()).unwrap(), None);
    assert!(core.tree().is_err(), "nothing was opened");
}

#[test]
fn a_folder_that_is_gone_is_dropped_from_the_list() {
    let folders = TempDir::new("gone");
    let keep = collection(folders.path(), "Keep");
    let gone = collection(folders.path(), "Gone");
    let core = Core::new(MemorySecrets::default()).unwrap();
    let keep = core.open(&keep).unwrap();
    let gone = core.open(&gone).unwrap();
    std::fs::remove_dir_all(&gone.id).unwrap();

    assert_eq!(open_recent_in(&core, &gone.id).unwrap(), None);
    assert_eq!(ids(&core.recent), [keep.id.as_str()]);
    assert_eq!(open_recent_in(&core, &keep.id).unwrap(), Some(keep));
}
