// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Secret rescue in the desktop shell (wave 12, strand G), without a window, through `Core`
//! and the functions behind the Tauri commands (`rt_desktop::rescue`): the history scan finds
//! a token committed outside the app; the rewrite refuses with unsaved changes, then puts the
//! value into the keychain (a stand-in here) and never into the result, an error or the
//! webview; the reopened collection shows `{{githubToken}}`. Throwaway repositories only.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::rescue::RescuePlan;
use rt_app::{AppError, SecretKey, SecretStore};
use rt_desktop::{Core, rescue};

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
    let dir = std::env::temp_dir().join(format!("rt-desktop-rescue-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(rt_git::canonical(&dir).unwrap())
}

/// Assembled at run time, so secret scanners of the repository stay quiet.
fn token() -> String {
    ["glpat-", "R3scue", "Desk", "top0123456789"].concat()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_value_goes_to_the_keychain_and_nowhere_else() {
    let work = temp("work");
    let token = token();
    write(
        &work.0,
        "opencollection.yml",
        "opencollection: 1.0.0\ninfo:\n  name: Rescue\n",
    );
    write(
        &work.0,
        "get-user.yml",
        &format!(
            "info:\n  name: Get user\n  type: http\nhttp:\n  method: GET\n  url: https://api.example.com/users/1\n  headers:\n    - name: PRIVATE-TOKEN\n      value: {token}\n"
        ),
    );
    // Committed outside the app (the app's own commit refuses credentials).
    let options = rt_git::Options { isolated: true };
    rt_git::init(&work.0, &options).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(work.0.join(".git/config"))
        .unwrap()
        .write_all(b"[user]\n\tname = Ada\n\temail = ada@example.com\n")
        .unwrap();
    let repo = rt_git::Repo::require(&work.0, &options).unwrap();
    let first = repo
        .commit(
            "Add the request",
            &["opencollection.yml".to_owned(), "get-user.yml".to_owned()],
            None,
        )
        .unwrap();

    let core = Core::new(MemorySecrets::default()).unwrap();
    let info = core.open(&work.0).unwrap();

    let mut progress = Vec::new();
    let scan = rescue::scan(&core, &mut |p| progress.push(p)).unwrap();
    assert_eq!(scan.commits, 1);
    assert_eq!(scan.findings.len(), 1, "{scan:?}");
    assert_eq!(scan.findings[0].id, format!("{}:get-user.yml:9", first.oid));
    assert!(scan.findings[0].in_head);
    assert_eq!(progress.last().map(|p| (p.scanned, p.total)), Some((1, 1)));
    assert!(!serde_json::to_string(&scan).unwrap().contains(&token));

    let plan = RescuePlan {
        findings: vec![scan.findings[0].id.clone()],
        variable: "gitlabToken".into(),
        environment: "Local".into(),
    };
    let key = SecretKey {
        collection: info.id.clone(),
        environment: "Local".into(),
        variable: "gitlabToken".into(),
    };

    // Unsaved changes: refused, nothing stored.
    let draft = core.create_request("", "Draft").unwrap();
    let err = rescue::rewrite(&core, &plan, 1_795_000_000).unwrap_err();
    assert!(matches!(err, AppError::Git(_)), "{err:?}");
    assert!(!err.to_string().contains(&token));
    assert_eq!(core.secrets().get(&key).unwrap(), None);
    // Saved, the new request is an untracked file: that is fine.
    core.save().unwrap();
    assert!(work.0.join(&draft).exists());

    let result = rescue::rewrite(&core, &plan, 1_795_000_000).unwrap();
    assert_eq!(
        core.secrets().get(&key).unwrap().as_deref(),
        Some(token.as_str())
    );
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains(&token), "{json}");
    assert_eq!(result.rewritten, 1);
    assert_eq!(result.branches.len(), 1);
    assert!(!result.branches[0].pushed);

    // The reopened collection uses the variable.
    let spec = core.get_request("get-user.yml").unwrap();
    let header = spec
        .headers
        .iter()
        .find(|h| h.name == "PRIVATE-TOKEN")
        .unwrap();
    assert_eq!(header.value, "{{gitlabToken}}");
    assert!(!core.has_unsaved_changes());
    let again = rescue::scan(&core, &mut |_| {}).unwrap();
    assert!(again.findings.is_empty(), "{again:?}");
    // Nothing of the value in any file of the work tree.
    let file = std::fs::read_to_string(work.0.join("get-user.yml")).unwrap();
    assert!(!file.contains(&token), "{file}");
}

#[test]
fn without_a_collection_or_repository_it_refuses() {
    let core = Core::new(MemorySecrets::default()).unwrap();
    assert!(rescue::scan(&core, &mut |_| {}).is_err());
    let dir = temp("plain");
    write(
        &dir.0,
        "opencollection.yml",
        "opencollection: 1.0.0\ninfo:\n  name: Plain\n",
    );
    core.open(&dir.0).unwrap();
    let err = rescue::scan(&core, &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("not in a Git repository"), "{err}");
}
