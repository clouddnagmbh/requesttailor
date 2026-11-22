// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance (wave 5, strand A): a plugin sees only what it asked for and was granted. A
//! plugin without the network permission cannot cause a request: `http` without the right, or
//! to a host outside its list, is refused and never reaches the transport.

mod support;

use rt_core::services::{HookContext, RequestHooks};
use rt_plugin::api::{Permissions, TabTarget};
use rt_plugin::host::{PluginError, PluginHost};
use serde_json::json;
use support::{Body, Fake, block_on, manifest, plugin, request};

fn context() -> HookContext {
    HookContext {
        request_path: "users/get.yml".into(),
        environment: Some("dev".into()),
        variables: vec![("base".into(), "https://api.example.com".into())],
    }
}

fn http_call(url: &str) -> String {
    json!({"op": "http", "request": {"method": "GET", "url": url, "headers": [], "body": null}})
        .to_string()
}

fn permissions(v: serde_json::Value) -> Permissions {
    serde_json::from_value(v).unwrap()
}

/// A host with one prepare plugin that makes `call` and returns the host's reply.
fn host_with(perms: serde_json::Value, body: Body) -> (PluginHost, Fake) {
    let fake = Fake::default();
    let mut host = PluginHost::new(Box::new(fake.clone()));
    let m = manifest(
        "probe",
        perms.clone(),
        json!({"prepare": true, "response": true,
               "tabs": [{"id": "t", "title": "T", "place": "request"}]}),
    );
    host.load(&plugin(&m, body), permissions(perms), true)
        .unwrap();
    (host, fake)
}

fn prepare(host: &PluginHost, url: &str) -> Result<(), String> {
    block_on(host.before_send(request(url), &[], &context())).map(|_| ())
}

#[test]
fn http_without_the_network_permission_is_refused_and_never_sent() {
    let (host, fake) = host_with(
        json!({}),
        Body::HostCall(http_call("https://api.example.com/x")),
    );
    let err = prepare(&host, "https://api.example.com/users").unwrap_err();
    assert!(err.contains("permission denied: network"), "{err}");
    assert_eq!(fake.http.borrow().len(), 0, "nothing reached the transport");

    // The same through a tab: the tab call fails, still nothing on the wire.
    let target = TabTarget {
        request_path: "users/get.yml".into(),
        response_id: None,
    };
    let err =
        block_on(host.tab("probe", "t", None, target, Default::default(), None, None)).unwrap_err();
    assert_eq!(
        err,
        PluginError::Refused("permission denied: network".into())
    );
    assert_eq!(fake.http.borrow().len(), 0);
}

#[test]
fn http_to_a_host_outside_the_list_is_refused_and_never_sent() {
    let (host, fake) = host_with(
        json!({"network": ["api.example.com", "*.cdn.example.com"]}),
        Body::HostCall(http_call("https://evil.example.org/steal")),
    );
    let err = prepare(&host, "https://api.example.com/users").unwrap_err();
    assert!(err.contains("permission denied: network"), "{err}");
    assert_eq!(fake.http.borrow().len(), 0);

    // A subdomain pattern does not cover the domain itself.
    let (host, fake) = host_with(
        json!({"network": ["*.cdn.example.com"]}),
        Body::HostCall(http_call("https://cdn.example.com/x")),
    );
    assert!(prepare(&host, "https://api.example.com/").is_err());
    assert_eq!(fake.http.borrow().len(), 0);
}

#[test]
fn http_to_a_listed_host_goes_out_once() {
    for (list, url) in [
        (json!(["api.example.com"]), "https://API.example.com:443/x"),
        (json!(["*.example.com"]), "https://eu.api.example.com/x"),
        (json!(["*"]), "http://anything.test/"),
    ] {
        let (host, fake) = host_with(json!({ "network": list }), Body::HostCall(http_call(url)));
        prepare(&host, "https://other.test/").unwrap();
        let sent = fake.http.borrow();
        assert_eq!(sent.len(), 1, "{url}");
        assert!(
            !sent[0].options.follow_redirects,
            "redirects would bypass the host check"
        );
    }
}

#[test]
fn request_host_entry_covers_only_the_request_of_the_hook() {
    let (host, fake) = host_with(
        json!({"network": ["{request}"]}),
        Body::HostCall(http_call("https://api.example.com/meta")),
    );
    prepare(&host, "https://api.example.com/users").unwrap();
    assert_eq!(fake.http.borrow().len(), 1);
    let err = prepare(&host, "https://other.example.com/users").unwrap_err();
    assert!(err.contains("permission denied: network"), "{err}");
    assert_eq!(fake.http.borrow().len(), 1);
}

#[test]
fn requested_but_not_granted_is_no_permission() {
    let fake = Fake::default();
    let mut host = PluginHost::new(Box::new(fake.clone()));
    let m = manifest("probe", json!({"network": ["*"]}), json!({"prepare": true}));
    let loaded = host
        .load(
            &plugin(&m, Body::HostCall(http_call("https://a.test/"))),
            Permissions::default(),
            true,
        )
        .unwrap();
    assert_eq!(loaded.granted, Permissions::default());
    let err = prepare(&host, "https://a.test/").unwrap_err();
    assert!(err.contains("permission denied: network"), "{err}");
    assert_eq!(fake.http.borrow().len(), 0);
}

#[test]
fn non_http_urls_are_refused_even_with_any_host() {
    for url in ["file:///etc/passwd", "ftp://a.test/", "not a url"] {
        let (host, fake) = host_with(json!({"network": ["*"]}), Body::HostCall(http_call(url)));
        assert!(prepare(&host, "https://a.test/").is_err(), "{url}");
        assert_eq!(fake.http.borrow().len(), 0, "{url}");
    }
}

#[test]
fn storage_needs_the_storage_permission() {
    let call = json!({"op": "storage-set", "key": "k", "value": 1}).to_string();
    let (host, fake) = host_with(json!({}), Body::HostCall(call.clone()));
    let err = prepare(&host, "https://a.test/").unwrap_err();
    assert!(err.contains("permission denied: storage"), "{err}");
    assert!(fake.storage.borrow().is_empty());

    let (host, fake) = host_with(json!({"storage": true}), Body::HostCall(call));
    prepare(&host, "https://a.test/").unwrap();
    assert_eq!(
        fake.storage.borrow().get(&("probe".into(), "k".into())),
        Some(&json!(1))
    );
}

#[test]
fn variables_need_the_variables_permission() {
    let (host, fake) = host_with(json!({"storage": true}), Body::Spy);
    prepare(&host, "https://a.test/").unwrap();
    let seen = fake.seen("probe");
    assert_eq!(seen["hook"], "prepare");
    assert_eq!(seen["context"]["variables"], json!([]));
    assert_eq!(seen["context"]["request_path"], "users/get.yml");

    let (host, fake) = host_with(json!({"storage": true, "variables": true}), Body::Spy);
    prepare(&host, "https://a.test/").unwrap();
    assert_eq!(
        fake.seen("probe")["context"]["variables"],
        json!([["base", "https://api.example.com"]])
    );
}

#[test]
fn plugins_never_see_secret_values() {
    let (host, fake) = host_with(json!({"storage": true, "variables": true}), Body::Spy);
    let mut req = request("https://api.example.com/users?key=s3cr%20t&x=1");
    req.headers = vec![
        ("Authorization".into(), "Bearer tok-123".into()),
        // user:s3cr t
        ("X-Basic".into(), "Basic dXNlcjpzM2NyIHQ=".into()),
    ];
    req.method = "POST".into();
    req.body = Some(rt_core::transport::HttpBody {
        content_type: Some("application/json".into()),
        bytes: r#"{"password":"s3cr t"}"#.into(),
    });
    let secrets = vec!["s3cr t".to_owned(), "tok-123".to_owned()];
    let mut ctx = context();
    ctx.variables
        .push(("echo".into(), "has s3cr t inside".into()));
    let (sent, _) = block_on(host.before_send(req.clone(), &secrets, &ctx)).unwrap();
    assert_eq!(sent, req, "the real request is unchanged");

    let seen = fake.seen("probe").to_string();
    for secret in ["s3cr", "tok-123", "dXNlcjpzM2NyIHQ="] {
        assert!(
            !seen.contains(secret),
            "{secret} reached the plugin: {seen}"
        );
    }
    let seen = fake.seen("probe");
    assert_eq!(
        seen["request"]["url"],
        "https://api.example.com/users?key=***&x=1"
    );
    assert_eq!(
        seen["request"]["headers"],
        json!([["Authorization", "Bearer ***"], ["X-Basic", "Basic ***"]])
    );
    assert_eq!(seen["request"]["body"]["text"], r#"{"password":"***"}"#);
}

#[test]
fn a_patch_that_contains_the_mask_is_refused() {
    for patch in [
        json!({"set_headers": [["Authorization", "Bearer ***"]]}),
        json!({"url": "https://api.example.com/?key=***"}),
        json!({"body": {"text": "{\"password\":\"***\"}"}}),
    ] {
        let reply = json!({"ok": {"patch": patch}}).to_string();
        let (host, _) = host_with(json!({}), Body::Fixed(reply));
        let err = prepare(&host, "https://api.example.com/").unwrap_err();
        assert!(err.contains("***"), "{err}");
    }
}

#[test]
fn a_patch_is_applied_to_the_real_request() {
    let reply = json!({"ok": {
        "patch": {
            "url": "https://api.example.com/v2/users",
            "remove_headers": ["x-old"],
            "set_headers": [["X-Signed", "yes"], ["accept", "text/plain"]],
        },
        "notes": [{"level": "info", "text": "signed"}],
    }})
    .to_string();
    let (host, _) = host_with(json!({}), Body::Fixed(reply));
    let mut req = request("https://api.example.com/users");
    req.headers = vec![
        ("Authorization".into(), "Bearer tok-123".into()),
        ("X-Old".into(), "1".into()),
        ("Accept".into(), "application/json".into()),
    ];
    let (sent, notes) = block_on(host.before_send(req, &["tok-123".into()], &context())).unwrap();
    assert_eq!(sent.url, "https://api.example.com/v2/users");
    assert_eq!(
        sent.headers,
        vec![
            ("Authorization".to_owned(), "Bearer tok-123".to_owned()),
            ("accept".to_owned(), "text/plain".to_owned()),
            ("X-Signed".to_owned(), "yes".to_owned()),
        ]
    );
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].source, "plugin:probe");
    assert_eq!(notes[0].text, "signed");
}

#[test]
fn credentials_go_only_to_the_origin_of_the_request() {
    let call = |url: &str| {
        json!({"op": "http", "credentials": true,
               "request": {"method": "GET", "url": url, "headers": [], "body": null}})
        .to_string()
    };
    let mut req = request("https://api.example.com/users");
    req.headers = vec![
        ("Authorization".into(), "Bearer tok-123".into()),
        ("Accept".into(), "*/*".into()),
    ];
    let secrets = vec!["tok-123".to_owned()];

    let (host, fake) = host_with(
        json!({"network": ["*"]}),
        Body::HostCall(call("https://api.example.com/$metadata")),
    );
    block_on(host.before_send(req.clone(), &secrets, &context())).unwrap();
    assert_eq!(
        fake.http.borrow()[0].headers,
        vec![("Authorization".to_owned(), "Bearer tok-123".to_owned())]
    );

    let (host, fake) = host_with(
        json!({"network": ["*"]}),
        Body::HostCall(call("https://other.example.com/")),
    );
    let err = block_on(host.before_send(req, &secrets, &context())).unwrap_err();
    assert!(err.contains("credentials"), "{err}");
    assert_eq!(fake.http.borrow().len(), 0);
}
