// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The SOAP plugin's manifest: no network, no storage, no variables (credentials are
//! placeholders the core fills in), two tabs and one importer.

use rt_plugin_sdk::api::TabPlace;
use rt_plugin_soap::MANIFEST;

#[test]
fn manifest_is_valid_and_asks_for_nothing() {
    let m = rt_plugin_sdk::parse_manifest(MANIFEST).unwrap();
    assert_eq!(m.id, "soap");
    assert!(m.permissions.network.is_empty());
    assert!(!m.permissions.storage && !m.permissions.variables);
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
            ("soap", TabPlace::Request),
            ("soap-response", TabPlace::Response)
        ]
    );
    let importers: Vec<_> = m
        .contributes
        .importers
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(importers, vec!["wsdl"]);
    // Not bundled: no first-start choice.
    assert!(m.contributes.welcome.is_none());
}
