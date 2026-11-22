// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: awsv4` ↔ `Auth::AwsSigV4` (OpenCollection `AuthAwsV4`).

use rt_core::model::AwsSigV4;

use crate::read::text;
use crate::yaml::{Node, Val};

pub(crate) fn read(node: &Node) -> AwsSigV4 {
    let field = |k: &str| text(node.get(k));
    AwsSigV4 {
        access_key_id: field("accessKeyId"),
        secret_access_key: field("secretAccessKey"),
        session_token: field("sessionToken"),
        service: field("service"),
        region: field("region"),
        profile_name: field("profileName"),
    }
}

/// The mapping after `type`, in the schema's order. Empty fields are left out.
pub(crate) fn fields(a: &AwsSigV4) -> Vec<(&'static str, Val)> {
    [
        ("accessKeyId", &a.access_key_id),
        ("secretAccessKey", &a.secret_access_key),
        ("sessionToken", &a.session_token),
        ("service", &a.service),
        ("region", &a.region),
        ("profileName", &a.profile_name),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .map(|(k, v)| (k, Val::str(v.as_str())))
    .collect()
}
