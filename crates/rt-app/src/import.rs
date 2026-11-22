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
pub use rt_format::import::SourceFile;
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
    /// Plan id → (suggested folder name, files).
    pending: BTreeMap<u64, (String, FileSet)>,
}

impl Imports {
    pub fn new() -> Self {
        Self::default()
    }

    /// Converts `files` as one collection (`rt_format::import::import_files`: a collection
    /// with its environment files, several `.http` or cURL files) as `format`, or as the
    /// detected format.
    pub fn analyze(
        &mut self,
        files: &[SourceFile<'_>],
        format: Option<ImportFormat>,
    ) -> Result<ImportPlan, AppError> {
        let import = rt_format::import::import_files(format, files)
            .map_err(|e| AppError::Import(e.to_string()))?;
        Ok(self.add(import))
    }

    /// Adds a collection converted elsewhere, by a plugin's importer (wave 5): the shell runs
    /// `PluginHost::import`, checks the files with `rt_format::read_collection`, and hands the
    /// result here so it is written like any other import.
    pub fn add(&mut self, import: rt_format::import::Import) -> ImportPlan {
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
        self.pending
            .insert(id, (plan.folder_name.clone(), import.files));
        plan
    }

    /// The suggested folder name and the files of plan `id`; the plan stays, so a cancelled
    /// or failed write can be tried again. Call [`Imports::discard`] once written.
    pub fn get(&self, id: u64) -> Result<(&str, &FileSet), AppError> {
        self.pending
            .get(&id)
            .map(|(name, files)| (name.as_str(), files))
            .ok_or_else(|| not_found(id))
    }

    /// The files of plan `id`, for writing; the plan is gone afterwards.
    pub fn take(&mut self, id: u64) -> Result<FileSet, AppError> {
        self.pending
            .remove(&id)
            .map(|(_, files)| files)
            .ok_or_else(|| not_found(id))
    }

    pub fn discard(&mut self, id: u64) {
        self.pending.remove(&id);
    }
}

fn not_found(id: u64) -> AppError {
    AppError::Import(format!("import {id} not found"))
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

    fn file<'a>(name: &'a str, data: &'a [u8]) -> SourceFile<'a> {
        SourceFile { name, data }
    }

    #[test]
    fn unknown_formats_are_an_import_error() {
        let mut imports = Imports::new();
        let e = imports
            .analyze(&[file("x.bin", b"\x00")], None)
            .unwrap_err();
        assert!(matches!(e, AppError::Import(_)), "{e:?}");
        assert!(imports.take(1).is_err());
    }

    const COLLECTION: &str = r#"{"info": {"name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "item": [{"name": "List", "request": {"method": "GET", "url": "{{base}}/items"}}]}"#;
    const ENVIRONMENT: &str = r#"{"name": "Prod", "_postman_variable_scope": "environment",
        "values": [{"key": "base", "value": "https://shop.example.com"}, {"key": "token", "value": "not-real", "type": "secret"}]}"#;

    #[test]
    fn analyze_then_take_hands_over_the_files_once() {
        let mut imports = Imports::new();
        let plan = imports
            .analyze(
                &[
                    file("shop.postman_collection.json", COLLECTION.as_bytes()),
                    file("prod.postman_environment.json", ENVIRONMENT.as_bytes()),
                ],
                None,
            )
            .unwrap();
        assert_eq!(plan.format, ImportFormat::Postman);
        assert_eq!(plan.collection_name, "Shop");
        assert_eq!(plan.folder_name, "Shop");
        assert_eq!((plan.report.requests, plan.report.environments), (1, 1));
        // The secret's value is reported, never written.
        assert!(
            plan.report
                .losses
                .iter()
                .any(|l| l.detail.contains("secret value of `token`"))
        );
        let (folder, kept) = imports.get(plan.id).unwrap();
        assert_eq!(folder, "Shop");
        assert_eq!(kept.files.len() as u32, plan.files);
        let files = imports.take(plan.id).unwrap();
        assert_eq!(files.files.len() as u32, plan.files);
        assert!(
            files.files.contains_key("environments/Prod.yml"),
            "{:?}",
            files.files.keys()
        );
        let all: Vec<u8> = files.files.values().flatten().copied().collect();
        assert!(!String::from_utf8_lossy(&all).contains("not-real"));
        assert!(imports.take(plan.id).is_err(), "taken twice");
    }

    #[test]
    fn discarded_plans_are_gone_and_ids_are_not_reused() {
        let mut imports = Imports::new();
        let a = imports
            .analyze(&[file("a.sh", b"curl https://a.example.com")], None)
            .unwrap();
        imports.discard(a.id);
        assert!(imports.take(a.id).is_err());
        let b = imports
            .analyze(&[file("b.sh", b"curl https://b.example.com")], None)
            .unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(b.format, ImportFormat::Curl);
    }
}
