// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! AWS Signature Version 4 (wave 5, strand D): signs the final request (method, URL,
//! headers, body hash) as the last step before the wire.
//!
//! Interface commit: signature only. Strand D owns this file.

use crate::model::AwsSigV4;
use crate::transport::HttpRequest;
use crate::vars::Scope;

/// Adds `Authorization`, `X-Amz-Date` (and `X-Amz-Security-Token`, `X-Amz-Content-Sha256`
/// where needed) to `request`. Templates in `auth` are resolved against `scope`. Returns the
/// resolved secret inputs (secret key, session token) to mask in the report.
pub fn sign(
    request: &mut HttpRequest,
    auth: &AwsSigV4,
    scope: &Scope,
    now: web_time::SystemTime,
) -> Result<Vec<String>, String> {
    let _ = (request, auth, scope, now);
    Err("AWS Signature V4 is not available yet".to_owned())
}
