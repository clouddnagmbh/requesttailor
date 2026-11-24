// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17: `ContractApi.stageSpec` on the desktop core. The spec the contract names is
//! written unsaved, in the collection folder or (with a leading `/`) at the repository root;
//! `save` writes it where it belongs, and nothing else is ever written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use rt_app::{AppError, SecretKey, SecretStore};
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

const SPEC: &str = "openapi: 3.0.3\ninfo:\n  title: Shop\n  version: 1.0.0\npaths:\n  /items:\n    get:\n      responses:\n        '200':\n          description: ok\n";
const NEWER: &str = "openapi: 3.0.3\ninfo:\n  title: Shop\n  version: 1.1.0\npaths:\n  /items:\n    get:\n      responses:\n        '200':\n          description: ok\n";

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rt-desktop-stage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(&dir).unwrap())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(dir: &Path, config: &str) {
    write(
        &dir.join("opencollection.yml"),
        &format!("opencollection: 1.0.0\ninfo:\n  name: Shop\n{config}"),
    );
}

fn core(dir: &Path) -> Core<MemorySecrets> {
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(dir).unwrap();
    core
}

#[test]
fn a_spec_in_the_collection_folder_is_staged_then_saved() {
    let tmp = TempDir::new("inner");
    collection(&tmp.0, "");
    write(&tmp.0.join("openapi.yaml"), SPEC);
    let core = core(&tmp.0);
    assert_eq!(core.stage_spec(NEWER).unwrap(), "openapi.yaml");
    assert_eq!(core.dirty().unwrap(), ["openapi.yaml"]);
    assert_eq!(
        std::fs::read_to_string(tmp.0.join("openapi.yaml")).unwrap(),
        SPEC
    );
    core.save().unwrap();
    assert_eq!(
        std::fs::read_to_string(tmp.0.join("openapi.yaml")).unwrap(),
        NEWER
    );
    assert!(core.dirty().unwrap().is_empty());
}

#[test]
fn a_spec_at_the_repository_root_is_written_there() {
    let tmp = TempDir::new("outer");
    let out = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&tmp.0)
        .output()
        .unwrap();
    assert!(out.status.success());
    let col = tmp.0.join("api");
    collection(&col, "");
    write(&tmp.0.join("openapi.yaml"), SPEC);
    let core = core(&col);
    assert_eq!(core.stage_spec(NEWER).unwrap(), "/openapi.yaml");
    assert_eq!(core.dirty().unwrap(), ["/openapi.yaml"]);
    assert_eq!(
        std::fs::read_to_string(tmp.0.join("openapi.yaml")).unwrap(),
        SPEC
    );
    let saved = core.save().unwrap();
    assert_eq!(
        saved.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        ["/openapi.yaml"]
    );
    assert_eq!(
        std::fs::read_to_string(tmp.0.join("openapi.yaml")).unwrap(),
        NEWER
    );
    assert!(
        !col.join("openapi.yaml").exists(),
        "nothing lands in the collection folder"
    );
    assert!(core.dirty().unwrap().is_empty());
}

#[test]
fn a_configured_spec_wins_and_a_missing_or_invalid_one_is_refused() {
    let tmp = TempDir::new("config");
    collection(
        &tmp.0,
        "extensions:\n  requesttailor:\n    contract:\n      spec: api/spec.yaml\n",
    );
    write(&tmp.0.join("openapi.yaml"), SPEC);
    let core = core(&tmp.0);
    // Configured but absent: the configuration is wrong; the default file is not written.
    let e = core.stage_spec(NEWER).unwrap_err().to_string();
    assert!(e.contains("does not exist"), "{e}");
    write(&tmp.0.join("api/spec.yaml"), SPEC);
    let core = self::core(&tmp.0);
    assert_eq!(core.stage_spec(NEWER).unwrap(), "api/spec.yaml");
    assert!(core.stage_spec("not: a spec").is_err());
    assert_eq!(core.dirty().unwrap(), ["api/spec.yaml"]);
    assert_eq!(
        std::fs::read_to_string(tmp.0.join("openapi.yaml")).unwrap(),
        SPEC
    );
}

#[test]
fn without_a_spec_nothing_is_written() {
    let tmp = TempDir::new("none");
    collection(&tmp.0, "");
    let core = core(&tmp.0);
    let e = core.stage_spec(NEWER).unwrap_err().to_string();
    assert!(e.contains("no OpenAPI spec"), "{e}");
    assert!(core.dirty().unwrap().is_empty());
}
