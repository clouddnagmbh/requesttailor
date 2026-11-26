// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What `send` needs from its surroundings beyond the transport (wave 5): a token cache and
//! interactive prompts for OAuth 2.0, a cookie jar, and request hooks (plugins). Every shell
//! (desktop, web, CLI) passes its own implementations to [`crate::send::send_with`];
//! [`Services::none`] gives the wave 2 behaviour (no tokens, no cookies, no plugins).
//!
//! The traits are object safe and their futures are not `Send`, so the same code runs on
//! wasm32. Implementations must never put a secret value anywhere but the shell's secret
//! storage: tokens are secrets (product principle 3).
//!
//! Owners in wave 5: `TokenStore` and `AuthPrompt` strand C, `CookieJar` strand D,
//! `RequestHooks` strand A. Changes here are interface changes.

use serde::{Deserialize, Serialize};

use crate::transport::{HttpRequest, ResponseHead};

/// Moved to `tk-net` (seam N4); same path as before.
pub use tk_net::services::{DeviceCodePrompt, LocalBoxFuture};

/// Everything optional `send_with` can use. Missing services switch the feature off: OAuth
/// without a `TokenStore` fetches a token for every send, without an `AuthPrompt` the
/// interactive flows fail with a clear error.
#[derive(Clone, Copy, Default)]
pub struct Services<'a> {
    pub tokens: Option<&'a dyn TokenStore>,
    pub prompt: Option<&'a dyn AuthPrompt>,
    pub cookies: Option<&'a dyn CookieJar>,
    pub hooks: Option<&'a dyn RequestHooks>,
    /// Runs `before-request`, `after-response` and `tests` scripts (wave 7, `rt-script`).
    /// Without it scripts are not run (and not reported as failed).
    pub scripts: Option<&'a dyn ScriptRunner>,
    /// Wave 10: the distribution's send guard ([`crate::guard`]); `None` in the builds of
    /// this repository.
    pub guard: Option<&'a dyn crate::guard::SendGuard>,
    /// Wave 10: who sends, for the guard. Shells set it; the default is the app.
    pub caller: crate::guard::Caller,
    /// Wave 17: observers of every finished send ([`crate::observe`]); `None` in the builds
    /// of this repository.
    pub observers: Option<&'a crate::observe::ObserverSet>,
    /// Wave 17: where `matchesSnapshot` assertions read their companion files
    /// ([`crate::assert::SnapshotStore`]); `None`: such an assertion fails with a hint.
    pub snapshots: Option<&'a dyn crate::assert::SnapshotStore>,
}

impl Services<'_> {
    pub fn none() -> Self {
        Self::default()
    }
}

// -------------------------------------------------------------------------------------------
// OAuth 2.0 (strand C)

/// A cached OAuth 2.0 token. Stored by the shell in its secret storage under `key`
/// (see [`crate::auth::oauth2::cache_key`]), never in collection files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    /// Unix milliseconds; `None`: the provider gave no lifetime.
    pub expires_at: Option<i64>,
    /// Unix milliseconds of the fetch.
    pub obtained_at: i64,
}

pub trait TokenStore {
    fn get<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, Option<StoredToken>>;
    fn put<'a>(&'a self, key: &'a str, token: &'a StoredToken) -> LocalBoxFuture<'a, ()>;
    fn remove<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, ()>;
}

/// The interactive part of a flow, done by the shell: open the system browser (desktop, CLI)
/// or a popup (web), and show device codes.
pub trait AuthPrompt {
    /// Runs the browser step of the authorization code flow: show `url` to the user and
    /// return the full URL the provider redirected to (`redirect_uri` with `code`, `state`
    /// or `error`). For a loopback `redirect_uri` the shell listens on it itself.
    fn authorize<'a>(
        &'a self,
        url: &'a str,
        redirect_uri: &'a str,
    ) -> LocalBoxFuture<'a, Result<String, String>>;

    /// Shows the device code to the user. Returns at once; `send_with` polls the token
    /// endpoint and calls [`AuthPrompt::device_code_done`] when it stops.
    fn device_code<'a>(&'a self, prompt: &'a DeviceCodePrompt) -> LocalBoxFuture<'a, ()>;

    fn device_code_done<'a>(&'a self) -> LocalBoxFuture<'a, ()>;

    /// `true` once the user cancelled the flow (the shell's cancel button). Checked between
    /// the polls of the device code flow, which has no other way to learn it. Default: never.
    fn cancelled(&self) -> bool {
        false
    }
}

// -------------------------------------------------------------------------------------------
// Cookies (strand D)

/// One cookie jar per environment (`scope` is the environment name, or `""`). `send_with`
/// adds matching cookies to the request and stores `Set-Cookie` of every response,
/// including redirects.
pub trait CookieJar {
    /// The `Cookie` header value for `url`, or `None`.
    fn cookie_header<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
    ) -> LocalBoxFuture<'a, Option<String>>;
    /// Stores the `Set-Cookie` headers of a response to `url`.
    fn store<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
        set_cookie: &'a [String],
    ) -> LocalBoxFuture<'a, ()>;
}

// -------------------------------------------------------------------------------------------
// Plugins (strand A)

/// Where the request comes from, for hooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookContext {
    pub request_path: String,
    pub environment: Option<String>,
    /// Non-secret variables in scope, resolved (only handed to plugins with the
    /// `variables` permission).
    pub variables: Vec<(String, String)>,
}

/// A message a hook (or the auth step) attaches to the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// `"auth"`, or `"plugin:<id>"`.
    pub source: String,
    pub level: NoteLevel,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteLevel {
    Info,
    Warning,
    Error,
}

/// Plugin hooks around the wire. Implemented by `rt-plugin`; `rt-core` knows no plugins.
/// Runs the scripts of one phase (wave 7). Implemented by `rt-script`; see [`crate::script`].
/// Never fails: problems are [`crate::script::ScriptOutcome::error`].
pub trait ScriptRunner {
    fn run<'a>(
        &'a self,
        call: crate::script::ScriptCall<'a>,
    ) -> LocalBoxFuture<'a, crate::script::ScriptOutcome>;
}

pub trait RequestHooks {
    /// Runs after auth is applied and before signing. `secrets` are the values that must be
    /// masked before anything reaches a plugin. An `Err` stops the send with that message.
    fn before_send<'a>(
        &'a self,
        request: HttpRequest,
        secrets: &'a [String],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Result<(HttpRequest, Vec<Note>), String>>;

    /// Runs on the complete response. `body` is at most the report's body limit.
    fn after_response<'a>(
        &'a self,
        head: &'a ResponseHead,
        body: &'a [u8],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Vec<Note>>;

    /// [`RequestHooks::after_response`] with the time segments the plugins reported (wave
    /// 18, [`crate::segments`]). `send_with` calls this one. The default reports none;
    /// implementations that forward to a plugin host override it.
    fn after_response_full<'a>(
        &'a self,
        head: &'a ResponseHead,
        body: &'a [u8],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, AfterResponse> {
        Box::pin(async move {
            AfterResponse {
                notes: self.after_response(head, body, context).await,
                segments: Vec::new(),
            }
        })
    }
}

/// What the `after_response` hooks answer (wave 18).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AfterResponse {
    pub notes: Vec<Note>,
    pub segments: Vec<crate::segments::TimingSegment>,
}
