// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: oauth2` ↔ `Auth::OAuth2` (OpenCollection `AuthOAuth2`, four flows), and the
//! collection's default auth for flows OpenCollection lacks (device code, SAML 2.0 bearer)
//! in `extensions.requesttailor.auth` of `opencollection.yml`.
//!
//! Interface commit: stubs that keep today's behaviour (OAuth 2.0 reads as
//! `Auth::Unsupported`, nothing is written). Strand C owns this file.

use rt_core::model::{Auth, OAuth2};

use crate::yaml::{Node, Val};

/// Reads an `auth` mapping with `type: oauth2`.
pub(crate) fn read(node: &Node) -> Auth {
    let _ = node;
    Auth::Unsupported {
        kind: "oauth2".to_owned(),
    }
}

/// The mapping after `type` for `auth`, or an error for flows that have no OpenCollection
/// form (device code, SAML bearer: only as the collection default, see [`read_extension`]).
pub(crate) fn fields(auth: &OAuth2) -> Result<Vec<(&'static str, Val)>, String> {
    let _ = auth;
    Err("OAuth 2.0 cannot be written yet".to_owned())
}

/// The collection default auth from `extensions.requesttailor.auth` of `opencollection.yml`
/// (`root` is that file's root), if set. It takes precedence over `request.auth`.
pub(crate) fn read_extension(root: &Node) -> Option<Auth> {
    let _ = root;
    None
}
