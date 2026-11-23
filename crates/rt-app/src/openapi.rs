// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OpenAPI import wizard in the app: fetching a spec from a URL (with the files its
//! relative `$ref`s name) over the core's [`Transport`], reading it for the wizard
//! ([`rt_format::import::inspect_openapi`]) and converting it with the user's choices into an
//! [`ImportPlan`] that is written like any other import (`ImportApi.write` / `writeInto`).
//!
//! The run wizard ("Ready to send?") and the response check are in [`crate::contract`].

use rt_core::transport::Transport;
use rt_format::import::{Loss, SourceFile};
pub use rt_format::import::{
    OpenApiExamples, OpenApiGrouping, OpenApiNaming, OpenApiOptions, OpenApiServerChoice,
    SpecOperation, SpecOverview, SpecSecurityScheme, SpecServer, SpecServerVariable, SpecTag,
};
use serde::{Deserialize, Serialize};

use crate::{AppError, ImportPlan, Imports};

/// Most files a fetched spec may have (the main file included).
pub const MAX_FETCHED_FILES: usize = 64;
/// Most bytes of all fetched files together.
pub const MAX_FETCHED_BYTES: usize = rt_format::import::MAX_INPUT_BYTES;

/// A spec fetched from a URL: the main file first, then the referenced files, named by their
/// path relative to the main file's URL (as the importer resolves `$ref`s).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchedSpec {
    /// The URL as given (after redirects).
    pub url: String,
    pub files: Vec<FetchedFile>,
    /// References that could not be fetched (or were over the limits), as import losses.
    pub losses: Vec<Loss>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchedFile {
    pub name: String,
    pub text: String,
}

/// Fetches `url` and, recursively, the relative files its `$ref`s name (same origin only,
/// at most [`MAX_FETCHED_FILES`] and [`MAX_FETCHED_BYTES`]). Fails with kind `import` if the
/// main file cannot be fetched (the message names the HTTP status or transport error).
pub async fn fetch_spec<T: Transport>(transport: &T, url: &str) -> Result<FetchedSpec, AppError> {
    let _ = (transport, url);
    Err(AppError::Unsupported(
        "fetching specs is not built yet".into(),
    ))
}

/// [`rt_format::import::inspect_openapi`] with the error as an [`AppError`].
pub fn inspect(files: &[SourceFile<'_>]) -> Result<SpecOverview, AppError> {
    rt_format::import::inspect_openapi(files).map_err(|e| AppError::Import(e.to_string()))
}

impl Imports {
    /// Converts the spec in `files` with the wizard's `options` and keeps the result as a plan
    /// (written with `take` / [`crate::import::add_to_workspace`] like any import).
    pub fn analyze_openapi(
        &mut self,
        files: &[SourceFile<'_>],
        options: &OpenApiOptions,
    ) -> Result<ImportPlan, AppError> {
        let import = rt_format::import::import_openapi(files, options)
            .map_err(|e| AppError::Import(e.to_string()))?;
        Ok(self.add(import))
    }
}
