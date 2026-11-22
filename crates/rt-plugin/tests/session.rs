// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 6, strand F: `http` with `session: true` runs in the cookie jar of the send (cookies
//! go along, `Set-Cookie` is stored), with the rules of `credentials: true`; tabs get their
//! `context`, variables only with the `variables` permission.

mod support;

use rt_core::cookies::{CookieJars, SharedJars};
use rt_core::services::{CookieJar, HookContext, RequestHooks};
use rt_plugin::api::{Permissions, PluginContext, TabTarget};
use rt_plugin::host::{PluginHost, TabScope};
use serde_json::json;
use support::{Body, Fake, block_on, manifest, plugin, request};

fn context(environment: Option<&str>) -> HookContext {
    HookContext {
        request_path: "orders/create.yml".into(),
        environment: environment.map(str::to_owned),
        variables: Vec::new(),
    }
}

fn session_call(url: &str) -> String {
    json!({"op": "http", "session": true,
           "request": {"method": "GET", "url": url, "headers": [["X-CSRF-Token", "Fetch"]], "body": null}})
    .to_string()
}

fn host_with(body: Body) -> (PluginHost, Fake) {
    host_with_network(body, "{request}")
}

fn host_with_network(body: Body, network: &str) -> (PluginHost, Fake) {
    let fake = Fake::default();
    let mut host = PluginHost::new(Box::new(fake.clone()));
    let m = manifest(
        "odata",
        json!({"network": [network]}),
        json!({"prepare": true, "tabs": [{"id": "c", "title": "C", "place": "collection"}]}),
    );
    let perms: Permissions = serde_json::from_value(m["permissions"].clone()).unwrap();
    host.load(&plugin(&m, body), perms, true).unwrap();
    (host, fake)
}

fn modifying_request() -> rt_core::transport::HttpRequest {
    let mut req =
        request("https://s4.example.com/sap/opu/odata/sap/API_SALES_ORDER_SRV/A_SalesOrder");
    req.method = "POST".into();
    req.headers = vec![("Authorization".into(), "Basic dXNlcjpzZWNyZXQ=".into())];
    req
}

#[test]
fn session_requests_share_the_cookie_jar_of_the_send() {
    let (host, fake) = host_with(Body::HostCall(session_call(
        "https://s4.example.com/sap/opu/odata/sap/API_SALES_ORDER_SRV/",
    )));
    fake.answer_headers.borrow_mut().extend([
        ("x-csrf-token".to_owned(), "tok-1".to_owned()),
        (
            "set-cookie".to_owned(),
            "SAP_SESSIONID=abc; Path=/; HttpOnly".to_owned(),
        ),
    ]);
    let jar = SharedJars::new(CookieJars::new());
    jar.with(|j| {
        j.store(
            "DEV",
            "https://s4.example.com/",
            &["sap-usercontext=sap-client=100; Path=/".to_owned()],
            rt_core::cookies::now_ms(),
        )
    });
    let secrets = vec!["secret".to_owned()];

    let hooks = host.in_session(Some(&jar));
    block_on(hooks.before_send(modifying_request(), &secrets, &context(Some("DEV")))).unwrap();

    let sent = fake.http.borrow()[0].clone();
    let header = |name: &str| {
        sent.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    // `session` implies `credentials`: the auth of the request goes along, unseen.
    assert_eq!(
        header("authorization").as_deref(),
        Some("Basic dXNlcjpzZWNyZXQ=")
    );
    assert_eq!(
        header("cookie").as_deref(),
        Some("sap-usercontext=sap-client=100")
    );
    assert_eq!(header("x-csrf-token").as_deref(), Some("Fetch"));
    // The session cookie is in the jar of the environment, for the request itself.
    let cookie = block_on(jar.cookie_header("DEV", "https://s4.example.com/sap/x")).unwrap();
    assert!(cookie.contains("SAP_SESSIONID=abc"), "{cookie}");
    assert!(
        block_on(jar.cookie_header("", "https://s4.example.com/sap/x")).is_none(),
        "other environments keep their own jar"
    );
}

#[test]
fn session_follows_the_rules_of_credentials() {
    // Another origin: refused, nothing on the wire, the jar untouched.
    let (host, fake) = host_with_network(
        Body::HostCall(session_call("https://other.example.com/")),
        "*",
    );
    let jar = SharedJars::new(CookieJars::new());
    let hooks = host.in_session(Some(&jar));
    let reply = block_on(hooks.before_send(modifying_request(), &[], &context(None)));
    // A host call error is the plugin's input; the fixture returns it as its reply.
    assert!(reply.unwrap_err().contains("the session"));
    assert!(fake.http.borrow().is_empty());

    // Not in a hook: a tab cannot use the session (even with any host allowed).
    let (host, fake) =
        host_with_network(Body::HostCall(session_call("https://s4.example.com/")), "*");
    let target = TabTarget {
        request_path: String::new(),
        response_id: None,
    };
    let err =
        block_on(host.tab("odata", "c", None, target, Default::default(), None, None)).unwrap_err();
    assert!(err.to_string().contains("the session"), "{err}");
    assert!(fake.http.borrow().is_empty());
}

#[test]
fn without_a_jar_session_is_credentials() {
    let (host, fake) = host_with(Body::HostCall(session_call("https://s4.example.com/")));
    fake.answer_headers
        .borrow_mut()
        .push(("set-cookie".to_owned(), "SAP_SESSIONID=abc".to_owned()));
    // The host itself (no jar), and a session without a jar.
    let secrets = vec!["secret".to_owned()];
    block_on(host.before_send(modifying_request(), &secrets, &context(None))).unwrap();
    block_on(
        host.in_session(None)
            .before_send(modifying_request(), &secrets, &context(None)),
    )
    .unwrap();
    for sent in fake.http.borrow().iter() {
        assert!(sent.headers.iter().any(|(n, _)| n == "Authorization"));
        assert!(
            !sent
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("cookie"))
        );
    }
    assert_eq!(fake.http.borrow().len(), 2);
}

#[test]
fn tabs_get_their_context_with_variables_only_if_granted() {
    let context = PluginContext {
        request_path: String::new(),
        environment: Some("DEV".into()),
        variables: vec![("baseUrl".into(), "https://s4.example.com".into())],
    };
    let target = TabTarget {
        request_path: String::new(),
        response_id: None,
    };
    for (variables, expected) in [
        (true, json!([["baseUrl", "https://s4.example.com"]])),
        (false, json!([])),
    ] {
        let fake = Fake::default();
        let mut host = PluginHost::new(Box::new(fake.clone()));
        let m = manifest(
            "spy",
            json!({"storage": true, "variables": variables}),
            json!({"tabs": [{"id": "c", "title": "C", "place": "collection"}]}),
        );
        let perms: Permissions = serde_json::from_value(m["permissions"].clone()).unwrap();
        host.load(&plugin(&m, Body::Spy), perms, true).unwrap();
        let _ = block_on(host.tab_with(
            "spy",
            "c",
            None,
            target.clone(),
            Default::default(),
            None,
            None,
            TabScope {
                context: Some(context.clone()),
                request_urls: Vec::new(),
            },
        ));
        let seen = fake.seen("spy");
        assert_eq!(seen["hook"], "render-tab");
        assert_eq!(seen["target"]["request_path"], "");
        assert_eq!(seen["context"]["environment"], "DEV");
        assert_eq!(seen["context"]["variables"], expected);
    }
}

/// Tabs send no request: `{request}` covers the hosts the shell names in the tab's scope
/// (the request's resolved URL, or the environment's URL variables for collection tabs).
#[test]
fn request_entry_in_tabs_covers_the_urls_of_the_tab_scope() {
    let call = |url: &str, credentials: bool| {
        json!({"op": "http", "credentials": credentials,
               "request": {"method": "GET", "url": url, "headers": [], "body": null}})
        .to_string()
    };
    let target = TabTarget {
        request_path: String::new(),
        response_id: None,
    };
    let scope = || TabScope {
        context: None,
        request_urls: vec!["https://s4.example.com/sap/opu/odata/sap/API_SALES_ORDER_SRV".into()],
    };
    let run = |body: String, scope: TabScope| {
        let (host, fake) = host_with(Body::HostCall(body));
        let result = block_on(host.tab_with(
            "odata",
            "c",
            None,
            target.clone(),
            Default::default(),
            None,
            None,
            scope,
        ));
        let sent = fake.http.borrow().len();
        (result.map(|_| ()).map_err(|e| e.to_string()), sent)
    };

    let metadata = "https://s4.example.com/sap/opu/odata/sap/API_SALES_ORDER_SRV/$metadata";
    // The view decodes the host's reply, not a view: the call itself went out.
    let (_, sent) = run(call(metadata, false), scope());
    assert_eq!(sent, 1);
    let (result, sent) = run(call("https://other.example.com/$metadata", false), scope());
    assert!(result.unwrap_err().contains("permission denied: network"));
    assert_eq!(sent, 0);
    let (result, sent) = run(call(metadata, false), TabScope::default());
    assert!(result.unwrap_err().contains("permission denied: network"));
    assert_eq!(sent, 0);
    // No send in progress: no credentials in tabs.
    let (result, sent) = run(call(metadata, true), scope());
    assert!(result.unwrap_err().contains("credentials"));
    assert_eq!(sent, 0);
}
