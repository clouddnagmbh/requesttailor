// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Contract between the Rust command layer and the TypeScript UI.
//!
//! Serializes one value of every variant the UI receives or sends and writes them, annotated
//! with their TypeScript types, to `apps/ui/src/commands/contract.generated.ts`. `npm run check`
//! (svelte-check) then fails if `types.ts` no longer matches the JSON.
//! This test fails if the committed file is stale; regenerate with
//! `RT_UPDATE_CONTRACT=1 cargo test -p rt-app --test contract`.

use std::time::Duration;

use rt_app::{AppError, FileChange, LineSlice, SecretKey, StoredResponse, TreeNode};
use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, KeyValue, MultipartPart, Param, ParamKind, PartKind,
    RawKind, RequestSpec, Settings, Variable,
};
use rt_core::send::{
    REPORT_VERSION, ReceivedResponse, ReportBody, SendError, SendReport, SentRequest,
};
use rt_core::transport::{HttpVersion, Redirect, Timing, TransportError};
use rt_core::vars::{VarSource, VarUse};
use serde::Serialize;

fn kv(name: &str, value: &str) -> KeyValue {
    KeyValue::new(name, value)
}

fn request_spec() -> RequestSpec {
    RequestSpec {
        name: "Create user".into(),
        seq: Some(2.0),
        method: "POST".into(),
        url: "{{baseUrl}}/users/:id?x=1".into(),
        headers: vec![kv("Accept", "application/json")],
        params: vec![
            Param {
                name: "x".into(),
                value: "1".into(),
                kind: ParamKind::Query,
                enabled: true,
            },
            Param {
                name: "id".into(),
                value: "7".into(),
                kind: ParamKind::Path,
                enabled: false,
            },
        ],
        body: Body::Raw {
            kind: RawKind::Json,
            data: "{}".into(),
        },
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        vars: vec![Variable::new("v", "1")],
        settings: Settings {
            timeout_ms: Some(0),
            follow_redirects: Some(true),
            max_redirects: None,
            encode_url: Some(false),
        },
    }
}

fn bodies() -> Vec<Body> {
    vec![
        Body::None,
        Body::Raw {
            kind: RawKind::Xml,
            data: "<a/>".into(),
        },
        Body::FormUrlEncoded {
            fields: vec![kv("a", "b")],
        },
        Body::Multipart {
            parts: vec![MultipartPart {
                name: "f".into(),
                kind: PartKind::File,
                values: vec!["files/a.txt".into()],
                content_type: Some("text/plain".into()),
                enabled: true,
            }],
        },
        Body::File {
            path: "files/a.bin".into(),
            content_type: None,
        },
    ]
}

fn auths() -> Vec<Auth> {
    vec![
        Auth::Inherit,
        Auth::None,
        Auth::Basic {
            username: "u".into(),
            password: "{{pw}}".into(),
        },
        Auth::Bearer { token: "t".into() },
        Auth::ApiKey {
            key: "k".into(),
            value: "v".into(),
            placement: ApiKeyPlacement::Query,
        },
        Auth::Unsupported {
            kind: "oauth2".into(),
        },
    ]
}

fn environment() -> Environment {
    Environment {
        name: "Local".into(),
        variables: vec![
            Variable::new("baseUrl", "http://127.0.0.1"),
            Variable {
                name: "token".into(),
                value: None,
                enabled: true,
                secret: true,
            },
        ],
    }
}

fn var_sources() -> Vec<VarSource> {
    vec![
        VarSource::Global,
        VarSource::Collection,
        VarSource::Environment {
            name: "Local".into(),
        },
        VarSource::Folder {
            path: "users".into(),
        },
        VarSource::Request,
        VarSource::Runtime,
    ]
}

fn transport_errors() -> Vec<TransportError> {
    vec![
        TransportError::InvalidUrl("x".into()),
        TransportError::Dns("x".into()),
        TransportError::Connect("x".into()),
        TransportError::Tls("x".into()),
        TransportError::Timeout,
        TransportError::TooManyRedirects(5),
        TransportError::Blocked("Failed to fetch".into()),
        TransportError::Other("x".into()),
    ]
}

fn send_errors() -> Vec<SendError> {
    vec![
        SendError::NotFound("a.yml".into()),
        SendError::Prepare("auth type \"oauth2\" is not supported yet".into()),
        SendError::Transport(TransportError::Timeout),
    ]
}

fn report_ok() -> SendReport {
    SendReport {
        version: REPORT_VERSION,
        request_path: "users/create.yml".into(),
        environment: Some("Local".into()),
        request: Some(SentRequest {
            method: "POST".into(),
            url: "http://127.0.0.1/users".into(),
            headers: vec![("Authorization".into(), "Bearer ***".into())],
            body_size: 2,
        }),
        response: Some(ReceivedResponse {
            status: 201,
            version: HttpVersion::Http2,
            headers: vec![("content-type".into(), "application/json".into())],
            url: "http://127.0.0.1/users".into(),
            redirects: vec![Redirect {
                status: 302,
                location: "http://127.0.0.1/u".into(),
            }],
            body: ReportBody::Utf8("{}".into()),
            size: 2,
            truncated: false,
            timing: Timing {
                dns: Some(Duration::from_micros(1500)),
                connect: Some(Duration::from_millis(2)),
                tls: None,
                ttfb: Duration::from_micros(12_500),
            },
            total: Duration::from_millis(14),
        }),
        variables: var_sources()
            .into_iter()
            .map(|source| VarUse {
                name: "v".into(),
                source,
                secret: false,
            })
            .collect(),
        unresolved: vec!["missing".into()],
        error: None,
    }
}

fn report_error() -> SendReport {
    SendReport {
        response: None,
        request: None,
        environment: None,
        error: Some(SendError::Transport(TransportError::Blocked(
            "Failed to fetch".into(),
        ))),
        ..report_ok()
    }
}

fn binary_body() -> ReportBody {
    ReportBody::Base64("AAE=".into())
}

fn tree() -> TreeNode {
    TreeNode::Folder {
        name: "Demo".into(),
        path: "".into(),
        children: vec![
            TreeNode::Folder {
                name: "users".into(),
                path: "users".into(),
                children: vec![],
            },
            TreeNode::Request {
                name: "Health".into(),
                path: "health.yml".into(),
                method: "GET".into(),
            },
        ],
    }
}

fn app_errors() -> Vec<AppError> {
    vec![
        AppError::Format("x".into()),
        AppError::RequestNotFound("a.yml".into()),
        AppError::FolderNotFound("f".into()),
        AppError::ResponseNotFound(3),
        AppError::Shell("x".into()),
    ]
}

/// `(TypeScript name, TypeScript type, value)`
fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
    fn j<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }
    vec![
        ("requestSpec", "RequestSpec", j(request_spec())),
        ("bodies", "Body[]", j(bodies())),
        ("auths", "Auth[]", j(auths())),
        ("environment", "Environment", j(environment())),
        ("varSources", "VarSource[]", j(var_sources())),
        ("transportErrors", "TransportError[]", j(transport_errors())),
        ("sendErrors", "SendError[]", j(send_errors())),
        ("reportOk", "SendReport", j(report_ok())),
        ("reportError", "SendReport", j(report_error())),
        ("binaryBody", "ReportBody", j(binary_body())),
        (
            "storedResponse",
            "StoredResponse",
            j(StoredResponse {
                report: report_ok(),
                response_id: Some(1),
                size: 2,
                line_count: 1,
                is_text: true,
            }),
        ),
        (
            "lineSlice",
            "LineSlice",
            j(LineSlice {
                start: 10,
                lines: vec!["a".into(), "b".into()],
                cut: vec![1],
            }),
        ),
        ("tree", "TreeNode", j(tree())),
        (
            "fileChange",
            "FileChange",
            j(FileChange {
                path: "a.yml".into(),
                text: "info:\n".into(),
            }),
        ),
        ("appErrors", "AppError[]", j(app_errors())),
        (
            "secretKey",
            "SecretKey",
            j(SecretKey {
                collection: "c".into(),
                environment: "Local".into(),
                variable: "token".into(),
            }),
        ),
    ]
}

fn render() -> String {
    let fixtures = fixtures();
    let mut types: Vec<&str> = fixtures
        .iter()
        .map(|(_, t, _)| t.trim_end_matches("[]"))
        .collect();
    types.sort_unstable();
    types.dedup();
    let mut out = String::from(
        "// SPDX-License-Identifier: Apache-2.0\n\
         // Copyright 2026 The RequestTailor Authors\n\n\
         // GENERATED by `RT_UPDATE_CONTRACT=1 cargo test -p rt-app --test contract`. Do not edit.\n\
         // Real serde JSON from the Rust types, annotated with the TypeScript types in ./types.ts:\n\
         // `npm run check` fails if they drift apart.\n\n",
    );
    out.push_str(&format!(
        "import type {{ {} }} from \"./types\";\n",
        types.join(", ")
    ));
    for (name, ty, value) in fixtures {
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
        "/../../apps/ui/src/commands/contract.generated.ts"
    );
    let expected = render();
    if std::env::var_os("RT_UPDATE_CONTRACT").is_some() {
        std::fs::write(path, &expected).unwrap();
        return;
    }
    let actual = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        actual == expected,
        "{path} is stale; run `RT_UPDATE_CONTRACT=1 cargo test -p rt-app --test contract`"
    );
}
