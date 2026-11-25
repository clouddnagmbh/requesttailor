// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The `service-key` importer: BTP service keys into a collection and an environment; the
//! client secrets stay out of every file and every report.

mod common;

use common::{environment, fixture, import, read, request};
use rt_plugin_sdk::api::{Call, DetectReply};
use rt_plugin_sdk::model::{Auth, OAuth2Flow};
use rt_plugin_sdk::testing;

const CANARIES: [&str; 3] = [
    "RT-CANARY-api-secret-7f3a",
    "RT-CANARY-runtime-secret-91c4",
    "RT-CANARY-uaa-secret-55d0",
];

fn everything(reply: &rt_plugin_sdk::api::ImportReply) -> String {
    let mut all = serde_json::to_string(reply).unwrap();
    for (_, text) in &reply.files {
        all.push_str(text);
    }
    all
}

#[test]
fn two_keys_make_a_collection_and_an_environment_without_secrets() {
    let reply = import(vec![
        fixture("service-key-api.json"),
        fixture("service-key-runtime.json"),
    ])
    .unwrap();
    assert_eq!(reply.collection_name, "Integration Suite rt-test");
    let all = everything(&reply);
    for secret in CANARIES {
        assert!(!all.contains(secret), "{secret} leaked into the import");
    }
    let collection = read(&reply);
    // One environment named after the tenant, the secrets declared without a value.
    let env = environment(&collection);
    assert_eq!(env.name, "rt-test");
    let var = |n: &str| {
        env.variables
            .iter()
            .find(|v| v.name == n)
            .unwrap_or_else(|| panic!("{n}"))
    };
    assert_eq!(
        var("apiUrl").value.as_deref().unwrap_or(""),
        "https://rt-test.it-cpi001.cfapps.eu10.hana.ondemand.com"
    );
    assert_eq!(
        var("runtimeUrl").value.as_deref().unwrap_or(""),
        "https://rt-test.it-cpi001-rt.cfapps.eu10.hana.ondemand.com"
    );
    assert_eq!(
        var("clientId").value.as_deref().unwrap_or(""),
        "sb-rt-api!b1001"
    );
    assert_eq!(
        var("runtimeClientId").value.as_deref().unwrap_or(""),
        "sb-rt-flow!b1002"
    );
    assert_eq!(
        var("tokenUrl").value.as_deref().unwrap_or(""),
        "https://rt-test.authentication.eu10.hana.ondemand.com/oauth/token"
    );
    for secret in ["clientSecret", "runtimeClientSecret"] {
        assert!(
            var(secret).secret && var(secret).value.is_none(),
            "{secret}"
        );
    }
    let env_file = reply
        .files
        .iter()
        .find(|(p, _)| p.starts_with("environments/"))
        .unwrap();
    assert!(env_file.1.contains("secret: true"));
    // Both keys were used: the only losses are the secrets and the two names to fill in.
    let details: Vec<_> = reply
        .report
        .losses
        .iter()
        .map(|l| l.detail.as_str())
        .collect();
    assert!(
        details
            .iter()
            .any(|d| d.contains("client secrets are not imported"))
    );
    assert!(
        !details.iter().any(|d| d.contains("derived")),
        "{details:?}"
    );
}

#[test]
fn the_folders_carry_oauth_client_credentials_from_variables() {
    let reply = import(vec![
        fixture("service-key-api.json"),
        fixture("service-key-runtime.json"),
    ])
    .unwrap();
    let collection = read(&reply);
    let (_, trigger) = request(&collection, "Trigger iFlow");
    let (_, log) = request(&collection, "Log of the last trigger");
    // The requests inherit; the folders declare the flow.
    assert_eq!(trigger.auth, Auth::Inherit);
    assert_eq!(log.auth, Auth::Inherit);
    let folder_auth = |name: &str| {
        collection
            .items
            .iter()
            .find_map(|i| match i {
                rt_plugin_sdk::model::Item::Folder(f) if f.name == name => {
                    Some(f.defaults.auth.clone())
                }
                _ => None,
            })
            .unwrap()
    };
    for (folder, token, id, secret) in [
        (
            "iFlow",
            "{{runtimeTokenUrl}}",
            "{{runtimeClientId}}",
            "{{runtimeClientSecret}}",
        ),
        (
            "Message processing log",
            "{{tokenUrl}}",
            "{{clientId}}",
            "{{clientSecret}}",
        ),
    ] {
        let Auth::OAuth2(o) = folder_auth(folder) else {
            panic!("{folder} has no OAuth 2.0");
        };
        assert_eq!(o.flow, OAuth2Flow::ClientCredentials);
        assert_eq!(o.access_token_url, token);
        assert_eq!(o.client_id, id);
        assert_eq!(o.client_secret, secret);
    }
}

#[test]
fn one_key_derives_the_other_side_and_says_so() {
    let reply = import(vec![fixture("service-key-api.json")]).unwrap();
    let collection = read(&reply);
    let env = environment(&collection);
    let var = |n: &str| {
        env.variables
            .iter()
            .find(|v| v.name == n)
            .unwrap()
            .value
            .clone()
            .unwrap_or_default()
    };
    assert_eq!(
        var("runtimeUrl"),
        "https://rt-test.it-cpi001-rt.cfapps.eu10.hana.ondemand.com"
    );
    assert_eq!(var("runtimeClientId"), "");
    assert!(
        reply
            .report
            .losses
            .iter()
            .any(|l| l.detail.contains("no runtime key") && l.detail.contains("derived"))
    );
    assert!(!everything(&reply).contains(CANARIES[0]));

    let reply = import(vec![fixture("service-key-runtime.json")]).unwrap();
    let env = environment(&read(&reply));
    let var = |n: &str| {
        env.variables
            .iter()
            .find(|v| v.name == n)
            .unwrap()
            .value
            .clone()
            .unwrap_or_default()
    };
    assert_eq!(
        var("apiUrl"),
        "https://rt-test.it-cpi001.cfapps.eu10.hana.ondemand.com"
    );
    assert_eq!(var("runtimeClientId"), "sb-rt-flow!b1002");
    assert!(
        reply
            .report
            .losses
            .iter()
            .any(|l| l.detail.contains("no API key"))
    );
}

#[test]
fn the_generic_uaa_shape_works() {
    let reply = import(vec![fixture("service-key-uaa.json")]).unwrap();
    assert!(!everything(&reply).contains(CANARIES[2]));
    let env = environment(&read(&reply));
    let var = |n: &str| {
        env.variables
            .iter()
            .find(|v| v.name == n)
            .unwrap()
            .value
            .clone()
            .unwrap_or_default()
    };
    assert_eq!(
        var("tokenUrl"),
        "https://rt-uaa.authentication.us10.hana.ondemand.com/oauth/token"
    );
    assert_eq!(
        var("apiUrl"),
        "https://rt-uaa.it-cpi002.cfapps.us10.hana.ondemand.com"
    );
    assert_eq!(
        var("runtimeUrl"),
        "https://rt-uaa.it-cpi002-rt.cfapps.us10.hana.ondemand.com"
    );
}

#[test]
fn only_service_keys_are_claimed() {
    let detect = |files| -> bool {
        let reply: DetectReply = testing::call(
            &rt_plugin_sap_integration::Integration,
            &Call::Detect {
                importer: "service-key".into(),
                files,
            },
        )
        .unwrap();
        reply.matches
    };
    assert!(detect(vec![fixture("service-key-api.json")]));
    assert!(!detect(vec![fixture("not-a-key.json")]));
    let err = import(vec![fixture("not-a-key.json")]).unwrap_err();
    assert!(err.contains("no service key"), "{err}");
}

#[test]
fn a_key_without_a_secret_is_reported() {
    let key = r#"{"oauth":{"clientid":"x","url":"https://t.it-cpi001.cfapps.eu10.hana.ondemand.com","tokenurl":"https://t.authentication.eu10.hana.ondemand.com/oauth/token"}}"#;
    let reply = import(vec![rt_plugin_sdk::text_file("k.json", key)]).unwrap();
    assert!(
        reply
            .report
            .losses
            .iter()
            .any(|l| l.detail.contains("no client secret"))
    );
}

#[test]
fn the_written_files_read_back_byte_for_byte() {
    let reply = import(vec![
        fixture("service-key-api.json"),
        fixture("service-key-runtime.json"),
    ])
    .unwrap();
    let files = common::file_set(&reply);
    let doc = rt_format::read_collection(&files).unwrap();
    let written = doc.write();
    for (path, text) in &reply.files {
        let back = written
            .files
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| b);
        assert_eq!(back.map(|b| b.as_slice()), Some(text.as_bytes()), "{path}");
    }
}
