// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-format`: reads and writes OpenCollection 1.0 (YAML) collections; importers later.
//!
//! This is the only crate that knows the file format (see CLAUDE.md, "Format"). It maps files
//! to `rt_core::model` and back. It works on an in-memory [`FileSet`], so the same code runs
//! natively and in the browser; only [`FileSet::from_dir`] touches the file system.
//!
//! Owner: wave 2, strand B.

use std::collections::BTreeMap;

use rt_core::model::Collection;

/// Files of one collection, keyed by path relative to the collection root, `/`-separated
/// (also on Windows), e.g. `opencollection.yml`, `users/folder.yml`, `environments/Local.yml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileSet {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl FileSet {
    /// Reads every file below `root` that belongs to a collection. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_dir(root: &std::path::Path) -> std::io::Result<Self> {
        let _ = root;
        todo!("wave 2, strand B")
    }

    /// Writes all files below `root`, creating folders as needed. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_to_dir(&self, root: &std::path::Path) -> std::io::Result<()> {
        let _ = root;
        todo!("wave 2, strand B")
    }
}

/// Name of the file that marks a collection root.
pub const COLLECTION_FILE: &str = "opencollection.yml";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("{COLLECTION_FILE} not found")]
    NotACollection,
    #[error("{path}: {message}")]
    Invalid { path: String, message: String },
    #[error("{path}: unsupported OpenCollection version {version}")]
    UnsupportedVersion { path: String, version: String },
}

/// A parsed collection that remembers everything needed to write the files back
/// byte-identically, including fields the core model does not represent (scripts,
/// assertions, docs, examples, unknown auth types, ...).
#[derive(Debug, Clone)]
pub struct CollectionDoc {
    model: Collection,
}

impl CollectionDoc {
    /// The format-neutral view the rest of the system works with.
    pub fn model(&self) -> &Collection {
        &self.model
    }

    /// Serializes the document back to files. For an unmodified document,
    /// `read_collection(files)?.write() == files` byte for byte (for files the format owns).
    pub fn write(&self) -> FileSet {
        todo!("wave 2, strand B")
    }
}

/// Parses a collection from its files.
pub fn read_collection(files: &FileSet) -> Result<CollectionDoc, FormatError> {
    let _ = files;
    todo!("wave 2, strand B")
}
