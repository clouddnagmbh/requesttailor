// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-format`: reads and writes OpenCollection 1.0 (YAML) collections; importers later.
//!
//! This is the only crate that knows the file format (see CLAUDE.md, "Format"). It maps files
//! to `rt_core::model` and back. It works on an in-memory [`FileSet`], so the same code runs
//! natively and in the browser; only [`FileSet::from_dir`] touches the file system.
//!
//! Files are parsed into a format-preserving YAML tree (module `yaml`): writing an unchanged
//! document reproduces every file byte for byte, including everything the model does not
//! represent (scripts, assertions, docs, examples, OAuth configurations, comments, ...).
//!
//! Layout of a collection: `opencollection.yml` at the root, one `folder.yml` per folder
//! (optional), one `*.yml` per item, `environments/<name>.yml` per environment. A folder that
//! contains its own `opencollection.yml` is a separate, nested collection and not part of
//! this one. How files map to the model is described in the `read` module.
//!
//! Owner: wave 2, strand B.

use std::collections::BTreeMap;

use rt_core::model::Collection;

#[cfg(not(target_arch = "wasm32"))]
mod fs;
mod read;
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
/// byte-identically, including fields the core model does not represent (scripts,
/// assertions, docs, examples, unknown auth types, ...).
#[derive(Debug, Clone)]
pub struct CollectionDoc {
    model: Collection,
    files: BTreeMap<String, Source>,
    issues: Vec<FormatError>,
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
    issues.extend(builder.issues);
    Ok(CollectionDoc {
        model,
        files: sources,
        issues,
    })
}

#[cfg(test)]
mod tests;
