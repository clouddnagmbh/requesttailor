// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic diff of two versions of a collection (wave 12): what changed per request, folder,
//! collection and environment field, instead of per YAML line.
//!
//! [`diff`] reads both [`FileSet`]s with [`read_collection`](crate::read_collection) and
//! compares the models. It works in memory, so the desktop app, the web app (rt-wasm), the
//! CLI (`rt diff`) and the PR review all get the same answer.
//!
//! # Field paths
//!
//! A [`FieldPath`] names a field of the model, segment by segment, with the serde names of
//! `rt_core::model` (what the JSON of `RequestSpec` uses). List entries are named by their
//! key, not their position, so a moved header is not a change:
//!
//! | Path | Field |
//! | --- | --- |
//! | `["name"]`, `["seq"]`, `["method"]`, `["url"]` | scalars of a request |
//! | `["headers", "<name>"]` | a header (a second header of the same name: `"<name>#2"`) |
//! | `["params", "<query\|path>", "<name>"]` | a query or path parameter |
//! | `["body", "<field>"]` | `type`, and per type `kind`, `data` (raw text: compared as code), `query`, `variables`, `fields`, `parts`, `path`, `content_type` |
//! | `["auth", "<field>"]` | `type`, then the fields of that type (`token`, `username`, ...); no field when the auth is `inherit` |
//! | `["vars", "<name>"]` | a request variable (folders and the collection: under `["defaults", ...]`) |
//! | `["settings", "<field>"]` | a setting |
//! | `["scripts", "<phase>"]` | the script of one phase (compared as code) |
//! | `["assertions", "<expression>"]` | an assertion, by its expression |
//! | `["actions", "<variable>"]` | a `set-variable` action, by its variable |
//! | `["protocol", "<field>"]`, `["protocol", "messages", "<title\|#n>", ("kind"\|"selected")]` | gRPC and WebSocket fields and saved messages |
//! | `["variables", "<name>"]` | a variable of an environment |
//! | `["defaults", …]` | request defaults of a folder or the collection (same segments) |
//! | `["config", "<field>"]` | collection configuration |
//!
//! A list entry also has `[..., "@enabled"]` (`"true"`/`"false"`); it is no change of its own
//! when the entry was added or removed whole and is enabled. A secret variable has
//! `[..., "@secret"]`. Environments use `["variables", "<name>"]`, `["client_certificates"]`
//! and `["external_secrets"]`.
//!
//! # Secrets
//!
//! A value never appears in a diff when the variable is secret on either side
//! ([`DiffValue::Secret`]); only the name is in the field path. Secret files hold no values
//! anyway; this also hides an old plain value of a variable that became secret. Values that
//! *look* like credentials (the secret scanner's rules) are masked by the app layer
//! (`rt_app::diffs`), which knows the scanner.
//!
//! Owner: wave 12, strand A. The types are interface: strands B
//! (merge conflicts), D (blame), E (PR review) and F (contract notes) build on them.

use std::collections::{BTreeSet, HashMap};

use rt_core::model::{Environment, Folder, Item, RequestEntry, RequestSpec};
use serde::{Deserialize, Serialize};

use crate::{
    COLLECTION_FILE, CollectionDoc, FOLDER_FILE, FileSet, FormatError, Source, read_collection,
    write,
};

mod fields;
mod lines;

/// A field of the model, e.g. `["headers", "Authorization"]`. See the module docs.
pub type FieldPath = Vec<String>;

/// What a change is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ChangeTarget {
    /// `opencollection.yml`: configuration and request defaults of the collection.
    Collection,
    /// A folder (`folder.yml`), by `Folder::path` (new side; old side if removed).
    Folder { path: String },
    /// A request, by its path relative to the collection root (new side; old if removed).
    Request { path: String },
    /// An environment, by name.
    Environment { name: String },
}

/// How the target changed as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Removed,
    /// Fields changed; see [`ModelChange::fields`].
    Modified,
    /// The file moved (rename or other folder), maybe with field changes too. `from` is the
    /// old path (requests, folders) or name (environments).
    Moved {
        from: String,
    },
}

/// A value in a diff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DiffValue {
    /// A scalar, a body or a script (the UI shows multi-line text as a code diff).
    Text { text: String },
    /// A structured value (an auth configuration, a gRPC message list, …) as model JSON.
    Json { value: serde_json::Value },
    /// The value of a secret: never shown, only that there is one.
    Secret,
}

/// One changed field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldChange {
    pub path: FieldPath,
    /// `None`: the field is new.
    pub old: Option<DiffValue>,
    /// `None`: the field was removed.
    pub new: Option<DiffValue>,
    /// 1-based line of the field in the new file (the old file for a removed field), where
    /// the format can tell. The PR review comments at this line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Everything that changed about one target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChange {
    pub target: ChangeTarget,
    pub change: ChangeKind,
    /// The target's file on the new side (old side if removed), e.g. `users/get.yml`,
    /// `users/folder.yml`, `environments/Local.yml`, `opencollection.yml`.
    pub file: String,
    /// Display name (request, folder or environment name; the collection's name).
    pub name: String,
    /// Changed fields in model order. Empty for `Added` and `Removed`.
    pub fields: Vec<FieldChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Removed,
    Modified,
}

/// One file that differs byte-wise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: FileStatus,
    /// `true`: [`CollectionDiff::changes`] describes every difference of this file. `false`:
    /// it also differs in ways the model does not hold (comments, docs, unknown fields,
    /// unparsable files, files that are not collection files); the UI offers its line diff.
    pub covered: bool,
}

/// The semantic diff of two versions of a collection.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CollectionDiff {
    /// Changes in tree order: collection, then folders and requests depth first, then
    /// environments.
    pub changes: Vec<ModelChange>,
    /// Every file that differs, sorted by path.
    pub files: Vec<ChangedFile>,
}

impl CollectionDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.files.is_empty()
    }
}

/// `old` → `new`. An empty [`FileSet`] on one side means the collection did not exist there
/// (everything added or removed). Fails only if a side is not readable as a collection.
///
/// Requests whose file is byte-identical on both sides are not looked at, so the cost follows
/// the number of changes, not the size of the collection (reading both sides is the rest).
///
/// A removed and an added request (or folder) that are the same thing are one
/// [`ChangeKind::Moved`]: equal apart from `seq`, or the same name when that name is unique
/// among the removed and among the added ones. Removed targets come after the others.
///
/// [`ChangedFile::covered`] is `true` for a modified request or environment file when
/// writing the model change into the old file with the format's writer gives exactly the new
/// file, and for added, removed and moved files that map to a target. Modified folder and
/// collection files are `covered: false` (their line diff stays one click away).
pub fn diff(old: &FileSet, new: &FileSet) -> Result<CollectionDiff, FormatError> {
    let mut files = changed_files(old, new);
    if files.is_empty() {
        return Ok(CollectionDiff::default());
    }
    let side = |set: &FileSet| {
        if set.files.is_empty() {
            Ok(None)
        } else {
            read_collection(set).map(Some)
        }
    };
    let (old_doc, new_doc) = (side(old)?, side(new)?);
    let mut run = Run {
        old,
        new,
        old_doc: old_doc.as_ref(),
        new_doc: new_doc.as_ref(),
        changes: Vec::new(),
        covered: HashMap::new(),
    };
    run.collection();
    let order = run.tree();
    run.environments();
    let mut changes = run.changes;
    // Collection first, the tree in new-tree order (removed targets after it), environments.
    changes.sort_by_key(|c| match &c.target {
        ChangeTarget::Collection => (0, 0),
        ChangeTarget::Folder { path } => {
            (1, order.get(&(true, path.clone())).copied().unwrap_or(0))
        }
        ChangeTarget::Request { path } => {
            (1, order.get(&(false, path.clone())).copied().unwrap_or(0))
        }
        ChangeTarget::Environment { .. } => (2, 0),
    });
    for f in &mut files {
        f.covered = run.covered.get(&f.path) == Some(&true);
    }
    Ok(CollectionDiff { changes, files })
}

type Order = HashMap<(bool, String), usize>;

struct Run<'a> {
    old: &'a FileSet,
    new: &'a FileSet,
    old_doc: Option<&'a CollectionDoc>,
    new_doc: Option<&'a CollectionDoc>,
    changes: Vec<ModelChange>,
    covered: HashMap<String, bool>,
}

/// Folders and requests of a tree, depth first.
fn walk<'a>(
    items: &'a [Item],
    folders: &mut Vec<&'a Folder>,
    requests: &mut Vec<&'a RequestEntry>,
) {
    for item in items {
        match item {
            Item::Folder(f) => {
                folders.push(f);
                walk(&f.items, folders, requests);
            }
            Item::Request(r) => requests.push(r),
        }
    }
}

fn tree_order<'a>(items: &'a [Item], out: &mut Vec<(bool, &'a str)>) {
    for item in items {
        match item {
            Item::Folder(f) => {
                out.push((true, &f.path));
                tree_order(&f.items, out);
            }
            Item::Request(r) => out.push((false, &r.path)),
        }
    }
}

fn seqless(spec: &RequestSpec) -> String {
    let mut spec = spec.clone();
    spec.seq = None;
    serde_json::to_string(&spec).unwrap_or_default()
}

/// Pairs removed with added entries: first by `same`, then by a name unique on both sides.
/// Returns `(removed index, added index)`.
fn pair<T>(
    removed: &[&T],
    added: &[&T],
    name: impl Fn(&T) -> &str,
    same: impl Fn(&T, &T) -> bool,
) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let (mut used_r, mut used_a) = (vec![false; removed.len()], vec![false; added.len()]);
    for (i, r) in removed.iter().enumerate() {
        if let Some(j) = (0..added.len()).find(|&j| !used_a[j] && same(r, added[j])) {
            used_r[i] = true;
            used_a[j] = true;
            pairs.push((i, j));
        }
    }
    for (i, r) in removed.iter().enumerate() {
        if used_r[i] {
            continue;
        }
        let in_r = |n: &str| {
            (0..removed.len())
                .filter(|&k| !used_r[k] && name(removed[k]) == n)
                .count()
        };
        let in_a = |n: &str| {
            (0..added.len())
                .filter(|&k| !used_a[k] && name(added[k]) == n)
                .count()
        };
        let n = name(r);
        if in_r(n) == 1 && in_a(n) == 1 {
            let j = (0..added.len())
                .find(|&k| !used_a[k] && name(added[k]) == n)
                .expect("counted");
            used_r[i] = true;
            used_a[j] = true;
            pairs.push((i, j));
        }
    }
    pairs
}

impl<'a> Run<'a> {
    fn text<'t>(&self, set: &'t FileSet, file: &str) -> Option<&'t str> {
        std::str::from_utf8(set.files.get(file)?).ok()
    }

    /// Adds the change; `from` is the old file of a move. Lines are looked up in the new file,
    /// removed fields in the old one.
    fn push(
        &mut self,
        target: ChangeTarget,
        change: ChangeKind,
        file: String,
        name: String,
        mut fields: Vec<FieldChange>,
        old_file: Option<&str>,
    ) {
        let old_file = old_file.unwrap_or(&file);
        let new_text = self.text(self.new, &file);
        let old_text = self.text(self.old, old_file);
        for f in &mut fields {
            let text = if f.new.is_some() { new_text } else { old_text };
            f.line = text.and_then(|t| lines::locate(t, &f.path));
        }
        self.changes.push(ModelChange {
            target,
            change,
            file,
            name,
            fields,
        });
    }

    fn collection(&mut self) {
        let (o, n) = (
            self.old_doc.map(CollectionDoc::model),
            self.new_doc.map(CollectionDoc::model),
        );
        let file = COLLECTION_FILE.to_owned();
        match (o, n) {
            (Some(o), Some(n)) => {
                let fields = fields::compare(
                    &fields::collection(&o.name, &o.defaults, &o.config),
                    &fields::collection(&n.name, &n.defaults, &n.config),
                );
                if !fields.is_empty() {
                    self.push(
                        ChangeTarget::Collection,
                        ChangeKind::Modified,
                        file,
                        n.name.clone(),
                        fields,
                        None,
                    );
                }
            }
            (None, Some(n)) => {
                self.covered.insert(file.clone(), true);
                self.push(
                    ChangeTarget::Collection,
                    ChangeKind::Added,
                    file,
                    n.name.clone(),
                    Vec::new(),
                    None,
                );
            }
            (Some(o), None) => {
                self.covered.insert(file.clone(), true);
                self.push(
                    ChangeTarget::Collection,
                    ChangeKind::Removed,
                    file,
                    o.name.clone(),
                    Vec::new(),
                    None,
                );
            }
            (None, None) => {}
        }
    }

    /// Folders and requests. Returns the position of each in the output order.
    fn tree(&mut self) -> Order {
        let empty: &[Item] = &[];
        let items = |m: Option<&'a CollectionDoc>| m.map_or(empty, |d| &d.model().items);
        let (old_items, new_items) = (items(self.old_doc), items(self.new_doc));
        let (mut of, mut orq, mut nf, mut nrq) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        walk(old_items, &mut of, &mut orq);
        walk(new_items, &mut nf, &mut nrq);
        let old_folders: HashMap<&str, &Folder> =
            of.iter().map(|f| (f.path.as_str(), *f)).collect();
        let new_folders: HashMap<&str, &Folder> =
            nf.iter().map(|f| (f.path.as_str(), *f)).collect();
        let old_requests: HashMap<&str, &RequestEntry> =
            orq.iter().map(|r| (r.path.as_str(), *r)).collect();
        let new_requests: HashMap<&str, &RequestEntry> =
            nrq.iter().map(|r| (r.path.as_str(), *r)).collect();

        // Folders.
        let folder_file = |p: &str| format!("{p}/{FOLDER_FILE}");
        let (mut removed, mut added) = (Vec::new(), Vec::new());
        for f in &nf {
            match old_folders.get(f.path.as_str()) {
                Some(o) => {
                    let fields = fields::compare(
                        &fields::folder(&o.name, o.seq, &o.defaults),
                        &fields::folder(&f.name, f.seq, &f.defaults),
                    );
                    if !fields.is_empty() {
                        self.push(
                            ChangeTarget::Folder {
                                path: f.path.clone(),
                            },
                            ChangeKind::Modified,
                            folder_file(&f.path),
                            f.name.clone(),
                            fields,
                            None,
                        );
                    }
                }
                None => added.push(*f),
            }
        }
        for f in &of {
            if !new_folders.contains_key(f.path.as_str()) {
                removed.push(*f);
            }
        }
        self.structural(
            &removed,
            &added,
            |f| &f.name,
            |a, b| a.name == b.name && a.defaults == b.defaults,
            |f| {
                (
                    ChangeTarget::Folder {
                        path: f.path.clone(),
                    },
                    folder_file(&f.path),
                    f.name.clone(),
                    f.path.clone(),
                )
            },
            |o, n| {
                fields::compare(
                    &fields::folder(&o.name, o.seq, &o.defaults),
                    &fields::folder(&n.name, n.seq, &n.defaults),
                )
            },
        );

        // Requests: a file that is byte-identical on both sides is not looked at.
        let (mut removed, mut added) = (Vec::new(), Vec::new());
        for r in &nrq {
            match old_requests.get(r.path.as_str()) {
                Some(o) => {
                    if self.old.files.get(&r.path) == self.new.files.get(&r.path) {
                        continue;
                    }
                    let fields =
                        fields::compare(&fields::request(&o.spec), &fields::request(&r.spec));
                    let exact = self.roundtrip_request(&r.path, &r.spec);
                    self.covered.insert(r.path.clone(), exact);
                    if !fields.is_empty() {
                        self.push(
                            ChangeTarget::Request {
                                path: r.path.clone(),
                            },
                            ChangeKind::Modified,
                            r.path.clone(),
                            r.spec.name.clone(),
                            fields,
                            None,
                        );
                    }
                }
                None => added.push(*r),
            }
        }
        for r in &orq {
            if !new_requests.contains_key(r.path.as_str()) {
                removed.push(*r);
            }
        }
        let keys: HashMap<*const RequestEntry, String> = removed
            .iter()
            .chain(added.iter())
            .map(|r| (*r as *const RequestEntry, seqless(&r.spec)))
            .collect();
        self.structural(
            &removed,
            &added,
            |r| &r.spec.name,
            |a, b| keys[&(a as *const RequestEntry)] == keys[&(b as *const RequestEntry)],
            |r| {
                (
                    ChangeTarget::Request {
                        path: r.path.clone(),
                    },
                    r.path.clone(),
                    r.spec.name.clone(),
                    r.path.clone(),
                )
            },
            |o, n| fields::compare(&fields::request(&o.spec), &fields::request(&n.spec)),
        );

        // Output order: the new tree, then what only the old one had.
        let mut order = Order::new();
        let mut list = Vec::new();
        tree_order(new_items, &mut list);
        tree_order(old_items, &mut list);
        for (kind, path) in list {
            let next = order.len();
            order.entry((kind, path.to_owned())).or_insert(next);
        }
        order
    }

    /// Added, removed and moved targets. `describe` gives target, file, name and the old
    /// path of a removed one.
    fn structural<T>(
        &mut self,
        removed: &[&T],
        added: &[&T],
        name: impl Fn(&T) -> &str + Copy,
        same: impl Fn(&T, &T) -> bool,
        describe: impl Fn(&T) -> (ChangeTarget, String, String, String),
        fields: impl Fn(&T, &T) -> Vec<FieldChange>,
    ) {
        let pairs = pair(removed, added, name, same);
        let (mut r_done, mut a_done) = (vec![false; removed.len()], vec![false; added.len()]);
        for (i, j) in pairs {
            r_done[i] = true;
            a_done[j] = true;
            let (target, file, name, _) = describe(added[j]);
            let (_, old_file, _, from) = describe(removed[i]);
            self.covered.insert(file.clone(), true);
            self.covered.insert(old_file.clone(), true);
            let f = fields(removed[i], added[j]);
            self.push(
                target,
                ChangeKind::Moved { from },
                file,
                name,
                f,
                Some(&old_file),
            );
        }
        for (list, done, kind) in [
            (added, &a_done, ChangeKind::Added),
            (removed, &r_done, ChangeKind::Removed),
        ] {
            for (k, t) in list.iter().enumerate() {
                if done[k] {
                    continue;
                }
                let (target, file, name, _) = describe(t);
                self.covered.insert(file.clone(), true);
                self.push(target, kind.clone(), file, name, Vec::new(), None);
            }
        }
    }

    /// Whether the format's writer turns the old request file into the new one.
    fn roundtrip_request(&self, path: &str, spec: &RequestSpec) -> bool {
        let (Some(old_doc), Some(new_bytes)) = (self.old_doc, self.new.files.get(path)) else {
            return false;
        };
        let Some(Source::Yaml(doc)) = old_doc.files.get(path) else {
            return false;
        };
        write::apply_request(doc, path, spec)
            .is_ok_and(|d| d.to_text().as_bytes() == new_bytes.as_slice())
    }

    fn environments(&mut self) {
        let envs = |d: Option<&'a CollectionDoc>| {
            d.map_or(Vec::new(), |d| {
                d.model().environments.iter().collect::<Vec<_>>()
            })
        };
        let (old_envs, new_envs) = (envs(self.old_doc), envs(self.new_doc));
        let find =
            |list: &[&'a Environment], name: &str| list.iter().find(|e| e.name == name).copied();
        for n in &new_envs {
            let file = self.env_file(self.new_doc, &n.name);
            match find(&old_envs, &n.name) {
                Some(o) if o == *n => {}
                Some(o) => {
                    let fields = fields::compare(&fields::environment(o), &fields::environment(n));
                    let exact = file != COLLECTION_FILE && self.roundtrip_env(&file, n);
                    self.covered.insert(file.clone(), exact);
                    if !fields.is_empty() {
                        self.push(
                            ChangeTarget::Environment {
                                name: n.name.clone(),
                            },
                            ChangeKind::Modified,
                            file,
                            n.name.clone(),
                            fields,
                            None,
                        );
                    }
                }
                None => {
                    self.covered.insert(file.clone(), file != COLLECTION_FILE);
                    self.push(
                        ChangeTarget::Environment {
                            name: n.name.clone(),
                        },
                        ChangeKind::Added,
                        file,
                        n.name.clone(),
                        Vec::new(),
                        None,
                    );
                }
            }
        }
        for o in &old_envs {
            if find(&new_envs, &o.name).is_none() {
                let file = self.env_file(self.old_doc, &o.name);
                self.covered.insert(file.clone(), file != COLLECTION_FILE);
                self.push(
                    ChangeTarget::Environment {
                        name: o.name.clone(),
                    },
                    ChangeKind::Removed,
                    file,
                    o.name.clone(),
                    Vec::new(),
                    None,
                );
            }
        }
    }

    /// The file an environment is declared in.
    fn env_file(&self, doc: Option<&CollectionDoc>, name: &str) -> String {
        let fallback = format!("environments/{name}.yml");
        let Some(doc) = doc else { return fallback };
        let inline = matches!(doc.files.get(COLLECTION_FILE), Some(Source::Yaml(d))
            if d.root.get("config").and_then(|c| c.get("environments"))
                .is_some_and(|l| l.items().any(|e| crate::read::environment(e, "").name == name)));
        if inline {
            return COLLECTION_FILE.to_owned();
        }
        let prefix = format!("{}/", crate::read::ENVIRONMENTS_DIR);
        doc.files
            .iter()
            .find_map(|(path, source)| {
                let rest = path.strip_prefix(&prefix)?;
                let Source::Yaml(d) = source else { return None };
                (!rest.contains('/')
                    && crate::read::environment(&d.root, crate::read::file_stem(path)).name == name)
                    .then(|| path.clone())
            })
            .unwrap_or(fallback)
    }

    fn roundtrip_env(&self, file: &str, env: &Environment) -> bool {
        let (Some(old_doc), Some(new_bytes)) = (self.old_doc, self.new.files.get(file)) else {
            return false;
        };
        let Some(Source::Yaml(doc)) = old_doc.files.get(file) else {
            return false;
        };
        write::apply_environment_doc(doc, env, crate::read::file_stem(file))
            .is_ok_and(|d| d.to_text().as_bytes() == new_bytes.as_slice())
    }
}

/// The files that differ byte-wise between `old` and `new`, sorted by path.
pub fn changed_files(old: &FileSet, new: &FileSet) -> Vec<ChangedFile> {
    let paths: BTreeSet<&String> = old.files.keys().chain(new.files.keys()).collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let status = match (old.files.get(path), new.files.get(path)) {
                (None, Some(_)) => FileStatus::Added,
                (Some(_), None) => FileStatus::Removed,
                (Some(a), Some(b)) if a != b => FileStatus::Modified,
                _ => return None,
            };
            Some(ChangedFile {
                path: path.clone(),
                status,
                covered: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(files: &[(&str, &str)]) -> FileSet {
        FileSet {
            files: files
                .iter()
                .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
                .collect(),
        }
    }

    #[test]
    fn files_that_differ_are_listed_by_path() {
        let old = set(&[("a.yml", "1"), ("b.yml", "1"), ("c.yml", "1")]);
        let new = set(&[("a.yml", "1"), ("b.yml", "2"), ("d.yml", "1")]);
        let files = changed_files(&old, &new);
        let got: Vec<(&str, FileStatus)> =
            files.iter().map(|f| (f.path.as_str(), f.status)).collect();
        assert_eq!(
            got,
            vec![
                ("b.yml", FileStatus::Modified),
                ("c.yml", FileStatus::Removed),
                ("d.yml", FileStatus::Added),
            ]
        );
        assert!(diff(&old, &old).unwrap().is_empty());
    }
}
