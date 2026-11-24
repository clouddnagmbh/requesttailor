// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand E: the request tab `abap` ("Copy as ABAP"). Golden files in
//! `testdata/abap/<case>.<flavor>.abap` for read (`$filter`, `$expand`), create, update with
//! ETag, delete and a read-only `$batch`, in both flavors (classic `cl_http_client`, cloud
//! `cl_web_http_client_manager`). There is no ABAP system in the tests: the snippets are checked
//! against the golden files, by a structure check (statements end, quotes and parentheses
//! balance, line length), and by what must never be in them (secrets, CSRF values, logon data).
//! The read requests are also sent to the OData test server, so the goldens describe requests
//! that work. Update the golden files with `RT_UPDATE_GOLDEN=1 cargo test -p rt-plugin-odata
//! --test abap` and review the diff.

use std::path::{Path, PathBuf};

use rt_odata_testserver::client::{self, TestServer};
use rt_plugin_odata::OData;
use rt_plugin_odata::abap::{self, Flavor};
use rt_plugin_sdk::api::{Block, Call, FieldKind, PluginContext, TabTarget, ViewReply};
use rt_plugin_sdk::model::{Auth, Body, KeyValue, RawKind, RequestSpec};
use rt_plugin_sdk::{NoteLevel, testing};

const CANARY: &str = "RT-CANARY-7f3a91";
const GW: &str = "/sap/opu/odata/sap/ZRT_SALES_SRV";
const BASE: &str = "https://s4.example.test:44300";

fn base_vars() -> Vec<(String, String)> {
    vec![("baseUrl".into(), BASE.into())]
}

fn spec(name: &str, method: &str, url: &str) -> RequestSpec {
    RequestSpec {
        name: name.into(),
        method: method.into(),
        url: url.into(),
        ..RequestSpec::default()
    }
}

fn json_body(spec: &mut RequestSpec, text: &str) {
    spec.body = Body::Raw {
        kind: RawKind::Json,
        data: text.into(),
    };
}

/// Headers and auth that carry secrets and must never reach the snippet.
fn with_secrets(mut spec: RequestSpec) -> RequestSpec {
    spec.headers
        .push(KeyValue::new("Authorization", format!("Bearer {CANARY}")));
    spec.headers
        .push(KeyValue::new("Cookie", format!("SAP_SESSIONID_X={CANARY}")));
    spec.headers.push(KeyValue::new("X-CSRF-Token", CANARY));
    spec.headers.push(KeyValue::new("X-Api-Key", CANARY));
    spec.auth = Auth::Basic {
        username: "TESTUSER".into(),
        password: CANARY.into(),
    };
    spec
}

struct Case {
    name: &'static str,
    spec: RequestSpec,
}

fn cases() -> Vec<Case> {
    let mut read = spec(
        "Open orders with items",
        "GET",
        "{{baseUrl}}/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet?$filter=GrossAmount%20gt%20100M\
         &$expand=ToItems&$top=5&$select=SalesOrderID,CustomerName,GrossAmount&sap-client=100",
    );
    read.headers
        .push(KeyValue::new("Accept", "application/json"));
    let read = with_secrets(read);

    let mut create = spec(
        "Create sales order",
        "POST",
        "{{baseUrl}}/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet",
    );
    json_body(
        &mut create,
        r#"{"CustomerName":"Acme Corp","GrossAmount":"250.00","CurrencyCode":"EUR"}"#,
    );
    let create = with_secrets(create);

    let mut update = spec(
        "Change customer name",
        "PATCH",
        "{{baseUrl}}/odata/v4/admin/Books(ID=1,IsActiveEntity=true)",
    );
    json_body(
        &mut update,
        "{\n  \"title\": \"Wuthering Heights\",\n  \"stock\": 12\n}",
    );
    let update = with_secrets(update);

    let delete = with_secrets(spec(
        "Delete sales order",
        "DELETE",
        "{{baseUrl}}/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet('0500000003')",
    ));

    let mut batch = spec(
        "Two reads in a batch",
        "POST",
        "{{baseUrl}}/sap/opu/odata/sap/ZRT_SALES_SRV/$batch",
    );
    batch.headers.push(KeyValue::new(
        "Content-Type",
        "multipart/mixed; boundary=batch_rt1",
    ));
    batch.body = Body::Raw {
        kind: RawKind::Text,
        data: batch_body(&["SalesOrderSet('0500000001')", "SalesOrderSet?$top=2"]),
    };
    let batch = with_secrets(batch);

    vec![
        Case {
            name: "read",
            spec: read,
        },
        Case {
            name: "create",
            spec: create,
        },
        Case {
            name: "update",
            spec: update,
        },
        Case {
            name: "delete",
            spec: delete,
        },
        Case {
            name: "batch-read",
            spec: batch,
        },
    ]
}

fn batch_body(reads: &[&str]) -> String {
    let mut body = String::new();
    for read in reads {
        body.push_str(&format!(
            "--batch_rt1\r\nContent-Type: application/http\r\nContent-Transfer-Encoding: binary\r\n\r\n\
             GET {read} HTTP/1.1\r\nAccept: application/json\r\n\r\n\r\n"
        ));
    }
    body.push_str("--batch_rt1--\r\n");
    body
}

fn golden_path(case: &str, flavor: Flavor) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/abap")
        .join(format!("{case}.{}.abap", flavor.id()))
}

#[test]
fn golden_files() {
    let update = std::env::var_os("RT_UPDATE_GOLDEN").is_some();
    for case in cases() {
        for flavor in [Flavor::Classic, Flavor::Cloud] {
            let snippet = abap::generate(&case.spec, &base_vars(), flavor)
                .unwrap_or_else(|e| panic!("{} {flavor:?}: {e}", case.name));
            let path = golden_path(case.name, flavor);
            if update {
                std::fs::write(&path, &snippet.code).unwrap();
            }
            let golden = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("missing golden file {}", path.display()));
            assert_eq!(
                snippet.code,
                golden,
                "{} {flavor:?} differs from {}",
                case.name,
                path.display()
            );
        }
    }
}

#[test]
fn snippets_are_well_formed_and_hold_no_secret() {
    for case in cases() {
        for flavor in [Flavor::Classic, Flavor::Cloud] {
            let snippet = abap::generate(&case.spec, &base_vars(), flavor).unwrap();
            let code = &snippet.code;
            let label = format!("{} {flavor:?}", case.name);
            // No secret, no token, no logon data anywhere (also not in the notes).
            for text in std::iter::once(code).chain(&snippet.notes) {
                for forbidden in [CANARY, "TESTUSER", "Bearer", "SAP_SESSIONID"] {
                    assert!(!text.contains(forbidden), "{label}: {forbidden} in output");
                }
            }
            // A CSRF token is fetched at run time, with the protocol keyword as the only value.
            for line in code.lines() {
                if line.to_ascii_lowercase().contains("x-csrf-token") && line.contains("value") {
                    assert!(
                        line.to_ascii_lowercase().contains("`fetch`") || line.contains("lv_csrf"),
                        "{label}: CSRF value in `{line}`"
                    );
                }
            }
            check_structure(code, &label);
        }
    }
}

/// Statements end with `.`, strings and parentheses balance, lines stay short, no tabs.
fn check_structure(code: &str, label: &str) {
    let mut statement = String::new();
    for (n, line) in code.lines().enumerate() {
        assert!(
            line.chars().count() <= 100,
            "{label}: line {} too long",
            n + 1
        );
        assert!(!line.contains('\t'), "{label}: tab in line {}", n + 1);
        assert!(
            !line.ends_with(' '),
            "{label}: trailing space in line {}",
            n + 1
        );
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('"') || trimmed.starts_with('*') {
            assert!(
                statement.is_empty(),
                "{label}: comment inside statement, line {}",
                n + 1
            );
            continue;
        }
        statement.push_str(&strip_comment(trimmed));
        statement.push(' ');
        if statement.trim_end().ends_with('.') {
            assert_eq!(
                statement.matches('`').count() % 2,
                0,
                "{label}: backticks in `{statement}`"
            );
            assert_eq!(
                statement.matches('(').count(),
                statement.matches(')').count(),
                "{label}: parentheses in `{statement}`"
            );
            statement.clear();
        }
    }
    assert!(
        statement.is_empty(),
        "{label}: the last statement does not end: {statement}"
    );
}

/// The line without a trailing `" comment` (outside strings).
fn strip_comment(line: &str) -> String {
    let mut in_tick = false;
    let mut in_quote = false;
    for (i, c) in line.char_indices() {
        match c {
            '`' if !in_quote => in_tick = !in_tick,
            '\'' if !in_tick => in_quote = !in_quote,
            '"' if !in_tick && !in_quote => return line[..i].trim_end().to_owned(),
            _ => {}
        }
    }
    line.to_owned()
}

#[test]
fn what_the_flavors_use() {
    let all = cases();
    let find = |name: &str| &all.iter().find(|c| c.name == name).unwrap().spec;
    let classic = |name: &str| {
        abap::generate(find(name), &base_vars(), Flavor::Classic)
            .unwrap()
            .code
    };
    let cloud = |name: &str| {
        abap::generate(find(name), &base_vars(), Flavor::Cloud)
            .unwrap()
            .code
    };

    let read = classic("read");
    assert!(read.contains("cl_http_client=>create_by_destination"));
    assert!(read.contains("/ui2/cl_json=>deserialize"));
    assert!(!read.contains("x-csrf-token"), "reads need no token");
    assert!(read.contains("sap-client=100"));
    let read_cloud = cloud("read");
    assert!(read_cloud.contains("cl_web_http_client_manager=>create_by_http_destination"));
    assert!(read_cloud.contains("/ui2/cl_json=>deserialize"));

    for name in ["create", "update", "delete"] {
        for code in [classic(name), cloud(name)] {
            assert!(
                code.contains("x-csrf-token") && code.contains("`fetch`"),
                "{name}: token fetch"
            );
            assert!(code.contains("lv_csrf"), "{name}: token variable");
        }
    }
    // An update and a delete need the ETag of the entity; a create does not.
    assert!(classic("update").contains("If-Match"));
    assert!(classic("delete").contains("If-Match"));
    assert!(!classic("create").contains("If-Match"));
    // Placeholders for what only the user knows.
    assert!(classic("create").contains("<destination>"));
    assert!(cloud("create").contains("<communication scenario>"));
    // The omitted headers are listed by name only.
    let notes = abap::generate(find("create"), &base_vars(), Flavor::Classic)
        .unwrap()
        .notes;
    let notes = notes.join("\n");
    assert!(
        notes.contains("Authorization") && notes.contains("Cookie") && notes.contains("X-Api-Key")
    );
}

#[test]
fn unresolved_variables_become_placeholders() {
    let mut s = spec("x", "GET", "{{host}}/odata/v4/admin/Books?$top={{limit}}");
    s.headers.push(KeyValue::new("X-Env", "{{secretThing}}"));
    let snippet = abap::generate(&s, &[("limit".into(), "3".into())], Flavor::Classic).unwrap();
    assert!(snippet.code.contains("/odata/v4/admin/Books?$top=3"));
    assert!(snippet.code.contains("<secretThing>"));
    assert!(snippet.notes.iter().any(|n| n.contains("secretThing")));
    assert!(!snippet.code.contains("{{"));
}

#[test]
fn secret_looking_query_parameters_are_not_copied() {
    let s = spec(
        "x",
        "GET",
        &format!("{BASE}/odata/v4/admin/Books?api_key={CANARY}&$top=1"),
    );
    let snippet = abap::generate(&s, &[], Flavor::Classic).unwrap();
    assert!(!snippet.code.contains(CANARY));
    assert!(snippet.code.contains("$top=1"));
    assert!(snippet.code.contains("api_key=<api_key>"));
}

#[test]
fn what_is_not_available() {
    // A batch with a changeset, GraphQL and a form body are refused with a reason.
    let mut changeset = spec("b", "POST", &format!("{BASE}{GW}/$batch"));
    changeset.headers.push(KeyValue::new(
        "Content-Type",
        "multipart/mixed; boundary=b1",
    ));
    changeset.body = Body::Raw {
        kind: RawKind::Text,
        data: "--b1\r\nContent-Type: multipart/mixed; boundary=c1\r\n\r\n--c1\r\n\
               Content-Type: application/http\r\nContent-Transfer-Encoding: binary\r\n\r\n\
               POST SalesOrderSet HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}\r\n--c1--\r\n--b1--\r\n"
            .into(),
    };
    let err = abap::generate(&changeset, &[], Flavor::Classic).unwrap_err();
    assert!(err.to_string().contains("batch"), "{err}");

    let mut form = spec("f", "POST", &format!("{BASE}{GW}/SalesOrderSet"));
    form.body = Body::FormUrlEncoded {
        fields: vec![KeyValue::new("a", "b")],
    };
    assert!(abap::generate(&form, &[], Flavor::Classic).is_err());

    let mut graphql = spec("g", "POST", &format!("{BASE}/graphql"));
    graphql.body = Body::GraphQl {
        query: "{ a }".into(),
        variables: String::new(),
    };
    assert!(abap::generate(&graphql, &[], Flavor::Cloud).is_err());
}

#[test]
fn the_tab_shows_the_snippet_and_switches_flavor() {
    let case = cases().into_iter().find(|c| c.name == "create").unwrap();
    let target = TabTarget {
        request_path: "orders/create.yml".into(),
        response_id: None,
    };
    let context = PluginContext {
        request_path: "orders/create.yml".into(),
        environment: Some("DEV".into()),
        variables: base_vars(),
    };
    let render = |state: Option<serde_json::Value>| -> ViewReply {
        testing::call(
            &OData,
            &Call::RenderTab {
                tab: "abap".into(),
                target: target.clone(),
                spec: case.spec.clone(),
                response: None,
                state,
                context: Some(context.clone()),
            },
        )
        .unwrap()
    };
    let code_of = |reply: &ViewReply| {
        reply.view.blocks.iter().find_map(|b| match b {
            Block::Code { language, text } => Some((language.clone(), text.clone())),
            _ => None,
        })
    };

    // The S/4 URL is on-premise, so the classic flavor is the default.
    let first = render(None);
    let (language, text) = code_of(&first).expect("a code block");
    assert_eq!(language.as_deref(), Some("abap"));
    assert_eq!(
        text,
        abap::generate(&case.spec, &base_vars(), Flavor::Classic)
            .unwrap()
            .code
    );

    // The form chooses the flavor; the choice is kept in the tab state.
    let form = first.view.blocks.iter().find_map(|b| match b {
        Block::Form { id, fields, .. } => Some((id.clone(), fields.clone())),
        _ => None,
    });
    let (id, fields) = form.expect("a form");
    assert_eq!(fields[0].kind, FieldKind::Select);
    let reply: ViewReply = testing::call(
        &OData,
        &Call::Action {
            tab: "abap".into(),
            action: id,
            target,
            spec: case.spec.clone(),
            response: None,
            state: first.state.clone(),
            input: vec![(fields[0].name.clone(), fields[0].options[1].clone())],
            context: Some(context),
        },
    )
    .unwrap();
    let (_, cloud) = code_of(&reply).unwrap();
    assert_eq!(
        cloud,
        abap::generate(&case.spec, &base_vars(), Flavor::Cloud)
            .unwrap()
            .code
    );
    assert!(
        reply.update_request.is_none(),
        "the tab never changes the request"
    );

    // Notes about omitted headers are warnings or infos, never errors.
    assert!(reply.view.blocks.iter().all(|b| !matches!(
        b,
        Block::Notice {
            level: NoteLevel::Error,
            ..
        }
    )));
}

#[test]
fn the_read_requests_of_the_goldens_work_against_the_test_server() {
    let server = TestServer::start();
    let url_of = |case: &Case| {
        let vars = vec![("baseUrl".to_owned(), format!("http://{}", server.addr))];
        let mut url = case.spec.url.clone();
        for (n, v) in &vars {
            url = url.replace(&format!("{{{{{n}}}}}"), v);
        }
        url
    };
    let headers = vec![
        ("Authorization".to_owned(), client::s4_basic_auth()),
        ("Accept".to_owned(), "application/json".to_owned()),
    ];
    for case in cases().iter().filter(|c| c.name == "read") {
        let r = client::send("GET", &url_of(case), &headers, b"").unwrap();
        assert_eq!(r.status, 200, "{}: {}", case.name, r.text());
    }

    // The read-only batch of the golden file.
    let batch = cases()
        .into_iter()
        .find(|c| c.name == "batch-read")
        .unwrap();
    let Body::Raw { data, .. } = &batch.spec.body else {
        unreachable!()
    };
    // As the snippet does: fetch the CSRF token on the service root, then POST in the session.
    let mut jar = client::CookieJar::default();
    let root = format!("http://{}{GW}/", server.addr);
    let mut fetch = headers.clone();
    fetch.push(("x-csrf-token".into(), "fetch".into()));
    let r = client::send("GET", &root, &fetch, b"").unwrap();
    jar.store(&r.headers);
    let token = r.header("x-csrf-token").expect("a token").to_owned();
    let mut h = headers.clone();
    h.push((
        "Content-Type".into(),
        "multipart/mixed; boundary=batch_rt1".into(),
    ));
    h.push(("x-csrf-token".into(), token));
    h.extend(jar.header().map(|c| ("Cookie".to_owned(), c)));
    let r = client::send("POST", &url_of(&batch), &h, data.as_bytes()).unwrap();
    assert_eq!(r.status, 202, "{}", r.text());
    assert!(r.text().contains("HTTP/1.1 200"), "{}", r.text());
}

/// The core knows no SAP: "ABAP" appears nowhere outside `plugins/odata` (and this check).
#[test]
fn the_core_has_no_abap() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut hits = Vec::new();
    for dir in [
        "crates",
        "apps/ui/src",
        "apps/web/src",
        "apps/desktop/src",
        "apps/desktop/src-tauri/src",
    ] {
        scan(&root.join(dir), &mut hits);
    }
    assert!(hits.is_empty(), "ABAP in the core: {hits:?}");
}

fn scan(dir: &Path, hits: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(
                name.as_str(),
                "node_modules" | "target" | "dist" | "wasm" | "fixtures"
            ) {
                scan(&path, hits);
            }
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "ts" | "svelte" | "md" | "json")
        ) && std::fs::read_to_string(&path)
            .is_ok_and(|t| t.to_ascii_lowercase().contains("abap"))
        {
            hits.push(path.display().to_string());
        }
    }
}
