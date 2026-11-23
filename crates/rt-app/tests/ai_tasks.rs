// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 16, strand B: the app's own assistant tasks.
//!
//! - Every prompt file is well formed, its version has recorded answers
//!   (`testdata/ai/fixtures/tasks/<id>@<version>/<case>.json`) and its profile is the role
//!   of its task.
//! - Each recorded answer parses to what the case expects; invalid answers are refused or
//!   dropped, never repaired.
//! - The context of each task holds what the prompt needs, and no secret value reaches a call.
//! - A `curl` snippet becomes a request without a model.
//!
//! No provider is called.

use std::path::{Path, PathBuf};

use rt_app::ai::{
    self, AiAnswer, AiContext, AiError, AiNameTarget, AiProvider, AiTask, prompts, snippet,
};
use rt_app::{ResponseStore, SendInput, Workspace};
use rt_core::send::SendReport;
use rt_format::{COLLECTION_FILE, FileSet};
use serde::Deserialize;
use serde_json::{Value, json};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ai/fixtures/tasks")
}

const SECRET: &str = "sk-live-9f8e7d6c5b4a";
const TOKEN: &str = "tok-session-1a2b3c4d5e";

fn collection() -> FileSet {
    let files = [
        (
            COLLECTION_FILE.to_owned(),
            "opencollection: \"1.0.0\"\ninfo:\n  name: Shop\nrequest:\n  variables:\n    - name: baseUrl\n      value: https://shop.example.com\n".to_owned(),
        ),
        (
            "orders/folder.yml".to_owned(),
            "info:\n  name: Orders\n  type: folder\n  seq: 1\n".to_owned(),
        ),
        (
            "orders/list.yml".to_owned(),
            format!(
                "info:\n  name: List orders\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/orders?status=open&api_key={SECRET}\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {TOKEN}\"\n    - name: Accept\n      value: application/json\n"
            ),
        ),
        (
            "orders/create.yml".to_owned(),
            "info:\n  name: Create order\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{baseUrl}}/orders\"\n  body:\n    type: json\n    data: |-\n      {\"sku\": \"a-1\"}\n".to_owned(),
        ),
        (
            "environments/Local.yml".to_owned(),
            "name: Local\nvariables:\n  - name: shopKey\n    secret: true\n".to_owned(),
        ),
    ];
    FileSet {
        files: files
            .into_iter()
            .map(|(p, t)| (p, t.into_bytes()))
            .collect(),
    }
}

fn input() -> SendInput {
    SendInput {
        environment: Some("Local".into()),
        vars: vec![],
        secrets: vec![("shopKey".into(), SECRET.into())],
    }
}

/// A workspace and a stored response of `orders/list.yml`.
fn setup() -> (Workspace, ResponseStore, u64) {
    let ws = Workspace::open(collection()).unwrap();
    let report: SendReport = serde_json::from_value(json!({
        "version": 1, "request_path": "orders/list.yml", "environment": "Local",
        "request": { "method": "GET", "url": format!("https://shop.example.com/orders?status=open&api_key={SECRET}"), "headers": [["Authorization", format!("Bearer {TOKEN}")]], "body_size": 0 },
        "response": {
            "status": 200, "version": "HTTP/1.1",
            "headers": [["Content-Type", "application/json"], ["X-Echo", SECRET]],
            "url": "https://shop.example.com/orders",
            "redirects": [],
            "body": { "encoding": "utf8", "data": format!("{{\"items\":[{{\"id\":7,\"status\":\"open\"}}],\"echo\":\"{SECRET}\"}}") },
            "size": 80, "truncated": false,
            "timing": { "dns": null, "connect": null, "tls": null, "ttfb": 1 }, "total": 2
        },
        "variables": [], "unresolved": [], "error": null
    }))
    .unwrap();
    let mut store = ResponseStore::new();
    let id = store.insert(report).response_id.unwrap();
    (ws, store, id)
}

fn tasks(response: u64) -> Vec<AiTask> {
    vec![
        AiTask::CreateRequest {
            description: format!("list open orders, my key is {SECRET}"),
            folder: "orders".into(),
        },
        AiTask::ExplainError {
            path: "orders/list.yml".into(),
            response,
        },
        AiTask::SuggestTests {
            path: "orders/list.yml".into(),
            response,
        },
        AiTask::RequestFromSnippet {
            snippet: format!("GET /orders?key={SECRET} on the shop API"),
            folder: "orders".into(),
        },
        AiTask::ExplainResponse {
            path: "orders/list.yml".into(),
            response,
        },
        AiTask::SuggestAssertions {
            path: "orders/list.yml".into(),
            response,
        },
        AiTask::SuggestNames {
            target: AiNameTarget::Request,
            path: "orders/list.yml".into(),
        },
        AiTask::SuggestNames {
            target: AiNameTarget::Folder,
            path: "orders".into(),
        },
        AiTask::SuggestNames {
            target: AiNameTarget::Variable,
            path: "orders/list.yml".into(),
        },
        AiTask::DraftDocs {
            path: "orders/list.yml".into(),
        },
    ]
}

// ---- prompt files ----

#[test]
fn every_prompt_file_is_well_formed_and_has_fixtures_for_its_version() {
    let all = prompts::all();
    assert_eq!(
        all.len(),
        prompts::SOURCES.len(),
        "a prompt file is malformed"
    );
    for (id, _) in prompts::SOURCES {
        let p = prompts::get(id).unwrap();
        assert_eq!(&p.id, id, "the file name and the id differ");
        assert!(matches!(p.answer, "json" | "markdown" | "none"), "{id}");
        assert!(matches!(p.profile, "fast" | "smart" | "none"), "{id}");
        if p.profile == "none" || *id == "system" {
            continue;
        }
        let dir = fixtures_dir().join(format!("{id}@{}", p.version));
        let cases = std::fs::read_dir(&dir)
            .unwrap_or_else(|_| panic!("{id}@{} has no recorded answers ({dir:?})", p.version))
            .count();
        assert!(cases >= 1, "{id}@{} has no cases", p.version);
    }
}

#[test]
fn a_task_uses_the_profile_its_prompt_names_and_all_ten_tasks_have_a_prompt() {
    let (_, _, id) = setup();
    for task in tasks(id) {
        let p = prompts::get(task.prompt_id()).unwrap();
        assert_eq!(p.profile, task.role(), "{}", task.prompt_id());
        assert_eq!(p.answer, "json");
        assert_eq!(task.prompt_version(), p.version);
    }
}

#[test]
fn prompts_leave_no_placeholder_behind_and_values_are_inserted_once() {
    let (ws, store, id) = setup();
    for task in tasks(id) {
        let context = AiContext::new(&ws, &store, &task, &input()).unwrap();
        let call = ai::prepare(&task, &context, &[]).unwrap();
        let text = &call.messages[0].content;
        assert!(!text.contains("<<"), "{}: {text}", task.prompt_id());
        assert!(text.contains("Context"), "{}", task.prompt_id());
        assert!(call.system.contains("***"), "the system prompt");
        assert!(call.system.contains("exactly one fenced code block"));
    }
    // A description that looks like a placeholder is text, not a placeholder.
    let task = AiTask::CreateRequest {
        description: "<<context>> and <<include:request-shape>>".into(),
        folder: String::new(),
    };
    let call = ai::prepare(&task, &AiContext::default(), &[]).unwrap();
    let text = &call.messages[0].content;
    assert!(text.contains("<<context>> and <<include:request-shape>>"));
}

#[test]
fn the_assertion_operators_of_the_prompt_are_the_ones_the_evaluator_knows() {
    let fragment = prompts::get("assertions-api").unwrap().body;
    let list = fragment
        .split("operator is one of ")
        .nth(1)
        .and_then(|s| s.split("; value").next())
        .unwrap();
    let operators: Vec<&str> = list.split(", ").collect();
    assert_eq!(operators.len(), 28);
    // Each is accepted by `rt_core::assert`; a made-up one is not.
    let response = rt_core::script::ScriptResponse {
        status: 200,
        headers: vec![],
        body: b"{}".to_vec(),
        time_ms: 1,
        url: String::new(),
    };
    let check = |operator: &str| {
        let a = rt_core::model::Assertion {
            expression: "res.status".into(),
            operator: operator.into(),
            value: "200".into(),
            description: None,
            enabled: true,
        };
        rt_core::assert::evaluate(&[a], &response, &rt_core::vars::Scope::default())
            .remove(0)
            .error
    };
    for op in &operators {
        let error = check(op);
        assert!(
            error.is_none_or(|e| !e.contains("unknown operator")),
            "{op} is unknown to rt_core::assert"
        );
    }
    assert!(check("looksGood").unwrap().contains("unknown operator"));
    // The parser accepts exactly these.
    for op in &operators {
        let answer = format!(
            "{{\"assertions\": [{{\"expression\": \"res.status\", \"operator\": \"{op}\"}}]}}"
        );
        let task = AiTask::SuggestAssertions {
            path: "a.yml".into(),
            response: 1,
        };
        assert!(ai::parse_answer(&task, &answer).is_ok(), "{op}");
    }
}

// ---- recorded answers ----

#[derive(Deserialize)]
struct Case {
    task: AiTask,
    answer: String,
    expect: Expect,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Expect {
    Answer(Value),
    Error(String),
}

#[test]
fn recorded_answers_parse_to_what_each_case_expects() {
    let mut seen = 0;
    let mut dirs: Vec<_> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let (id, version) = name.split_once('@').expect("<id>@<version>");
        let prompt = prompts::get(id).unwrap_or_else(|| panic!("{name}: no such prompt"));
        let version: u32 = version.parse().unwrap();
        assert!(version <= prompt.version, "{name}: newer than the prompt");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        for file in files {
            let case: Case = serde_json::from_str(&std::fs::read_to_string(&file).unwrap())
                .unwrap_or_else(|e| panic!("{file:?}: {e}"));
            assert_eq!(
                case.task.prompt_id(),
                id,
                "{file:?}: task and directory differ"
            );
            let got = ai::parse_answer(&case.task, &case.answer);
            match (case.expect, got) {
                (Expect::Answer(want), Ok(answer)) => {
                    assert_eq!(serde_json::to_value(&answer).unwrap(), want, "{file:?}");
                }
                (Expect::Error(reason), Err(AiError::Answer(message))) => {
                    assert!(message.contains(&reason), "{file:?}: {message}");
                }
                (_, other) => panic!("{file:?}: {other:?}"),
            }
            seen += 1;
        }
    }
    assert!(seen >= 20, "{seen} cases");
}

// ---- context ----

fn context_of(task: &AiTask) -> Value {
    let (ws, store, _) = setup();
    let context = AiContext::new(&ws, &store, task, &input()).unwrap();
    serde_json::to_value(context).unwrap()
}

#[test]
fn the_context_of_each_new_task_holds_what_its_prompt_needs() {
    let (_, _, id) = setup();
    let t = tasks(id);

    let snippet_ctx = context_of(&t[3]);
    assert_eq!(snippet_ctx["folder"], "orders");
    let siblings = snippet_ctx["siblings"].to_string();
    assert!(siblings.contains("List orders") && siblings.contains("Create order"));
    assert!(snippet_ctx["variables"].to_string().contains("baseUrl"));
    assert!(snippet_ctx.get("report").is_none());

    for task in [&t[4], &t[5]] {
        let ctx = context_of(task);
        assert_eq!(ctx["request"]["name"], "List orders");
        assert_eq!(ctx["report"]["response"]["status"], 200);
        assert!(
            ctx["report"]["response"]["body"]
                .as_str()
                .unwrap()
                .contains("items")
        );
    }

    for task in [&t[6], &t[8], &t[9]] {
        let ctx = context_of(task);
        assert_eq!(ctx["request"]["method"], "GET");
        assert!(
            ctx.get("report").is_none(),
            "no response is sent for names or docs"
        );
    }
    assert!(
        context_of(&t[8])["variables"]
            .to_string()
            .contains("baseUrl")
    );

    let folder = context_of(&t[7]);
    assert_eq!(folder["folder"], "orders");
    assert!(folder["siblings"].to_string().contains("Create order"));
}

#[test]
fn a_missing_request_or_response_is_an_error_not_a_call() {
    let (ws, store, id) = setup();
    for task in [
        AiTask::DraftDocs {
            path: "nope.yml".into(),
        },
        AiTask::SuggestAssertions {
            path: "orders/list.yml".into(),
            response: id + 100,
        },
        AiTask::SuggestNames {
            target: AiNameTarget::Folder,
            path: "nope".into(),
        },
    ] {
        assert!(
            AiContext::new(&ws, &store, &task, &input()).is_err(),
            "{task:?}"
        );
    }
}

// ---- secrets ----

#[test]
fn no_secret_value_reaches_a_call_of_any_task() {
    let (ws, store, id) = setup();
    let secrets = ai::secrets_in_scope(ws.collection(), &input());
    assert!(secrets.iter().any(|s| s == SECRET));
    let mut all_secrets = secrets;
    // The response echoes the key; the literal bearer token in the request is not a variable.
    all_secrets.push(TOKEN.to_owned());
    for task in tasks(id) {
        let context = AiContext::new(&ws, &store, &task, &input()).unwrap();
        let call = ai::prepare(&task, &context, &all_secrets).unwrap();
        let json = serde_json::to_string(&call).unwrap();
        assert!(!json.contains(SECRET), "{}: {json}", task.prompt_id());
        assert!(!json.contains(TOKEN), "{}: {json}", task.prompt_id());
    }
}

/// A provider that records its calls and answers a fixed text.
struct Recorder(std::cell::RefCell<Vec<ai::AiCall>>, String);

impl AiProvider for Recorder {
    fn complete<'a>(
        &'a self,
        call: ai::AiCall,
    ) -> rt_core::services::LocalBoxFuture<'a, Result<String, AiError>> {
        self.0.borrow_mut().push(call);
        let text = self.1.clone();
        Box::pin(async move { Ok(text) })
    }
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

#[test]
fn run_calls_the_provider_once_per_model_task_and_parses_the_answer() {
    let (ws, store, _) = setup();
    let task = AiTask::SuggestNames {
        target: AiNameTarget::Request,
        path: "orders/list.yml".into(),
    };
    let context = AiContext::new(&ws, &store, &task, &input()).unwrap();
    let provider = Recorder(
        Default::default(),
        "```json\n{\"names\": [\"List open orders\"]}\n```".into(),
    );
    let answer: AiAnswer = block_on(ai::run(&task, &context, &provider, &[SECRET.into()])).unwrap();
    assert_eq!(answer.names, ["List open orders"]);
    assert_eq!(provider.0.borrow().len(), 1);
}

// ---- cURL without a model ----

#[test]
fn a_curl_snippet_becomes_a_request_without_a_model() {
    let task = AiTask::RequestFromSnippet {
        snippet: "curl -X POST 'https://api.example.com/orders' -H 'Content-Type: application/json' -H \"Authorization: Bearer $TOKEN\" -d '{\"sku\":\"a-1\"}'".into(),
        folder: "orders".into(),
    };
    let answer = ai::local_answer(&task)
        .expect("curl is answered locally")
        .unwrap();
    let request = answer.request.unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "https://api.example.com/orders");
    assert_eq!(request.auth, rt_core::model::Auth::Inherit);
    assert!(
        request
            .headers
            .iter()
            .any(|h| h.name == "Content-Type" && h.value == "application/json")
    );
    // A shell variable became a template, not a value.
    assert!(
        serde_json::to_string(&request)
            .unwrap()
            .contains("{{TOKEN}}"),
        "{request:?}"
    );
    assert!(answer.text.contains("without a model"));

    // The prompt of the other snippets is never built for it: `local_answer` needs no
    // settings, no context and no provider.
    let prompted = AiTask::RequestFromSnippet {
        snippet: "fetch('https://api.example.com/orders')".into(),
        folder: String::new(),
    };
    assert!(ai::local_answer(&prompted).is_none());
    assert!(
        ai::local_answer(&AiTask::DraftDocs {
            path: "a.yml".into()
        })
        .is_none()
    );
}

#[test]
fn what_counts_as_curl_and_what_the_importer_cannot_read() {
    assert!(snippet::is_curl("curl https://x.test"));
    assert!(snippet::is_curl("  $ curl -s https://x.test"));
    assert!(snippet::is_curl("curl"));
    assert!(!snippet::is_curl("curling is a sport"));
    assert!(!snippet::is_curl("fetch('curl')"));
    // A curl command that cannot be read is an error with a reason, not a model call.
    let task = AiTask::RequestFromSnippet {
        snippet: "curl 'unterminated".into(),
        folder: String::new(),
    };
    match ai::local_answer(&task) {
        Some(Err(AiError::Answer(m))) => assert!(m.contains("cURL"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ignored_curl_options_are_reported_in_the_note() {
    let task = AiTask::RequestFromSnippet {
        snippet: "curl -k --proxy http://proxy:3128 https://x.test/a".into(),
        folder: String::new(),
    };
    let answer = ai::local_answer(&task).unwrap().unwrap();
    assert!(answer.text.contains("\n- "), "{}", answer.text);
}
