// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 for `send_with` (wave 5, strand C): client credentials, password, authorization
//! code with PKCE, device code (RFC 8628), SAML 2.0 bearer (RFC 7522) and refresh. Tokens
//! are cached through `Services::tokens` and refreshed before they expire; the token
//! endpoint is called through the same `Transport` as the request itself, so the flow works
//! identically natively, in the browser and over the companion.
//!
//! Interface commit: signatures only. Strand C owns this file.

use crate::model::OAuth2;
use crate::services::{Note, Services};
use crate::transport::{HttpRequest, Transport};
use crate::vars::Scope;

/// The request with the token applied.
#[derive(Debug)]
pub struct Authorized {
    pub request: HttpRequest,
    /// Token values (and resolved client secrets) to mask in the report.
    pub secrets: Vec<String>,
    /// E.g. "token refreshed", "new token fetched".
    pub notes: Vec<Note>,
}

/// Gets a token for `auth` (from the cache, by refreshing, or by running the flow) and puts
/// it on `request` as `auth.token` says. Templates in `auth` are resolved against `scope`.
/// `Err` is a message for the report; it is masked before it gets there.
pub async fn authorize<T: Transport>(
    auth: &OAuth2,
    scope: &Scope,
    request: HttpRequest,
    transport: &T,
    services: &Services<'_>,
) -> Result<Authorized, String> {
    let _ = (auth, scope, request, transport, services);
    Err("OAuth 2.0 is not available yet".to_owned())
}

/// The key under which the token for `auth` is cached (resolved against `scope`): stable
/// for the same provider, client, scope and `token.id`, different otherwise. Contains no
/// secret value.
pub fn cache_key(auth: &OAuth2, scope: &Scope) -> String {
    let _ = (auth, scope);
    String::new()
}
