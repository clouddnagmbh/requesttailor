// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! gRPC requests as sessions (wave 8, strand B): the [`StreamOpen::Grpc`] for a prepared
//! request (URL via [`grpc_target`], metadata from the headers, timeout from the settings).
//! The transport adds `content-type: application/grpc`, `te: trailers` and `grpc-timeout`.

use super::{StreamOpen, grpc_target};
use crate::model::GrpcSpec;
use crate::transport::HttpRequest;

/// Headers that are not gRPC metadata: the transport sets them itself.
const NOT_METADATA: &[&str] = &[
    "content-type",
    "content-length",
    "te",
    "host",
    "connection",
    "transfer-encoding",
    "grpc-timeout",
    "grpc-encoding",
    "grpc-accept-encoding",
];

/// `request` is the prepared request (URL and headers resolved, auth applied).
pub fn open_for(spec: &GrpcSpec, request: &HttpRequest) -> Result<StreamOpen, String> {
    if spec.method.trim().trim_matches('/').is_empty() {
        return Err("gRPC request without a method (/package.Service/Method)".into());
    }
    let (url, path) = grpc_target(&request.url, &spec.method)?;
    Ok(StreamOpen::Grpc {
        url,
        path,
        metadata: request
            .headers
            .iter()
            .filter(|(name, _)| !NOT_METADATA.iter().any(|n| name.eq_ignore_ascii_case(n)))
            .cloned()
            .collect(),
        timeout_ms: request
            .options
            .timeout
            .map(|t| t.as_millis().min(u64::MAX as u128) as u64),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::open_for;
    use crate::model::GrpcSpec;
    use crate::stream::StreamOpen;
    use crate::transport::{HttpRequest, SendOptions};

    fn request(url: &str, timeout: Option<Duration>) -> HttpRequest {
        HttpRequest {
            method: "GRPC".into(),
            url: url.into(),
            headers: vec![
                ("Authorization".into(), "Bearer t".into()),
                ("Content-Type".into(), "application/json".into()),
                ("x-tenant".into(), "acme".into()),
            ],
            body: None,
            options: SendOptions {
                timeout,
                ..SendOptions::default()
            },
        }
    }

    #[test]
    fn maps_url_method_metadata_and_timeout() {
        let spec = GrpcSpec {
            method: "/rt.echo.v1.EchoService/Echo".into(),
            ..GrpcSpec::default()
        };
        let open = open_for(
            &spec,
            &request(
                "grpcs://api.example:443/prefix",
                Some(Duration::from_secs(2)),
            ),
        )
        .unwrap();
        assert_eq!(
            open,
            StreamOpen::Grpc {
                url: "https://api.example:443".into(),
                path: "/prefix/rt.echo.v1.EchoService/Echo".into(),
                metadata: vec![
                    ("Authorization".into(), "Bearer t".into()),
                    ("x-tenant".into(), "acme".into()),
                ],
                timeout_ms: Some(2000),
            }
        );
        let StreamOpen::Grpc { timeout_ms, .. } =
            open_for(&spec, &request("localhost:50051", None)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(timeout_ms, None);
    }

    #[test]
    fn refuses_a_request_without_method() {
        let err = open_for(&GrpcSpec::default(), &request("grpc://h:1", None)).unwrap_err();
        assert!(err.contains("without a method"), "{err}");
    }
}
