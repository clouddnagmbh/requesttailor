// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-plugin`: the plugin API ([`api`], public and versioned, see `API.md`) and the host
//! that runs plugins ([`host`], feature `host`).
//!
//! Plugins are WebAssembly only, on every target (iOS allows no native code to be loaded).
//! The host interprets them (no JIT, so the same engine runs natively, on iOS and inside the
//! browser's WebAssembly), meters their fuel and memory, and enforces the permissions of
//! their manifest. Plugins see no secret value, no file and no network except through the
//! host calls of `API.md`.
//!
//! Owners in wave 5: `api` interface commit (changes need a `CHANGELOG.md` entry), `host`
//! strand A, the SDKs (`crates/rt-plugin-sdk`, `sdk/ts`) strand B.

pub mod api;
#[cfg(feature = "host")]
pub mod host;

pub use api::{API_VERSION, MANIFEST_SECTION, Manifest};

#[cfg(test)]
mod tests {
    use super::api::*;

    #[test]
    fn manifest_defaults_are_empty_rights() {
        let m: Manifest =
            serde_json::from_str(r#"{"id": "x", "name": "X", "version": "0.1.0", "api": 1}"#)
                .unwrap();
        assert_eq!(m.permissions, Permissions::default());
        assert!(m.permissions.network.is_empty());
        assert_eq!(m.contributes, Contributions::default());
    }

    #[test]
    fn calls_and_replies_are_tagged_snake_case_json() {
        let call = Call::Detect {
            importer: "edmx".into(),
            files: vec![],
        };
        assert_eq!(
            serde_json::to_value(&call).unwrap(),
            serde_json::json!({"hook": "detect", "importer": "edmx", "files": []})
        );
        let reply: Reply<DetectReply> =
            serde_json::from_str(r#"{"ok": {"matches": true}}"#).unwrap();
        assert_eq!(reply.ok, Some(DetectReply { matches: true }));
        let host: HostCall = serde_json::from_str(r#"{"op": "storage-get", "key": "k"}"#).unwrap();
        assert_eq!(host, HostCall::StorageGet { key: "k".into() });
    }
}
