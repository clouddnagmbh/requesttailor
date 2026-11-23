// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Types of the OpenAPI import wizard (interface): what a spec holds ([`SpecOverview`], for the
//! wizard's "Overview" and "Choose" steps) and what the user chose ([`OpenApiOptions`]).
//!
//! Flow: [`super::inspect_openapi`] reads the spec (one file, several files of a multi-file
//! spec, or the entries of a zip) without converting it; [`super::import_openapi`] converts it
//! with the choices into an [`super::Import`] like any other importer. Swagger 2.0 is converted
//! to the OpenAPI 3 model first; external `$ref`s to relative files among the given files are
//! resolved, missing ones are reported.
//!
//! `OpenApiOptions::default()` gives exactly what `import(Some(Openapi), …)` gives (byte for
//! byte), so the import corpus stays the reference.

use serde::{Deserialize, Serialize};

use super::Loss;

/// A spec as the wizard shows it before anything is converted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecOverview {
    /// `info.title` (or the file name).
    pub title: String,
    /// `info.version`.
    pub version: String,
    /// `info.description` (Markdown), if any.
    pub description: Option<String>,
    /// The spec's own version: `3.0.3`, `3.1.0`, `2.0` (Swagger) …
    pub spec_version: String,
    /// Name of the main file among the given ones.
    pub main_file: String,
    /// Files read (the main file and the files it references).
    pub files: u32,
    pub servers: Vec<SpecServer>,
    /// Tags in the spec's order (top-level `tags` first, then as used), with their operation
    /// counts; operations without a tag are not in any.
    pub tags: Vec<SpecTag>,
    /// Operations in path order.
    pub operations: Vec<SpecOperation>,
    pub security: Vec<SpecSecurityScheme>,
    /// What reading already lost (unresolved external references, …), worded like the import
    /// report.
    pub losses: Vec<Loss>,
}

/// One server of `servers` (Swagger 2.0: from `schemes`, `host` and `basePath`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecServer {
    /// As written, with `{var}` templates.
    pub url: String,
    pub description: Option<String>,
    /// The environment name the import suggests (description's first line, else the host).
    pub suggested_name: String,
    pub variables: Vec<SpecServerVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecServerVariable {
    pub name: String,
    pub default: String,
    /// `enum` values, empty if free text.
    #[serde(default)]
    pub values: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecTag {
    pub name: String,
    pub description: Option<String>,
    pub operations: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecOperation {
    /// `GET /pets/{petId}`: the key [`OpenApiOptions::operations`] lists.
    pub key: String,
    /// Upper case.
    pub method: String,
    /// The path template.
    pub path: String,
    /// All tags; the import files the request under the first one.
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub operation_id: Option<String>,
    pub deprecated: bool,
}

/// A security scheme and what the import makes of it (the wizard's auth mapping preview).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecSecurityScheme {
    /// The scheme's name in `components.securitySchemes` (`securityDefinitions`).
    pub name: String,
    /// `basic`, `bearer`, `digest`, `api-key`, `oauth2`, `openid-connect`, `mutual-tls` or
    /// `other`.
    pub kind: String,
    /// e.g. `header X-API-Key`, `client credentials flow`, `JWT`.
    pub detail: String,
    /// What the requests get, in plain words, e.g. `Bearer token {{token}}`; `None` if the
    /// scheme is not imported.
    pub maps_to: Option<String>,
    /// Variables the auth uses (added to every environment; secrets without a value).
    pub variables: Vec<String>,
}

/// How requests are grouped into folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenApiGrouping {
    /// One folder per operation's first tag (the default).
    #[default]
    Tag,
    /// One folder per first path segment (`/pets/{id}` → `pets`).
    Path,
    /// No folders.
    Flat,
}

/// What a request is named after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenApiNaming {
    /// `summary`, else `operationId`, else `METHOD /path` (the default).
    #[default]
    Summary,
    /// `operationId`, else `summary`, else `METHOD /path`.
    OperationId,
}

/// Where example values of bodies and parameters come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenApiExamples {
    /// The spec's examples, generated from the schema where there are none (the default).
    #[default]
    Spec,
    /// Always generated from the schema.
    Schema,
}

/// What one server becomes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenApiServerChoice {
    /// Index into [`SpecOverview::servers`].
    pub index: u32,
    /// Name of the environment it becomes; `None`: no environment for it.
    pub environment: Option<String>,
}

/// The wizard's choices. `Default` is the plain import.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OpenApiOptions {
    /// Keys of the operations to import ([`SpecOperation::key`]); `None`: all.
    #[serde(default)]
    pub operations: Option<Vec<String>>,
    #[serde(default)]
    pub grouping: OpenApiGrouping,
    #[serde(default)]
    pub naming: OpenApiNaming,
    #[serde(default)]
    pub examples: OpenApiExamples,
    /// Environments from servers; `None`: every server, named as suggested.
    #[serde(default)]
    pub servers: Option<Vec<OpenApiServerChoice>>,
    /// The server whose URL becomes the collection's `baseUrl` (index into
    /// [`SpecOverview::servers`]); `None`: the first.
    #[serde(default)]
    pub default_server: Option<u32>,
    /// Name of the collection; `None`: the spec's title.
    #[serde(default)]
    pub collection_name: Option<String>,
    /// Keep the spec in the collection (`openapi.yaml` or `openapi.json`; a multi-file spec
    /// bundled into `openapi.json`) and name it in `extensions.requesttailor.contract.spec`,
    /// so the contract check and the run wizard know it.
    #[serde(default)]
    pub keep_spec: bool,
}
