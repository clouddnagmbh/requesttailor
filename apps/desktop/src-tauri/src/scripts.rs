// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts in the desktop app (wave 7, strand A owns this file): trust per collection
//! (`trusted-scripts.json` in the data folder) and the script runner for sends
//! (`rt_script::Sandbox`). Commands: `scripts_trust`, `scripts_set_trusted`.
//!
//! A collection's scripts run only after the user trusted it (the banner of
//! `TrustPrompt.svelte`). A send from an untrusted collection runs without scripts and carries
//! the note `rt_app::scripts::UNTRUSTED_NOTE` if the request has scripts.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::AppError;
use rt_app::scripts::ScriptTrust;
use rt_core::model::Collection;
use rt_core::send::SendReport;
use rt_core::services::ScriptRunner;
use rt_script::Sandbox;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{AppCore, CommandError};

/// File in the data folder with the ids of the trusted collections.
pub const TRUST_FILE: &str = "trusted-scripts.json";

#[tauri::command]
pub async fn scripts_trust(core: State<'_, AppCore>) -> Result<ScriptTrust, CommandError> {
    Ok(trust_of(&core))
}

#[tauri::command]
pub async fn scripts_set_trusted(
    core: State<'_, AppCore>,
    trusted: bool,
) -> Result<(), CommandError> {
    let id = core
        .collection_id()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    core.services.scripts.set_trusted(&id, trusted)?;
    Ok(())
}

fn trust_of(core: &AppCore) -> ScriptTrust {
    let trusted = core
        .collection_id()
        .is_some_and(|id| core.services.scripts.is_trusted(&id));
    ScriptTrust {
        has_scripts: core.has_scripts(),
        trusted,
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustFile {
    version: u32,
    /// `CollectionInfo::id` of every trusted collection.
    trusted: BTreeSet<String>,
}

/// Long-lived script state: the trust store and the sandbox (the engine itself is compiled
/// once per process, on the first script).
pub struct ScriptParts {
    file: Option<PathBuf>,
    trusted: Mutex<BTreeSet<String>>,
    sandbox: Sandbox,
}

impl ScriptParts {
    /// `data_dir`: where `trusted-scripts.json` lives; `None` keeps trust in memory (tests).
    pub fn new(data_dir: Option<&Path>) -> Self {
        let file = data_dir.map(|d| d.join(TRUST_FILE));
        let trusted = file.as_deref().map(read_trust).unwrap_or_default();
        Self {
            file,
            trusted: Mutex::new(trusted),
            sandbox: Sandbox::default(),
        }
    }

    pub fn is_trusted(&self, collection: &str) -> bool {
        lock(&self.trusted).contains(collection)
    }

    /// Trusts (or stops trusting) `collection` and writes the trust file.
    pub fn set_trusted(&self, collection: &str, trusted: bool) -> Result<(), AppError> {
        let mut set = lock(&self.trusted);
        let changed = if trusted {
            set.insert(collection.to_owned())
        } else {
            set.remove(collection)
        };
        if let (true, Some(file)) = (changed, &self.file) {
            let json = serde_json::to_vec_pretty(&TrustFile {
                version: 1,
                trusted: set.clone(),
            })
            .map_err(|e| AppError::Shell(e.to_string()))?;
            write_atomic(file, &json)
                .map_err(|e| AppError::Shell(format!("cannot write {}: {e}", file.display())))?;
        }
        Ok(())
    }

    /// The runner for a send from `collection`: the sandbox if the collection is trusted.
    pub fn for_send(&self, collection: &str) -> ScriptSend<'_> {
        ScriptSend {
            sandbox: self.is_trusted(collection).then_some(&self.sandbox),
        }
    }

    /// After a send from `collection` (id) whose model is `model`: notes that scripts did not
    /// run if the collection is untrusted and the request has scripts.
    pub fn note_untrusted(&self, collection: &str, model: &Collection, report: &mut SendReport) {
        if !self.is_trusted(collection) {
            rt_app::scripts::note_untrusted(report, model);
        }
    }
}

pub struct ScriptSend<'a> {
    sandbox: Option<&'a Sandbox>,
}

impl ScriptSend<'_> {
    pub fn runner(&self) -> Option<&dyn ScriptRunner> {
        self.sandbox.map(|s| s as &dyn ScriptRunner)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A missing or unreadable file trusts nothing (scripts stay off; the user is asked again).
fn read_trust(file: &Path) -> BTreeSet<String> {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<TrustFile>(&bytes).ok())
        .map(|f| f.trusted)
        .unwrap_or_default()
}

fn write_atomic(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::model::ScriptPhase;
    use rt_core::script::{ScriptCall, ScriptInfo, ScriptRequest, ScriptSource};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rt-desktop-scripts-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn trust_is_kept_in_the_data_folder() {
        let dir = temp_dir();
        let parts = ScriptParts::new(Some(&dir));
        assert!(!parts.is_trusted("/a"));
        assert!(parts.for_send("/a").runner().is_none());
        parts.set_trusted("/a", true).unwrap();
        parts.set_trusted("/b", true).unwrap();
        assert!(parts.for_send("/a").runner().is_some());

        let again = ScriptParts::new(Some(&dir));
        assert!(again.is_trusted("/a") && again.is_trusted("/b"));
        again.set_trusted("/b", false).unwrap();
        let third = ScriptParts::new(Some(&dir));
        assert!(third.is_trusted("/a"));
        assert!(!third.is_trusted("/b"));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join(TRUST_FILE)).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({ "version": 1, "trusted": ["/a"] }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_trust_file_trusts_nothing() {
        let dir = temp_dir();
        std::fs::write(dir.join(TRUST_FILE), b"{ not json").unwrap();
        let parts = ScriptParts::new(Some(&dir));
        assert!(!parts.is_trusted("/a"));
        parts.set_trusted("/a", true).unwrap();
        assert!(ScriptParts::new(Some(&dir)).is_trusted("/a"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn untrusted_sends_carry_the_note() {
        let files = rt_format::FileSet {
            files: [
                (
                    "opencollection.yml",
                    "opencollection: \"1.0.0\"\ninfo:\n  name: c\n",
                ),
                (
                    "a.yml",
                    "info:\n  name: a\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://h/\n\nruntime:\n  scripts:\n    - type: before-request\n      code: console.log(1)\n",
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.as_bytes().to_vec()))
            .collect(),
        };
        let model = rt_format::read_collection(&files).unwrap().model().clone();
        let parts = ScriptParts::new(None);
        let report = || SendReport {
            version: 1,
            request_path: "a.yml".into(),
            environment: None,
            request: None,
            response: None,
            variables: Vec::new(),
            unresolved: Vec::new(),
            error: None,
            notes: Vec::new(),
            tests: Vec::new(),
            assertions: Vec::new(),
            logs: Vec::new(),
            var_changes: Vec::new(),
            flow: None,
            segments: Vec::new(),
            rate_limit: None,
            trace: None,
            checks: Vec::new(),
        };
        let mut r = report();
        parts.note_untrusted("/c", &model, &mut r);
        assert_eq!(r.notes.len(), 1);
        assert_eq!(r.notes[0].source, "script");
        assert_eq!(r.notes[0].text, rt_app::scripts::UNTRUSTED_NOTE);
        parts.set_trusted("/c", true).unwrap();
        let mut r = report();
        parts.note_untrusted("/c", &model, &mut r);
        assert!(r.notes.is_empty());
    }

    /// The desktop runs the same sandbox; an endless loop ends at the time limit.
    #[test]
    fn an_endless_loop_stops_at_the_time_limit() {
        let parts = ScriptParts::new(None);
        parts.set_trusted("/c", true).unwrap();
        let send = parts.for_send("/c");
        let runner = send.runner().unwrap();
        let started = std::time::Instant::now();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime.block_on(runner.run(ScriptCall {
            phase: ScriptPhase::BeforeRequest,
            scripts: vec![ScriptSource {
                origin: "request".into(),
                code: "console.log('start'); while (true) {}".into(),
            }],
            request: ScriptRequest {
                method: "GET".into(),
                url: "http://h/".into(),
                headers: Vec::new(),
                body: None,
            },
            response: None,
            variables: Vec::new(),
            info: ScriptInfo::default(),
            sender: None,
        }));
        let elapsed = started.elapsed();
        assert_eq!(
            outcome.error.as_deref(),
            Some("Script stopped: time limit of 5 s exceeded")
        );
        assert_eq!(outcome.logs.len(), 1);
        assert!(
            elapsed >= std::time::Duration::from_millis(4900)
                && elapsed < std::time::Duration::from_secs(8),
            "{elapsed:?}"
        );
    }
}
