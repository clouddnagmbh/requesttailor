// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importers: other tools' files → a new OpenCollection collection, plus a loss report.
//!
//! Every importer is its own module with the same signature (see [`import`]) and turns its
//! source into a complete collection ([`Import::files`], written by this crate's writer, so
//! it reads back with [`crate::read_collection`]). Whatever it cannot carry over goes into the
//! [`LossReport`]: nothing is dropped silently (CLAUDE.md, "Importer").
//!
//! Like the rest of the crate this is in memory and I/O-free, so it runs in the browser too.
//! Test data: `testdata/import/<source>/`, every file with a snapshot test of the result and
//! of its loss report.
//!
//! Owner: wave 4, strand B. The types in this file are interface (working rule 1).

use serde::{Deserialize, Serialize};

use crate::FileSet;

mod curl;
mod har;
mod http;
mod insomnia;
mod openapi;
mod postman;

/// The formats RequestTailor imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportFormat {
    /// Postman collection v2.0 / v2.1 (JSON).
    Postman,
    /// Insomnia export v4 (JSON or YAML) and v5 (YAML).
    Insomnia,
    /// OpenAPI 3.0 / 3.1 (JSON or YAML).
    Openapi,
    /// One or more `curl` command lines.
    Curl,
    /// HTTP Archive 1.2 (JSON).
    Har,
    /// `.http` / `.rest` files (VS Code REST Client, JetBrains HTTP Client).
    Http,
}

impl ImportFormat {
    pub const ALL: [ImportFormat; 6] = [
        ImportFormat::Postman,
        ImportFormat::Insomnia,
        ImportFormat::Openapi,
        ImportFormat::Curl,
        ImportFormat::Har,
        ImportFormat::Http,
    ];
}

/// A converted collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// Name for the collection (and the default folder name).
    pub collection_name: String,
    /// The complete collection: `opencollection.yml`, folders, requests, environments.
    pub files: FileSet,
    pub report: LossReport,
}

/// What an import could not carry over, item by item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossReport {
    pub format: ImportFormat,
    /// Requests written to the collection.
    pub requests: u32,
    /// Folders written to the collection.
    pub folders: u32,
    /// Environments written to the collection.
    pub environments: u32,
    /// Sorted by `location`, then `kind`.
    pub losses: Vec<Loss>,
}

impl LossReport {
    /// `true` if nothing was lost.
    pub fn is_complete(&self) -> bool {
        self.losses.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loss {
    /// Where in the source, as the user knows it: `Users / Get user` for an item,
    /// `environment Staging`, `collection` for collection-level settings.
    pub location: String,
    pub kind: LossKind,
    pub effect: LossEffect,
    /// One readable sentence, e.g. `pre-request script (12 lines) not imported`.
    pub detail: String,
}

/// What was lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LossKind {
    /// Pre-request or post-response script.
    Script,
    /// Tests and assertions.
    Test,
    /// An auth scheme the core does not support yet (OAuth 2.0, Digest, AWS, ...).
    Auth,
    /// A body type or part that could not be represented (GraphQL variables, binary, ...).
    Body,
    /// Variables, dynamic values (`{{$guid}}`) and their scopes.
    Variable,
    /// Request settings (proxy, certificates, timeouts, cookie handling, ...).
    Setting,
    /// Descriptions and documentation.
    Documentation,
    /// Saved example responses.
    Example,
    /// Items of another protocol (WebSocket, gRPC, GraphQL subscriptions, ...).
    Protocol,
    Other,
}

/// How much of it was lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LossEffect {
    /// Not in the collection at all.
    Dropped,
    /// In the collection, but not exactly as in the source (the detail says how).
    Approximated,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("unknown file format; choose the format explicitly")]
    UnknownFormat,
    #[error("not a valid {format:?} file: {message}")]
    Invalid {
        format: ImportFormat,
        message: String,
    },
    #[error("{format:?} version {version} is not supported")]
    UnsupportedVersion {
        format: ImportFormat,
        version: String,
    },
}

/// Guesses the format from the file name and the first bytes; `None` if it is none of them.
pub fn detect(file_name: &str, data: &[u8]) -> Option<ImportFormat> {
    // Owner: strand B. Until then only the file name decides.
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".har") {
        Some(ImportFormat::Har)
    } else if lower.ends_with(".http") || lower.ends_with(".rest") {
        Some(ImportFormat::Http)
    } else if lower.ends_with(".postman_collection.json") {
        Some(ImportFormat::Postman)
    } else if data.trim_ascii_start().starts_with(b"curl ") {
        Some(ImportFormat::Curl)
    } else {
        None
    }
}

/// Imports `data` (the file `file_name`) as `format`, or as the detected format.
pub fn import(
    format: Option<ImportFormat>,
    file_name: &str,
    data: &[u8],
) -> Result<Import, ImportError> {
    let format = format
        .or_else(|| detect(file_name, data))
        .ok_or(ImportError::UnknownFormat)?;
    match format {
        ImportFormat::Postman => postman::import(file_name, data),
        ImportFormat::Insomnia => insomnia::import(file_name, data),
        ImportFormat::Openapi => openapi::import(file_name, data),
        ImportFormat::Curl => curl::import(file_name, data),
        ImportFormat::Har => har::import(file_name, data),
        ImportFormat::Http => http::import(file_name, data),
    }
}

/// Placeholder result of an importer that does not exist yet.
fn not_yet(format: ImportFormat) -> Result<Import, ImportError> {
    Err(ImportError::Invalid {
        format,
        message: "this importer is not implemented yet".into(),
    })
}
