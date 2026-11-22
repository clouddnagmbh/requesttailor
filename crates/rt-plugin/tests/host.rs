// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The host beyond permissions: manifests and the ABI, limits, hooks, tabs and importers.

mod support;

use rt_core::services::{HookContext, NoteLevel, RequestHooks};
use rt_core::transport::{HttpVersion, ResponseHead, Timing};
use rt_plugin::api::{Permissions, TabTarget};
use rt_plugin::host::{Limits, PluginError, PluginHost};
use serde_json::json;
use support::{Body, Fake, block_on, manifest, plugin, plugin_with_pages, request};

fn context() -> HookContext {
    HookContext {
        request_path: "a.yml".into(),
        environment: None,
        variables: Vec::new(),
    }
}

fn all_hooks() -> serde_json::Value {
    json!({
        "prepare": true,
        "response": true,
        "tabs": [
            {"id": "req", "title": "Req", "place": "request"},
            {"id": "res", "title": "Res", "place": "response", "content_types": ["application/json"]}
        ],
        "importers": [{"id": "edmx", "label": "EDMX", "extensions": ["edmx"]}]
    })
}

fn new_host() -> (PluginHost, Fake) {
    let fake = Fake::default();
    (PluginHost::new(Box::new(fake.clone())), fake)
}

fn target() -> TabTarget {
    TabTarget {
        request_path: "a.yml".into(),
        response_id: Some(1),
    }
}

fn head() -> ResponseHead {
    ResponseHead {
        status: 200,
        version: HttpVersion::Http11,
        headers: vec![("content-type".into(), "application/json".into())],
        url: "https://a.test/".into(),
        redirects: Vec::new(),
        timing: Timing::default(),
    }
}

#[test]
fn inspect_reads_the_manifest_without_running_anything() {
    let m = manifest("jwt", json!({"network": ["a.test"]}), all_hooks());
    let wasm = plugin(&m, Body::Trap);
    let read = PluginHost::inspect(&wasm).unwrap();
    assert_eq!(read.id, "jwt");
    assert_eq!(read.permissions.network, vec!["a.test".to_owned()]);
    assert_eq!(read.contributes.tabs.len(), 2);
}

#[test]
fn invalid_plugins_do_not_load() {
    let ok = manifest("x", json!({}), json!({}));
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (b"not wasm".to_vec(), "not a WebAssembly module"),
        (wat::parse_str("(module)").unwrap(), "no rt-plugin section"),
        (
            plugin(&manifest("Bad_Id", json!({}), json!({})), Body::Trap),
            "invalid id",
        ),
        (
            plugin(
                &json!({"id": "x", "name": "X", "version": "1", "api": 2}),
                Body::Trap,
            ),
            "API version 2",
        ),
        (
            plugin(
                &manifest("x", json!({"network": ["http://a.test"]}), json!({})),
                Body::Trap,
            ),
            "invalid network permission",
        ),
        (
            wat::parse_str(format!(
                r#"(module (import "env" "system" (func)) (@custom "rt-plugin" "{}"))"#,
                ok.to_string().replace('"', "\\\"")
            ))
            .unwrap(),
            "env.system",
        ),
        (
            wat::parse_str(format!(
                r#"(module (memory (export "memory") 1) (@custom "rt-plugin" "{}"))"#,
                ok.to_string().replace('"', "\\\"")
            ))
            .unwrap(),
            "missing export",
        ),
    ];
    for (wasm, expected) in cases {
        let err = PluginHost::inspect(&wasm).unwrap_err();
        assert!(
            err.to_string().contains(expected),
            "{err} should mention {expected}"
        );
        let (mut host, _) = new_host();
        assert!(host.load(&wasm, Permissions::default(), true).is_err());
        assert!(host.plugins().is_empty());
    }
}

#[test]
fn the_wasi_subset_links_and_stdout_goes_to_the_log() {
    let m = manifest("js", json!({}), json!({"prepare": true}));
    let wat = format!(
        r#"(module
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "clock_time_get" (func (param i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "random_get" (func $random (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_sizes_get" (func (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 100) "hello from js\n")
  (data (i32.const 200) "{{\"ok\":null}}")
  (func (export "rt_alloc") (param i32) (result i32) (i32.const 4096))
  (func (export "rt_dealloc") (param i32 i32))
  (func (export "rt_call") (param i32 i32) (result i64)
    (i32.store (i32.const 0) (i32.const 100))
    (i32.store (i32.const 4) (i32.const 14))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))
    (drop (call $random (i32.const 300) (i32.const 16)))
    (i64.or (i64.shl (i64.const 200) (i64.const 32)) (i64.const 11)))
  (@custom "rt-plugin" "{}"))"#,
        m.to_string().replace('"', "\\\"")
    );
    let (mut host, fake) = new_host();
    host.load(&wat::parse_str(wat).unwrap(), Permissions::default(), true)
        .unwrap();
    block_on(host.before_send(request("https://a.test/"), &[], &context())).unwrap();
    assert_eq!(
        *fake.logs.borrow(),
        vec![("js".to_owned(), NoteLevel::Info, "hello from js".to_owned())]
    );
}

#[test]
fn fuel_memory_and_traps_fail_the_call_and_the_host_stays_up() {
    let m = || manifest("bad", json!({}), all_hooks());
    let cases = [
        (plugin(&m(), Body::Loop), "fuel"),
        (plugin(&m(), Body::Trap), "trapped"),
        (plugin(&m(), Body::Grow(1100)), "trapped"),
        (plugin_with_pages(&m(), Body::Trap, 1100), "memory"),
        (
            plugin(&m(), Body::Fixed("not json".into())),
            "invalid reply",
        ),
    ];
    for (wasm, expected) in cases {
        let (mut host, _) = new_host();
        host.set_limits(Limits {
            fuel: 5_000_000,
            ..Limits::default()
        });
        host.load(&wasm, Permissions::default(), true).unwrap();
        let err = block_on(host.tab("bad", "req", None, target(), Default::default(), None, None))
            .unwrap_err();
        assert!(
            matches!(&err, PluginError::Failed { id, message } if id == "bad" && message.contains(expected)),
            "{err:?} should mention {expected}"
        );
        // A failing prepare stops the send with the plugin's name; a failing response hook
        // becomes an error note.
        let err =
            block_on(host.before_send(request("https://a.test/"), &[], &context())).unwrap_err();
        assert!(err.contains("plugin bad failed"), "{err}");
        let notes = block_on(host.after_response(&head(), b"{}", &context()));
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].level, NoteLevel::Error);
        assert_eq!(notes[0].source, "plugin:bad");
    }
    // 64 MiB is the default memory limit: 1024 pages fit, 1025 do not.
    let (mut host, _) = new_host();
    host.load(
        &plugin_with_pages(
            &m(),
            Body::Fixed(r#"{"ok":{"view":{"blocks":[]}}}"#.into()),
            1024,
        ),
        Permissions::default(),
        true,
    )
    .unwrap();
    block_on(host.tab("bad", "req", None, target(), Default::default(), None, None)).unwrap();
}

#[test]
fn hooks_run_in_order_and_disabled_plugins_do_not_run() {
    let (mut host, _) = new_host();
    let reply = |h: &str| {
        json!({"ok": {"patch": {"set_headers": [["X-Order", h]]}, "notes": [{"level": "info", "text": h}]}})
            .to_string()
    };
    for id in ["first", "second", "third"] {
        host.load(
            &plugin(
                &manifest(id, json!({}), json!({"prepare": true})),
                Body::Fixed(reply(id)),
            ),
            Permissions::default(),
            true,
        )
        .unwrap();
    }
    host.set_enabled("third", false).unwrap();
    let (sent, notes) =
        block_on(host.before_send(request("https://a.test/"), &[], &context())).unwrap();
    assert_eq!(
        sent.headers,
        vec![("X-Order".to_owned(), "second".to_owned())]
    );
    let texts: Vec<_> = notes.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(texts, ["first", "second"]);
    assert!(matches!(
        host.set_enabled("nope", true),
        Err(PluginError::NotFound(_))
    ));

    // An update keeps the position; unload removes.
    host.load(
        &plugin(
            &manifest("first", json!({}), json!({"prepare": true})),
            Body::Fixed(reply("first-v2")),
        ),
        Permissions::default(),
        true,
    )
    .unwrap();
    let ids: Vec<_> = host
        .plugins()
        .iter()
        .map(|p| p.manifest.id.as_str())
        .collect();
    assert_eq!(ids, ["first", "second", "third"]);
    host.unload("second");
    assert_eq!(host.plugins().len(), 2);
}

#[test]
fn prepare_errors_stop_the_send_and_response_errors_become_notes() {
    let (mut host, _) = new_host();
    let m = manifest("p", json!({}), json!({"prepare": true, "response": true}));
    host.load(
        &plugin(&m, Body::Fixed(r#"{"error": "token expired"}"#.into())),
        Permissions::default(),
        true,
    )
    .unwrap();
    let err = block_on(host.before_send(request("https://a.test/"), &[], &context())).unwrap_err();
    assert_eq!(err, "plugin p: token expired");
    let notes = block_on(host.after_response(&head(), b"{}", &context()));
    assert_eq!(notes[0].text, "token expired");
    assert_eq!(notes[0].level, NoteLevel::Error);
}

#[test]
fn response_hooks_see_the_masked_request_and_the_response() {
    let (mut host, fake) = new_host();
    let m = manifest(
        "spy",
        json!({"storage": true}),
        json!({"prepare": false, "response": true}),
    );
    host.load(&plugin(&m, Body::Spy), m_perms(&m), true)
        .unwrap();
    let mut req = request("https://a.test/?k=sekrit");
    req.headers
        .push(("Authorization".into(), "Bearer sekrit".into()));
    block_on(host.before_send(req, &["sekrit".into()], &context())).unwrap();
    let notes = block_on(host.after_response(&head(), br#"{"token":"sekrit"}"#, &context()));
    assert!(notes.is_empty(), "{notes:?}");
    let seen = fake.seen("spy");
    assert_eq!(seen["hook"], "response");
    assert_eq!(seen["request"]["url"], "https://a.test/?k=***");
    assert_eq!(seen["response"]["status"], 200);
    assert_eq!(seen["response"]["body"]["text"], r#"{"token":"***"}"#);
    assert!(!seen.to_string().contains("sekrit"));
}

fn m_perms(m: &serde_json::Value) -> Permissions {
    serde_json::from_value(m["permissions"].clone()).unwrap()
}

#[test]
fn tabs_render_views_and_only_request_tabs_update_the_request() {
    let (mut host, fake) = new_host();
    let mut spec = serde_json::to_value(rt_core::model::RequestSpec::default()).unwrap();
    spec["url"] = json!("https://a.test/?$top=5");
    let reply = json!({"ok": {
        "view": {"blocks": [{"type": "heading", "text": "Claims"}, {"type": "key-value", "rows": [["sub", "42"]]}]},
        "state": {"page": 2},
        "update_request": spec
    }})
    .to_string();
    let m = manifest("tabs", json!({}), all_hooks());
    host.load(
        &plugin(&m, Body::Fixed(reply)),
        Permissions::default(),
        true,
    )
    .unwrap();
    let view = block_on(host.tab(
        "tabs",
        "req",
        None,
        target(),
        Default::default(),
        None,
        None,
    ))
    .unwrap();
    assert_eq!(view.view.blocks.len(), 2);
    assert_eq!(view.state, Some(json!({"page": 2})));
    assert!(view.update_request.is_some());
    let view = block_on(host.tab(
        "tabs",
        "res",
        Some(("next", vec![("q".into(), "1".into())])),
        target(),
        Default::default(),
        None,
        Some(json!({"page": 2})),
    ))
    .unwrap();
    assert!(
        view.update_request.is_none(),
        "response tabs cannot change the request"
    );

    assert!(matches!(
        block_on(host.tab(
            "tabs",
            "nope",
            None,
            target(),
            Default::default(),
            None,
            None
        )),
        Err(PluginError::NotFound(_))
    ));
    host.set_enabled("tabs", false).unwrap();
    assert!(matches!(
        block_on(host.tab(
            "tabs",
            "req",
            None,
            target(),
            Default::default(),
            None,
            None
        )),
        Err(PluginError::Refused(_))
    ));

    // What a tab gets: the call with spec, response and state.
    let m = manifest("spy", json!({"storage": true}), all_hooks());
    host.load(&plugin(&m, Body::Spy), m_perms(&m), true)
        .unwrap();
    let _ = block_on(host.tab(
        "spy",
        "req",
        Some(("go", vec![("name".into(), "v".into())])),
        target(),
        Default::default(),
        None,
        Some(json!(1)),
    ));
    let seen = fake.seen("spy");
    assert_eq!(seen["hook"], "action");
    assert_eq!(seen["action"], "go");
    assert_eq!(seen["input"], json!([["name", "v"]]));
    assert_eq!(seen["state"], 1);
    assert_eq!(seen["target"]["response_id"], 1);
}

#[test]
fn importers_detect_and_import() {
    let (mut host, _) = new_host();
    let reply = json!({"ok": {
        "matches": true,
        "collection_name": "Northwind",
        "files": [["opencollection.yml", "opencollection: 1.0.0\ninfo:\n  name: Northwind\n"]],
        "report": {"format": "plugin", "requests": 0, "folders": 0, "environments": 0, "losses": []}
    }})
    .to_string();
    host.load(
        &plugin(
            &manifest("odata", json!({}), all_hooks()),
            Body::Fixed(reply),
        ),
        Permissions::default(),
        true,
    )
    .unwrap();
    assert!(block_on(host.detect("odata", "edmx", Vec::new())).unwrap());
    let import = block_on(host.import("odata", "edmx", Vec::new())).unwrap();
    assert_eq!(import.collection_name, "Northwind");
    assert_eq!(import.files.len(), 1);
    assert!(matches!(
        block_on(host.detect("odata", "csv", Vec::new())),
        Err(PluginError::NotFound(_))
    ));
    assert!(matches!(
        block_on(host.import("missing", "edmx", Vec::new())),
        Err(PluginError::NotFound(_))
    ));
}

#[test]
fn now_and_log_need_no_permission() {
    let (mut host, fake) = new_host();
    let call = json!({"op": "log", "level": "warning", "message": "careful"}).to_string();
    host.load(
        &plugin(
            &manifest("l", json!({}), json!({"prepare": true})),
            Body::HostCall(call),
        ),
        Permissions::default(),
        true,
    )
    .unwrap();
    block_on(host.before_send(request("https://a.test/"), &[], &context())).unwrap();
    assert_eq!(
        *fake.logs.borrow(),
        vec![("l".to_owned(), NoteLevel::Warning, "careful".to_owned())]
    );
    let (mut host, _) = new_host();
    host.load(
        &plugin(
            &manifest("n", json!({}), json!({"prepare": true})),
            Body::HostCall(r#"{"op":"now"}"#.into()),
        ),
        Permissions::default(),
        true,
    )
    .unwrap();
    // `{"ok": <ms>}` is not a prepare reply: the call fails, which shows the reply arrived.
    let err = block_on(host.before_send(request("https://a.test/"), &[], &context())).unwrap_err();
    assert!(err.contains("invalid reply"), "{err}");
}
