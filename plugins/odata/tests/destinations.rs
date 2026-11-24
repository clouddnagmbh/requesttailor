// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand K: the importer `btp-destinations` turns exported BTP destinations into
//! environments. Synthetic exports in `testdata/destinations/`; the result is read back with
//! `rt-format` as the host does. Secrets never reach a file or the loss report (canary).

use rt_format::{FileSet, read_collection};
use rt_plugin_odata::OData;
use rt_plugin_sdk::api::{Call, DetectReply, ImportReply, PluginFile};
use rt_plugin_sdk::model::Collection;
use rt_plugin_sdk::testing::{self, MockHost};
use rt_plugin_sdk::{LossEffect, LossKind, text_file};

fn fixture(path: &str) -> PluginFile {
    let full = format!("{}/testdata/{path}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&full).unwrap();
    text_file(path.rsplit('/').next().unwrap(), &text)
}

fn d(name: &str) -> PluginFile {
    fixture(&format!("destinations/{name}"))
}

fn detect(files: Vec<PluginFile>) -> bool {
    let _guard = MockHost::new().install();
    let reply: DetectReply = testing::call(
        &OData,
        &Call::Detect {
            importer: "btp-destinations".into(),
            files,
        },
    )
    .unwrap();
    reply.matches
}

fn import(files: Vec<PluginFile>) -> Result<ImportReply, String> {
    let _guard = MockHost::new().install();
    testing::call(
        &OData,
        &Call::Import {
            importer: "btp-destinations".into(),
            files,
        },
    )
}

/// The files as the host checks them; also writes back byte-identically.
fn read(reply: &ImportReply) -> Collection {
    let mut set = FileSet::default();
    for (path, text) in &reply.files {
        assert!(
            !path.starts_with('/') && !path.split('/').any(|p| p.is_empty() || p == ".."),
            "{path}"
        );
        assert!(
            set.files
                .insert(path.clone(), text.as_bytes().to_vec())
                .is_none(),
            "{path} twice"
        );
    }
    let doc = read_collection(&set).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    assert_eq!(doc.write(), set);
    doc.model().clone()
}

/// `(name, value)` of an environment's variables; a secret shows as `<secret>` and must
/// have no value.
fn vars(c: &Collection, env: &str) -> Vec<(String, String)> {
    let e = c
        .environment(env)
        .unwrap_or_else(|| panic!("no environment {env}"));
    e.variables
        .iter()
        .map(|v| {
            if v.secret {
                assert!(v.value.is_none(), "secret {} with a value", v.name);
                (v.name.clone(), "<secret>".to_owned())
            } else {
                (v.name.clone(), v.value.clone().unwrap_or_default())
            }
        })
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

/// Nothing secret in any file or in the report (the report is serialised as the host shows it).
fn assert_no_canary(reply: &ImportReply) {
    for (path, text) in &reply.files {
        assert!(!text.contains("RT-CANARY"), "canary in {path}");
    }
    let report = serde_json::to_string(&reply.report).unwrap();
    assert!(
        !report.contains("RT-CANARY"),
        "canary in the report: {report}"
    );
    assert!(
        !serde_json::to_string(&reply.files)
            .unwrap()
            .contains("CANARY")
    );
}

#[test]
fn detects_destination_exports_only() {
    for name in [
        "basic.json",
        "oauth.json",
        "none.json",
        "multi.json",
        "wrapped.json",
        "s4.properties",
        "unsupported.json",
    ] {
        assert!(detect(vec![d(name)]), "{name}");
    }
    // Several files at once.
    assert!(detect(vec![
        d("basic.json"),
        d("s4.properties"),
        d("multi.json")
    ]));

    // Other files of this plugin and plain data are not destinations.
    assert!(!detect(vec![fixture("hub/API_RT_PRODUCT_SRV.json")]));
    assert!(!detect(vec![fixture("s4/ZRT_SALES_SRV.edmx")]));
    assert!(!detect(vec![text_file(
        "a.json",
        r#"{"name": "x", "url": "y"}"#
    )]));
    assert!(!detect(vec![text_file(
        "a.json",
        r#"{"Name": "only a name"}"#
    )]));
    assert!(!detect(vec![text_file("a.json", "[]")]));
    assert!(!detect(vec![text_file("a.json", "not json")]));
    assert!(!detect(vec![text_file("a.properties", "just text\n")]));
    assert!(!detect(vec![PluginFile {
        name: "b.json".into(),
        base64: "//79".into()
    }]));
    assert!(!detect(Vec::new()));
    // One bad file among good ones: not ours.
    assert!(!detect(vec![d("basic.json"), text_file("x.json", "nope")]));
}

#[test]
fn basic_authentication_becomes_user_and_a_secret_password() {
    let reply = import(vec![d("basic.json")]).unwrap();
    assert_eq!(reply.collection_name, "BTP destinations");
    let c = read(&reply);
    assert_eq!(
        vars(&c, "RT_S4_DEV"),
        pairs(&[
            (
                "baseUrl",
                "https://s4-dev.example.invalid/sap/opu/odata/sap/ZRT_SALES_SRV"
            ),
            ("sapClient", "100"),
            ("user", "RT_USER"),
            ("password", "<secret>"),
        ])
    );
    assert_eq!(reply.report.environments, 1);
    assert_eq!(reply.report.requests, 0);
    assert_eq!(reply.report.folders, 0);
    assert!(c.items.is_empty());
    assert_no_canary(&reply);
}

#[test]
fn oauth_client_credentials_becomes_token_url_client_id_and_a_secret() {
    let reply = import(vec![d("oauth.json")]).unwrap();
    let c = read(&reply);
    assert_eq!(
        vars(&c, "RT_CAP_QAS"),
        pairs(&[
            ("baseUrl", "https://cap-qas.example.invalid/odata/v4/admin"),
            ("tokenUrl", "https://auth.example.invalid/oauth/token"),
            ("clientId", "sb-rt-demo!t1"),
            ("clientSecret", "<secret>"),
        ])
    );
    assert_no_canary(&reply);
    // The token service's own credentials are not imported, and the report says so (by
    // property name).
    let lost: Vec<_> = reply
        .report
        .losses
        .iter()
        .filter(|l| l.location == "environment RT_CAP_QAS")
        .collect();
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(lost[0].kind, LossKind::Auth);
    assert_eq!(lost[0].effect, LossEffect::Dropped);
    assert!(
        lost[0].detail.contains("tokenServiceUser"),
        "{}",
        lost[0].detail
    );
    assert!(
        lost[0].detail.contains("tokenServicePassword"),
        "{}",
        lost[0].detail
    );
}

#[test]
fn no_authentication_has_only_url_and_client() {
    let reply = import(vec![d("none.json")]).unwrap();
    let c = read(&reply);
    assert_eq!(
        vars(&c, "RT_PUBLIC"),
        pairs(&[
            ("baseUrl", "https://public.example.invalid/odata/v4/catalog"),
            ("sapClient", "200"),
        ])
    );
    assert!(reply.report.losses.is_empty(), "{:?}", reply.report.losses);
}

#[test]
fn several_destinations_in_an_array_and_in_several_files() {
    let reply = import(vec![d("multi.json")]).unwrap();
    let c = read(&reply);
    assert_eq!(reply.report.environments, 3);
    let mut names: Vec<_> = c.environments.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["RT_CAP_DEV", "RT_S4_DEV", "RT_S4_PRD"]);
    assert_eq!(vars(&c, "RT_S4_PRD")[1], ("sapClient".into(), "300".into()));
    assert_eq!(
        vars(&c, "RT_CAP_DEV")[3],
        ("clientSecret".into(), "<secret>".into())
    );
    assert_no_canary(&reply);

    // Files in different formats together.
    let reply = import(vec![d("none.json"), d("s4.properties"), d("wrapped.json")]).unwrap();
    let c = read(&reply);
    let mut names: Vec<_> = c.environments.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["RT_PUBLIC", "RT_S4_PROPS", "RT_WRAPPED"]);
    assert_no_canary(&reply);

    // The same destination name twice: the second is renamed and the report says so.
    let reply = import(vec![d("basic.json"), d("basic.json")]).unwrap();
    let c = read(&reply);
    let mut names: Vec<_> = c.environments.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["RT_S4_DEV", "RT_S4_DEV 2"]);
    assert!(
        reply.report.losses.iter().any(
            |l| l.location == "environment RT_S4_DEV 2" && l.effect == LossEffect::Approximated
        ),
        "{:?}",
        reply.report.losses
    );
}

#[test]
fn the_properties_export_is_read_with_its_escapes() {
    let reply = import(vec![d("s4.properties")]).unwrap();
    let c = read(&reply);
    assert_eq!(
        vars(&c, "RT_S4_PROPS"),
        pairs(&[
            (
                "baseUrl",
                "https://s4-props.example.invalid/sap/opu/odata/sap/ZRT_SALES_SRV?a=b"
            ),
            ("sapClient", "100"),
            ("user", "PROPS_USER"),
            ("password", "<secret>"),
        ])
    );
    assert_no_canary(&reply);

    // Continuation lines, `:` and whitespace separators, comments and unicode escapes.
    let text = "! comment\nName : P2\nURL=https\\://x.example.invalid/a\\\n  b\nAuthentication NoAuthentication\nDescription=caf\\u00e9\n";
    let reply = import(vec![text_file("p2.properties", text)]).unwrap();
    let c = read(&reply);
    assert_eq!(
        vars(&c, "P2"),
        pairs(&[("baseUrl", "https://x.example.invalid/ab")])
    );
}

#[test]
fn the_wrapped_destination_service_shape_is_read() {
    let reply = import(vec![d("wrapped.json")]).unwrap();
    let c = read(&reply);
    assert_eq!(
        vars(&c, "RT_WRAPPED"),
        pairs(&[
            ("baseUrl", "https://wrapped.example.invalid/odata"),
            ("sapClient", "010"),
            ("user", "WRAPPED_USER"),
            ("password", "<secret>"),
        ])
    );
    assert_no_canary(&reply);
}

#[test]
fn unsupported_kinds_keep_url_and_client_and_are_reported_without_values() {
    let reply = import(vec![d("unsupported.json")]).unwrap();
    let c = read(&reply);
    assert_eq!(reply.report.environments, 6);
    assert_no_canary(&reply);

    // Only baseUrl and sapClient, no auth variable at all.
    assert_eq!(
        vars(&c, "RT_SAML"),
        pairs(&[
            ("baseUrl", "https://saml.example.invalid/odata"),
            ("sapClient", "100"),
        ])
    );
    assert_eq!(
        vars(&c, "RT_EXCHANGE"),
        pairs(&[("baseUrl", "https://exchange.example.invalid/odata")])
    );
    assert_eq!(
        vars(&c, "RT_PRINCIPAL"),
        pairs(&[
            ("baseUrl", "https://principal.example.invalid/odata"),
            ("sapClient", "400"),
        ])
    );
    assert_eq!(
        vars(&c, "RT_CERT"),
        pairs(&[("baseUrl", "https://cert.example.invalid/odata")])
    );
    // On premise with Basic: the Cloud Connector is not reachable from here; still only the
    // URL and the client, and the report says why.
    assert_eq!(
        vars(&c, "RT_CLOUD_CONNECTOR"),
        pairs(&[
            (
                "baseUrl",
                "http://s4-onprem.example.invalid:8000/sap/opu/odata/sap/ZRT_SALES_SRV"
            ),
            ("sapClient", "100"),
        ])
    );
    assert!(vars(&c, "RT_MAIL").is_empty());

    let at = |env: &str| -> Vec<_> {
        reply
            .report
            .losses
            .iter()
            .filter(|l| l.location == format!("environment {env}"))
            .collect()
    };
    for env in [
        "RT_SAML",
        "RT_EXCHANGE",
        "RT_PRINCIPAL",
        "RT_CERT",
        "RT_CLOUD_CONNECTOR",
        "RT_MAIL",
    ] {
        let lost = at(env);
        assert!(!lost.is_empty(), "{env}: nothing reported");
        for l in &lost {
            assert_eq!(l.effect, LossEffect::Dropped, "{env}");
            // Property and type names, never a value.
            for value in ["example.invalid", "ONPREM_USER", "RT-CANARY", "sb-"] {
                assert!(
                    !l.detail.contains(value),
                    "{env}: value {value} in {:?}",
                    l.detail
                );
            }
        }
    }
    assert!(at("RT_SAML")[0].kind == LossKind::Auth);
    assert!(
        at("RT_SAML")[0]
            .detail
            .contains("OAuth2SAMLBearerAssertion")
    );
    assert!(
        at("RT_EXCHANGE")[0]
            .detail
            .contains("OAuth2UserTokenExchange")
    );
    assert!(
        at("RT_PRINCIPAL")
            .iter()
            .any(|l| l.detail.contains("PrincipalPropagation"))
    );
    assert!(
        at("RT_PRINCIPAL")
            .iter()
            .any(|l| l.kind == LossKind::Setting && l.detail.contains("OnPremise"))
    );
    assert!(
        at("RT_CERT")[0]
            .detail
            .contains("ClientCertificateAuthentication")
    );
    assert!(
        at("RT_CLOUD_CONNECTOR")
            .iter()
            .any(|l| l.kind == LossKind::Setting && l.detail.contains("Cloud Connector"))
    );
    assert!(at("RT_MAIL")[0].detail.contains("MAIL"));
    // The report is ordered like the other importers': `location`, then `kind`.
    let keys: Vec<_> = reply
        .report
        .losses
        .iter()
        .map(|l| (l.location != "collection", l.location.clone(), l.kind))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn an_unknown_authentication_type_is_reported_without_its_text() {
    let json = r#"{"Name": "X", "Type": "HTTP", "URL": "https://x.example.invalid",
        "Authentication": "RT-CANARY-not-a-known-type"}"#;
    let reply = import(vec![text_file("x.json", json)]).unwrap();
    assert_no_canary(&reply);
    assert_eq!(reply.report.losses.len(), 1);
    assert_eq!(reply.report.losses[0].kind, LossKind::Auth);
}

#[test]
fn a_password_without_a_known_slot_is_never_written() {
    // A password next to NoAuthentication: not imported, not named with its value.
    let json = r#"{"Name": "X", "Type": "HTTP", "URL": "https://x.example.invalid",
        "Authentication": "NoAuthentication", "Password": "RT-CANARY-stray", "ExtraSecret": "RT-CANARY-stray2"}"#;
    let reply = import(vec![text_file("x.json", json)]).unwrap();
    assert_no_canary(&reply);
    let c = read(&reply);
    assert_eq!(vars(&c, "X").len(), 1);
    // Properties that were not carried over are named once, without values.
    assert!(
        reply
            .report
            .losses
            .iter()
            .any(|l| l.detail.contains("ExtraSecret") && l.detail.contains("Password"))
    );
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert!(import(Vec::new()).is_err());
    assert!(import(vec![text_file("a.json", "not json")]).is_err());
    assert!(import(vec![text_file("a.json", "{}")]).is_err());
    assert!(import(vec![text_file("a.json", "[1, 2]")]).is_err());
    assert!(import(vec![d("basic.json"), text_file("a.json", "{}")]).is_err());
    assert!(
        import(vec![PluginFile {
            name: "b.json".into(),
            base64: "//79".into()
        }])
        .is_err()
    );
    // An absurd name is made into a valid file name; nothing escapes the environments folder.
    let json = r#"{"Name": "../../etc/passwd", "Type": "HTTP", "URL": "https://x.example.invalid", "Authentication": "NoAuthentication"}"#;
    let reply = import(vec![text_file("x.json", json)]).unwrap();
    read(&reply);
}
