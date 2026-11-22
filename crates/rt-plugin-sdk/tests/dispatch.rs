// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The JSON side of `rt_call` and `rt.host`: every hook through `handle`, error replies, and
//! host calls with the permission checks of `MockHost`.

use rt_plugin_sdk::api::{
    Block, Call, ImportReply, NotesReply, Permissions, PluginContext, PluginFile, PluginRequest,
    PluginResponse, PrepareReply, TabTarget, View, ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::testing::{self, MockHost};
use rt_plugin_sdk::{
    Action, ImportFormat, LossReport, NoteLevel, Plugin, Result, Tab, bail, host, info, set_header,
};
use serde_json::json;

/// Implements every hook; each one uses a host call where it makes sense.
struct Everything;

impl Plugin for Everything {
    fn prepare(&self, request: PluginRequest, _: PluginContext) -> Result<PrepareReply> {
        if request.url.contains("refuse") {
            bail!("refused {}", request.url);
        }
        host::log(NoteLevel::Info, format!("preparing {}", request.url));
        let now = host::now()?;
        Ok(PrepareReply {
            patch: set_header("X-Now", now.to_string()),
            notes: vec![info("prepared")],
        })
    }

    fn response(
        &self,
        _: PluginRequest,
        response: PluginResponse,
        _: PluginContext,
    ) -> Result<NotesReply> {
        let count = host::storage_get("count")?
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            + 1;
        host::storage_set("count", json!(count))?;
        Ok(NotesReply {
            notes: vec![info(format!("status {} (#{count})", response.status))],
        })
    }

    fn render_tab(&self, tab: Tab) -> Result<ViewReply> {
        Ok(ViewReply {
            view: View {
                blocks: vec![Block::Heading {
                    text: format!("{} for {}", tab.tab, tab.spec.name),
                }],
            },
            state: Some(json!({"opened": true})),
            update_request: None,
        })
    }

    fn action(&self, action: Action) -> Result<ViewReply> {
        let mut spec = action.spec.clone();
        spec.url = action.input("url").unwrap_or_default().to_owned();
        Ok(ViewReply {
            view: View::default(),
            state: action.state,
            update_request: Some(spec),
        })
    }

    fn detect(&self, importer: &str, files: Vec<PluginFile>) -> Result<bool> {
        Ok(importer == "txt" && files.iter().all(|f| f.name.ends_with(".txt")))
    }

    fn import(&self, importer: &str, _: Vec<PluginFile>) -> Result<ImportReply> {
        let response = host::http(testing::request("GET", "https://api.example.com/spec"))?;
        Ok(ImportReply {
            collection_name: format!("{importer} {}", response.status),
            files: vec![(
                "opencollection.yml".into(),
                "opencollection: 1.0.0\n".into(),
            )],
            report: LossReport {
                format: ImportFormat::Plugin,
                requests: 0,
                folders: 0,
                environments: 0,
                losses: vec![],
            },
        })
    }
}

/// Implements nothing.
struct Nothing;
impl Plugin for Nothing {}

fn raw(plugin: &dyn Plugin, json: &str) -> serde_json::Value {
    serde_json::from_slice(&rt_plugin_sdk::handle(plugin, json.as_bytes())).unwrap()
}

fn target() -> TabTarget {
    TabTarget {
        request_path: "users/get.yml".into(),
        response_id: Some(3),
    }
}

fn spec() -> RequestSpec {
    RequestSpec {
        name: "Get user".into(),
        method: "GET".into(),
        url: "{{base}}/users".into(),
        ..RequestSpec::default()
    }
}

#[test]
fn prepare_replies_with_patch_notes_and_uses_the_host() {
    let host = MockHost::new().now(1_790_000_000_000);
    let _guard = host.install();
    let reply: PrepareReply = testing::call(
        &Everything,
        &Call::Prepare {
            request: testing::request("GET", "https://a.example/x"),
            context: testing::context("a.yml"),
        },
    )
    .unwrap();
    assert_eq!(
        reply.patch.set_headers,
        vec![("X-Now".to_owned(), "1790000000000".to_owned())]
    );
    assert_eq!(reply.notes, vec![info("prepared")]);
    assert_eq!(
        host.logs(),
        vec![(NoteLevel::Info, "preparing https://a.example/x".to_owned())]
    );
}

#[test]
fn replies_are_exactly_ok_or_error() {
    let _guard = MockHost::new().install();
    let call = json!({
        "hook": "prepare",
        "request": {"method": "GET", "url": "https://x/refuse", "headers": [], "body": null},
        "context": {"request_path": "a.yml", "environment": null}
    });
    assert_eq!(
        raw(&Everything, &call.to_string()),
        json!({"error": "refused https://x/refuse"})
    );
    let ok = raw(
        &Everything,
        &json!({"hook": "detect", "importer": "txt", "files": [{"name": "a.txt", "base64": ""}]})
            .to_string(),
    );
    assert_eq!(ok, json!({"ok": {"matches": true}}));
}

#[test]
fn unknown_fields_are_ignored() {
    let reply = raw(
        &Everything,
        &json!({"hook": "detect", "importer": "txt", "files": [], "future": 1}).to_string(),
    );
    assert_eq!(reply, json!({"ok": {"matches": true}}));
}

#[test]
fn bad_input_gives_error_replies_not_panics() {
    let reply = raw(&Everything, "not json");
    assert!(reply["error"].as_str().unwrap().starts_with("invalid call"));
    let reply = raw(&Everything, r#"{"hook": "teleport"}"#);
    assert_eq!(reply, json!({"error": "unsupported hook: teleport"}));
    let reply = raw(&Everything, r#"{"hook": "prepare"}"#);
    assert!(reply["error"].as_str().unwrap().starts_with("invalid call"));
}

#[test]
fn hooks_a_plugin_does_not_implement_answer_with_an_error() {
    for call in [
        json!({"hook": "prepare", "request": {"method": "GET", "url": "u", "headers": [], "body": null},
               "context": {"request_path": "a.yml", "environment": null}}),
        json!({"hook": "detect", "importer": "x", "files": []}),
        json!({"hook": "import", "importer": "x", "files": []}),
    ] {
        let reply = raw(&Nothing, &call.to_string());
        let error = reply["error"].as_str().unwrap();
        assert!(error.contains("does not implement"), "{error}");
    }
}

#[test]
fn response_needs_the_storage_permission() {
    let call = Call::Response {
        request: testing::request("GET", "https://a/"),
        response: testing::response(201, "text/plain", "hi"),
        context: testing::context("a.yml"),
    };
    {
        let _guard = MockHost::new().install();
        let reply: std::result::Result<NotesReply, String> = testing::call(&Everything, &call);
        assert_eq!(reply.unwrap_err(), "permission denied: storage");
    }
    let host = MockHost::new().granted(Permissions {
        storage: true,
        ..Permissions::default()
    });
    let _guard = host.install();
    for n in 1..=2 {
        let reply: NotesReply = testing::call(&Everything, &call).unwrap();
        assert_eq!(reply.notes, vec![info(format!("status 201 (#{n})"))]);
    }
    assert_eq!(host.storage()["count"], json!(2));
}

#[test]
fn tabs_and_actions_carry_state_and_request_updates() {
    let reply: ViewReply = testing::call(
        &Everything,
        &Call::RenderTab {
            tab: "info".into(),
            target: target(),
            spec: spec(),
            response: None,
            state: None,
        },
    )
    .unwrap();
    assert_eq!(
        reply.view.blocks,
        vec![Block::Heading {
            text: "info for Get user".into()
        }]
    );
    assert_eq!(reply.state, Some(json!({"opened": true})));

    let reply: ViewReply = testing::call(
        &Everything,
        &Call::Action {
            tab: "info".into(),
            action: "set-url".into(),
            target: target(),
            spec: spec(),
            response: None,
            state: Some(json!(7)),
            input: vec![("url".into(), "https://new/".into())],
        },
    )
    .unwrap();
    assert_eq!(reply.state, Some(json!(7)));
    assert_eq!(reply.update_request.unwrap().url, "https://new/");
}

#[test]
fn http_needs_a_covering_network_permission() {
    let call = Call::Import {
        importer: "txt".into(),
        files: vec![],
    };
    {
        let _guard = MockHost::new().install();
        let reply: std::result::Result<ImportReply, String> = testing::call(&Everything, &call);
        assert_eq!(reply.unwrap_err(), "permission denied: network");
    }
    let host = MockHost::new()
        .granted(Permissions {
            network: vec!["*.example.com".into()],
            ..Permissions::default()
        })
        .on_http(|request, credentials| {
            assert!(!credentials);
            Ok(testing::response(200, "text/plain", &request.url))
        });
    let _guard = host.install();
    let reply: ImportReply = testing::call(&Everything, &call).unwrap();
    assert_eq!(reply.collection_name, "txt 200");
}

#[test]
fn without_a_host_calls_fail_cleanly() {
    let reply: std::result::Result<PrepareReply, String> = testing::call(
        &Everything,
        &Call::Prepare {
            request: testing::request("GET", "https://a/"),
            context: testing::context("a.yml"),
        },
    );
    assert!(reply.unwrap_err().starts_with("no host"));
}
