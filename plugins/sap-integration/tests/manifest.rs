// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin's manifest: it asks for no rights (it sends nothing itself), contributes the
//! response hook, two tabs and one importer, and is not bundled (no first-start choice).

use rt_plugin_sap_integration::MANIFEST;
use rt_plugin_sdk::api::TabPlace;

#[test]
fn manifest_is_valid_and_asks_for_nothing() {
    let m = rt_plugin_sdk::parse_manifest(MANIFEST).unwrap();
    assert_eq!(m.id, "sap-integration");
    assert!(m.permissions.network.is_empty());
    assert!(!m.permissions.storage && !m.permissions.variables);
    assert!(m.contributes.response && !m.contributes.prepare);
    let tabs: Vec<_> = m
        .contributes
        .tabs
        .iter()
        .map(|t| (t.id.as_str(), t.place))
        .collect();
    assert_eq!(
        tabs,
        vec![
            ("integration", TabPlace::Request),
            ("integration-log", TabPlace::Response)
        ]
    );
    let importers: Vec<_> = m
        .contributes
        .importers
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(importers, vec!["service-key"]);
    assert!(m.contributes.welcome.is_none(), "not bundled");
    assert!(m.contributes.paging.is_empty() && m.contributes.compensations.is_empty());
}
