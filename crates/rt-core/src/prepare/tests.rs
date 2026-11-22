// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance tests for `prepare`. Models are built in code; files come from an in-memory
//! `FileSource`. No network, no file system, so they mean the same on native and wasm32.

use std::collections::HashMap;
use std::time::Duration;

use super::*;
use crate::model::{
    ApiKeyPlacement, Auth, Body, Collection, Environment, Folder, Item, KeyValue, MultipartPart,
    Param, ParamKind, PartKind, RawKind, RequestDefaults, RequestEntry, RequestSpec, Settings,
    Variable,
};
use crate::transport::SendOptions;
use crate::vars::VarSource;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct MemFiles(HashMap<String, Vec<u8>>);

impl MemFiles {
    fn with(mut self, path: &str, bytes: &[u8]) -> Self {
        self.0.insert(path.into(), bytes.to_vec());
        self
    }
}

impl FileSource for MemFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no such file: {path}"))
    }
}

const REQ: &str = "outer/inner/req.yml";

fn spec(url: &str) -> RequestSpec {
    RequestSpec {
        name: "req".into(),
        method: "GET".into(),
        url: url.into(),
        ..RequestSpec::default()
    }
}

/// Collection with `outer/inner/req.yml` and the folders `outer` and `outer/inner`.
struct Tree {
    collection: Collection,
    outer: RequestDefaults,
    inner: RequestDefaults,
    request: RequestSpec,
}

impl Tree {
    fn new(url: &str) -> Self {
        Self {
            collection: Collection {
                name: "c".into(),
                ..Collection::default()
            },
            outer: RequestDefaults::default(),
            inner: RequestDefaults::default(),
            request: spec(url),
        }
    }

    fn build(&self) -> Collection {
        let inner = Folder {
            name: "inner".into(),
            path: "outer/inner".into(),
            defaults: self.inner.clone(),
            items: vec![Item::Request(RequestEntry {
                path: REQ.into(),
                spec: self.request.clone(),
            })],
            ..Folder::default()
        };
        let outer = Folder {
            name: "outer".into(),
            path: "outer".into(),
            defaults: self.outer.clone(),
            items: vec![Item::Folder(inner)],
            ..Folder::default()
        };
        let mut c = self.collection.clone();
        c.items = vec![Item::Folder(outer)];
        c
    }

    fn prepare(&self, input: &PrepareInput<'_>) -> Result<Prepared, PrepareError> {
        self.prepare_with(input, &MemFiles::default())
    }

    fn prepare_with(
        &self,
        input: &PrepareInput<'_>,
        files: &dyn FileSource,
    ) -> Result<Prepared, PrepareError> {
        let c = self.build();
        let ctx = c.request(REQ).expect("request in tree");
        prepare(&ctx, input, files)
    }

    fn ok(&self) -> Prepared {
        self.prepare(&PrepareInput::default()).expect("prepare")
    }
}

fn kv(name: &str, value: &str) -> KeyValue {
    KeyValue::new(name, value)
}

fn disabled_kv(name: &str, value: &str) -> KeyValue {
    KeyValue {
        enabled: false,
        ..KeyValue::new(name, value)
    }
}

fn var(name: &str, value: &str) -> Variable {
    Variable::new(name, value)
}

fn param(kind: ParamKind, name: &str, value: &str) -> Param {
    Param {
        name: name.into(),
        value: value.into(),
        kind,
        enabled: true,
    }
}

fn header<'a>(p: &'a Prepared, name: &str) -> Option<&'a str> {
    p.request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn header_count(p: &Prepared, name: &str) -> usize {
    p.request
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case(name))
        .count()
}

fn body(p: &Prepared) -> (Option<&str>, &[u8]) {
    let b = p.request.body.as_ref().expect("body");
    (b.content_type.as_deref(), &b.bytes[..])
}

fn unresolved(p: &Prepared) -> Vec<&str> {
    p.unresolved.iter().map(String::as_str).collect()
}

// ---------------------------------------------------------------------------------------------
// Variables: scope building, precedence, provenance
// ---------------------------------------------------------------------------------------------

/// Defines `x` in every layer; `skip` names one layer to leave out.
fn precedence_tree(skip: Option<&str>) -> (Tree, PrepareInput<'static>) {
    let mut t = Tree::new("https://example.com/{{x}}");
    let on = |layer: &str| skip != Some(layer);
    if on("collection") {
        t.collection.defaults.vars.push(var("x", "collection"));
    }
    let mut dev = Environment {
        client_certificates: Vec::new(),
        external_secrets: None,
        name: "dev".into(),
        variables: vec![],
    };
    if on("environment") {
        dev.variables.push(var("x", "environment"));
    }
    t.collection.environments.push(dev);
    if on("outer") {
        t.outer.vars.push(var("x", "outer"));
    }
    if on("inner") {
        t.inner.vars.push(var("x", "inner"));
    }
    if on("request") {
        t.request.vars.push(var("x", "request"));
    }
    let input = PrepareInput {
        environment: Some("dev"),
        globals: if on("global") {
            vec![("x".into(), "global".into())]
        } else {
            vec![]
        },
        runtime: if on("runtime") {
            vec![("x".into(), "runtime".into())]
        } else {
            vec![]
        },
    };
    (t, input)
}

#[test]
fn precedence_table_through_prepare() {
    let order = [
        ("global", VarSource::Global),
        ("collection", VarSource::Collection),
        ("environment", VarSource::Environment { name: "dev".into() }),
        (
            "outer",
            VarSource::Folder {
                path: "outer".into(),
            },
        ),
        (
            "inner",
            VarSource::Folder {
                path: "outer/inner".into(),
            },
        ),
        ("request", VarSource::Request),
        ("runtime", VarSource::Runtime),
    ];

    let (t, input) = precedence_tree(None);
    let p = t.prepare(&input).unwrap();
    assert_eq!(p.request.url, "https://example.com/runtime");
    assert_eq!(p.used[0].source, VarSource::Runtime);

    // Remove each layer in turn: the highest remaining one wins.
    for (i, (removed, _)) in order.iter().enumerate() {
        let (t, input) = precedence_tree(Some(removed));
        let p = t.prepare(&input).unwrap();
        let (winner, source) = if i == order.len() - 1 {
            &order[i - 1]
        } else {
            &order[order.len() - 1]
        };
        assert_eq!(
            p.request.url,
            format!("https://example.com/{winner}"),
            "removed {removed}"
        );
        assert_eq!(&p.used[0].source, source, "removed {removed}");
    }

    // Peel from the top: each layer wins once all above it are gone.
    for keep in 1..=order.len() {
        let mut t = Tree::new("https://example.com/{{x}}");
        t.collection.environments.push(Environment {
            client_certificates: Vec::new(),
            external_secrets: None,
            name: "dev".into(),
            variables: vec![],
        });
        let mut input = PrepareInput {
            environment: Some("dev"),
            ..PrepareInput::default()
        };
        for (layer, _) in &order[..keep] {
            match *layer {
                "global" => input.globals.push(("x".into(), "global".into())),
                "collection" => t.collection.defaults.vars.push(var("x", layer)),
                "environment" => t.collection.environments[0].variables.push(var("x", layer)),
                "outer" => t.outer.vars.push(var("x", layer)),
                "inner" => t.inner.vars.push(var("x", layer)),
                "request" => t.request.vars.push(var("x", layer)),
                _ => input.runtime.push(("x".into(), "runtime".into())),
            }
        }
        let p = t.prepare(&input).unwrap();
        let (winner, source) = &order[keep - 1];
        assert_eq!(p.request.url, format!("https://example.com/{winner}"));
        assert_eq!(&p.used[0].source, source);
    }
}

#[test]
fn scope_layers_are_built_lowest_first() {
    let (t, input) = precedence_tree(None);
    let p = t.prepare(&input).unwrap();
    let sources: Vec<&VarSource> = p.scope.layers.iter().map(|l| &l.source).collect();
    assert_eq!(
        sources,
        vec![
            &VarSource::Global,
            &VarSource::Collection,
            &VarSource::Environment { name: "dev".into() },
            &VarSource::Folder {
                path: "outer".into()
            },
            &VarSource::Folder {
                path: "outer/inner".into()
            },
            &VarSource::Request,
            &VarSource::Runtime,
        ]
    );
}

#[test]
fn no_environment_means_no_environment_layer() {
    let (t, mut input) = precedence_tree(Some("runtime"));
    input.environment = None;
    let p = t.prepare(&input).unwrap();
    assert!(
        !p.scope
            .layers
            .iter()
            .any(|l| matches!(l.source, VarSource::Environment { .. }))
    );
    assert_eq!(p.request.url, "https://example.com/request");
}

#[test]
fn unknown_environment_is_an_error() {
    let t = Tree::new("https://example.com/");
    let err = t
        .prepare(&PrepareInput {
            environment: Some("nope"),
            ..PrepareInput::default()
        })
        .unwrap_err();
    assert_eq!(err, PrepareError::UnknownEnvironment("nope".into()));
}

#[test]
fn disabled_variables_are_skipped() {
    let mut t = Tree::new("https://example.com/{{x}}/{{y}}");
    t.outer.vars.push(var("x", "outer"));
    t.request.vars.push(Variable {
        enabled: false,
        ..var("x", "request-disabled")
    });
    t.request.vars.push(Variable {
        enabled: false,
        ..var("y", "disabled")
    });
    let p = t.ok();
    assert_eq!(p.request.url, "https://example.com/outer/%7B%7By%7D%7D");
    assert_eq!(unresolved(&p), ["y"]);
}

#[test]
fn secret_without_value_is_unresolved_unless_runtime_provides_it() {
    let mut t = Tree::new("https://example.com/");
    t.request.headers.push(kv("X-Token", "{{token}}"));
    t.collection.environments.push(Environment {
        client_certificates: Vec::new(),
        external_secrets: None,
        name: "dev".into(),
        variables: vec![
            Variable {
                name: "token".into(),
                value: None,
                enabled: true,
                secret: true,
            },
            Variable {
                name: "kept".into(),
                value: Some("from-keychain".into()),
                enabled: true,
                secret: true,
            },
        ],
    });
    t.request.headers.push(kv("X-Kept", "{{kept}}"));

    let mut input = PrepareInput {
        environment: Some("dev"),
        ..PrepareInput::default()
    };
    let p = t.prepare(&input).unwrap();
    assert_eq!(header(&p, "X-Token"), Some("{{token}}"));
    assert_eq!(unresolved(&p), ["token"]);
    // A secret with a value (filled in from the keychain) resolves and is flagged.
    assert_eq!(header(&p, "X-Kept"), Some("from-keychain"));
    let kept = p.used.iter().find(|u| u.name == "kept").unwrap();
    assert!(kept.secret);
    assert_eq!(kept.source, VarSource::Environment { name: "dev".into() });

    input.runtime.push(("token".into(), "abc".into()));
    let p = t.prepare(&input).unwrap();
    assert_eq!(header(&p, "X-Token"), Some("abc"));
    assert!(p.unresolved.is_empty());
    let token = p.used.iter().find(|u| u.name == "token").unwrap();
    assert_eq!(token.source, VarSource::Runtime);
    // Declared secret in a file: the runtime value is treated as secret too.
    assert!(token.secret);
}

#[test]
fn used_and_unresolved_are_aggregated_across_url_headers_auth_and_body() {
    let mut t = Tree::new("https://{{host}}/{{u1}}");
    t.request.vars.push(var("host", "example.com"));
    t.request.vars.push(var("tok", "T"));
    t.request.headers.push(kv("X-A", "{{host}}-{{u2}}"));
    t.request.auth = Auth::Bearer {
        token: "{{tok}}{{u3}}".into(),
    };
    t.request.body = Body::Raw {
        kind: RawKind::Text,
        data: "{{u1}} {{u4}} {{tok}}".into(),
    };
    let p = t.ok();
    assert_eq!(unresolved(&p), ["u1", "u2", "u3", "u4"]);
    let used: Vec<&str> = p.used.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(used, ["host", "tok"]);
}

// ---------------------------------------------------------------------------------------------
// URL: interpolation, path params, query params, encoding
// ---------------------------------------------------------------------------------------------

#[test]
fn url_without_scheme_gets_http_like_bruno() {
    let mut t = Tree::new("{{host}}/api");
    t.request.vars.push(var("host", "localhost:8080"));
    assert_eq!(t.ok().request.url, "http://localhost:8080/api");
}

#[test]
fn path_params_replace_whole_segments() {
    let mut t = Tree::new("https://example.com/users/:id/posts/:postId/:missing/:empty/x:id");
    t.request.vars.push(var("pid", "7"));
    t.request.params = vec![
        param(ParamKind::Path, "id", "42"),
        param(ParamKind::Path, "postId", "{{pid}}"),
        param(ParamKind::Path, "empty", " "),
        Param {
            enabled: false,
            ..param(ParamKind::Path, "missing", "nope")
        },
    ];
    let p = t.ok();
    assert_eq!(
        p.request.url,
        "https://example.com/users/42/posts/7/:missing/:empty/x:id"
    );
    assert_eq!(p.used[0].name, "pid");
}

#[test]
fn path_params_in_odata_style_segments() {
    let mut t = Tree::new("https://example.com/odata/Users(:id)/Orders(Key1=:k1,Key2=':k2')");
    t.request.params = vec![
        param(ParamKind::Path, "id", "42"),
        param(ParamKind::Path, "k1", "1"),
        param(ParamKind::Path, "k2", "b"),
    ];
    assert_eq!(
        t.ok().request.url,
        "https://example.com/odata/Users(42)/Orders(Key1=1,Key2='b')"
    );
}

#[test]
fn path_params_are_encoded_when_encoding_is_on() {
    let mut t = Tree::new("https://example.com/files/:name");
    t.request.params = vec![param(ParamKind::Path, "name", "a b#c")];
    assert_eq!(t.ok().request.url, "https://example.com/files/a%20b%23c");

    t.request.settings.encode_url = Some(false);
    assert_eq!(t.ok().request.url, "https://example.com/files/a b#c");
}

#[test]
fn the_url_is_the_truth_query_params_are_never_appended() {
    // Bruno writes the query into the URL and mirrors it in the params list; only the URL is
    // sent. Params that are missing from the URL, or disabled, change nothing.
    let mut t = Tree::new("http://localhost:8081/api/try?version={{v}}");
    t.request.vars.push(var("v", "2"));
    t.request.params = vec![
        param(ParamKind::Query, "version", "{{v}}"),
        param(ParamKind::Query, "extra", "x"),
        Param {
            enabled: false,
            ..param(ParamKind::Query, "off", "1")
        },
    ];
    assert_eq!(
        t.ok().request.url,
        "http://localhost:8081/api/try?version=2"
    );

    t.request.url = "https://example.com/search".into();
    assert_eq!(t.ok().request.url, "https://example.com/search");
}

#[test]
fn invalid_url_error_names_unresolved_variables() {
    let t = Tree::new("{{baseUrl}}:port/users");
    let err = t.prepare(&PrepareInput::default()).unwrap_err();
    let PrepareError::InvalidUrl(message) = err else {
        panic!("expected InvalidUrl, got {err:?}");
    };
    assert!(
        message.starts_with("http://{{baseUrl}}:port/users: "),
        "{message}"
    );
    assert!(
        message.ends_with("(unresolved variables: baseUrl)"),
        "{message}"
    );
}

#[test]
fn encoding_on_encodes_once_and_never_double_encodes() {
    let t = Tree::new("https://example.com/a b/%20ok/ä?q=x y&pre=%2F&amp=a+b&odd=100%&enc=%41");
    assert_eq!(
        t.ok().request.url,
        "https://example.com/a%20b/%20ok/%C3%A4?q=x%20y&pre=%2F&amp=a%2Bb&odd=100%25&enc=%41"
    );
}

#[test]
fn encoding_off_sends_the_url_as_written() {
    let mut t = Tree::new("https://example.com/a b/ä?q=x y");
    t.request.settings.encode_url = Some(false);
    assert_eq!(t.ok().request.url, "https://example.com/a b/ä?q=x y");
}

#[test]
fn encode_url_is_inherited_from_folders_and_collection() {
    let mut t = Tree::new("https://example.com/a b");
    t.collection.defaults.settings.encode_url = Some(false);
    assert_eq!(t.ok().request.url, "https://example.com/a b");
    t.inner.settings.encode_url = Some(true);
    assert_eq!(t.ok().request.url, "https://example.com/a%20b");
    t.request.settings.encode_url = Some(false);
    assert_eq!(t.ok().request.url, "https://example.com/a b");
}

#[test]
fn authority_is_left_alone_when_encoding() {
    let t = Tree::new("http://user:p%40ss@[::1]:8080/x");
    assert_eq!(t.ok().request.url, "http://user:p%40ss@[::1]:8080/x");
}

#[test]
fn invalid_url_after_substitution_is_an_error() {
    for url in [
        "https://example.com:99999/",
        "http://[::1/",
        "http://exa mple.com/",
    ] {
        let t = Tree::new(url);
        assert!(
            matches!(
                t.prepare(&PrepareInput::default()),
                Err(PrepareError::InvalidUrl(_))
            ),
            "{url}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Headers
// ---------------------------------------------------------------------------------------------

#[test]
fn headers_merge_collection_folders_request_later_wins_case_insensitively() {
    let mut t = Tree::new("https://example.com/");
    t.collection.defaults.headers = vec![
        kv("X-Layer", "collection"),
        kv("Accept", "application/json"),
        kv("X-Collection", "c"),
    ];
    t.outer.headers = vec![kv("x-layer", "outer"), kv("X-Outer", "o")];
    t.inner.headers = vec![kv("X-LAYER", "inner"), disabled_kv("X-Outer", "disabled")];
    t.request.vars.push(var("h", "X-From-Var"));
    t.request.headers = vec![
        kv("X-Layer", "request"),
        disabled_kv("Accept", "text/plain"),
        kv("{{h}}", "{{h}}!"),
        kv("", "nameless"),
    ];
    let p = t.ok();
    assert_eq!(
        p.request.headers,
        vec![
            ("X-Layer".to_owned(), "request".to_owned()),
            ("Accept".to_owned(), "application/json".to_owned()),
            ("X-Collection".to_owned(), "c".to_owned()),
            ("X-Outer".to_owned(), "o".to_owned()),
            ("X-From-Var".to_owned(), "X-From-Var!".to_owned()),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------------------------

fn bearer(token: &str) -> Auth {
    Auth::Bearer {
        token: token.into(),
    }
}

fn basic(u: &str, p: &str) -> Auth {
    Auth::Basic {
        username: u.into(),
        password: p.into(),
    }
}

#[test]
fn auth_inherits_from_nearest_folder_across_two_levels() {
    let mut t = Tree::new("https://example.com/");
    t.collection.defaults.auth = bearer("collection");
    t.outer.auth = basic("Aladdin", "open sesame");
    // inner and request inherit
    let p = t.ok();
    assert_eq!(p.auth, basic("Aladdin", "open sesame"));
    assert_eq!(
        header(&p, "Authorization"),
        Some("Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==")
    );

    t.inner.auth = bearer("{{tok}}");
    t.request.vars.push(var("tok", "inner-token"));
    let p = t.ok();
    assert_eq!(
        p.auth,
        bearer("{{tok}}"),
        "effective auth is pre-interpolation"
    );
    assert_eq!(header(&p, "Authorization"), Some("Bearer inner-token"));

    t.request.auth = bearer("own");
    assert_eq!(header(&t.ok(), "Authorization"), Some("Bearer own"));
}

#[test]
fn auth_none_stops_inheritance() {
    let mut t = Tree::new("https://example.com/");
    t.collection.defaults.auth = bearer("collection");
    t.inner.auth = Auth::None;
    let p = t.ok();
    assert_eq!(p.auth, Auth::None);
    assert_eq!(header(&p, "Authorization"), None);

    t.inner.auth = Auth::Inherit;
    t.request.auth = Auth::None;
    assert_eq!(header(&t.ok(), "Authorization"), None);
}

#[test]
fn collection_level_auth_and_collection_inherit_means_none() {
    let mut t = Tree::new("https://example.com/");
    t.collection.defaults.auth = bearer("collection");
    let p = t.ok();
    assert_eq!(p.auth, bearer("collection"));
    assert_eq!(header(&p, "Authorization"), Some("Bearer collection"));

    t.collection.defaults.auth = Auth::Inherit;
    let p = t.ok();
    assert_eq!(p.auth, Auth::None);
    assert!(p.request.headers.is_empty());
}

#[test]
fn basic_auth_with_interpolated_values() {
    let mut t = Tree::new("https://example.com/");
    t.request.vars.push(var("user", "ü"));
    t.request.auth = basic("{{user}}", "p:w");
    // base64("ü:p:w") with UTF-8 bytes
    assert_eq!(header(&t.ok(), "Authorization"), Some("Basic w7w6cDp3"));
}

#[test]
fn api_key_in_header() {
    let mut t = Tree::new("https://example.com/");
    t.request.vars.push(var("key", "s3cret"));
    t.request.headers.push(kv("x-api-key", "from-header"));
    t.outer.auth = Auth::ApiKey {
        key: "X-Api-Key".into(),
        value: "{{key}}".into(),
        placement: ApiKeyPlacement::Header,
    };
    let p = t.ok();
    assert_eq!(header(&p, "X-Api-Key"), Some("s3cret"));
    assert_eq!(header_count(&p, "x-api-key"), 1);
}

#[test]
fn api_key_in_query() {
    let mut t = Tree::new("https://example.com/x?a=1&api_key=old");
    t.request.auth = Auth::ApiKey {
        key: "api_key".into(),
        value: "a b&c".into(),
        placement: ApiKeyPlacement::Query,
    };
    assert_eq!(
        t.ok().request.url,
        "https://example.com/x?a=1&api_key=a%20b%26c"
    );

    // Encoded even when URL encoding is off (Bruno uses URLSearchParams here).
    t.request.url = "https://example.com/x".into();
    t.request.settings.encode_url = Some(false);
    assert_eq!(
        t.ok().request.url,
        "https://example.com/x?api_key=a%20b%26c"
    );
}

#[test]
fn api_key_with_empty_key_sends_nothing() {
    let mut t = Tree::new("https://example.com/x");
    t.request.auth = Auth::ApiKey {
        key: String::new(),
        value: "v".into(),
        placement: ApiKeyPlacement::Header,
    };
    let p = t.ok();
    assert!(p.request.headers.is_empty());
    assert_eq!(p.request.url, "https://example.com/x");
}

#[test]
fn unsupported_auth_is_an_error_also_when_inherited() {
    let mut t = Tree::new("https://example.com/");
    t.outer.auth = Auth::Unsupported {
        kind: "oauth2".into(),
    };
    assert_eq!(
        t.prepare(&PrepareInput::default()).unwrap_err(),
        PrepareError::UnsupportedAuth("oauth2".into())
    );
    // A nearer declaration shadows it.
    t.inner.auth = bearer("t");
    assert!(t.prepare(&PrepareInput::default()).is_ok());
}

#[test]
fn auth_replaces_a_same_name_header_like_bruno() {
    let mut t = Tree::new("https://example.com/");
    t.request.headers.push(kv("authorization", "Custom abc"));
    t.request.headers.push(kv("X-Other", "1"));
    // No effective auth: the explicit header is sent.
    assert_eq!(header(&t.ok(), "Authorization"), Some("Custom abc"));
    // Effective auth (here inherited from the collection) overrides it.
    t.collection.defaults.auth = bearer("t");
    let p = t.ok();
    assert_eq!(header(&p, "Authorization"), Some("Bearer t"));
    assert_eq!(header_count(&p, "Authorization"), 1);
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

#[test]
fn settings_defaults() {
    let p = Tree::new("https://example.com/").ok();
    assert_eq!(p.request.options, SendOptions::default());
    assert_eq!(p.request.options.timeout, None);
    assert!(p.request.options.follow_redirects);
    assert_eq!(p.request.options.max_redirects, 5);
}

#[test]
fn settings_inherit_request_folders_collection() {
    let mut t = Tree::new("https://example.com/");
    t.collection.defaults.settings = Settings {
        timeout_ms: Some(1000),
        follow_redirects: Some(true),
        max_redirects: Some(9),
        encode_url: None,
    };
    t.outer.settings.follow_redirects = Some(false);
    t.inner.settings.max_redirects = Some(3);
    let p = t.ok();
    assert_eq!(
        p.request.options,
        SendOptions {
            client_identity: None,
            timeout: Some(Duration::from_millis(1000)),
            follow_redirects: false,
            max_redirects: 3,
        }
    );

    t.request.settings.timeout_ms = Some(0);
    t.request.settings.max_redirects = Some(0);
    t.inner.settings.follow_redirects = Some(true);
    let p = t.ok();
    assert_eq!(p.request.options.timeout, None, "0 means no timeout");
    assert_eq!(p.request.options.max_redirects, 0);
    assert!(p.request.options.follow_redirects);
}

#[test]
fn method_is_passed_through() {
    let mut t = Tree::new("https://example.com/");
    t.request.method = "PATCH".into();
    assert_eq!(t.ok().request.method, "PATCH");
}

// ---------------------------------------------------------------------------------------------
// Bodies
// ---------------------------------------------------------------------------------------------

#[test]
fn no_body_means_no_body() {
    assert_eq!(Tree::new("https://example.com/").ok().request.body, None);
}

#[test]
fn raw_bodies_are_interpolated_with_default_content_types() {
    for (kind, ct) in [
        (RawKind::Json, "application/json"),
        (RawKind::Text, "text/plain"),
        (RawKind::Xml, "application/xml"),
        (RawKind::Sparql, "application/sparql-query"),
    ] {
        let mut t = Tree::new("https://example.com/");
        t.request.vars.push(var("name", "Bruno \"the dog\""));
        t.request.body = Body::Raw {
            kind,
            data: "{\"name\": \"{{name}}\"}".into(),
        };
        let p = t.ok();
        let (got_ct, bytes) = body(&p);
        assert_eq!(got_ct, Some(ct), "{kind:?}");
        // Values are inserted as they are (Bruno escapes only its dynamic `$` values).
        assert_eq!(bytes, b"{\"name\": \"Bruno \"the dog\"\"}", "{kind:?}");
    }
}

#[test]
fn user_content_type_header_suppresses_the_default() {
    let mut t = Tree::new("https://example.com/");
    t.outer
        .headers
        .push(kv("content-type", "application/vnd.api+json"));
    t.request.body = Body::Raw {
        kind: RawKind::Json,
        data: "{}".into(),
    };
    let p = t.ok();
    assert_eq!(body(&p).0, None);
    assert_eq!(header(&p, "Content-Type"), Some("application/vnd.api+json"));
}

#[test]
fn form_urlencoded_body() {
    let mut t = Tree::new("https://example.com/");
    t.request.vars.push(var("v", "x y&z"));
    t.request.body = Body::FormUrlEncoded {
        fields: vec![
            kv("a", "1"),
            disabled_kv("off", "1"),
            kv("b", "{{v}}"),
            kv("ü=", "*-._~!"),
        ],
    };
    let p = t.ok();
    let (ct, bytes) = body(&p);
    assert_eq!(ct, Some("application/x-www-form-urlencoded"));
    assert_eq!(
        std::str::from_utf8(bytes).unwrap(),
        "a=1&b=x+y%26z&%C3%BC%3D=*-._%7E%21"
    );
}

fn boundary_of(ct: &str) -> &str {
    ct.strip_prefix("multipart/form-data; boundary=")
        .expect("multipart content type with boundary")
}

fn multipart_tree(file_content_type: Option<&str>) -> Tree {
    let mut t = Tree::new("https://example.com/upload");
    t.request.vars.push(var("who", "Bruno"));
    t.request.vars.push(var("dir", "files"));
    t.request.body = Body::Multipart {
        parts: vec![
            MultipartPart {
                name: "greeting".into(),
                kind: PartKind::Text,
                values: vec!["hi {{who}}".into()],
                content_type: None,
                enabled: true,
            },
            MultipartPart {
                name: "off".into(),
                kind: PartKind::Text,
                values: vec!["no".into()],
                content_type: None,
                enabled: false,
            },
            MultipartPart {
                name: "meta".into(),
                kind: PartKind::Text,
                values: vec!["{}".into()],
                content_type: Some("application/json".into()),
                enabled: true,
            },
            MultipartPart {
                name: "upload".into(),
                kind: PartKind::File,
                values: vec!["{{dir}}/hello.txt".into()],
                content_type: file_content_type.map(Into::into),
                enabled: true,
            },
        ],
    };
    t
}

fn expected_multipart(b: &str, file: &[u8], file_ct: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(
        format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"greeting\"\r\n\r\nhi Bruno\r\n\
             --{b}\r\nContent-Disposition: form-data; name=\"meta\"\r\n\
             Content-Type: application/json\r\n\r\n{{}}\r\n\
             --{b}\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"hello.txt\"\r\n\
             Content-Type: {file_ct}\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(file);
    out.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    out
}

#[test]
fn multipart_with_text_and_file_parts_from_the_file_source() {
    let files = MemFiles::default().with("files/hello.txt", b"Hello\x00World");
    let t = multipart_tree(None);
    let p = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    let (ct, bytes) = body(&p);
    let b = boundary_of(ct.unwrap());
    assert!(!b.is_empty() && b.len() <= 70);
    assert_eq!(
        bytes,
        expected_multipart(b, b"Hello\x00World", "application/octet-stream")
    );

    // Deterministic for the same input.
    let again = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    assert_eq!(again.request.body, p.request.body);

    let t = multipart_tree(Some("text/plain"));
    let p = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    let (ct, bytes) = body(&p);
    let b = boundary_of(ct.unwrap());
    assert_eq!(
        bytes,
        expected_multipart(b, b"Hello\x00World", "text/plain")
    );
}

#[test]
fn multipart_boundary_never_occurs_in_the_content() {
    let t = multipart_tree(None);
    let files = MemFiles::default().with("files/hello.txt", b"x");
    let p = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    let first = boundary_of(body(&p).0.unwrap()).to_owned();

    // A file that contains the boundary the first run picked.
    let evil = format!("\r\n--{first}--\r\n");
    let files = MemFiles::default().with("files/hello.txt", evil.as_bytes());
    let p = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    let (ct, bytes) = body(&p);
    let b = boundary_of(ct.unwrap());
    let delimiter = format!("--{b}");
    let hits = bytes
        .windows(delimiter.len())
        .filter(|w| *w == delimiter.as_bytes())
        .count();
    assert_eq!(hits, 4, "three parts plus the closing delimiter");
    assert_eq!(
        bytes,
        expected_multipart(b, evil.as_bytes(), "application/octet-stream")
    );
}

#[test]
fn multipart_keeps_a_user_boundary_and_completes_a_bare_user_header() {
    let files = MemFiles::default().with("files/hello.txt", b"x");
    let mut t = multipart_tree(None);
    t.request.headers.push(kv(
        "Content-Type",
        "multipart/form-data; boundary=myboundary",
    ));
    let p = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    assert_eq!(body(&p).0, None);
    assert_eq!(
        body(&p).1,
        expected_multipart("myboundary", b"x", "application/octet-stream")
    );

    let mut t = multipart_tree(None);
    t.request
        .headers
        .push(kv("Content-Type", "multipart/form-data"));
    let p = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    let ct = header(&p, "content-type").unwrap().to_owned();
    let b = boundary_of(&ct);
    assert_eq!(header_count(&p, "content-type"), 1);
    assert_eq!(
        body(&p).1,
        expected_multipart(b, b"x", "application/octet-stream")
    );
}

#[test]
fn multipart_file_that_cannot_be_read_is_an_error() {
    let t = multipart_tree(None);
    let err = t.prepare(&PrepareInput::default()).unwrap_err();
    assert_eq!(
        err,
        PrepareError::BodyFile {
            path: "files/hello.txt".into(),
            message: "no such file: files/hello.txt".into()
        }
    );
}

#[test]
fn file_body_from_the_file_source() {
    let files = MemFiles::default().with("data/payload.bin", &[0, 159, 146, 150]);
    let mut t = Tree::new("https://example.com/");
    t.request.body = Body::File {
        path: "data/payload.bin".into(),
        content_type: None,
    };
    let p = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    assert_eq!(
        body(&p),
        (Some("application/octet-stream"), &[0u8, 159, 146, 150][..])
    );

    t.request.body = Body::File {
        path: "data/payload.bin".into(),
        content_type: Some("image/png".into()),
    };
    let p = t.prepare_with(&PrepareInput::default(), &files).unwrap();
    assert_eq!(body(&p).0, Some("image/png"));

    t.request.body = Body::File {
        path: "data/missing.bin".into(),
        content_type: None,
    };
    assert!(matches!(
        t.prepare_with(&PrepareInput::default(), &files),
        Err(PrepareError::BodyFile { path, .. }) if path == "data/missing.bin"
    ));
}

#[test]
fn unresolved_placeholder_in_the_host_is_a_prepare_error() {
    // The `url` crate accepts `{` in a host, so this must be caught explicitly.
    for template in ["http://{{noSuchHost}}/echo", "{{scheme}}://example.com/x"] {
        let t = Tree::new(template);
        let err = t.prepare(&PrepareInput::default()).unwrap_err();
        let PrepareError::InvalidUrl(message) = err else {
            panic!("expected InvalidUrl for {template}, got {err:?}");
        };
        assert!(message.contains("unresolved variables: "), "{message}");
    }
}
