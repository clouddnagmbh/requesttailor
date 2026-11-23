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
//! Owner: wave 12, strand F.
//!
//! Scope: requests against operations (method and path template, parameters, request body
//! schema), changes between two versions of the spec, the changelog. Response schemas against
//! real responses are not checked.

use serde::{Deserialize, Serialize};

use crate::diff::CollectionDiff;
use crate::{COLLECTION_FILE, FileSet};

mod changelog;
mod changes;
mod check;
mod matching;
mod schema;
mod spec;

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
    /// Something else stops the check, e.g. the collection cannot be read.
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

/// The spec configured in `opencollection.yml` of `files`, if any (path only; the shell reads
/// the spec's bytes from the work tree or with `GitApi.show`).
pub fn configured_spec(files: &FileSet) -> Option<String> {
    let bytes = files.files.get(COLLECTION_FILE)?;
    let text = std::str::from_utf8(bytes).ok()?;
    let doc = crate::yaml::parse(text).ok()?;
    let path = doc
        .root
        .get("extensions")?
        .get("requesttailor")?
        .get("contract")?
        .get("spec")?
        .text()?
        .trim()
        .to_owned();
    (!path.is_empty()).then_some(path)
}

/// `info.title` and `info.version` of a spec, for [`ContractSpec`].
pub fn spec_info(spec: &[u8]) -> Result<(String, String), ContractError> {
    let spec = spec::Spec::parse(spec)?;
    Ok((spec.title, spec.version))
}

/// Checks the requests of the collection in `files` against the spec.
pub fn check(files: &FileSet, spec: &[u8]) -> Result<ContractReport, ContractError> {
    let spec = spec::Spec::parse(spec)?;
    check::check(files, &spec)
}

/// Changes from the spec `old` to `new` (`None`: the spec did not exist on that side), with
/// the requests of `files` (the new collection) that each change affects.
pub fn changes(
    old: Option<&[u8]>,
    new: Option<&[u8]>,
    files: &FileSet,
) -> Result<SpecChanges, ContractError> {
    let old = old.map(spec::Spec::parse).transpose()?;
    let new = new.map(spec::Spec::parse).transpose()?;
    changes::changes(old.as_ref(), new.as_ref(), files)
}

/// The API changelog (Markdown) for a range: spec changes (breaking first) and the
/// collection's own changes (the semantic diff of the same range). `title`: the heading, e.g.
/// `Orders API 1.2.0 → 1.3.0`.
pub fn changelog(title: &str, spec: &SpecChanges, collection: &CollectionDiff) -> String {
    changelog::render(title, spec, collection)
}

// ---- the run wizard ("Ready to send?") and the response check (interface) ----

/// A spec parsed once, for many [`requirements`] and [`validate_response`] calls (the shells
/// cache it per collection; parsing a large spec takes far longer than a check).
#[derive(Debug, Clone)]
pub struct ParsedSpec {
    spec: spec::Spec,
}

impl ParsedSpec {
    pub fn parse(bytes: &[u8]) -> Result<Self, ContractError> {
        Ok(Self {
            spec: spec::Spec::parse(bytes)?,
        })
    }

    pub fn title(&self) -> &str {
        &self.spec.title
    }

    pub fn version(&self) -> &str {
        &self.spec.version
    }

    /// Number of operations.
    pub fn operations(&self) -> usize {
        self.spec.ops.len()
    }
}

/// Whether a variable the request uses has a value when it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VarState {
    /// No layer defines it (or a secret without a stored value).
    Unresolved,
    /// Defined, but the value is empty.
    Empty,
    Set,
}

/// A variable the request uses, as the app resolves it for the chosen environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarFact {
    pub name: String,
    pub state: VarState,
    /// Declared secret somewhere, or a credential of the auth (token, password, API key,
    /// client secret).
    pub secret: bool,
    /// Used by the request's effective auth (its own or inherited).
    pub auth: bool,
    /// The resolved value; `None` for secrets and unresolved ones.
    pub value: Option<String>,
}

/// What the check needs to know about a request besides its spec: the variables it uses.
#[derive(Debug, Clone, Copy)]
pub struct RequestFacts<'a> {
    /// The request as edited (unsaved changes included).
    pub request: &'a rt_core::model::RequestSpec,
    pub variables: &'a [VarFact],
}

/// Where an input of the run wizard goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputPlace {
    /// A `{{variable}}` (`baseUrl`, server variables, auth credentials).
    Variable,
    Path,
    Query,
    Header,
    /// The whole request body (`name` is `body`).
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProblemKind {
    Missing,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputProblem {
    pub kind: ProblemKind,
    /// One plain sentence, e.g. `petId is required` or `must be one of available, sold`.
    pub message: String,
    /// More, for the body: `$.name: required property is missing`, at most a few.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
}

/// One input of the operation, with the spec's schema for the form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputField {
    /// Unique within the requirements: `variable:baseUrl`, `path:petId`, `query:limit`,
    /// `header:X-Request-Id`, `body`.
    pub id: String,
    pub place: InputPlace,
    pub name: String,
    pub required: bool,
    /// Its value belongs in the keychain (or secret store), never in a file.
    pub secret: bool,
    /// A credential or setting of the auth.
    pub auth: bool,
    /// The current value as written in the request (path, query, header; the body text), or
    /// the resolved one of a variable; `None` for secrets and when absent.
    pub value: Option<String>,
    /// `None`: fine as it is.
    pub problem: Option<InputProblem>,
    /// The JSON Schema of the value, local `$ref`s inlined (bounded depth, cycles as `{}`);
    /// `None` if the spec gives none.
    pub schema: Option<serde_json::Value>,
    pub description: Option<String>,
    /// An example value from the spec (text; JSON for the body), for "fill".
    pub example: Option<String>,
}

/// A server of the spec, for choosing `baseUrl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerOption {
    /// With `{{var}}` templates, as the import writes it.
    pub url: String,
    pub description: Option<String>,
}

/// What the run wizard asks before a request is sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendRequirements {
    /// e.g. `GET /pets/{petId}`.
    pub operation: String,
    pub summary: Option<String>,
    pub deprecated: bool,
    /// The spec's path as configured or found (`openapi.yaml`, `/api/openapi.json`).
    pub spec: String,
    /// `true` when no field has a problem: send without asking.
    pub ready: bool,
    /// Problem fields first (variables, path, query, header, body), then the others.
    pub fields: Vec<InputField>,
    pub servers: Vec<ServerOption>,
}

/// Where a response differs from the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DifferencePlace {
    Status,
    Header,
    /// Content type of the body.
    ContentType,
    Body,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseDifference {
    pub place: DifferencePlace,
    /// `$.items[0].id` for the body, the header name, `status`.
    pub path: String,
    pub expected: String,
    pub got: String,
}

/// A response against the operation's responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseCheck {
    pub operation: String,
    pub status: u16,
    /// The response entry it was checked against (`200`, `2XX`, `default`); `None` if the
    /// spec declares none for this status.
    pub matched: Option<String>,
    /// Empty: matches the spec.
    pub differences: Vec<ResponseDifference>,
    /// The body was not checked (too large, not JSON, truncated), and why.
    pub body_skipped: Option<String>,
}

/// A response as received.
#[derive(Debug, Clone, Copy)]
pub struct ResponseFacts<'a> {
    pub status: u16,
    pub headers: &'a [(String, String)],
    /// `None`: not available (truncated, too large).
    pub body: Option<&'a [u8]>,
}

/// Largest body [`validate_response`] parses.
pub const MAX_CHECKED_BODY: usize = 8 * 1024 * 1024;

/// What the request still needs before it can be sent, per the operation it matches; `None`
/// if no operation of the spec matches it (or it is not HTTP).
pub fn requirements(spec: &ParsedSpec, facts: &RequestFacts<'_>) -> Option<SendRequirements> {
    let _ = (&spec.spec, facts);
    None
}

/// `request`'s response against the operation it matches; `None` if none matches.
pub fn validate_response(
    spec: &ParsedSpec,
    request: &rt_core::model::RequestSpec,
    response: &ResponseFacts<'_>,
) -> Option<ResponseCheck> {
    let _ = (&spec.spec, request, response);
    None
}
