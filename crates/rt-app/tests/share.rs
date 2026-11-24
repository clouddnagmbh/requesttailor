// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand C: sharing without a server. A link made from a request opens as the same
//! request; no credential is ever in it (a property over every request of the Bruno corpus
//! with canary secrets put into every place that can hold one); the scanner refuses what the
//! rules miss; the preview is exactly what the link carries.

use std::path::{Path, PathBuf};

use rt_app::share::{
    self, FRAGMENT_KEY, MAX_LINK_LEN, MAX_QR_LEN, ShareError, ShareOptions, TEXT_PREFIX, build,
};
use rt_app::{AppError, Workspace};
use rt_core::model::{
    Auth, Body, Environment, Item, KeyValue, Param, ParamKind, RawKind, RequestSpec, Variable,
};
use rt_format::{FileSet, read_collection};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno")
}

fn requests(items: &[Item], out: &mut Vec<RequestSpec>) {
    for item in items {
        match item {
            Item::Request(r) => out.push(r.spec.clone()),
            Item::Folder(f) => requests(&f.items, out),
        }
    }
}

/// Every request of every collection of the corpus.
fn corpus_requests() -> Vec<RequestSpec> {
    let mut out = Vec::new();
    let mut dirs: Vec<_> = std::fs::read_dir(corpus_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let Ok(set) = FileSet::from_dir(&dir) else {
            continue;
        };
        let Ok(doc) = read_collection(&set) else {
            continue;
        };
        requests(&doc.model().items, &mut out);
    }
    out
}

fn sample() -> RequestSpec {
    RequestSpec {
        name: "Create user".into(),
        seq: Some(3.0),
        method: "POST".into(),
        url: "{{baseUrl}}/users/:id".into(),
        headers: vec![
            KeyValue::new("Content-Type", "application/json"),
            KeyValue::new("Authorization", "Bearer {{token}}"),
        ],
        params: vec![Param {
            name: "id".into(),
            value: "7".into(),
            kind: ParamKind::Path,
            enabled: true,
        }],
        body: Body::Raw {
            kind: RawKind::Json,
            data: "{\n  \"name\": \"{{userName}}\",\n  \"tags\": [\"a\", \"b\"]\n}".into(),
        },
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        ..RequestSpec::default()
    }
}

fn env() -> Environment {
    Environment {
        name: "Dev".into(),
        variables: vec![
            Variable::new("baseUrl", "https://api.dev.test"),
            Variable::new("userName", "ada"),
            Variable {
                name: "token".into(),
                value: None,
                enabled: true,
                secret: true,
            },
        ],
        client_certificates: vec![],
        external_secrets: None,
    }
}

fn opts() -> ShareOptions {
    ShareOptions::default()
}

#[test]
fn a_link_opens_as_the_same_request() {
    let spec = sample();
    let link = build("Create user", &spec, Some(&env()), &opts()).unwrap();
    assert!(link.text.starts_with(TEXT_PREFIX), "no base: {}", link.text);
    let opened = share::open(&link.text).unwrap();
    let mut want = spec.clone();
    want.seq = None;
    assert_eq!(opened.request, want);
    assert_eq!(opened.name, "Create user");
    assert_eq!(opened, link.payload);
}

#[test]
fn a_link_with_a_base_keeps_everything_in_the_fragment() {
    let o = ShareOptions {
        base: Some("https://app.example.test/".into()),
        ..opts()
    };
    let link = build("x", &sample(), None, &o).unwrap();
    let (base, fragment) = link.text.split_once('#').unwrap();
    assert_eq!(base, "https://app.example.test/");
    assert!(fragment.starts_with(&format!("{FRAGMENT_KEY}=1.")));
    assert!(
        !base.contains("1."),
        "payload must not be in the path or query"
    );
    // A base that has a fragment already loses it.
    let o = ShareOptions {
        base: Some("https://app.example.test/?a=1#old".into()),
        ..opts()
    };
    let link = build("x", &sample(), None, &o).unwrap();
    assert_eq!(link.text.matches('#').count(), 1);
    assert_eq!(share::open(&link.text).unwrap().name, "x");
}

#[test]
fn every_way_of_pasting_is_read() {
    let o = ShareOptions {
        base: Some("https://app.example.test/".into()),
        ..opts()
    };
    let link = build("x", &sample(), None, &o).unwrap();
    let payload = link
        .text
        .split_once('#')
        .unwrap()
        .1
        .strip_prefix("rt=")
        .unwrap();
    for text in [
        link.text.clone(),
        format!("#rt={payload}"),
        format!("rt={payload}"),
        format!("rt:{payload}"),
        format!("  {}\n", link.text),
        format!("see {} for details", link.text.replace("https", "http")),
        format!("https://elsewhere.test/app/#a=1&rt={payload}"),
    ] {
        assert_eq!(share::open(&text).unwrap().name, "x", "{text}");
    }
    for text in [
        "",
        "hello",
        "https://x.test/#section",
        "rt:",
        "rt:abc",
        "rt:1.",
        "curl https://x.test",
    ] {
        assert!(
            matches!(share::open(text), Err(AppError::Format(_))),
            "{text:?} is no link"
        );
    }
}

#[test]
fn a_newer_or_damaged_link_is_refused_with_a_reason() {
    let e = share::decode("rt:2.abcdef").unwrap_err();
    assert_eq!(e, ShareError::Version(2));
    for damaged in ["rt:1.!!!!", "rt:1.abcd", "rt:1.AAAA"] {
        assert!(
            matches!(share::decode(damaged), Err(ShareError::Invalid(_))),
            "{damaged}"
        );
    }
    // A crafted link that inflates to something huge is stopped.
    let bomb = miniz_oxide_bomb();
    assert!(matches!(share::decode(&bomb), Err(ShareError::Invalid(_))));
}

fn miniz_oxide_bomb() -> String {
    use base64::Engine as _;
    let big = vec![b' '; 16 * 1024 * 1024];
    let packed = miniz_oxide::deflate::compress_to_vec(&big, 9);
    format!(
        "rt:1.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(packed)
    )
}

#[test]
fn the_preview_is_exactly_what_the_link_carries() {
    let link = build("Create user", &sample(), Some(&env()), &opts()).unwrap();
    let decoded = share::decode(&link.text).unwrap();
    assert_eq!(
        link.preview,
        serde_json::to_string_pretty(&decoded).unwrap()
    );
    // Names of the variables used, never values, by default.
    let names: Vec<_> = link
        .payload
        .variables
        .iter()
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(names, ["baseUrl", "token", "userName"]);
    assert!(link.payload.variables.iter().all(|v| v.value.is_none()));
    assert!(
        link.payload
            .variables
            .iter()
            .find(|v| v.name == "token")
            .unwrap()
            .secret
    );
    assert!(!link.preview.contains("api.dev.test"));
}

#[test]
fn values_of_non_secret_variables_only_when_asked() {
    let o = ShareOptions {
        include_values: true,
        environment: Some("Dev".into()),
        ..opts()
    };
    let link = build("x", &sample(), Some(&env()), &o).unwrap();
    let get = |n: &str| {
        link.payload
            .variables
            .iter()
            .find(|v| v.name == n)
            .unwrap()
            .clone()
    };
    assert_eq!(
        get("baseUrl").value.as_deref(),
        Some("https://api.dev.test")
    );
    assert_eq!(get("userName").value.as_deref(), Some("ada"));
    // The secret has none, also with the option on.
    assert_eq!(get("token").value, None);
    assert!(
        link.preview.contains("api.dev.test"),
        "the preview shows what goes in"
    );
}

const CANARY: &str = "RT-CANARY-d41d8cd98f00b204";

fn canary(n: u32) -> String {
    format!("{CANARY}-{n}")
}

/// A request with a canary in every place that can hold a credential.
fn poisoned(base: &RequestSpec) -> RequestSpec {
    let mut s = base.clone();
    s.url = format!(
        "https://user:{}@api.test/items?api_key={}&page=1",
        canary(1),
        canary(2)
    );
    s.params.push(Param {
        name: "access_token".into(),
        value: canary(3),
        kind: ParamKind::Query,
        enabled: true,
    });
    s.headers.push(KeyValue::new(
        "Authorization",
        format!("Bearer {}", canary(4)),
    ));
    s.headers
        .push(KeyValue::new("Cookie", format!("sid={}", canary(5))));
    s.headers.push(KeyValue::new("X-Api-Key", canary(6)));
    s.headers.push(KeyValue::new(
        "Proxy-Authorization",
        format!("Basic {}", canary(7)),
    ));
    s.body = Body::Raw {
        kind: RawKind::Json,
        data: format!(
            "{{\"name\":\"n\",\"password\":\"{}\",\"nested\":{{\"client_secret\": \"{}\"}}}}",
            canary(8),
            canary(9)
        ),
    };
    s.auth = Auth::Basic {
        username: "ada".into(),
        password: canary(10),
    };
    s.vars.push(Variable::new("api_token", canary(11)));
    s
}

fn poisoned_env() -> Environment {
    let mut e = env();
    // A plain-looking variable whose name says what it is, and a value in the open.
    e.variables.push(Variable::new("password", canary(12)));
    e.variables.push(Variable::new("host", "h.test"));
    e
}

fn assert_no_canary(text: &str, what: &str) {
    assert!(
        !text.contains(CANARY),
        "{what} carries a canary secret:\n{text}"
    );
}

#[test]
fn no_secret_is_in_a_link_whatever_the_request() {
    let environment = poisoned_env();
    let o = ShareOptions {
        include_values: true,
        environment: Some("Dev".into()),
        ..opts()
    };
    let corpus = corpus_requests();
    assert!(
        corpus.len() > 200,
        "the corpus has {} requests",
        corpus.len()
    );
    let mut roundtrips = 0;
    let mut refused = 0;
    for spec in &corpus {
        // 1. As it is: a link, or a refusal because of something the rules cannot replace.
        match build(&spec.name, spec, Some(&environment), &o) {
            Ok(link) => {
                let opened = share::decode(&link.text).unwrap();
                let mut want = share::sanitize(spec).spec;
                want.name = spec.name.clone();
                assert_eq!(opened.request, want, "{}", spec.name);
                let mut same = spec.clone();
                same.seq = None;
                if share::sanitize(spec).spec == same {
                    assert_eq!(opened.request, same, "{} round trip", spec.name);
                    roundtrips += 1;
                }
            }
            Err(ShareError::SecretsFound(_)) => refused += 1,
            Err(ShareError::TooLong(_)) => {}
            Err(e) => panic!("{}: {e}", spec.name),
        }
        // 2. With canaries everywhere: never a canary, in the link or in what it decodes to.
        let mut bad = poisoned(spec);
        bad.url = format!("{}&host={{{{host}}}}&p={{{{password}}}}", bad.url);
        match build(&spec.name, &bad, Some(&environment), &o) {
            Ok(link) => {
                assert_no_canary(&link.text, "the link");
                assert_no_canary(&link.preview, "the preview");
                let decoded = share::decode(&link.text).unwrap();
                assert_no_canary(&serde_json::to_string(&decoded).unwrap(), "the payload");
                // The names are there for the recipient to fill in.
                assert!(
                    link.payload
                        .variables
                        .iter()
                        .any(|v| v.name == "password" && v.value.is_none())
                );
                assert!(
                    link.payload
                        .variables
                        .iter()
                        .any(|v| v.name == "host" && v.value.as_deref() == Some("h.test"))
                );
            }
            // The corpus request itself may hold something the scanner refuses.
            Err(ShareError::SecretsFound(m)) => assert_no_canary(&m, "the refusal"),
            Err(ShareError::TooLong(_)) => {}
            Err(e) => panic!("{}: {e}", spec.name),
        }
    }
    assert!(
        roundtrips > 100,
        "only {roundtrips} requests made a clean round trip"
    );
    // The corpus is mostly clean; a handful are refused, not dozens.
    assert!(
        refused < corpus.len() / 10,
        "{refused} of {} refused",
        corpus.len()
    );
}

#[test]
fn the_secret_rule_in_each_place() {
    let link = build("x", &poisoned(&sample()), Some(&poisoned_env()), &opts()).unwrap();
    let p = &link.payload.request;
    assert_eq!(
        p.headers
            .iter()
            .find(|h| h.name == "Authorization" && h.value.starts_with("Bearer {{token"))
            .map(|h| h.value.as_str()),
        Some("Bearer {{token}}")
    );
    let get = |n: &str| {
        p.headers
            .iter()
            .find(|h| h.name == n)
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(get("Cookie"), "{{secret:cookie}}");
    assert_eq!(get("X-Api-Key"), "{{secret:x_api_key}}");
    assert_eq!(get("Proxy-Authorization"), "{{secret:proxy_authorization}}");
    assert_eq!(
        p.url,
        "https://user:{{secret:password}}@api.test/items?api_key={{secret:api_key}}&page=1"
    );
    assert_eq!(
        p.auth,
        Auth::Basic {
            username: "ada".into(),
            password: "{{secret:password}}".into()
        }
    );
    let Body::Raw { data, .. } = &p.body else {
        panic!()
    };
    assert_eq!(
        data,
        "{\"name\":\"n\",\"password\":\"{{secret:password}}\",\"nested\":{\"client_secret\": \"{{secret:client_secret}}\"}}"
    );
    let var = p.vars.iter().find(|v| v.name == "api_token").unwrap();
    assert!(var.secret && var.value.is_none());
    // An auth block that is only a template stays as it is.
    let mut s = sample();
    s.auth = Auth::Bearer {
        token: "{{secret:mine}}".into(),
    };
    let link = build("x", &s, None, &opts()).unwrap();
    assert_eq!(link.payload.request.auth, s.auth);
    assert!(
        link.payload
            .variables
            .iter()
            .any(|v| v.name == "mine" && v.secret)
    );
}

#[test]
fn the_scanner_refuses_what_the_rules_do_not_replace() {
    let mut s = sample();
    // A token in a script is not a header or a key of a body; the scanner finds its format.
    s.scripts.push(rt_core::model::Script {
        phase: rt_core::model::ScriptPhase::BeforeRequest,
        code: "const t = 'ghp_0123456789abcdefghijklmnopqrstuvwxyz';".into(),
    });
    match build("x", &s, None, &opts()) {
        Err(ShareError::SecretsFound(m)) => assert!(!m.contains("ghp_0123456789"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(matches!(
        AppError::from(ShareError::SecretsFound("x".into())),
        AppError::SecretsFound(_)
    ));
}

#[test]
fn a_large_request_gets_a_hint_instead_of_a_link() {
    let mut s = sample();
    // Incompressible text: it will not fit.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let noise: String = (0..30_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            char::from(b'a' + (x % 26) as u8)
        })
        .collect();
    s.body = Body::Raw {
        kind: RawKind::Text,
        data: noise,
    };
    let e = build("x", &s, None, &opts()).unwrap_err();
    let ShareError::TooLong(n) = e else {
        panic!("{e:?}")
    };
    assert!(n > MAX_LINK_LEN);
    assert!(e.to_string().contains("save it as a file"));
    // A request between the two limits has a link but no QR code.
    let mut s = sample();
    let mut x: u64 = 0x1234_5678_9ABC_DEF1;
    let noise: String = (0..2_400)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            char::from(b'a' + (x % 26) as u8)
        })
        .collect();
    s.body = Body::Raw {
        kind: RawKind::Text,
        data: noise,
    };
    let link = build("x", &s, None, &opts()).unwrap();
    assert!(
        link.length > MAX_QR_LEN && link.length < MAX_LINK_LEN,
        "{}",
        link.length
    );
    assert!(!link.fits_qr);
    let hint = share::qr(&link.text).unwrap_err();
    assert!(
        hint.to_string().contains("too long for a QR code"),
        "{hint}"
    );
}

#[test]
fn a_qr_code_is_an_svg() {
    let link = build("x", &sample(), None, &opts()).unwrap();
    assert!(link.fits_qr);
    let svg = share::qr(&link.text).unwrap();
    assert!(svg.contains("<svg"), "{svg}");
    assert!(svg.len() > 500);
}

#[test]
fn a_workspace_gives_the_request_as_edited_and_the_chosen_environment() {
    let files = FileSet {
        files: [
            ("opencollection.yml", "opencollection: 1.0.0\ninfo:\n  name: Shop\n"),
            ("environments/Dev.yml", "name: Dev\nvariables:\n  - name: baseUrl\n    value: https://api.dev.test\n  - name: token\n    value: ''\n    secret: true\n"),
            ("items.yml", "info:\n  name: Items\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: '{{baseUrl}}/items'\n"),
        ]
        .into_iter()
        .map(|(p, t)| (p.to_owned(), t.as_bytes().to_vec()))
        .collect(),
    };
    let mut ws = Workspace::open(files).unwrap();
    let mut spec = ws.request("items.yml").unwrap();
    spec.url = "{{baseUrl}}/items?limit=5".into();
    ws.update_request("items.yml", spec).unwrap();
    let o = ShareOptions {
        include_values: true,
        environment: Some("Dev".into()),
        ..opts()
    };
    let link = share::link(&ws, "items.yml", &o).unwrap();
    assert_eq!(link.payload.request.url, "{{baseUrl}}/items?limit=5");
    assert_eq!(
        link.payload.variables[0].value.as_deref(),
        Some("https://api.dev.test")
    );
    assert!(matches!(
        share::link(&ws, "nope.yml", &opts()),
        Err(AppError::RequestNotFound(_))
    ));
    let o = ShareOptions {
        environment: Some("Nope".into()),
        ..opts()
    };
    assert!(matches!(
        share::link(&ws, "items.yml", &o),
        Err(AppError::EnvironmentNotFound(_))
    ));
}
