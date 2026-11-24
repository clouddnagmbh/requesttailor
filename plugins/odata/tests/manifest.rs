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
    assert_eq!(importers, vec!["edmx", "sap-hub", "btp-destinations"]);
    assert!(m.contributes.welcome.is_some());
}

/// Wave 18: everything the modules of the wave contribute is in `plugin.json`.
#[test]
fn plugin_json_lists_what_the_wave_18_modules_contribute() {
    let m = rt_plugin_sdk::parse_manifest(MANIFEST).unwrap();
    let extra = rt_plugin_odata::manifest::wave18();
    for tab in &extra.tabs {
        assert!(
            m.contributes.tabs.iter().any(|t| t.id == tab.id),
            "tab {}",
            tab.id
        );
    }
    for importer in &extra.importers {
        assert!(
            m.contributes.importers.iter().any(|i| i.id == importer.id),
            "importer {}",
            importer.id
        );
    }
    for rule in &extra.paging {
        assert!(
            m.contributes.paging.iter().any(|p| p == rule),
            "paging rule {}",
            rule.id
        );
    }
}
