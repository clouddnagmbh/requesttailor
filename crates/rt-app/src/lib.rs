// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-app`: the Rust side of the app's command layer.
//!
//! The UI (`apps/ui`) never talks to `rt-core` or `rt-format` directly. It calls the commands
//! of its `Backend` (`apps/ui/src/commands/backend.ts`); the desktop shell (Tauri) and the web
//! shell (`rt-wasm`) implement those commands by calling this crate. One implementation, two
//! shells: what is green in the desktop app is green in the browser (product principle 5).
//!
//! Everything here is in memory and I/O-free. Shells read the collection into a
//! [`rt_format::FileSet`], and write the [`FileChange`]s that [`Workspace::save`] returns.
//! Secret values never reach a file: they live in a [`SecretStore`] and are injected only
//! into the send snapshot.
//!
//! Module owners in wave 3: `workspace` (strand F), `response` (strand C), `secrets` (strand D).
//! Wave 4: `git` (strands A1/A2), `import` (strand B), `history` and `search` (strand E).
//! Wave 5: `plugins` (strand A), `auth` (strand C), `cookies` (strand D), `secret_scan` and
//! `secret_sources` (strand E).
//! Wave 7: `scripts` (strands A and C), `runner` (strand D).
//! Changes to the public types here are interface changes (working rule 1).

pub mod agents;
pub mod ai;
pub mod auth;
pub mod cookies;
pub mod git;
// Wave 8, strand B: gRPC services and templates, GraphQL schemas.
pub mod graphql;
pub mod grpc;
pub mod history;
pub mod import;
pub mod mcp_client;
pub mod plugins;
pub mod response;
pub mod runner;
pub mod scripts;
pub mod search;
pub mod secret_scan;
pub mod secret_sources;
pub mod secrets;
mod workspace;

pub use history::{History, HistoryEntry};
pub use import::{ImportPlan, Imports};
pub use response::{LineSlice, ResponseStore, StoredResponse};
pub use search::{SearchField, SearchHit};
pub use secrets::{SecretKey, SecretStore};
pub use workspace::{Preview, PreviewVar, SendInput, SendSnapshot, Workspace};

use serde::{Deserialize, Serialize};

/// One node of the sidebar tree. Folders and requests in display order (`seq`, then name).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TreeNode {
    Folder {
        name: String,
        /// `""` for the collection root, else `Folder::path`.
        path: String,
        children: Vec<TreeNode>,
    },
    Request {
        name: String,
        /// Request path relative to the collection root, e.g. `users/get-user.yml`.
        path: String,
        method: String,
    },
}

/// A file the shell must write (or delete), relative to the collection root, `/`-separated.
/// Collection files are UTF-8 YAML, so the content is text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    /// New content; empty when `deleted`.
    pub text: String,
    /// The shell deletes the file (a file that is already gone is fine).
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum AppError {
    /// The files are not a readable collection.
    #[error("{0}")]
    Format(String),
    #[error("request not found: {0}")]
    RequestNotFound(String),
    #[error("folder not found: {0}")]
    FolderNotFound(String),
    #[error("response not found: {0}")]
    ResponseNotFound(u64),
    #[error("environment not found: {0}")]
    EnvironmentNotFound(String),
    /// A JSONPath filter failed (bad expression, body not JSON).
    #[error("{0}")]
    Filter(String),
    /// An import failed (unknown or invalid file).
    #[error("{0}")]
    Import(String),
    /// A Git operation failed or was refused (unsaved changes, not a repository, ...).
    #[error("{0}")]
    Git(String),
    /// The remote asked for credentials, or rejected them; the value is the host.
    #[error("authentication required for {0}")]
    GitAuth(String),
    /// A plugin could not be installed or failed (wave 5).
    #[error("{0}")]
    Plugin(String),
    /// An OAuth 2.0 flow failed or was cancelled, or a token could not be stored (wave 5).
    #[error("{0}")]
    Auth(String),
    /// A commit was refused because files contain credentials; `GitApi.scan` lists them.
    #[error("{0}")]
    SecretsFound(String),
    /// The command is not available in this shell (yet).
    #[error("{0}")]
    Unsupported(String),
    /// Anything the shell failed at (I/O, keychain, ...), passed through for display.
    #[error("{0}")]
    Shell(String),
}

impl From<rt_format::FormatError> for AppError {
    fn from(e: rt_format::FormatError) -> Self {
        AppError::Format(e.to_string())
    }
}
