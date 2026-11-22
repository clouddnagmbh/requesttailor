// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! gRPC requests as sessions (wave 8, strand B): the [`StreamOpen::Grpc`] for a prepared
//! request (URL via [`grpc_target`], metadata from the headers, `te: trailers`, timeout).

use super::{StreamOpen, grpc_target};
use crate::model::GrpcSpec;
use crate::transport::HttpRequest;

/// Strand B. `request` is the prepared request (URL and headers resolved, auth applied).
pub fn open_for(spec: &GrpcSpec, request: &HttpRequest) -> Result<StreamOpen, String> {
    let (url, path) = grpc_target(&request.url, &spec.method)?;
    Ok(StreamOpen::Grpc {
        url,
        path,
        metadata: request.headers.clone(),
        timeout_ms: None,
    })
}
