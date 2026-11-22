// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-format`: reads and writes OpenCollection 1.0 (YAML) collections; importers (`import`).
//!
//! This is the only crate that knows the file format (see CLAUDE.md, "Format"). It maps files
//! to `rt_core::model` and back. It works on an in-memory [`FileSet`], so the same code runs
//! natively and in the browser; only [`FileSet::from_dir`] touches the file system.
//!
//! Files are parsed into a format-preserving YAML tree (module `yaml`): writing an unchanged
//! document reproduces every file byte for byte, including everything the model does not
//! represent (gRPC scripts, docs, examples, OAuth configurations, comments, ...).
//!
//! Layout of a collection: `opencollection.yml` at the root, one `folder.yml` per folder
//! (optional), one `*.yml` per item, `environments/<name>.yml` per environment. A folder that
//! contains its own `opencollection.yml` is a separate, nested collection and not part of
//! this one. How files map to the model is described in the `read` module; how the model is
//! written back (format-preserving edits) in the `write` module.
//!
//! Owner: wave 2, strand B (reading); wave 3, strand F (writing); wave 4, strand B (`import`).

use std::collections::BTreeMap;

use rt_core::model::Collection;

/// Wave 8, strand C: `extensions.requesttailor.agents`.
mod agents;
mod auth;
mod edit;
/// Wave 8, strand F: saved examples and docs of items (read only), for `rt mock` and `rt docs`.
pub mod examples;
#[cfg(not(target_arch = "wasm32"))]
mod fs;
pub mod import;
/// Wave 8: GraphQL, gRPC and WebSocket items.
mod protocols;
mod read;
mod runtime;
mod write;
mod yaml;

/// Files of one collection, keyed by path relative to the collection root, `/`-separated
/// (also on Windows), e.g. `opencollection.yml`, `users/folder.yml`, `environments/Local.yml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileSet {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl FileSet {
    /// Reads every file below `root` that belongs to a collection: all `*.yml` files,
    /// skipping hidden folders (`.git`, ...), `node_modules`, nested collections and
    /// symbolic links. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_dir(root: &std::path::Path) -> std::io::Result<Self> {
        fs::from_dir(root)
    }

    /// Writes all files below `root`, creating folders as needed. Paths that would leave
    /// `root` (`..`, absolute paths, `\`) are rejected with `InvalidInput`. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_to_dir(&self, root: &std::path::Path) -> std::io::Result<()> {
        fs::write_to_dir(self, root)
    }
}

/// Name of the file that marks a collection root.
pub const COLLECTION_FILE: &str = "opencollection.yml";

/// Name of the file that holds a folder's metadata and request defaults.
pub const FOLDER_FILE: &str = "folder.yml";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("{COLLECTION_FILE} not found")]
    NotACollection,
    #[error("{path}: {message}")]
    Invalid { path: String, message: String },
    #[error("{path}: unsupported OpenCollection version {version}")]
    UnsupportedVersion { path: String, version: String },
}

/// One file of the collection as the document holds it.
#[derive(Debug, Clone)]
enum Source {
    Yaml(yaml::Document),
    /// A file that could not be parsed; it is kept unchanged so writing never destroys it.
    Unparsed(Vec<u8>),
}

/// A parsed collection that remembers everything needed to write the files back
/// byte-identically, including fields the core model does not represent (gRPC scripts,
/// docs, examples, unknown auth types, ...).
#[derive(Debug, Clone)]
pub struct CollectionDoc {
    model: Collection,
    files: BTreeMap<String, Source>,
    issues: Vec<FormatError>,
    /// Files that are not valid YAML (part of `issues`), kept for rebuilding the model.
    parse_issues: Vec<FormatError>,
}

impl CollectionDoc {
    /// The format-neutral view the rest of the system works with.
    pub fn model(&self) -> &Collection {
        &self.model
    }

    /// Problems in individual files that did not stop reading the collection: files that
    /// are not valid YAML (kept unchanged, not in the model), unknown item or body types.
    pub fn issues(&self) -> &[FormatError] {
        &self.issues
    }

    /// Serializes the document back to files. For an unmodified document,
    /// `read_collection(files)?.write() == files` byte for byte (for files the format owns).
    pub fn write(&self) -> FileSet {
        FileSet {
            files: self
                .files
                .iter()
                .map(|(path, source)| {
                    let bytes = match source {
                        Source::Yaml(doc) => doc.to_text().into_bytes(),
                        Source::Unparsed(bytes) => bytes.clone(),
                    };
                    (path.clone(), bytes)
                })
                .collect(),
        }
    }

    /// Replaces the request at `path` (relative to the collection root) with `spec`.
    /// Only what differs is rewritten; everything the model does not represent (gRPC scripts,
    /// action types other than `set-variable`, docs, examples, comments, layout) stays as it
    /// was, and the result is formatted the way Bruno writes it. The model view is updated
    /// too. Scripts, assertions and `set-variable` actions are written since wave 7.
    ///
    /// Secret request variables are written without a value. An `Auth::Unsupported` can only
    /// be kept as the file has it; setting another one is an error. Every edit is read back
    /// and must give `spec`, otherwise it is refused and the file is left as it was.
    ///
    /// Owner: wave 3, strand F.
    pub fn set_request(
        &mut self,
        path: &str,
        spec: &rt_core::model::RequestSpec,
    ) -> Result<(), FormatError> {
        self.edit_request(path, spec)
    }

    /// Adds a new request file in `folder` (`""` for the collection root, else a folder path
    /// as in `Folder::path`). The file name is derived from `spec.name` and made unique;
    /// `seq` is set after the last item if `spec.seq` is `None`. Returns the new request path.
    ///
    /// File name: a slug of the name (lower case, `-` between words, at most 64 characters,
    /// `request` if nothing is left), then `-2`, `-3`, ... if taken (ignoring case, for
    /// case-insensitive file systems). Layout as Bruno 3 writes new requests.
    ///
    /// Owner: wave 3, strand F.
    pub fn add_request(
        &mut self,
        folder: &str,
        spec: &rt_core::model::RequestSpec,
    ) -> Result<String, FormatError> {
        self.new_request(folder, spec)
    }

    /// Adds an empty folder `name` in `parent` (`""` for the root): a `folder.yml` with name,
    /// type and `seq` after the last item. The directory is the name with characters not
    /// allowed in file names replaced by `-`, with `-2`, `-3`, ... if taken (ignoring case).
    /// Returns the new folder's path. Added in wave 6 (plugin views add requests in folders).
    pub fn add_folder(&mut self, parent: &str, name: &str) -> Result<String, FormatError> {
        self.new_folder(parent, name)
    }

    /// Sets the scripts of the collection (`folder` is `""`: `request.scripts` of
    /// `opencollection.yml`) or of a folder (`request.scripts` of its `folder.yml`, created
    /// if the folder has none). Entries of phases RequestTailor does not run (gRPC) stay;
    /// everything else in the file stays as it was. The edit is read back and must give
    /// `scripts`, otherwise it is refused and the file is left as it was.
    ///
    /// Added in wave 7 (strand B); request scripts, assertions and actions are written by
    /// [`set_request`](Self::set_request).
    pub fn set_folder_scripts(
        &mut self,
        folder: &str,
        scripts: &[rt_core::model::Script],
    ) -> Result<(), FormatError> {
        self.edit_default_scripts(folder, scripts)
    }

    /// Creates or replaces the environment `env.name` (file `environments/<name>.yml`).
    /// Secret variables are written as `secret: true` without a value, whatever `value`
    /// holds. Returns the file path.
    ///
    /// An environment declared inline in `opencollection.yml` (`config.environments`) is
    /// edited there and `opencollection.yml` is returned. A new file is named after the
    /// environment, with characters not allowed in file names replaced by `-`.
    ///
    /// Owner: wave 3, strand F.
    pub fn set_environment(
        &mut self,
        env: &rt_core::model::Environment,
    ) -> Result<String, FormatError> {
        self.edit_environment(env)
    }

    /// Removes the environment `name` and its file (or its entry in `opencollection.yml`).
    ///
    /// Owner: wave 3, strand F.
    pub fn remove_environment(&mut self, name: &str) -> Result<(), FormatError> {
        self.delete_environment(name)
    }

    /// The parsed YAML of one file, for edits that the model does not cover yet.
    #[cfg(test)]
    fn yaml_mut(&mut self, path: &str) -> Option<&mut yaml::Document> {
        match self.files.get_mut(path)? {
            Source::Yaml(doc) => Some(doc),
            Source::Unparsed(_) => None,
        }
    }
}

/// Whether the format owns this file: a `.yml` file that is not inside a nested collection.
fn owned(path: &str, files: &FileSet) -> bool {
    if !path.ends_with(".yml") {
        return false;
    }
    let mut dir = read::dir_of(path);
    while !dir.is_empty() {
        if files
            .files
            .contains_key(&format!("{dir}/{COLLECTION_FILE}"))
        {
            return false;
        }
        dir = read::dir_of(dir);
    }
    true
}

/// Parses a collection from its files.
pub fn read_collection(files: &FileSet) -> Result<CollectionDoc, FormatError> {
    if !files.files.contains_key(COLLECTION_FILE) {
        return Err(FormatError::NotACollection);
    }
    let mut sources = BTreeMap::new();
    let mut issues = Vec::new();
    for (path, bytes) in &files.files {
        if !owned(path, files) {
            continue;
        }
        let parsed = std::str::from_utf8(bytes)
            .map_err(|e| format!("not UTF-8: {e}"))
            .and_then(|text| yaml::parse(text).map_err(|e| e.to_string()));
        match parsed {
            Ok(doc) => {
                sources.insert(path.clone(), Source::Yaml(doc));
            }
            Err(message) if path == COLLECTION_FILE => {
                return Err(FormatError::Invalid {
                    path: path.clone(),
                    message,
                });
            }
            Err(message) => {
                issues.push(FormatError::Invalid {
                    path: path.clone(),
                    message,
                });
                sources.insert(path.clone(), Source::Unparsed(bytes.clone()));
            }
        }
    }
    let docs: BTreeMap<&str, &yaml::Document> = sources
        .iter()
        .filter_map(|(path, source)| match source {
            Source::Yaml(doc) => Some((path.as_str(), doc)),
            Source::Unparsed(_) => None,
        })
        .collect();
    read::check_version(&docs[COLLECTION_FILE].root)?;
    let mut builder = read::Builder::new(&docs);
    let model = builder.collection();
    let parse_issues = issues.clone();
    issues.extend(builder.issues);
    Ok(CollectionDoc {
        model,
        files: sources,
        issues,
        parse_issues,
    })
}

#[cfg(test)]
mod tests;
