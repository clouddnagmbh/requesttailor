// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Files → `rt_core::model`: representative corpus collections plus small in-memory cases.

use std::path::Path;

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Collection, Folder, Item, KeyValue, MultipartPart, Param,
    ParamKind, PartKind, RawKind, RequestEntry, Settings, Variable,
};
use rt_format::{CollectionDoc, FileSet, FormatError, read_collection};

fn corpus(name: &str) -> CollectionDoc {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/collections/bruno")
        .join(name);
    let files = FileSet::from_dir(&dir).unwrap();
    read_collection(&files).unwrap()
}

fn mem(files: &[(&str, &str)]) -> FileSet {
    FileSet {
        files: files
            .iter()
            .map(|(p, c)| (p.to_string(), c.as_bytes().to_vec()))
            .collect(),
    }
}

fn read_mem(files: &[(&str, &str)]) -> Collection {
    read_collection(&mem(files)).unwrap().model().clone()
}

fn request<'a>(c: &'a Collection, path: &str) -> &'a RequestEntry {
    c.request(path)
        .unwrap_or_else(|| panic!("no request {path}"))
        .request
}

fn folder<'a>(items: &'a [Item], name: &str) -> &'a Folder {
    items
        .iter()
        .find_map(|i| match i {
            Item::Folder(f) if f.name == name => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no folder {name}"))
}

fn names(items: &[Item]) -> Vec<&str> {
    items
        .iter()
        .map(|i| match i {
            Item::Folder(f) => f.name.as_str(),
            Item::Request(r) => r.spec.name.as_str(),
        })
        .collect()
}

const ROOT: (&str, &str) = (
    "opencollection.yml",
    "opencollection: 1.0.0\ninfo:\n  name: mem\n",
);

#[test]
fn auth_inheritance_through_folders() {
    let doc = corpus("auth-auth-mode-parent-bearer-inherit");
    let c = doc.model();
    assert_eq!(c.name, "parent-bearer-inherit");
    // No `auth` key in opencollection.yml: no auth.
    assert_eq!(c.defaults.auth, Auth::None);
    let parent = folder(&c.items, "parent-folder");
    assert_eq!(parent.path, "parent-folder");
    assert_eq!(parent.seq, Some(1.0));
    assert_eq!(
        parent.defaults.auth,
        Auth::Bearer {
            token: "your_secret_token".into()
        }
    );
    let nested = folder(&parent.items, "nested-folder");
    assert_eq!(nested.path, "parent-folder/nested-folder");
    assert_eq!(nested.defaults.auth, Auth::Inherit);
}

#[test]
fn folder_basic_auth_and_inherit_on_requests() {
    let c = corpus("auth-auth-mode-folder-basic-inherit")
        .model()
        .clone();
    let f = folder(&c.items, "basic-folder");
    assert_eq!(
        f.defaults.auth,
        Auth::Basic {
            username: "bruno".into(),
            password: "della".into()
        }
    );

    let c = corpus("request-settings-requests-settings-yml")
        .model()
        .clone();
    assert_eq!(request(&c, "timeout.yml").spec.auth, Auth::Inherit);
}

#[test]
fn unsupported_auth_types_keep_their_kind() {
    let c = corpus("auth-inherited-auth").model().clone();
    assert_eq!(
        folder(&c.items, "oauth1-folder").defaults.auth,
        Auth::Unsupported {
            kind: "oauth1".into()
        }
    );
    assert_eq!(
        request(&c, "inherited-request.yml").spec.auth,
        Auth::Inherit
    );

    let c = corpus("auth-ntlm-ntlm").model().clone();
    assert_eq!(
        request(&c, "api.yml").spec.auth,
        Auth::Unsupported {
            kind: "ntlm".into()
        }
    );
    // `auth: {type: none}` as written by hand in the corpus.
    assert_eq!(request(&c, "no-auth.yml").spec.auth, Auth::None);
}

#[test]
fn no_auth_is_written_as_absence() {
    let c = read_mem(&[
        ROOT,
        (
            "a.yml",
            "info:\n  name: a\n  type: http\nhttp:\n  method: GET\n  url: http://x\n",
        ),
        (
            "b.yml",
            "info:\n  name: b\n  type: http\nhttp:\n  method: GET\n  url: http://x\n  auth: none\n",
        ),
        (
            "c.yml",
            "info:\n  name: c\n  type: http\nhttp:\n  method: GET\n  url: http://x\n  auth:\n    type: apikey\n    key: X-Key\n    value: \"{{k}}\"\n    placement: query\n",
        ),
        ("f/folder.yml", "info:\n  name: f\n  type: folder\n"),
        (
            "f/d.yml",
            "info:\n  name: d\n  type: http\nhttp:\n  method: GET\n  url: http://x\n  auth: inherit\n",
        ),
    ]);
    assert_eq!(c.defaults.auth, Auth::None);
    assert_eq!(request(&c, "a.yml").spec.auth, Auth::None);
    assert_eq!(request(&c, "b.yml").spec.auth, Auth::None);
    assert_eq!(
        request(&c, "c.yml").spec.auth,
        Auth::ApiKey {
            key: "X-Key".into(),
            value: "{{k}}".into(),
            placement: ApiKeyPlacement::Query
        }
    );
    // A folder without `auth` defers to its parent (Bruno CLI semantics).
    assert_eq!(folder(&c.items, "f").defaults.auth, Auth::Inherit);
    assert_eq!(request(&c, "f/d.yml").spec.auth, Auth::Inherit);
}

#[test]
fn environments_with_secrets() {
    let c = corpus("environments-api-setenvvar-secret-yml")
        .model()
        .clone();
    let env = c.environment("Local").unwrap();
    assert_eq!(
        env.variables,
        vec![
            Variable::new("baseUrl", "http://localhost:8081"),
            Variable {
                name: "apiToken".into(),
                value: None,
                enabled: true,
                secret: true
            },
        ]
    );
}

#[test]
fn typed_and_disabled_variables() {
    let c = corpus("environments-datatype-preservation-yml")
        .model()
        .clone();
    let env = c.environment("typed_env").unwrap();
    let get = |n: &str| env.variables.iter().find(|v| v.name == n).unwrap();
    assert_eq!(get("env_str").value.as_deref(), Some("env_string"));
    assert_eq!(get("env_num").value.as_deref(), Some("300"));
    assert_eq!(get("env_obj").value.as_deref(), Some(r#"{"scope":"env"}"#));
    assert_eq!(get("env_untyped_num").value.as_deref(), Some("42"));
    assert!(get("env_secret_num").secret);
    assert_eq!(get("env_secret_num").value, None);

    let c = corpus("environments-environment-inheritance-workspace-collections-yml")
        .model()
        .clone();
    let names: Vec<_> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["base", "dev", "qa", "scripted", "staging"]);
    let dev = c.environment("dev").unwrap();
    let shadowed = dev
        .variables
        .iter()
        .find(|v| v.name == "shadowed_by_disabled")
        .unwrap();
    assert!(!shadowed.enabled);
}

#[test]
fn folder_and_collection_defaults() {
    let c = corpus("folder-description-yml").model().clone();
    let api = folder(&c.items, "api");
    assert_eq!(
        api.defaults.headers,
        vec![
            KeyValue::new("X-Version", "2.0"),
            KeyValue::new("X-Multi", "value"),
            KeyValue::new("X-Plain", "plain-value"),
        ]
    );
    assert_eq!(
        api.defaults.vars,
        vec![
            Variable::new("baseUrl", "https://example.com"),
            Variable::new("apiKey", "abc123"),
            Variable::new("plain", "no-desc"),
        ]
    );

    let c = corpus("response-timeline-headers-yml").model().clone();
    assert_eq!(c.defaults.headers.len(), 8);
    assert_eq!(
        c.defaults.headers[3],
        KeyValue::new("collection-interpolated-1", "{{collection-token-1}}")
    );
}

#[test]
fn body_variants() {
    let c = corpus("request-description-yml").model().clone();
    assert_eq!(
        request(&c, "formurlencoded-with-descriptions.yml")
            .spec
            .body,
        Body::FormUrlEncoded {
            fields: vec![
                KeyValue::new("username", "alice"),
                KeyValue::new("password", "secret"),
                KeyValue::new("plain-field", "no-desc"),
            ]
        }
    );
    let text = |name: &str, value: &str| MultipartPart {
        name: name.into(),
        kind: PartKind::Text,
        values: vec![value.into()],
        content_type: None,
        enabled: true,
    };
    assert_eq!(
        request(&c, "multipart-with-descriptions.yml").spec.body,
        Body::Multipart {
            parts: vec![
                text("username", "alice"),
                text("email", "alice@example.com"),
                text("plain-field", "no-desc"),
            ]
        }
    );
    assert_eq!(
        request(&c, "file-with-descriptions.yml").spec.body,
        Body::File {
            path: "/tmp/readme.pdf".into(),
            content_type: Some("application/pdf".into())
        }
    );

    let c = corpus("sse").model().clone();
    assert_eq!(request(&c, "ping-request.yml").spec.body, Body::None);
}

#[test]
fn json_body_params_and_headers() {
    let c = corpus("response-example-try-example").model().clone();
    let r = &request(&c, "try-example.yml").spec;
    assert_eq!(r.name, "try-example");
    assert_eq!(r.seq, Some(1.0));
    assert_eq!(r.method, "POST");
    assert_eq!(
        r.url,
        "http://localhost:8081/api/echo/anything/try?version={{apiVersion}}"
    );
    assert_eq!(
        r.params,
        vec![Param {
            name: "version".into(),
            value: "{{apiVersion}}".into(),
            kind: ParamKind::Query,
            enabled: true
        }]
    );
    assert_eq!(
        r.headers,
        vec![
            KeyValue::new("Content-Type", "application/json"),
            KeyValue::new("X-Api-Token", "{{apiToken}}"),
        ]
    );
    assert_eq!(
        r.body,
        Body::Raw {
            kind: RawKind::Json,
            data: "{\n  \"message\": \"Hello World\"\n}".into()
        }
    );
}

#[test]
fn request_settings() {
    let c = corpus("request-settings-requests-settings-yml")
        .model()
        .clone();
    assert_eq!(
        request(&c, "timeout.yml").spec.settings,
        Settings {
            timeout_ms: Some(5),
            follow_redirects: Some(false),
            max_redirects: Some(0),
            encode_url: None,
        }
    );
    let c = corpus("request-description-yml").model().clone();
    assert_eq!(
        request(&c, "file-with-descriptions.yml").spec.settings,
        Settings {
            timeout_ms: Some(0),
            follow_redirects: Some(true),
            max_redirects: Some(5),
            encode_url: Some(true),
        }
    );
}

#[test]
fn in_memory_details() {
    let c = read_mem(&[
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: mem\nconfig:\n  environments:\n    - name: Inline\n      variables:\n        - name: a\n          value: \"1\"\nrequest:\n  settings:\n    http:\n      timeout: 3000\n      followRedirects: inherit\n",
        ),
        (
            "r.yml",
            concat!(
                "info:\n  name: r\n  type: http\n  seq: 2\n",
                "http:\n  method: PUT\n  url: http://x/:id\n",
                "  headers:\n    - name: A\n      value: \"1\"\n      disabled: true\n",
                "  params:\n    - name: id\n      value: \"7\"\n      type: path\n",
                "  body:\n    - title: first\n      body:\n        type: text\n        data: one\n",
                "    - title: second\n      selected: true\n      body:\n        type: xml\n        data: <a/>\n",
                "runtime:\n  variables:\n    - name: v\n      value:\n        - title: x\n          value: vx\n        - title: y\n          selected: true\n          value: vy\n",
                "settings:\n  timeout: inherit\n  encodeUrl: false\n",
            ),
        ),
        (
            "m.yml",
            "info:\n  name: m\n  type: http\n  seq: 1\nhttp:\n  method: POST\n  url: http://x\n  body:\n    type: multipart-form\n    data:\n      - name: f\n        type: file\n        value:\n          - a.bin\n          - b.bin\n        contentType: application/octet-stream\n        disabled: true\n",
        ),
        (
            "g.yml",
            "info:\n  name: g\n  type: graphql\n  seq: 0\ngraphql:\n  url: http://x\n",
        ),
        ("environments/Disk.yml", "name: Disk\nvariables: []\n"),
    ]);
    assert_eq!(c.defaults.settings.timeout_ms, Some(3000));
    assert_eq!(c.defaults.settings.follow_redirects, None);
    let envs: Vec<_> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(envs, ["Inline", "Disk"]);
    assert_eq!(
        c.environment("Inline").unwrap().variables,
        [Variable::new("a", "1")]
    );

    // Ordering by seq; the graphql item is a request with a GraphQL body (wave 8).
    assert_eq!(names(&c.items), ["g", "m", "r"]);
    let g = &request(&c, "g.yml").spec;
    assert_eq!((g.method.as_str(), g.url.as_str()), ("POST", "http://x"));
    assert_eq!(
        g.body,
        Body::GraphQl {
            query: String::new(),
            variables: String::new()
        }
    );
    let r = &request(&c, "r.yml").spec;
    assert!(!r.headers[0].enabled);
    assert_eq!(r.params[0].kind, ParamKind::Path);
    assert_eq!(
        r.body,
        Body::Raw {
            kind: RawKind::Xml,
            data: "<a/>".into()
        }
    );
    assert_eq!(r.vars, [Variable::new("v", "vy")]);
    assert_eq!(r.settings.timeout_ms, None);
    assert_eq!(r.settings.encode_url, Some(false));

    let m = &request(&c, "m.yml").spec;
    assert_eq!(
        m.body,
        Body::Multipart {
            parts: vec![MultipartPart {
                name: "f".into(),
                kind: PartKind::File,
                values: vec!["a.bin".into(), "b.bin".into()],
                content_type: Some("application/octet-stream".into()),
                enabled: false,
            }]
        }
    );
}

#[test]
fn ordering_and_paths() {
    let c = corpus("runner-folder-run").model().clone();
    assert_eq!(c.name, "folder-run");
    assert_eq!(names(&c.items), ["parent", "rerun"]);
    let parent = folder(&c.items, "parent");
    // Folders and requests share one sequence space.
    assert_eq!(names(&parent.items), ["first", "second", "third", "child"]);
    let child = folder(&parent.items, "child");
    assert_eq!(child.path, "parent/child");
    assert_eq!(names(&child.items), ["child-first", "child-second"]);
    assert_eq!(
        request(&c, "parent/child/child-second.yml").spec.name,
        "child-second"
    );
}

#[test]
fn non_http_items_are_not_requests() {
    // gRPC items are requests since wave 8 (strand B, `tests/protocols_grpc.rs`).
    let c = corpus("grpc-subpath").model().clone();
    assert_eq!(c.items.len(), 5);
    let c = corpus("websockets-multi-message-yml").model().clone();
    fn has_requests(items: &[Item]) -> bool {
        items.iter().any(|i| match i {
            Item::Request(_) => true,
            Item::Folder(f) => has_requests(&f.items),
        })
    }
    assert!(!has_requests(&c.items));
}

#[test]
fn broken_files_are_reported_and_kept() {
    let doc = corpus("collection-invalid-file-indicator-yml");
    let c = doc.model();
    assert_eq!(
        request(c, "parent/healthy.yml").spec.url,
        "https://example.com/healthy"
    );
    assert!(c.request("parent/nested/broken.yml").is_none());
    let broken: Vec<_> = doc
        .issues()
        .iter()
        .map(|e| match e {
            FormatError::Invalid { path, .. } => path.as_str(),
            other => panic!("unexpected issue {other}"),
        })
        .collect();
    assert_eq!(
        broken,
        ["parent/nested/broken.yml", "parent/shallow-broken.yml"]
    );
}

#[test]
fn version_check() {
    // Short forms as found in the corpus.
    corpus("collection-moving-requests-yml-collection");
    for v in ["1.0.0", "1.0.7", "\"1.0\"", "'1.0.0'"] {
        let root = format!("opencollection: {v}\ninfo:\n  name: x\n");
        read_collection(&mem(&[("opencollection.yml", &root)])).unwrap();
    }
    for v in ["2.0.0", "1.1.0", "0.9"] {
        let root = format!("opencollection: {v}\ninfo:\n  name: x\n");
        let err = read_collection(&mem(&[("opencollection.yml", &root)])).unwrap_err();
        assert_eq!(
            err,
            FormatError::UnsupportedVersion {
                path: "opencollection.yml".into(),
                version: v.into()
            }
        );
    }
    assert_eq!(
        read_collection(&mem(&[("a.yml", "info:\n  name: a\n")])).unwrap_err(),
        FormatError::NotACollection
    );
    assert!(matches!(
        read_collection(&mem(&[("opencollection.yml", "info: [unclosed\n")])).unwrap_err(),
        FormatError::Invalid { .. }
    ));
}

/// Wave 5: OAuth 2.0 maps to the model (the two Bruno corpus files that use it).
#[test]
fn oauth2_from_the_bruno_corpus() {
    use rt_core::model::{CredentialsPlacement, OAuth2Flow, OAuth2Step, TokenPlacement};
    let c = corpus("auth-inherited-auth").model().clone();
    let Auth::OAuth2(o) = &c.defaults.auth else {
        panic!("{:?}", c.defaults.auth);
    };
    assert!(matches!(
        &o.flow,
        OAuth2Flow::AuthorizationCode { authorization_url, pkce: None, .. }
            if authorization_url == "http://localhost:8081/authorize"
    ));
    assert_eq!(o.client_id, "inherited-client-id");
    assert_eq!(o.credentials_placement, CredentialsPlacement::Body);
    let steps: Vec<OAuth2Step> = o.parameters.iter().map(|p| p.step).collect();
    assert_eq!(
        steps,
        [
            OAuth2Step::Authorization,
            OAuth2Step::AccessToken,
            OAuth2Step::Refresh
        ]
    );
    assert!(o.auto_fetch && !o.auto_refresh);

    let Auth::OAuth2(f) = &folder(&c.items, "oauth2-folder").defaults.auth else {
        panic!("folder auth");
    };
    assert_eq!(f.flow, OAuth2Flow::ClientCredentials);
    assert_eq!(f.scope, "admin");
    assert_eq!(f.token.id, "credentials");
    assert_eq!(
        f.token.placement,
        TokenPlacement::Header {
            prefix: "Bearer".into()
        }
    );
    assert!(!f.auto_fetch && !f.auto_refresh);
}
