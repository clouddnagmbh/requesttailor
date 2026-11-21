// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! An open collection being edited: tree, requests, environments, dirty tracking, save, and
//! the snapshot used for sending.
//!
//! Owner: wave 3, strand F.

use rt_core::model::{Collection, Environment, RequestSpec};
use rt_core::send::{SendOptions, SendReport};
use rt_core::transport::Transport;
use rt_format::{CollectionDoc, FileSet};
use serde::{Deserialize, Serialize};

use crate::{AppError, FileChange, TreeNode};

/// What the UI passes when sending.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendInput {
    /// Environment name; `None` for no environment.
    pub environment: Option<String>,
    /// Runtime variables (highest precedence), e.g. typed into the UI.
    #[serde(default)]
    pub vars: Vec<(String, String)>,
    /// Values of secret variables of the selected environment, read by the shell from its
    /// `SecretStore`. They are injected into the environment layer of the snapshot only, so
    /// provenance stays "environment" and the values are masked in the report.
    #[serde(default)]
    pub secrets: Vec<(String, String)>,
}

/// One variable as the editor shows it: where its value comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewVar {
    pub name: String,
    /// `None` for secrets (never shown resolved) and for unresolved names.
    pub value: Option<String>,
    pub source: rt_core::vars::VarSource,
    pub secret: bool,
}

/// What a request would send, without sending it: for variable highlighting with provenance
/// in the editor. Computed by `rt_core::prepare`, so it matches what `send` does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    /// Resolved URL with secret values masked as `***`; `None` if preparation failed.
    pub url: Option<String>,
    /// Resolved headers, secrets masked.
    pub headers: Vec<(String, String)>,
    pub variables: Vec<PreviewVar>,
    pub unresolved: Vec<String>,
    /// Preparation error message (e.g. unsupported auth), if any.
    pub error: Option<String>,
}

/// An owned copy of everything needed to send one request, so sending (async, possibly long)
/// never borrows the `Workspace` across an `.await`.
#[derive(Debug, Clone)]
pub struct SendSnapshot {
    pub collection: Collection,
    pub request_path: String,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    /// Files referenced by bodies, from the collection's file set.
    pub files: FileSet,
}

impl SendSnapshot {
    /// Sends the request through `transport`. `options.body_limit` should be large (the
    /// response store keeps the full body for virtual rendering).
    pub async fn send<T: Transport>(&self, transport: &T, options: &SendOptions) -> SendReport {
        let _ = (transport, options);
        todo!("wave 3, strand F")
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    doc: CollectionDoc,
}

impl Workspace {
    pub fn open(files: FileSet) -> Result<Self, AppError> {
        let _ = files;
        todo!("wave 3, strand F")
    }

    pub fn collection(&self) -> &Collection {
        self.doc.model()
    }

    pub fn tree(&self) -> TreeNode {
        todo!("wave 3, strand F")
    }

    pub fn request(&self, path: &str) -> Result<RequestSpec, AppError> {
        let _ = path;
        todo!("wave 3, strand F")
    }

    /// Creates a request with method GET, empty URL and the given name in `folder`
    /// (`""` = root). Returns its path. Not on disk until `save`.
    pub fn create_request(&mut self, folder: &str, name: &str) -> Result<String, AppError> {
        let _ = (folder, name);
        todo!("wave 3, strand F")
    }

    /// Replaces a request in memory. Not on disk until `save`.
    pub fn update_request(&mut self, path: &str, spec: RequestSpec) -> Result<(), AppError> {
        let _ = (path, spec);
        todo!("wave 3, strand F")
    }

    /// Request paths (and `environments/<name>.yml` paths) with unsaved changes.
    pub fn dirty(&self) -> Vec<String> {
        todo!("wave 3, strand F")
    }

    /// Files that changed since open or the last save; the shell writes them. Clears `dirty`.
    pub fn save(&mut self) -> Result<Vec<FileChange>, AppError> {
        todo!("wave 3, strand F")
    }

    /// Environments as in the files: secret variables have `value: None`.
    pub fn environments(&self) -> Vec<Environment> {
        self.doc.model().environments.clone()
    }

    /// Resolves the request as `send` would, without sending.
    pub fn preview(&self, request_path: &str, input: SendInput) -> Result<Preview, AppError> {
        let _ = (request_path, input);
        todo!("wave 3, strand F")
    }

    /// Creates or replaces an environment (by `env.name`). Secret values are never written.
    pub fn update_environment(&mut self, env: Environment) -> Result<(), AppError> {
        let _ = env;
        todo!("wave 3, strand F")
    }

    pub fn delete_environment(&mut self, name: &str) -> Result<(), AppError> {
        let _ = name;
        todo!("wave 3, strand F")
    }

    pub fn snapshot(&self, request_path: &str, input: SendInput) -> Result<SendSnapshot, AppError> {
        let _ = (request_path, input);
        todo!("wave 3, strand F")
    }
}
