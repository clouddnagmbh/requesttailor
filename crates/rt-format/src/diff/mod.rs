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
//! | `["body"]` | the whole body (text: compared as code) |
//! | `["auth"]`, `["auth", "<field>"]` | the auth, or one field of it when the type stayed |
//! | `["vars", "<name>"]` | a request, folder or collection variable |
//! | `["settings", "<field>"]` | a setting |
//! | `["scripts", "<phase>"]` | the script of one phase (compared as code) |
//! | `["assertions", "<expression>"]` | an assertion, by its expression |
//! | `["actions", "<variable>"]` | a `set-variable` action, by its variable |
//! | `["protocol", …]` | gRPC and WebSocket fields |
//! | `["variables", "<name>"]` | a variable of an environment |
//! | `["defaults", …]` | request defaults of a folder or the collection (same segments) |
//! | `["config", …]` | collection configuration |
//!
//! # Secrets
//!
//! A value never appears in a diff when the variable is secret on either side
//! ([`DiffValue::Secret`]); only the name is in the field path. Secret files hold no values
//! anyway; this also hides an old plain value of a variable that became secret. Values that
//! *look* like credentials (the secret scanner's rules) are masked by the app layer
//! (`rt_app::diffs`), which knows the scanner.
//!
//! Owner: wave 12 interface; implementation strand A. The types are interface: strands B
//! (merge conflicts), D (blame), E (PR review) and F (contract notes) build on them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{FileSet, FormatError};

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
/// Interface state (wave 12): only the file level ([`CollectionDiff::files`], all
/// `covered: false`); strand A fills in [`CollectionDiff::changes`].
pub fn diff(old: &FileSet, new: &FileSet) -> Result<CollectionDiff, FormatError> {
    Ok(CollectionDiff {
        changes: Vec::new(),
        files: changed_files(old, new),
    })
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
