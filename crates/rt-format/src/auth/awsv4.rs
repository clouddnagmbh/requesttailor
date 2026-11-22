// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: awsv4` ↔ `Auth::AwsSigV4` (OpenCollection `AuthAwsV4`).

use rt_core::model::AwsSigV4;

use crate::read::text;
use crate::yaml::{Map, Node, Val};

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

/// Key order of the mapping (Bruno's, which is the schema's).
const ORDER: &[&str] = &[
    "type",
    "accessKeyId",
    "secretAccessKey",
    "sessionToken",
    "service",
    "region",
    "profileName",
];

/// Edits an existing `awsv4` mapping from `old` to `new`: changed fields are set in place,
/// emptied ones removed, everything else (comments, unknown keys) stays byte for byte.
pub(crate) fn edit(map: &mut Map, old: &AwsSigV4, new: &AwsSigV4, nl: &str) {
    let pairs = [
        ("accessKeyId", &old.access_key_id, &new.access_key_id),
        (
            "secretAccessKey",
            &old.secret_access_key,
            &new.secret_access_key,
        ),
        ("sessionToken", &old.session_token, &new.session_token),
        ("service", &old.service, &new.service),
        ("region", &old.region, &new.region),
        ("profileName", &old.profile_name, &new.profile_name),
    ];
    for (key, old, new) in pairs {
        if old == new {
            continue;
        }
        if new.is_empty() {
            map.remove(key);
        } else {
            map.put(key, &Val::str(new.as_str()), ORDER, false, nl);
        }
    }
}
