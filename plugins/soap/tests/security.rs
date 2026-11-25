// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! WS-Security against the local test service (port 48813): the UsernameToken credentials are
//! placeholders the app fills in, the plugin sees only `***`, an expired timestamp is renewed
//! where the body allows it and by the tab where it does not.

mod common;

use common::{PASSWORD, USER, Var, body, import, read, request, send, start, testdata};
use rt_plugin_sdk::api::{Call, TabTarget, ViewReply};
use rt_plugin_sdk::model::{Body, RawKind, RequestSpec};
use rt_plugin_sdk::testing;
use rt_plugin_soap::Soap;
use rt_plugin_soap::security::{expires, iso};

fn vars(password: &str) -> Vec<Var> {
    vec![
        Var("wsse_username", USER.into(), false),
        Var("wsse_password", password.into(), true),
    ]
}

fn vault() -> RequestSpec {
    request(
        &read(&import("wsdl", vec![testdata("secure.wsdl")])),
        "VaultSoap",
        "GetSecret",
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn expired(spec: &RequestSpec) -> RequestSpec {
    let mut spec = spec.clone();
    let text = body(&spec).to_owned();
    let fresh = rt_plugin_soap::security::refresh_timestamp(&text, 946_684_800_000).unwrap();
    spec.body = Body::Raw {
        kind: RawKind::Xml,
        data: fresh,
    };
    spec
}

fn no_secret_in(output: &str) {
    assert!(
        !output.contains(PASSWORD),
        "the plugin returned the password: {output}"
    );
}

#[test]
fn credentials_are_filled_in_by_the_app_and_never_seen_by_the_plugin() {
    start(48813);
    let mut spec = vault();
    // The imported timestamp is old (the importer writes none the host could renew).
    spec = expired(&spec);
    let host = testing::MockHost::new().now(now_ms());
    let _guard = host.install();
    let sent = send(&spec, &vars(PASSWORD));
    // The service saw the real values; the plugin could not renew the timestamp (secret in body).
    assert!(
        sent.request
            .body
            .as_ref()
            .unwrap()
            .text
            .as_ref()
            .unwrap()
            .contains(PASSWORD)
    );
    assert!(sent.prepare.patch.body.is_none());
    assert!(
        sent.prepare
            .notes
            .iter()
            .any(|n| n.text.contains("Refresh timestamp")),
        "{:?}",
        sent.prepare.notes
    );
    no_secret_in(&sent.plugin_output);
    assert_eq!(sent.response.status, 500);
    assert!(
        sent.notes.notes[0].text.contains("MessageExpired"),
        "{:?}",
        sent.notes
    );
}

#[test]
fn the_tab_refreshes_the_timestamp_and_sending_works() {
    start(48813);
    let spec = expired(&vault());
    let now = now_ms();
    let host = testing::MockHost::new().now(now);
    let _guard = host.install();
    let reply: ViewReply = testing::call(
        &Soap,
        &Call::Action {
            tab: "soap".into(),
            action: "refresh".into(),
            target: TabTarget {
                request_path: "v.yml".into(),
                response_id: None,
            },
            spec: spec.clone(),
            response: None,
            state: None,
            input: vec![],
            context: None,
        },
    )
    .unwrap();
    let json = serde_json::to_string(&reply).unwrap();
    let updated = reply.update_request.expect("the request is updated");
    let text = body(&updated);
    assert!(
        text.contains("{{wsse_password}}"),
        "placeholders stay: {text}"
    );
    assert_eq!(
        expires(text),
        Some(now / 1000 * 1000 + 300_000),
        "{}",
        iso(now)
    );
    no_secret_in(&json);
    // And the refreshed request is accepted.
    let sent = send(&updated, &vars(PASSWORD));
    assert_eq!(
        sent.response.status,
        200,
        "{}",
        common::body_of(&sent.response)
    );
}

#[test]
fn a_wrong_password_is_a_plain_fault() {
    start(48813);
    let sent = send(&vault_fresh(), &vars("not-the-password"));
    assert_eq!(sent.response.status, 500);
    assert_eq!(
        sent.notes.notes[0].text,
        "SOAP fault: env:Client: wsse:FailedAuthentication: bad username or password"
    );
    no_secret_in(&sent.plugin_output);
}

/// The vault request with a timestamp that is valid now.
fn vault_fresh() -> RequestSpec {
    let mut spec = vault();
    let text = rt_plugin_soap::security::refresh_timestamp(body(&spec), now_ms()).unwrap();
    spec.body = Body::Raw {
        kind: RawKind::Xml,
        data: text,
    };
    spec
}

#[test]
fn the_right_password_gets_the_answer() {
    start(48813);
    let sent = send(&vault_fresh(), &vars(PASSWORD));
    assert_eq!(
        sent.response.status,
        200,
        "{}",
        common::body_of(&sent.response)
    );
    assert!(common::body_of(&sent.response).contains("<v:value>value-of-string</v:value>"));
    no_secret_in(&sent.plugin_output);
}

#[test]
fn an_expired_timestamp_without_secrets_is_renewed_by_prepare() {
    start(48813);
    // A timestamp only (no token): the body holds no secret, so the plugin may patch it.
    let mut spec = vault_fresh();
    let text = body(&spec)
        .replace("<wsse:UsernameToken", "<!--")
        .replace("</wsse:UsernameToken>", "-->");
    let text = rt_plugin_soap::security::refresh_timestamp(&text, 946_684_800_000).unwrap();
    spec.body = Body::Raw {
        kind: RawKind::Xml,
        data: text,
    };
    spec.url = spec.url.replace("/secure", "/ts");
    // The native test host has no clock (now = 0), so prepare leaves the timestamp alone; with
    // a clock it renews it. Run the hook with a mock clock.
    let host = testing::MockHost::new().now(now_ms());
    let _guard = host.install();
    let sent = send(&spec, &[]);
    assert!(sent.prepare.patch.body.is_some(), "{:?}", sent.prepare);
    assert!(
        sent.prepare
            .notes
            .iter()
            .any(|n| n.text.contains("renewed"))
    );
    assert_eq!(
        sent.response.status,
        200,
        "{}",
        common::body_of(&sent.response)
    );
}

#[test]
fn a_password_over_plain_http_is_warned_about_except_on_loopback() {
    let mut spec = vault();
    spec.url = "http://example.com/secure".into();
    let call = Call::Prepare {
        request: rt_plugin_sdk::api::PluginRequest {
            method: "POST".into(),
            url: spec.url.clone(),
            headers: vec![],
            body: Some(rt_plugin_sdk::api::PluginBody {
                content_type: None,
                text: Some(body(&spec).replace("{{wsse_password}}", "***")),
                base64: None,
            }),
        },
        context: common::context(),
    };
    let reply: rt_plugin_sdk::api::PrepareReply = testing::call(&Soap, &call).unwrap();
    assert!(
        reply.notes.iter().any(|n| n.text.contains("clear text")),
        "{:?}",
        reply.notes
    );
}
