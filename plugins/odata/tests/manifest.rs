// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OData plugin's manifest: its permissions, tabs, importers and first-start choice.

use rt_plugin_odata::MANIFEST;
use rt_plugin_sdk::api::TabPlace;

#[test]
fn manifest_is_valid() {
    let m = rt_plugin_sdk::parse_manifest(MANIFEST).unwrap();
    assert_eq!(m.id, "odata");
    // Network only to the host of the request it runs for, never elsewhere.
    assert_eq!(m.permissions.network, vec!["{request}".to_owned()]);
    assert!(m.permissions.storage && m.permissions.variables);
    assert!(m.contributes.prepare && m.contributes.response);
    let tabs: Vec<_> = m
        .contributes
        .tabs
        .iter()
        .map(|t| (t.id.as_str(), t.place))
        .collect();
    assert_eq!(
        tabs,
        vec![
            ("entities", TabPlace::Collection),
            ("query", TabPlace::Request),
            ("batch", TabPlace::Request),
            ("draft", TabPlace::Request),
            ("abap", TabPlace::Request),
            ("landscape", TabPlace::Collection),
        ]
    );
    let importers: Vec<_> = m
        .contributes
        .importers
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(importers, vec!["edmx", "sap-hub"]);
    assert!(m.contributes.welcome.is_some());
}
