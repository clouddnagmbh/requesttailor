// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Auth inheritance and application.
//!
//! Inheritance follows Bruno's `mergeAuth` (`packages/bruno-electron/src/utils/collection.js`):
//! a request with `Inherit` takes the nearest folder whose auth is not `Inherit`, else the
//! collection's; `Inherit` on the collection means no auth; `None` anywhere stops the search.
//!
//! Applying follows Bruno's `setAuthHeaders` (`.../ipc/network/prepare-request.js`): the auth
//! header is set after the merged headers, so it replaces a header of the same name, also an
//! explicit `Authorization` header. An API key with an empty name sends nothing.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::{Interp, PrepareError, set_header};
use crate::model::{ApiKeyPlacement, Auth, RequestContext};

/// The auth that applies to the request after inheritance, still with templates.
pub(super) fn effective(context: &RequestContext<'_>) -> Result<Auth, PrepareError> {
    let declared = std::iter::once(&context.request.spec.auth)
        .chain(context.folders.iter().rev().map(|f| &f.defaults.auth))
        .chain(std::iter::once(&context.collection.defaults.auth))
        .find(|a| **a != Auth::Inherit);
    match declared {
        None => Ok(Auth::None),
        Some(Auth::Unsupported { kind }) => Err(PrepareError::UnsupportedAuth(kind.clone())),
        Some(auth) => Ok(auth.clone()),
    }
}

/// Adds the auth header to `headers`, or returns the API key query pair for the URL.
pub(super) fn apply(
    auth: &Auth,
    headers: &mut Vec<(String, String)>,
    interp: &mut Interp<'_>,
) -> Option<(String, String)> {
    match auth {
        Auth::Inherit | Auth::None | Auth::Unsupported { .. } => None,
        Auth::Basic { username, password } => {
            let credentials = format!("{}:{}", interp.text(username), interp.text(password));
            let value = format!("Basic {}", STANDARD.encode(credentials));
            set_header(headers, "Authorization", value);
            None
        }
        Auth::Bearer { token } => {
            let value = format!("Bearer {}", interp.text(token));
            set_header(headers, "Authorization", value);
            None
        }
        Auth::ApiKey {
            key,
            value,
            placement,
        } => {
            let key = interp.text(key);
            let value = interp.text(value);
            if key.is_empty() {
                return None;
            }
            match placement {
                ApiKeyPlacement::Header => {
                    set_header(headers, &key, value);
                    None
                }
                ApiKeyPlacement::Query => Some((key, value)),
            }
        }
    }
}
