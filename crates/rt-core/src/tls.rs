// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Client certificates (mTLS, wave 5, strand D): picks the certificate for a request's host
//! (environment first, then collection; first enabled match wins) and reads its files.
//!
//! Interface commit: signature only. Strand D owns this file.

use crate::model::RequestContext;
use crate::prepare::FileSource;
use crate::transport::ClientIdentity;
use crate::vars::Scope;

/// The identity to present for `url`, or `None` if no certificate matches. Paths are read
/// through `files`; the passphrase template is resolved against `scope`.
pub fn client_identity(
    context: &RequestContext<'_>,
    environment: Option<&str>,
    url: &str,
    scope: &Scope,
    files: &dyn FileSource,
) -> Result<Option<ClientIdentity>, String> {
    let _ = (context, environment, url, scope, files);
    Ok(None)
}
