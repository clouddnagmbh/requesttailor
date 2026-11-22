// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Time travel per request (`TimelineApi`, `rt_app::timeline`): file history with `rt-git`
//! (`Repo::file_log`, which follows renames), blame per field, old versions, and the test
//! results recorded per commit in the data folder (`rt_app::timeline::results_file`, never in
//! the repository).
//!
//! Wave 12, strand D. [`RunRecorder`] is called by every run of the runner (`runner.rs`). It
//! decides before the run whether the run uses exactly `HEAD`'s files: the repository has a
//! commit, nothing is unsaved in the app and no `*.yml` file of the collection is changed,
//! new or deleted in the working tree. Only then it keeps the finished requests (no bodies, no
//! header values) and writes them under `HEAD`'s commit when the run ends.
//!
//! Runs at a revision (compare, bisect; `compare.rs`) used exactly that commit's request files,
//! so [`RevisionRecorder`] writes them under the commit they ran at.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use rt_app::git::GitFileChange;
use rt_app::timeline::{
    self, FieldBlame, RecordSource, RequestTimeline, RunRecords, TestRecord, Version,
};
use rt_app::{AppError, SecretStore};
use rt_core::model::RequestSpec;
use rt_core::runner::RunEvent;
use rt_git::{Options, Repo};

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

type Result<T, E = AppError> = std::result::Result<T, E>;

/// Versions kept in a blame; older history is ignored (the oldest one stands for it).
const BLAME_LIMIT: u32 = 500;

fn repo<S: SecretStore>(core: &Core<S>) -> Result<Repo> {
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    Repo::require(&root, &Options::default())
}

/// The recorded results below the data folder (`None`: nothing is persisted, as in tests
/// without a data folder).
#[derive(Debug, Clone)]
pub struct ResultsDir(Option<PathBuf>);

impl ResultsDir {
    pub fn new(data_dir: Option<&Path>) -> Self {
        Self(data_dir.map(Path::to_path_buf))
    }

    fn file(&self, oid: &str) -> Option<PathBuf> {
        let name = timeline::results_file(oid);
        Some(self.0.as_ref()?.join(name))
    }

    /// The records under `oid` (all collections).
    pub fn read(&self, oid: &str) -> Vec<TestRecord> {
        self.file(oid)
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|t| timeline::parse_records(&t))
            .unwrap_or_default()
    }

    /// Adds `records` to the commit's file (at most 20 per request stay).
    pub fn append(&self, oid: &str, records: &[TestRecord]) -> Result<()> {
        let Some(file) = self.file(oid) else {
            return Ok(());
        };
        let io = |e: std::io::Error| AppError::Shell(format!("cannot keep the test results: {e}"));
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let existing = std::fs::read_to_string(&file).unwrap_or_default();
        let text = timeline::merge_records(&existing, records);
        // Written aside and renamed, so a crash never leaves a half-written file.
        let tmp = file.with_extension("jsonl.tmp");
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, &file).map_err(io)
    }
}

fn now() -> i64 {
    // Wall-clock time for a record, not an elapsed time (`web_time::Instant` is for those).
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Watches one run of the open collection and records its results under `HEAD` when the run
/// used exactly `HEAD`'s files (see the module docs). Created before the run starts.
pub struct RunRecorder {
    target: Option<Target>,
}

struct Target {
    results: ResultsDir,
    oid: String,
    collection: String,
    records: Mutex<RunRecords>,
}

impl RunRecorder {
    pub fn for_run<S: SecretStore>(core: &Core<S>) -> Self {
        Self {
            target: Target::of(core),
        }
    }

    /// Every event of the run, as the runner emits it.
    pub fn observe(&self, event: &RunEvent) {
        let Some(t) = &self.target else {
            return;
        };
        let mut records = t.records.lock().unwrap_or_else(PoisonError::into_inner);
        records.observe(&t.collection, now(), RecordSource::Runner, event);
        if matches!(event, RunEvent::Finished { .. }) {
            let all = std::mem::take(&mut *records).finish();
            drop(records);
            if !all.is_empty() {
                // Recording is a convenience: a full disk must not fail the run.
                let _ = t.results.append(&t.oid, &all);
            }
        }
    }
}

/// Records runs at a revision (compare and bisect, `compare.rs`) under the commit they ran
/// at, with their source.
pub struct RevisionRecorder {
    results: ResultsDir,
    collection: String,
    source: RecordSource,
}

impl RevisionRecorder {
    pub fn new(results: ResultsDir, collection: String, source: RecordSource) -> Self {
        Self {
            results,
            collection,
            source,
        }
    }

    /// The finished requests of one run at `oid` (skipped ones are left out).
    pub fn record(&self, oid: &str, steps: &[rt_core::runner::RunStep]) {
        let records = timeline::records_of_steps(&self.collection, now(), self.source, steps);
        if !records.is_empty() {
            // Recording is a convenience: a full disk must not fail the compare or bisect.
            let _ = self.results.append(oid, &records);
        }
    }
}

impl Target {
    fn of<S: SecretStore>(core: &Core<S>) -> Option<Self> {
        core.data_dir.as_ref()?;
        if core.has_unsaved_changes() {
            return None;
        }
        let repo = repo(core).ok()?;
        let oid = repo.rev_parse("HEAD").ok()?;
        let status = rt_git::status(&core.root()?, &Options::default()).ok()?;
        let clean = status.repo
            && !status.merging
            && status
                .files
                .iter()
                .all(|f| !rt_app::git::tree_path(&f.path));
        clean.then(|| Self {
            results: ResultsDir::new(core.data_dir.as_deref()),
            oid,
            collection: repo.collection_prefix(),
            records: Mutex::new(RunRecords::default()),
        })
    }
}

fn versions(repo: &Repo, path: &str, limit: u32) -> Result<Vec<Version>> {
    repo.file_log(path, limit)?
        .into_iter()
        .map(|commit| {
            let text = if commit.change == GitFileChange::Deleted {
                None
            } else {
                // A version that is no text file has no readable fields.
                repo.show(&commit.commit.oid, &commit.path).ok().flatten()
            };
            Ok(Version { commit, text })
        })
        .collect()
}

pub fn history<S: SecretStore>(core: &Core<S>, path: &str, limit: u32) -> Result<RequestTimeline> {
    let repo = repo(core)?;
    let versions = versions(&repo, path, limit)?;
    let results = ResultsDir::new(core.data_dir.as_deref());
    let records: BTreeMap<String, Vec<TestRecord>> = versions
        .iter()
        .map(|v| {
            (
                v.commit.commit.oid.clone(),
                results.read(&v.commit.commit.oid),
            )
        })
        .collect();
    Ok(timeline::build(
        path,
        &repo.collection_prefix(),
        &versions,
        &records,
    ))
}

pub fn blame<S: SecretStore>(core: &Core<S>, path: &str) -> Result<Vec<FieldBlame>> {
    Ok(timeline::blame(&versions(&repo(core)?, path, BLAME_LIMIT)?))
}

pub fn version<S: SecretStore>(core: &Core<S>, rev: &str, path: &str) -> Result<RequestSpec> {
    let text = repo(core)?
        .show(rev, path)?
        .ok_or_else(|| AppError::Git(format!("{path} does not exist at {rev}")))?;
    timeline::spec_of(&text)
}

pub fn results<S: SecretStore>(core: &Core<S>, rev: &str) -> Result<Vec<TestRecord>> {
    let repo = repo(core)?;
    let oid = repo.rev_parse(rev)?;
    let all = ResultsDir::new(core.data_dir.as_deref()).read(&oid);
    let collection = repo.collection_prefix();
    let mut mine: Vec<TestRecord> = all
        .into_iter()
        .filter(|r| r.collection == collection)
        .collect();
    mine.sort_by_key(|r| std::cmp::Reverse(r.time));
    Ok(mine)
}

async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppCore) -> Result<T> + Send + 'static,
) -> Result<T, CommandError> {
    use tauri::Manager as _;
    tokio::task::spawn_blocking(move || f(&app.state::<AppCore>()))
        .await
        .map_err(|e| AppError::Shell(format!("History failed: {e}")))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn timeline_history(
    app: tauri::AppHandle,
    path: String,
    limit: u32,
) -> Result<RequestTimeline, CommandError> {
    blocking(app, move |core| history(core, &path, limit)).await
}

#[tauri::command]
pub async fn timeline_blame(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<FieldBlame>, CommandError> {
    blocking(app, move |core| blame(core, &path)).await
}

#[tauri::command]
pub async fn timeline_version(
    app: tauri::AppHandle,
    rev: String,
    path: String,
) -> Result<RequestSpec, CommandError> {
    blocking(app, move |core| version(core, &rev, &path)).await
}

#[tauri::command]
pub async fn timeline_results(
    app: tauri::AppHandle,
    rev: String,
) -> Result<Vec<TestRecord>, CommandError> {
    blocking(app, move |core| results(core, &rev)).await
}
