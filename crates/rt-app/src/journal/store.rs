// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The journal's memory and storage (wave 19, strand C); rules at the module documentation of
//! [`super`].
//!
//! Storage is device state: `journal/<key>.jsonl` (one entry per line) and
//! `journal/<key>.settings.json` in the data folder; the web shell keeps the same lines in
//! IndexedDB (it drains a [`QueueStorage`]). Nothing here is ever written into a collection
//! file or into the response memory.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use rt_core::observe::Exchange;
use rt_core::send::ReportBody;

use super::{
    EntryStates, JournalEntry, JournalPage, JournalQuery, JournalSettings, PreRead, RING_DAYS,
    RING_ENTRIES,
};

/// Entries of one page when the query names no limit.
const DEFAULT_LIMIT: usize = 200;
/// Longest id kept from a response.
const MAX_ID_LEN: usize = 256;
/// Largest response body kept in memory as an entry's "after" state.
const STATE_BODY_LIMIT: usize = 256 * 1024;
/// Entries whose states are kept in memory.
const STATE_ENTRIES: usize = 500;
/// Pending "before" reads kept in memory.
const PENDING_READS: usize = 200;
/// The file is rewritten after this many dropped entries, not for every one.
const TRIM_BATCH: usize = RING_ENTRIES / 10;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Where entries and settings live.
pub trait JournalStorage: Send + Sync {
    /// All stored entries, oldest first. Lines that do not parse are skipped.
    fn load(&self) -> Vec<JournalEntry>;
    /// Adds one entry. Quick: native storage hands it to a background writer.
    fn append(&self, entry: &JournalEntry);
    /// Replaces everything (trimming, marking an entry reverted, clearing).
    fn rewrite(&self, entries: &[JournalEntry]);
    /// Keeps the newest `keep` entries only. `entries` lists them for storage that has no file
    /// of its own to cut; the default rewrites with it.
    fn trim(&self, _keep: usize, entries: &dyn Fn() -> Vec<JournalEntry>) {
        self.rewrite(&entries());
    }
    /// Waits until everything handed over is written.
    fn flush(&self) {}
    fn load_settings(&self) -> JournalSettings {
        JournalSettings::default()
    }
    fn save_settings(&self, _settings: &JournalSettings) {}
}

/// One change a [`QueueStorage`] has not handed on yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageOp {
    Append(JournalEntry),
    Rewrite(Vec<JournalEntry>),
    Settings(JournalSettings),
}

/// Storage that keeps nothing itself: it starts from the entries it was given and queues every
/// change for the shell to [`drain`](Self::drain) (the web shell writes IndexedDB from it;
/// tests use it as memory).
#[derive(Default)]
pub struct QueueStorage {
    initial: Mutex<Vec<JournalEntry>>,
    settings: Mutex<JournalSettings>,
    ops: Mutex<Vec<StorageOp>>,
}

impl QueueStorage {
    pub fn new(entries: Vec<JournalEntry>, settings: JournalSettings) -> Self {
        Self {
            initial: Mutex::new(entries),
            settings: Mutex::new(settings),
            ops: Mutex::new(Vec::new()),
        }
    }

    /// The changes since the last call, oldest first.
    pub fn drain(&self) -> Vec<StorageOp> {
        std::mem::take(&mut *self.ops.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl JournalStorage for QueueStorage {
    fn load(&self) -> Vec<JournalEntry> {
        std::mem::take(&mut *self.initial.lock().unwrap_or_else(|e| e.into_inner()))
    }
    fn append(&self, entry: &JournalEntry) {
        self.ops
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(StorageOp::Append(entry.clone()));
    }
    fn rewrite(&self, entries: &[JournalEntry]) {
        let mut ops = self.ops.lock().unwrap_or_else(|e| e.into_inner());
        // A rewrite makes every queued change before it obsolete.
        ops.clear();
        ops.push(StorageOp::Rewrite(entries.to_vec()));
    }
    fn load_settings(&self) -> JournalSettings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn save_settings(&self, settings: &JournalSettings) {
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
        self.ops
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(StorageOp::Settings(settings.clone()));
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use file::FileStorage;

#[cfg(not(target_arch = "wasm32"))]
mod file {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::mpsc::{Sender, SyncSender, channel, sync_channel};

    use super::{JournalEntry, JournalSettings, JournalStorage};

    enum Op {
        Append(String),
        Rewrite(String),
        Trim(usize),
        Flush(SyncSender<()>),
    }

    /// `journal/<key>.jsonl` in the data folder, written by a background thread so a send
    /// never waits for the disk.
    pub struct FileStorage {
        lines: PathBuf,
        settings: PathBuf,
        tx: Mutex<Option<Sender<Op>>>,
    }

    /// A file name for a collection id (a path): readable, and different ids stay different.
    pub fn file_key(collection_id: &str) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in collection_id.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        let tail: String = collection_id
            .chars()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("{tail}-{hash:016x}")
    }

    impl FileStorage {
        /// `data_dir` is the app's data folder; `collection_id` the id of the collection.
        pub fn new(data_dir: &Path, collection_id: &str) -> Self {
            let dir = data_dir.join("journal");
            let key = file_key(collection_id);
            Self {
                lines: dir.join(format!("{key}.jsonl")),
                settings: dir.join(format!("{key}.settings.json")),
                tx: Mutex::new(None),
            }
        }

        fn send(&self, op: Op) {
            let mut slot = self.tx.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_none() {
                let (tx, rx) = channel::<Op>();
                let path = self.lines.clone();
                let spawned =
                    std::thread::Builder::new()
                        .name("rt-journal".into())
                        .spawn(move || {
                            if let Some(dir) = path.parent() {
                                let _ = std::fs::create_dir_all(dir);
                            }
                            while let Ok(op) = rx.recv() {
                                match op {
                                    Op::Append(line) => {
                                        if let Ok(mut f) = std::fs::OpenOptions::new()
                                            .create(true)
                                            .append(true)
                                            .open(&path)
                                        {
                                            let _ = f.write_all(line.as_bytes());
                                        }
                                    }
                                    Op::Rewrite(text) => {
                                        let tmp = path.with_extension("jsonl.tmp");
                                        if std::fs::write(&tmp, text).is_ok() {
                                            let _ = std::fs::rename(&tmp, &path);
                                        }
                                    }
                                    Op::Trim(keep) => {
                                        if let Ok(text) = std::fs::read_to_string(&path) {
                                            let lines: Vec<&str> = text.lines().collect();
                                            let from = lines.len().saturating_sub(keep);
                                            let mut kept = lines[from..].join("\n");
                                            kept.push('\n');
                                            let tmp = path.with_extension("jsonl.tmp");
                                            if std::fs::write(&tmp, kept).is_ok() {
                                                let _ = std::fs::rename(&tmp, &path);
                                            }
                                        }
                                    }
                                    Op::Flush(done) => {
                                        let _ = done.send(());
                                    }
                                }
                            }
                        });
                if spawned.is_err() {
                    return;
                }
                *slot = Some(tx);
            }
            if let Some(tx) = slot.as_ref() {
                let _ = tx.send(op);
            }
        }
    }

    fn line(entry: &JournalEntry) -> String {
        let mut line = serde_json::to_string(entry).unwrap_or_default();
        line.push('\n');
        line
    }

    impl JournalStorage for FileStorage {
        fn load(&self) -> Vec<JournalEntry> {
            let Ok(text) = std::fs::read_to_string(&self.lines) else {
                return Vec::new();
            };
            text.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        }
        fn append(&self, entry: &JournalEntry) {
            self.send(Op::Append(line(entry)));
        }
        fn rewrite(&self, entries: &[JournalEntry]) {
            self.send(Op::Rewrite(entries.iter().map(line).collect()));
        }
        fn trim(&self, keep: usize, _entries: &dyn Fn() -> Vec<JournalEntry>) {
            self.send(Op::Trim(keep));
        }
        fn flush(&self) {
            let (done, wait) = sync_channel(1);
            let sent = self
                .tx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .is_some_and(|tx| tx.send(Op::Flush(done)).is_ok());
            if sent {
                let _ = wait.recv();
            }
        }
        fn load_settings(&self) -> JournalSettings {
            std::fs::read_to_string(&self.settings)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default()
        }
        fn save_settings(&self, settings: &JournalSettings) {
            if let Some(dir) = self.settings.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string_pretty(settings) {
                let _ = std::fs::write(&self.settings, text);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use file::file_key;

pub(super) fn now_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The session of this process: entries of one app start or CLI run share it.
pub fn session_id() -> &'static str {
    static SESSION: OnceLock<String> = OnceLock::new();
    SESSION.get_or_init(|| format!("s-{:x}", now_ms()))
}

struct Inner {
    entries: VecDeque<JournalEntry>,
    next_id: u64,
    settings: JournalSettings,
    id_path: Option<(String, serde_json_path::JsonPath)>,
    states: HashMap<u64, EntryStates>,
    state_order: VecDeque<u64>,
    /// Reads made before a write (`pre_read`), by masked URL without the query, claimed by
    /// the next entry with that URL.
    pending: VecDeque<(String, PreRead)>,
    dropped_since_rewrite: usize,
}

/// The journal of one collection on this device.
pub struct Journal {
    name: String,
    storage: Arc<dyn JournalStorage>,
    inner: Mutex<Inner>,
}

fn url_key(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_owned()
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

impl Journal {
    /// Opens the journal of the collection called `name` (the name the send reports: see
    /// `Exchange::collection`). Entries older than [`RING_DAYS`] and beyond [`RING_ENTRIES`]
    /// are dropped. "Track test data" starts **off**: the setting holds for the session.
    pub fn open(name: &str, storage: Arc<dyn JournalStorage>) -> Arc<Self> {
        let mut entries: VecDeque<JournalEntry> = storage.load().into();
        let cutoff = now_ms() - i64::from(RING_DAYS) * DAY_MS;
        let before = entries.len();
        entries.retain(|e| e.at >= cutoff);
        while entries.len() > RING_ENTRIES {
            entries.pop_front();
        }
        if entries.len() != before {
            let list: Vec<_> = entries.iter().cloned().collect();
            storage.rewrite(&list);
        }
        let next_id = entries.iter().map(|e| e.id).max().map_or(1, |m| m + 1);
        let mut settings = storage.load_settings();
        settings.enabled = false;
        let id_path = compile(&settings);
        Arc::new(Self {
            name: name.to_owned(),
            storage,
            inner: Mutex::new(Inner {
                entries,
                next_id,
                settings,
                id_path,
                states: HashMap::new(),
                state_order: VecDeque::new(),
                pending: VecDeque::new(),
                dropped_since_rewrite: 0,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The name of the collection this journal belongs to.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn settings(&self) -> JournalSettings {
        self.lock().settings.clone()
    }

    /// Changes the settings. `enabled` holds for this session only; `id_path` is kept.
    /// A path that is not valid JSONPath is refused.
    pub fn set_settings(&self, settings: JournalSettings) -> Result<(), crate::AppError> {
        let mut settings = settings;
        settings.id_path = settings
            .id_path
            .take()
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty());
        if let Some(path) = &settings.id_path
            && !path.starts_with("var:")
            && serde_json_path::JsonPath::parse(path).is_err()
        {
            return Err(crate::AppError::Shell(format!(
                "the id path {path:?} is not a valid JSONPath (for example $.id)"
            )));
        }
        let compiled = compile(&settings);
        {
            let mut inner = self.lock();
            inner.settings = settings.clone();
            inner.id_path = compiled;
        }
        self.storage.save_settings(&settings);
        Ok(())
    }

    pub fn enabled(&self) -> bool {
        self.lock().settings.enabled
    }

    /// Takes a finished, successful, writing exchange (`rt_core::journal::record` has
    /// filtered). Keeps no body and no header value but `Location` and `ETag`; the id comes
    /// from the configured path. Returns the new entry's id.
    pub fn record(&self, exchange: &Exchange) -> Option<u64> {
        let report = &exchange.report;
        let response = report.response.as_ref()?;
        let sent = report.request.as_ref()?;
        let mut inner = self.lock();
        if !inner.settings.enabled {
            return None;
        }
        let resource_id = match &inner.settings.id_path {
            Some(path) if path.starts_with("var:") => report
                .var_changes
                .iter()
                .rev()
                .find(|c| c.name == path["var:".len()..])
                .and_then(|c| c.value.clone()),
            _ => inner
                .id_path
                .as_ref()
                .and_then(|(_, p)| match &response.body {
                    ReportBody::Utf8(text) => id_in(p, text),
                    ReportBody::Base64(_) => None,
                }),
        }
        .map(|id| cut(&id, MAX_ID_LEN));
        let id = inner.next_id;
        inner.next_id += 1;
        let entry = JournalEntry {
            id,
            session: session_id().to_owned(),
            at: exchange.at,
            environment: exchange.environment.clone(),
            request_path: report.request_path.clone(),
            method: sent.method.clone(),
            url: sent.url.clone(),
            status: response.status,
            location: header(&response.headers, "location").map(str::to_owned),
            etag: header(&response.headers, "etag").map(str::to_owned),
            resource_id,
            reverted_at: None,
        };
        // In memory only: what the server held before (a read the caller made) and what the
        // write answered. Never saved.
        let key = url_key(&entry.url);
        let before = inner
            .pending
            .iter()
            .position(|(k, _)| *k == key)
            .and_then(|i| inner.pending.remove(i))
            .map(|(_, state)| state);
        let after = match &response.body {
            ReportBody::Utf8(text) => Some(PreRead {
                status: response.status,
                body: Some(cut(text, STATE_BODY_LIMIT)),
                etag: entry.etag.clone(),
            }),
            ReportBody::Base64(_) => None,
        };
        if before.is_some() || after.is_some() {
            inner.states.insert(
                id,
                EntryStates {
                    entry_id: id,
                    before,
                    after,
                },
            );
            inner.state_order.push_back(id);
            while inner.state_order.len() > STATE_ENTRIES {
                if let Some(old) = inner.state_order.pop_front() {
                    inner.states.remove(&old);
                }
            }
        }
        inner.entries.push_back(entry.clone());
        let mut trimmed = false;
        if inner.entries.len() > RING_ENTRIES {
            inner.entries.pop_front();
            inner.dropped_since_rewrite += 1;
            if inner.dropped_since_rewrite >= TRIM_BATCH {
                inner.dropped_since_rewrite = 0;
                trimmed = true;
            }
        }
        if trimmed {
            // The trim removes the oldest lines in the background; this entry goes first.
            self.storage.append(&entry);
            let list = || inner.entries.iter().cloned().collect::<Vec<_>>();
            self.storage.trim(RING_ENTRIES, &list);
        } else {
            drop(inner);
            self.storage.append(&entry);
        }
        Some(id)
    }

    /// A page of entries, newest first.
    pub fn entries(&self, query: &JournalQuery) -> JournalPage {
        let inner = self.lock();
        let limit = if query.limit == 0 {
            DEFAULT_LIMIT
        } else {
            query.limit as usize
        };
        let matches = |e: &&JournalEntry| {
            query.before.is_none_or(|b| e.id < b)
                && query.session.as_ref().is_none_or(|s| &e.session == s)
                && (!query.only_open || e.reverted_at.is_none())
        };
        let total = inner.entries.iter().filter(matches).count() as u64;
        let entries = inner
            .entries
            .iter()
            .rev()
            .filter(matches)
            .take(limit)
            .cloned()
            .collect();
        JournalPage { entries, total }
    }

    pub fn entry(&self, id: u64) -> Option<JournalEntry> {
        self.lock().entries.iter().find(|e| e.id == id).cloned()
    }

    /// Marks an entry as reverted now. Returns whether it exists.
    pub fn mark_reverted(&self, id: u64) -> bool {
        let list = {
            let mut inner = self.lock();
            let Some(entry) = inner.entries.iter_mut().find(|e| e.id == id) else {
                return false;
            };
            entry.reverted_at = Some(now_ms());
            inner.entries.iter().cloned().collect::<Vec<_>>()
        };
        self.storage.rewrite(&list);
        true
    }

    /// Forgets every entry (and what is kept in memory for them).
    pub fn clear(&self) {
        {
            let mut inner = self.lock();
            inner.entries.clear();
            inner.states.clear();
            inner.state_order.clear();
            inner.pending.clear();
            inner.dropped_since_rewrite = 0;
        }
        self.storage.rewrite(&[]);
    }

    /// What is kept in memory for these entries (a restart forgets it).
    pub fn states(&self, ids: &[u64]) -> Vec<EntryStates> {
        let inner = self.lock();
        ids.iter()
            .map(|id| {
                inner.states.get(id).cloned().unwrap_or(EntryStates {
                    entry_id: *id,
                    before: None,
                    after: None,
                })
            })
            .collect()
    }

    /// Keeps a read made before a write; the next entry with the same URL (query aside) gets
    /// it as its "before". In memory only.
    pub fn keep_pre_read(&self, url: &str, state: PreRead) {
        let mut inner = self.lock();
        let key = url_key(url);
        inner.pending.retain(|(k, _)| *k != key);
        inner.pending.push_back((key, state));
        while inner.pending.len() > PENDING_READS {
            inner.pending.pop_front();
        }
    }

    /// Waits until the storage has written everything.
    pub fn flush(&self) {
        self.storage.flush();
    }

    pub fn storage(&self) -> &Arc<dyn JournalStorage> {
        &self.storage
    }
}

fn compile(settings: &JournalSettings) -> Option<(String, serde_json_path::JsonPath)> {
    let path = settings.id_path.as_ref()?;
    if path.starts_with("var:") {
        return None;
    }
    let parsed = serde_json_path::JsonPath::parse(path).ok()?;
    Some((path.clone(), parsed))
}

fn id_in(path: &serde_json_path::JsonPath, body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    match path.query(&value).first()? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
