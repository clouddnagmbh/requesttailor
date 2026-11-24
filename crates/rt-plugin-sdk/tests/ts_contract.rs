// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Contract between the Rust plugin API (`rt_plugin::api`) and the TypeScript SDK
//! (`sdk/typescript/src/api.ts`).
//!
//! Writes one value of every message variant, serialized by serde, to
//! `sdk/typescript/src/contract.generated.ts`, annotated with the TypeScript types;
//! `npm run check -w sdk/typescript` (tsc) fails if the types no longer accept the JSON.
//! Minimal replies (what a plugin may leave out) go the other way: each must deserialize
//! here, and must type-check there.
//!
//! The test fails if the committed file is stale; regenerate with
//! `RT_UPDATE_CONTRACT=1 cargo test -p rt-plugin-sdk --test ts_contract`.

use rt_plugin_sdk::api::*;
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::{ImportFormat, Loss, LossEffect, LossKind, LossReport, NoteLevel};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn j(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

fn request() -> PluginRequest {
    PluginRequest {
        method: "POST".into(),
        url: "https://api.example.com/users?key=***".into(),
        headers: pairs(&[("Authorization", "Bearer ***")]),
        body: Some(PluginBody {
            content_type: Some("application/json".into()),
            text: Some("{}".into()),
            base64: None,
        }),
    }
}

fn response() -> PluginResponse {
    PluginResponse {
        status: 200,
        headers: pairs(&[("Content-Type", "image/png")]),
        url: "https://api.example.com/users/7".into(),
        body: PluginBody {
            content_type: Some("image/png".into()),
            text: None,
            base64: Some("iVBORw==".into()),
        },
        truncated: true,
    }
}

fn context() -> PluginContext {
    PluginContext {
        request_path: "users/create.yml".into(),
        environment: Some("Local".into()),
        variables: pairs(&[("baseUrl", "https://api.example.com")]),
    }
}

fn target() -> TabTarget {
    TabTarget {
        request_path: "users/create.yml".into(),
        response_id: Some(3),
    }
}

fn spec() -> RequestSpec {
    RequestSpec {
        name: "Create user".into(),
        seq: Some(1.0),
        method: "POST".into(),
        url: "{{baseUrl}}/users".into(),
        ..RequestSpec::default()
    }
}

fn files() -> Vec<PluginFile> {
    vec![PluginFile {
        name: "users.urls".into(),
        base64: "R0VUIGh0dHBzOi8vYS8K".into(),
    }]
}

fn manifest() -> Manifest {
    Manifest {
        id: "example".into(),
        name: "Example".into(),
        version: "1.0.0".into(),
        api: API_VERSION,
        description: "Everything".into(),
        license: Some("Apache-2.0".into()),
        homepage: Some("https://example.com".into()),
        permissions: Permissions {
            network: vec!["{request}".into(), "*.example.com".into()],
            storage: true,
            variables: true,
        },
        contributes: Contributions {
            welcome: None,
            prepare: true,
            response: true,
            tabs: vec![
                TabContribution {
                    id: "summary".into(),
                    title: "Summary".into(),
                    place: TabPlace::Response,
                    content_types: vec!["application/json".into()],
                },
                TabContribution {
                    id: "builder".into(),
                    title: "Builder".into(),
                    place: TabPlace::Request,
                    content_types: vec![],
                },
            ],
            importers: vec![ImporterContribution {
                id: "url-list".into(),
                label: "URL list".into(),
                extensions: vec!["urls".into()],
            }],
            paging: vec![rt_plugin::api::PagingContribution {
                id: "next-link".into(),
                matches: rt_plugin::api::PagingMatch {
                    hosts: vec!["*.example.com".into()],
                    content_types: vec!["application/json".into()],
                    url_pattern: Some("/items*".into()),
                },
                rule: rt_core::paging::PagingSpec {
                    rule: rt_core::paging::PagingRule::JsonUrl {
                        next: "$.next".into(),
                    },
                    merge: Some("$.items".into()),
                },
            }],
            compensations: vec![rt_plugin::api::CompensationContribution {
                id: "created-resources".into(),
                matches: rt_plugin::api::CompensationMatch {
                    hosts: vec!["*.example.com".into()],
                    content_types: vec!["application/json".into()],
                    url_pattern: Some("/items*".into()),
                    methods: vec!["POST".into()],
                },
                rule: rt_plugin::api::CompensationRule {
                    id_from: vec!["header:Location".into(), "$.id".into()],
                    resource_url: Some("{collection}/{id}".into()),
                    etag_from: vec!["header:ETag".into()],
                    token: Some(rt_plugin::api::CompensationToken {
                        fetch_header: "x-token".into(),
                        fetch_value: "fetch".into(),
                        read_header: "x-token".into(),
                        send_header: "x-token".into(),
                    }),
                    not_reversible: vec![rt_plugin::api::NotReversibleRule {
                        url_pattern: "/items/draft*".into(),
                        reason: "drafts are not reverted".into(),
                    }],
                },
            }],
        },
    }
}

fn calls() -> Vec<Call> {
    vec![
        Call::Prepare {
            request: request(),
            context: context(),
        },
        Call::Response {
            request: request(),
            response: response(),
            context: context(),
        },
        Call::RenderTab {
            context: None,
            tab: "summary".into(),
            target: target(),
            spec: spec(),
            response: Some(response()),
            state: Some(json!({"stats": true})),
        },
        Call::RenderTab {
            context: None,
            tab: "builder".into(),
            target: TabTarget {
                request_path: "a.yml".into(),
                response_id: None,
            },
            spec: spec(),
            response: None,
            state: None,
        },
        Call::Action {
            context: None,
            tab: "builder".into(),
            action: "apply".into(),
            target: target(),
            spec: spec(),
            response: None,
            state: Some(json!([1, 2])),
            input: pairs(&[("filter", "x eq 1"), ("all", "true")]),
        },
        Call::Detect {
            importer: "url-list".into(),
            files: files(),
        },
        Call::Import {
            importer: "url-list".into(),
            files: files(),
        },
    ]
}

fn view() -> View {
    View {
        blocks: vec![
            Block::Heading { text: "H".into() },
            Block::Text { text: "T".into() },
            Block::Notice {
                level: NoteLevel::Warning,
                text: "N".into(),
            },
            Block::KeyValue {
                rows: pairs(&[("Status", "200")]),
            },
            Block::Table {
                columns: vec!["A".into(), "B".into()],
                rows: vec![vec!["1".into(), "2".into()]],
            },
            Block::Code {
                language: Some("json".into()),
                text: "{}".into(),
            },
            Block::Code {
                language: None,
                text: "plain".into(),
            },
            Block::Tree {
                nodes: vec![TreeItem {
                    label: "Users".into(),
                    detail: Some("EntitySet".into()),
                    children: vec![TreeItem {
                        label: "Id".into(),
                        detail: None,
                        children: vec![],
                        action: Some("select-id".into()),
                    }],
                    action: None,
                }],
            },
            Block::Form {
                id: "query".into(),
                fields: vec![
                    FormField {
                        name: "filter".into(),
                        label: "Filter".into(),
                        kind: FieldKind::Text,
                        value: "".into(),
                        options: vec![],
                    },
                    FormField {
                        name: "format".into(),
                        label: "Format".into(),
                        kind: FieldKind::Select,
                        value: "json".into(),
                        options: vec!["json".into(), "xml".into()],
                    },
                    FormField {
                        name: "all".into(),
                        label: "All".into(),
                        kind: FieldKind::Checkbox,
                        value: "false".into(),
                        options: vec![],
                    },
                ],
                submit: "Apply".into(),
            },
            Block::Actions {
                actions: vec![ActionButton {
                    id: "body-stats".into(),
                    label: "Stats".into(),
                }],
            },
        ],
    }
}

fn host_calls() -> Vec<HostCall> {
    vec![
        HostCall::Log {
            level: NoteLevel::Info,
            message: "hi".into(),
        },
        HostCall::Http {
            session: false,
            request: PluginRequest {
                body: None,
                ..request()
            },
            credentials: true,
        },
        HostCall::StorageGet { key: "k".into() },
        HostCall::StorageSet {
            key: "k".into(),
            value: json!({"n": 1}),
        },
        HostCall::StorageDelete { key: "k".into() },
        HostCall::Now,
    ]
}

/// (name, TypeScript type, JSON).
fn fixtures() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("manifest", "Manifest", j(manifest())),
        ("calls", "Call[]", j(calls())),
        (
            "prepareReply",
            "Reply<PrepareReply>",
            j(Reply {
                ok: Some(PrepareReply {
                    patch: RequestPatch {
                        url: Some("https://b/".into()),
                        remove_headers: vec!["X-Old".into()],
                        set_headers: pairs(&[("X-Request-ID", "rt-1")]),
                        body: Some(PluginBody {
                            content_type: None,
                            text: Some("x".into()),
                            base64: None,
                        }),
                    },
                    notes: vec![PluginNote {
                        level: NoteLevel::Info,
                        text: "set".into(),
                    }],
                }),
                error: None,
            }),
        ),
        (
            "errorReply",
            "Reply<PrepareReply>",
            j(Reply::<PrepareReply> {
                ok: None,
                error: Some("refused".into()),
            }),
        ),
        (
            "notesReply",
            "NotesReply",
            j(NotesReply {
                notes: vec![PluginNote {
                    level: NoteLevel::Error,
                    text: "bad".into(),
                }],
                segments: vec![rt_core::TimingSegment {
                    id: "db".into(),
                    label: "Database".into(),
                    ms: 41,
                    parent: Some("server".into()),
                    offset_ms: Some(1.5),
                }],
            }),
        ),
        (
            "viewReply",
            "ViewReply",
            j(ViewReply {
                add_environments: Vec::new(),
                create_requests: Vec::new(),
                view: view(),
                state: Some(json!({"stats": false})),
                update_request: Some(spec()),
            }),
        ),
        (
            "detectReply",
            "DetectReply",
            j(DetectReply { matches: true }),
        ),
        (
            "importReply",
            "ImportReply",
            j(ImportReply {
                collection_name: "users".into(),
                files: pairs(&[("opencollection.yml", "opencollection: 1.0.0\n")]),
                report: LossReport {
                    format: ImportFormat::Plugin,
                    requests: 1,
                    folders: 0,
                    environments: 0,
                    losses: vec![Loss {
                        location: "users.urls line 2".into(),
                        kind: LossKind::Other,
                        effect: LossEffect::Dropped,
                        detail: "not a request line".into(),
                    }],
                },
            }),
        ),
        ("hostCalls", "HostCall[]", j(host_calls())),
        (
            "httpReply",
            "Reply<PluginResponse>",
            j(Reply {
                ok: Some(response()),
                error: None,
            }),
        ),
    ]
}

/// What a plugin may leave out: each must deserialize into the Rust type.
fn minimal() -> Vec<(&'static str, &'static str, Value)> {
    fn check<T: DeserializeOwned>(value: Value) -> Value {
        serde_json::from_value::<T>(value.clone())
            .unwrap_or_else(|e| panic!("{value} does not deserialize: {e}"));
        value
    }
    vec![
        (
            "minimalManifest",
            "Manifest",
            check::<Manifest>(json!({"id": "x", "name": "X", "version": "0.1.0", "api": 1})),
        ),
        (
            "minimalManifestParts",
            "Manifest",
            check::<Manifest>(json!({
                "id": "x", "name": "X", "version": "0.1.0", "api": 1,
                "permissions": {},
                "contributes": {
                    "tabs": [{"id": "t", "title": "T", "place": "request"}],
                    "importers": [{"id": "i", "label": "I"}]
                }
            })),
        ),
        (
            "minimalPrepareReply",
            "Reply<PrepareReply>",
            check::<Reply<PrepareReply>>(json!({"ok": {"patch": {}}})),
        ),
        (
            "minimalErrorReply",
            "Reply<PrepareReply>",
            check::<Reply<PrepareReply>>(json!({"error": "no"})),
        ),
        (
            "minimalNotesReply",
            "NotesReply",
            check::<NotesReply>(json!({})),
        ),
        (
            "minimalViewReply",
            "ViewReply",
            check::<ViewReply>(json!({"view": {"blocks": [
                {"type": "code", "text": "x"},
                {"type": "tree", "nodes": [{"label": "a"}]},
                {"type": "form", "id": "f", "submit": "Go",
                 "fields": [{"name": "n", "label": "N", "kind": "text"}]}
            ]}})),
        ),
        (
            "minimalHostCalls",
            "HostCall[]",
            check::<Vec<HostCall>>(json!([
                {"op": "http", "request": {"method": "GET", "url": "https://a/", "headers": []}}
            ])),
        ),
        (
            "minimalCalls",
            "Call[]",
            check::<Vec<Call>>(json!([
                {"hook": "prepare",
                 "request": {"method": "GET", "url": "https://a/", "headers": []},
                 "context": {"request_path": "a.yml"}},
                {"hook": "render-tab", "tab": "t",
                 "target": {"request_path": "a.yml"},
                 "spec": j(spec())}
            ])),
        ),
    ]
}

fn render() -> String {
    let all: Vec<_> = fixtures().into_iter().chain(minimal()).collect();
    let mut types: Vec<&str> = all
        .iter()
        .flat_map(|(_, t, _)| t.split(['<', '>', '[', ']']))
        .filter(|t| !t.is_empty())
        .collect();
    types.sort_unstable();
    types.dedup();
    let mut out = String::from(
        "// SPDX-License-Identifier: Apache-2.0\n\
         // Copyright 2026 The RequestTailor Authors\n\n\
         // GENERATED by `RT_UPDATE_CONTRACT=1 cargo test -p rt-plugin-sdk --test ts_contract`.\n\
         // Do not edit. Real serde JSON from `rt_plugin::api`, annotated with the types in\n\
         // ./api.ts: `npm run check -w sdk/typescript` fails if they drift apart.\n\n",
    );
    out.push_str(&format!(
        "import type {{ {} }} from \"./api.ts\";\n",
        types.join(", ")
    ));
    for (name, ty, value) in all {
        out.push_str(&format!(
            "\nexport const {name}: {ty} = {};\n",
            serde_json::to_string_pretty(&value).unwrap()
        ));
    }
    out
}

#[test]
fn typescript_contract_is_up_to_date() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../sdk/typescript/src/contract.generated.ts"
    );
    let expected = render();
    if std::env::var_os("RT_UPDATE_CONTRACT").is_some() {
        std::fs::write(path, &expected).unwrap();
        return;
    }
    let actual = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        actual == expected,
        "{path} is stale; run `RT_UPDATE_CONTRACT=1 cargo test -p rt-plugin-sdk --test ts_contract`"
    );
}

#[test]
fn every_message_round_trips() {
    for call in calls() {
        let json = serde_json::to_string(&call).unwrap();
        assert_eq!(serde_json::from_str::<Call>(&json).unwrap(), call, "{json}");
    }
    for call in host_calls() {
        let json = serde_json::to_string(&call).unwrap();
        assert_eq!(serde_json::from_str::<HostCall>(&json).unwrap(), call);
    }
    let m = manifest();
    assert_eq!(
        serde_json::from_value::<Manifest>(j(&m)).unwrap(),
        m,
        "manifest"
    );
}
