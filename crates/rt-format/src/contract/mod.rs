// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The contract between a collection and an OpenAPI specification in the same repository
//! (wave 12): requests checked against the spec's operations, breaking changes between two
//! versions of the spec, and an API changelog in Markdown.
//!
//! In `rt-format` because the OpenAPI reader of the importer (`import::openapi`) lives here;
//! everything works in memory (bytes of the spec, [`FileSet`] of the collection), the same
//! natively, in the browser and in `rt changelog`.
//!
//! Where the spec is: `extensions.requesttailor.contract.spec` in `opencollection.yml` (a path
//! relative to the collection root; a leading `/` is relative to the repository root, as in
//! `GitApi.show`), else the first of [`SPEC_CANDIDATES`] that exists.
//!
//! Owner: wave 12 interface; implementation strand F.

use serde::{Deserialize, Serialize};

use crate::FileSet;
use crate::diff::CollectionDiff;

/// Looked for, in this order, when `opencollection.yml` names no spec.
pub const SPEC_CANDIDATES: &[&str] = &[
    "openapi.yaml",
    "openapi.yml",
    "openapi.json",
    "/openapi.yaml",
    "/openapi.yml",
    "/openapi.json",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum ContractError {
    /// The spec is not readable OpenAPI 3.x (or Swagger 2.0).
    #[error("{0}")]
    Spec(String),
    /// Not available yet (wave 12 interface; strand F replaces it).
    #[error("{0}")]
    Unsupported(String),
}

/// The spec a collection is checked against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractSpec {
    /// As configured or found, e.g. `openapi.yaml` or `/api/openapi.json`.
    pub path: String,
    /// `info.title` and `info.version` of the spec.
    pub title: String,
    pub version: String,
    /// `true`: named in `opencollection.yml`; `false`: found by [`SPEC_CANDIDATES`].
    pub configured: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingKind {
    /// No operation of the spec matches the request's method and path.
    NoOperation,
    /// A query, path or header parameter the operation does not declare.
    UnknownParameter,
    /// A required parameter the request does not send.
    MissingParameter,
    /// The body does not fit the operation's request body (media type or schema).
    BodyMismatch,
}

/// One problem of a request against the spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractFinding {
    /// Request path relative to the collection root.
    pub request: String,
    pub kind: FindingKind,
    /// The operation it was checked against, e.g. `GET /users/{id}`; `None` for
    /// `NoOperation`.
    pub operation: Option<String>,
    pub message: String,
}

/// The collection checked against the spec.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContractReport {
    pub findings: Vec<ContractFinding>,
    /// Operations of the spec, e.g. `DELETE /users/{id}`, that no request covers.
    pub uncovered: Vec<String>,
    /// Operations in the spec, and how many of them requests cover.
    pub operations: u32,
    pub covered: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpecChangeKind {
    OperationAdded,
    OperationRemoved,
    ParameterAdded,
    ParameterRemoved,
    /// A parameter or request body field became required (or a new required one).
    RequiredAdded,
    /// A field of a request or response schema was removed.
    FieldRemoved,
    FieldAdded,
    /// A field, parameter or schema changed its type or format.
    TypeChanged,
    /// A response status was removed or added.
    ResponseChanged,
    /// Anything else (descriptions, examples, …): never breaking.
    Other,
}

/// One change of the spec between two versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecChange {
    pub kind: SpecChangeKind,
    /// Breaks existing clients (removed fields, changed types, new required parameters, …).
    pub breaking: bool,
    /// e.g. `GET /users/{id}`; `None` for changes outside operations (components).
    pub operation: Option<String>,
    /// Where in the operation, e.g. `query parameter limit`, `response 200: items[].id`.
    pub location: String,
    pub message: String,
    /// Requests of the collection that use the operation.
    pub requests: Vec<String>,
}

/// The changes of the spec between two versions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SpecChanges {
    /// `info.version` on each side (`None` where the spec did not exist).
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub changes: Vec<SpecChange>,
}

impl SpecChanges {
    pub fn breaking(&self) -> impl Iterator<Item = &SpecChange> {
        self.changes.iter().filter(|c| c.breaking)
    }
}

fn not_yet() -> ContractError {
    ContractError::Unsupported("the contract check arrives in wave 12 (strand F)".into())
}

/// The spec configured in `opencollection.yml` of `files`, if any (path only; the shell reads
/// the spec's bytes from the work tree or with `GitApi.show`).
pub fn configured_spec(files: &FileSet) -> Option<String> {
    let _ = files;
    None
}

/// `info.title` and `info.version` of a spec, for [`ContractSpec`].
pub fn spec_info(spec: &[u8]) -> Result<(String, String), ContractError> {
    let _ = spec;
    Err(not_yet())
}

/// Checks the requests of the collection in `files` against the spec.
pub fn check(files: &FileSet, spec: &[u8]) -> Result<ContractReport, ContractError> {
    let _ = (files, spec);
    Err(not_yet())
}

/// Changes from the spec `old` to `new` (`None`: the spec did not exist on that side), with
/// the requests of `files` (the new collection) that each change affects.
pub fn changes(
    old: Option<&[u8]>,
    new: Option<&[u8]>,
    files: &FileSet,
) -> Result<SpecChanges, ContractError> {
    let _ = (old, new, files);
    Err(not_yet())
}

/// The API changelog (Markdown) for a range: spec changes (breaking first) and the
/// collection's own changes (the semantic diff of the same range). `title`: the heading, e.g.
/// `Orders API 1.2.0 → 1.3.0`.
pub fn changelog(title: &str, spec: &SpecChanges, collection: &CollectionDiff) -> String {
    let _ = (spec, collection);
    format!("# {title}\n\nThe API changelog arrives in wave 12 (strand F).\n")
}
