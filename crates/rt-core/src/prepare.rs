// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Turns a `RequestContext` into a wire-ready `HttpRequest`: builds the variable scope,
//! interpolates, merges inherited headers and settings, resolves auth inheritance and encodes
//! the body.
//!
//! Owner: wave 2, strand C.

use crate::model::{Auth, RequestContext};
use crate::transport::HttpRequest;
use crate::vars::{Scope, VarUse};

/// Reads files referenced by bodies (file body, multipart file parts), relative to the
/// collection root. Native: the file system. Browser: the in-memory file set.
pub trait FileSource {
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
}

/// Inputs that are not in the collection files.
#[derive(Debug, Clone, Default)]
pub struct PrepareInput<'a> {
    /// Environment to use, by name. `None`: no environment layer.
    pub environment: Option<&'a str>,
    /// Global variables (lowest layer).
    pub globals: Vec<(String, String)>,
    /// Runtime variables (highest layer), e.g. `--var name=value`.
    pub runtime: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub request: HttpRequest,
    /// The effective auth after inheritance, before interpolation (for display).
    pub auth: Auth,
    pub scope: Scope,
    pub used: Vec<VarUse>,
    /// Placeholders that had no value anywhere, deduplicated, in first-seen order.
    pub unresolved: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrepareError {
    #[error("environment \"{0}\" not found")]
    UnknownEnvironment(String),
    #[error("auth type \"{0}\" is not supported yet")]
    UnsupportedAuth(String),
    #[error("invalid URL after variable substitution: {0}")]
    InvalidUrl(String),
    #[error("cannot read body file \"{path}\": {message}")]
    BodyFile { path: String, message: String },
}

pub fn prepare(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
) -> Result<Prepared, PrepareError> {
    let _ = (context, input, files);
    todo!("wave 2, strand C")
}
