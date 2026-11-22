// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Import in the app: convert a file (`rt_format::import`), show the loss report, then write
//! the new collection into a folder the user picks.
//!
//! Two steps, so the user sees what would be lost before anything is written:
//! 1. [`Imports::analyze`] converts and keeps the result under an id; the UI shows the
//!    [`ImportPlan`] (counts and loss report).
//! 2. [`Imports::take`] hands the converted files to the shell, which writes them into a new,
//!    empty folder and opens it as the collection (`ImportApi.write`).
//!
//! Owner: wave 4, strand B. Changes to the public types are interface changes.

use std::collections::BTreeMap;

use rt_format::FileSet;
use rt_format::import::{ImportFormat, LossReport};
use serde::{Deserialize, Serialize};

use crate::AppError;

/// Result of the first step, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPlan {
    /// Pass to `write` or `discard`.
    pub id: u64,
    pub format: ImportFormat,
    pub collection_name: String,
    /// Suggested name of the new folder (file-system safe).
    pub folder_name: String,
    /// Number of files the collection will have.
    pub files: u32,
    pub report: LossReport,
}

/// Converted collections waiting to be written, by id.
#[derive(Debug, Default)]
pub struct Imports {
    next: u64,
    pending: BTreeMap<u64, FileSet>,
}

impl Imports {
    pub fn new() -> Self {
        Self::default()
    }

    /// Converts `data` (the file `file_name`) as `format`, or as the detected format.
    pub fn analyze(
        &mut self,
        file_name: &str,
        data: &[u8],
        format: Option<ImportFormat>,
    ) -> Result<ImportPlan, AppError> {
        let import = rt_format::import::import(format, file_name, data)
            .map_err(|e| AppError::Import(e.to_string()))?;
        self.next += 1;
        let id = self.next;
        let plan = ImportPlan {
            id,
            format: import.report.format,
            folder_name: folder_name(&import.collection_name),
            collection_name: import.collection_name,
            files: import.files.files.len() as u32,
            report: import.report,
        };
        self.pending.insert(id, import.files);
        Ok(plan)
    }

    /// The files of plan `id`, for writing; the plan is gone afterwards.
    pub fn take(&mut self, id: u64) -> Result<FileSet, AppError> {
        self.pending
            .remove(&id)
            .ok_or_else(|| AppError::Import(format!("import {id} not found")))
    }

    pub fn discard(&mut self, id: u64) {
        self.pending.remove(&id);
    }
}

/// A folder name for `collection_name` that is safe on every file system.
pub fn folder_name(collection_name: &str) -> String {
    let cleaned: String = collection_name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "Imported collection".to_owned()
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(folder_name("My API"), "My API");
        assert_eq!(folder_name("a/b:c"), "a-b-c");
        assert_eq!(folder_name("  ..  "), "Imported collection");
    }

    #[test]
    fn unknown_formats_are_an_import_error() {
        let mut imports = Imports::new();
        let e = imports.analyze("x.bin", b"\x00", None).unwrap_err();
        assert!(matches!(e, AppError::Import(_)), "{e:?}");
        assert!(imports.take(1).is_err());
    }
}
