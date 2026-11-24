// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand C: copying a request as curl, `fetch` or Python. Variables stay variables of
//! the target tool, secrets are never put in, and where `curl`, `node` or `python3` exist the
//! snippet is run against `rt-testserver` and must send what `rt send` sends.

use rt_app::share::{CopyKind, CopyOptions, ShareError, copy_spec};
use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, GrpcSpec, KeyValue, MessageVariant, MultipartPart,
    PartKind, Protocol, RawKind, RequestSpec, Variable,
};

fn json_post() -> RequestSpec {
    RequestSpec {
        name: "Create".into(),
        method: "POST".into(),
        url: "{{baseUrl}}/echo/items?x=1&y=two words".into(),
        headers: vec![
            KeyValue::new("X-Trace", "abc"),
            KeyValue {
                name: "X-Off".into(),
                value: "no".into(),
                enabled: false,
            },
        ],
        body: Body::Raw {
            kind: RawKind::Json,
            data: "{\"name\":\"it's\",\"owner\":\"{{owner}}\"}".into(),
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
            Variable::new("owner", "ada"),
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

fn copy(spec: &RequestSpec, kind: CopyKind) -> String {
    copy_spec(spec, kind, Some(&env()), &CopyOptions::default())
        .unwrap()
        .text
}

#[test]
fn curl_keeps_variables_as_environment_variables() {
    let text = copy(&json_post(), CopyKind::Curl);
    assert_eq!(
        text,
        "curl -X POST \"$BASE_URL/echo/items?x=1&y=two words\" \\\n  \
         -H 'X-Trace: abc' \\\n  \
         -H \"Authorization: Bearer $TOKEN\" \\\n  \
         -H 'Content-Type: application/json' \\\n  \
         --data-raw \"{\\\"name\\\":\\\"it's\\\",\\\"owner\\\":\\\"$OWNER\\\"}\""
    );
}

#[test]
fn fetch_keeps_variables_as_environment_variables() {
    let text = copy(&json_post(), CopyKind::Fetch);
    assert_eq!(
        text,
        "const response = await fetch(`${process.env.BASE_URL}/echo/items?x=1&y=two words`, {\n  \
         method: \"POST\",\n  \
         headers: {\n    \
         \"X-Trace\": \"abc\",\n    \
         \"Authorization\": `Bearer ${process.env.TOKEN}`,\n    \
         \"Content-Type\": \"application/json\",\n  },\n  \
         body: `{\"name\":\"it's\",\"owner\":\"${process.env.OWNER}\"}`,\n\
         });\nconsole.log(response.status);\nconsole.log(await response.text());\n"
    );
}

#[test]
fn python_keeps_variables_as_environment_variables() {
    let text = copy(&json_post(), CopyKind::Python);
    assert_eq!(
        text,
        "import os\nimport requests\n\nresponse = requests.post(\n    \
         os.environ[\"BASE_URL\"] + \"/echo/items?x=1&y=two words\",\n    \
         headers={\n        \"X-Trace\": \"abc\",\n        \
         \"Authorization\": \"Bearer \" + os.environ[\"TOKEN\"],\n        \
         \"Content-Type\": \"application/json\",\n    },\n    \
         data=\"{\\\"name\\\":\\\"it's\\\",\\\"owner\\\":\\\"\" + os.environ[\"OWNER\"] + \"\\\"}\",\n)\n\
         print(response.status_code)\nprint(response.text)\n"
    );
}

#[test]
fn resolving_puts_in_non_secret_values_and_never_a_secret() {
    let o = CopyOptions {
        resolve_variables: true,
        environment: Some("Dev".into()),
    };
    for kind in [CopyKind::Curl, CopyKind::Fetch, CopyKind::Python] {
        let c = copy_spec(&json_post(), kind, Some(&env()), &o).unwrap();
        assert!(
            c.text.contains("https://api.dev.test/echo/items"),
            "{}",
            c.text
        );
        assert!(c.text.contains("ada"), "{}", c.text);
        assert!(
            c.text.to_uppercase().contains("TOKEN"),
            "the token stays a variable: {}",
            c.text
        );
        assert!(
            !c.text.contains("BASE_URL") && !c.text.contains("OWNER"),
            "{}",
            c.text
        );
        assert!(c.notes.iter().any(|n| n.contains("TOKEN")), "{:?}", c.notes);
    }
}

#[test]
fn a_value_that_cannot_be_resolved_stays_a_variable_even_when_asked() {
    // A "plain" variable named like a credential is not put in either.
    let mut e = env();
    e.variables
        .push(Variable::new("api_key", "RT-CANARY-visible"));
    let mut s = json_post();
    s.url = "{{baseUrl}}/x?k={{api_key}}".into();
    let o = CopyOptions {
        resolve_variables: true,
        environment: Some("Dev".into()),
    };
    let c = copy_spec(&s, CopyKind::Curl, Some(&e), &o).unwrap();
    assert!(!c.text.contains("RT-CANARY"), "{}", c.text);
    assert!(c.text.contains("$API_KEY"), "{}", c.text);
}

#[test]
fn a_literal_credential_is_never_in_a_snippet() {
    let mut s = json_post();
    s.headers
        .push(KeyValue::new("Authorization", "Bearer RT-CANARY-1"));
    s.headers.push(KeyValue::new("X-Api-Key", "RT-CANARY-2"));
    s.auth = Auth::Basic {
        username: "ada".into(),
        password: "RT-CANARY-3".into(),
    };
    s.url = "https://api.test/x?access_token=RT-CANARY-4".into();
    s.body = Body::Raw {
        kind: RawKind::Json,
        data: "{\"password\":\"RT-CANARY-5\"}".into(),
    };
    for kind in [CopyKind::Curl, CopyKind::Fetch, CopyKind::Python] {
        let c = copy_spec(&s, kind, None, &CopyOptions::default()).unwrap();
        assert!(!c.text.contains("RT-CANARY"), "{kind:?}: {}", c.text);
        assert!(
            c.text.contains("X_API_KEY") && c.text.contains("PASSWORD"),
            "{kind:?}: {}",
            c.text
        );
        assert!(
            c.notes.iter().any(|n| n.contains("replaced")),
            "{:?}",
            c.notes
        );
    }
}

#[test]
fn every_kind_of_body_is_written() {
    let mut s = json_post();
    s.url = "https://x.test/a".into();
    s.auth = Auth::None;
    s.headers.clear();
    s.body = Body::FormUrlEncoded {
        fields: vec![KeyValue::new("a", "1"), KeyValue::new("b", "{{b}}")],
    };
    let c = copy(&s, CopyKind::Curl);
    assert!(
        c.contains("--data-urlencode 'a=1'") && c.contains("--data-urlencode \"b=$B\""),
        "{c}"
    );
    assert!(
        copy(&s, CopyKind::Fetch)
            .contains("new URLSearchParams([[\"a\", \"1\"], [\"b\", `${process.env.B}`]])")
    );
    assert!(
        copy(&s, CopyKind::Python).contains("data=[(\"a\", \"1\"), (\"b\", os.environ[\"B\"])]")
    );

    s.body = Body::Multipart {
        parts: vec![
            MultipartPart {
                name: "title".into(),
                kind: PartKind::Text,
                values: vec!["Hi".into()],
                content_type: None,
                enabled: true,
            },
            MultipartPart {
                name: "file".into(),
                kind: PartKind::File,
                values: vec!["files/a.png".into()],
                content_type: None,
                enabled: true,
            },
        ],
    };
    let c = copy(&s, CopyKind::Curl);
    assert!(
        c.contains("-F 'title=Hi'") && c.contains("-F 'file=@files/a.png'"),
        "{c}"
    );
    let c = copy(&s, CopyKind::Fetch);
    assert!(
        c.contains("form.append(\"title\", \"Hi\")") && c.contains("readFile(\"files/a.png\")"),
        "{c}"
    );
    assert!(copy(&s, CopyKind::Python).contains(
        "files=[(\"title\", (None, \"Hi\")), (\"file\", open(\"files/a.png\", \"rb\"))]"
    ));

    s.body = Body::GraphQl {
        query: "query { me { id } }".into(),
        variables: "{\"id\": 1}".into(),
    };
    let c = copy(&s, CopyKind::Curl);
    assert!(
        c.contains("Content-Type: application/json")
            && c.contains("\"query\":\"query { me { id } }\""),
        "{c}"
    );

    s.body = Body::None;
    s.method = "GET".into();
    assert_eq!(copy(&s, CopyKind::Curl), "curl 'https://x.test/a'");
    s.method = "DELETE".into();
    assert!(copy(&s, CopyKind::Python).contains("requests.delete("));
    s.method = "PURGE".into();
    assert!(copy(&s, CopyKind::Python).contains("requests.request(\"PURGE\", "));
}

#[test]
fn auth_kinds_and_path_parameters() {
    let mut s = RequestSpec {
        method: "GET".into(),
        url: "https://x.test/users/:id".into(),
        params: vec![rt_core::model::Param {
            name: "id".into(),
            value: "7".into(),
            kind: rt_core::model::ParamKind::Path,
            enabled: true,
        }],
        auth: Auth::Basic {
            username: "ada".into(),
            password: "{{password}}".into(),
        },
        ..RequestSpec::default()
    };
    let c = copy(&s, CopyKind::Curl);
    assert!(
        c.contains("'https://x.test/users/7'") && c.contains("-u \"ada:$PASSWORD\""),
        "{c}"
    );
    assert!(copy(&s, CopyKind::Python).contains("auth=(\"ada\", os.environ[\"PASSWORD\"])"));
    assert!(copy(&s, CopyKind::Fetch).contains("Buffer.from(`ada:${process.env.PASSWORD}`)"));
    s.auth = Auth::ApiKey {
        key: "api_key".into(),
        value: "{{key}}".into(),
        placement: ApiKeyPlacement::Query,
    };
    assert!(copy(&s, CopyKind::Curl).contains("users/7?api_key=$KEY"));
    s.auth = Auth::Inherit;
    let notes = copy_spec(&s, CopyKind::Curl, None, &CopyOptions::default())
        .unwrap()
        .notes;
    assert!(notes.iter().any(|n| n.contains("inherited")), "{notes:?}");
}

#[test]
fn what_a_kind_cannot_say_is_not_available() {
    let grpc = RequestSpec {
        method: "GRPC".into(),
        url: "grpc://localhost:50051".into(),
        protocol: Protocol::Grpc(Box::new(GrpcSpec {
            method: "/pkg.Greeter/SayHello".into(),
            messages: vec![MessageVariant {
                data: "{\"name\":\"x\"}".into(),
                selected: true,
                ..MessageVariant::default()
            }],
            ..GrpcSpec::default()
        })),
        ..RequestSpec::default()
    };
    let c = copy_spec(&grpc, CopyKind::Curl, None, &CopyOptions::default()).unwrap();
    assert!(
        c.text
            .starts_with("curl --http2 -X POST 'http://localhost:50051/pkg.Greeter/SayHello'"),
        "{}",
        c.text
    );
    assert!(c.text.contains("application/grpc"));
    assert!(!c.notes.is_empty());
    for kind in [CopyKind::Fetch, CopyKind::Python] {
        let e = copy_spec(&grpc, kind, None, &CopyOptions::default()).unwrap_err();
        assert_eq!(
            e,
            ShareError::Unavailable("not available for this request".into())
        );
    }
    let ws = RequestSpec {
        method: "WS".into(),
        url: "ws://x.test/echo".into(),
        protocol: Protocol::WebSocket(Box::default()),
        ..RequestSpec::default()
    };
    for kind in [CopyKind::Curl, CopyKind::Fetch, CopyKind::Python] {
        let e = copy_spec(&ws, kind, None, &CopyOptions::default()).unwrap_err();
        assert!(e.to_string().contains("not available for this request"));
    }
}

// ---- run the snippets against rt-testserver ----

#[cfg(not(target_arch = "wasm32"))]
mod running {
    use std::process::Command;

    use rt_app::{SendInput, Workspace};
    use rt_core::send::{ReportBody, SendOptions};
    use rt_core::transport::native::NativeTransport;
    use rt_format::FileSet;

    use super::*;

    fn available(program: &str, args: &[&str]) -> bool {
        Command::new(program)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// What the server saw, reduced to what must be the same however it was sent.
    fn essence(echo: &serde_json::Value) -> serde_json::Value {
        let header = |name: &str| {
            echo["headers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|h| h[0] == name)
                .map(|h| h[1].clone())
                .unwrap_or(serde_json::Value::Null)
        };
        serde_json::json!({
            "method": echo["method"],
            "path": echo["path"],
            "query": echo["query"],
            "body": echo["body"],
            "authorization": header("authorization"),
            "x-trace": header("x-trace"),
            "content-type": header("content-type"),
        })
    }

    fn echo_of_report(report: &rt_core::send::SendReport) -> serde_json::Value {
        let response = report
            .response
            .as_ref()
            .unwrap_or_else(|| panic!("{:?}", report.error));
        let ReportBody::Utf8(body) = &response.body else {
            panic!("binary echo")
        };
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn the_snippets_send_what_rt_send_sends() {
        let curl = available("curl", &["--version"]);
        let node = available("node", &["--version"]);
        let python = available("python3", &["-c", "import requests"]);
        if !(curl || node || python) {
            eprintln!("skipped: none of curl, node, python3 with requests is available");
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async {
            let port = rt_testserver::spawn_local().await.port();
            let base = format!("http://127.0.0.1:{port}");
            let mut spec = json_post();
            // A body without the apostrophe keeps the comparison about the wire, not about shells.
            spec.body = Body::Raw {
                kind: RawKind::Json,
                data: "{\"name\":\"a b\",\"owner\":\"{{owner}}\"}".into(),
            };
            spec.url = "{{baseUrl}}/echo/items?x=1&y=two".into();
            // rt send, with the same values the snippets get from the environment.
            let files = FileSet {
                files: [(
                    "opencollection.yml",
                    "opencollection: 1.0.0\ninfo:\n  name: T\n",
                )]
                .into_iter()
                .map(|(p, t)| (p.to_owned(), t.as_bytes().to_vec()))
                .collect(),
            };
            let mut ws = Workspace::open(files).unwrap();
            let path = ws.create_request("", "Create").unwrap();
            ws.update_request(&path, spec.clone()).unwrap();
            let mut e = env();
            e.variables[0].value = Some(base.clone());
            ws.update_environment(e.clone()).unwrap();
            let input = SendInput {
                environment: Some("Dev".into()),
                vars: Vec::new(),
                secrets: vec![("token".into(), "tok-123".into())],
            };
            let snapshot = ws.snapshot(&path, input).unwrap();
            let transport = NativeTransport::new().unwrap();
            let report = snapshot.send(&transport, &SendOptions::default()).await;
            let want = essence(&echo_of_report(&report));
            assert_eq!(want["method"], "POST");

            let dir = std::env::temp_dir().join(format!("rt-copy-as-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            // The server runs on this thread: the child must not block it.
            let run = |mut cmd: Command| {
                let base = base.clone();
                async move {
                    tokio::task::spawn_blocking(move || {
                        let out = cmd
                            .env("BASE_URL", &base)
                            .env("OWNER", "ada")
                            .env("TOKEN", "tok-123")
                            .output()
                            .unwrap();
                        assert!(
                            out.status.success(),
                            "{}",
                            String::from_utf8_lossy(&out.stderr)
                        );
                        String::from_utf8_lossy(&out.stdout).into_owned()
                    })
                    .await
                    .unwrap()
                }
            };
            let json_in = |out: &str| -> serde_json::Value {
                let start = out.find('{').unwrap_or_else(|| panic!("no JSON in {out}"));
                serde_json::from_str(&out[start..]).unwrap()
            };
            if curl {
                let text = copy(&spec, CopyKind::Curl);
                let mut cmd = Command::new("sh");
                cmd.arg("-c").arg(&text);
                assert_eq!(essence(&json_in(&run(cmd).await)), want, "curl: {text}");
            }
            if node {
                let text = copy(&spec, CopyKind::Fetch);
                let file = dir.join("snippet.mjs");
                std::fs::write(&file, &text).unwrap();
                let mut cmd = Command::new("node");
                cmd.arg(&file);
                assert_eq!(essence(&json_in(&run(cmd).await)), want, "fetch: {text}");
            }
            if python {
                let text = copy(&spec, CopyKind::Python);
                let file = dir.join("snippet.py");
                std::fs::write(&file, &text).unwrap();
                let mut cmd = Command::new("python3");
                cmd.arg("-I").arg(&file);
                assert_eq!(essence(&json_in(&run(cmd).await)), want, "python: {text}");
            }
            let _ = std::fs::remove_dir_all(&dir);
        });
    }
}
