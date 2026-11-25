// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! API notebooks (wave 19, strand W): the file format round-trips byte for byte, blocks run in
//! order and pass variables on, writes and production are refused, nothing secret reaches a
//! result, and no output is ever stored. Requests are counted at a fake transport.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::agents::{AuditEntry, AuditOutcome};
use rt_app::envcompare::EnvRun;
use rt_app::notebook::{
    Block, Notebook, NotebookEnv, NotebookRun, SKIPPED_EARLIER, SKIPPED_PRODUCTION, SKIPPED_WRITES,
    Show, parse, render,
};
use rt_app::runner::RunSnapshot;
use rt_core::guard::{GuardVerdict, SendCheck, SendGuard};
use rt_core::model::{
    Collection, Environment, Item, KeyValue, RequestEntry, RequestSpec, Variable,
};
use rt_core::send::{MASK, ReportBody, SendOptions};
use rt_core::services::Services;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::FileSet;

const SAMPLE: &str = include_str!("../../../testdata/insight/notebooks/orders.notebook.md");
const CANARY: &str = "RT-CANARY-notebook-secret-5d2a";
const TOKEN: &str = "RT-CANARY-notebook-token-77b1";
const PERSONAL: &str = "dana.canary@example.test";

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The file format

#[test]
fn a_notebook_file_reads_and_writes_back_byte_for_byte() {
    let notebook = parse(SAMPLE).unwrap();
    assert_eq!(notebook.title, "Orders walkthrough");
    assert_eq!(render(&notebook).unwrap(), SAMPLE);
    // Twice: stable.
    assert_eq!(
        render(&parse(&render(&notebook).unwrap()).unwrap()).unwrap(),
        SAMPLE
    );
}

#[test]
fn blocks_prose_and_the_fenced_example_are_told_apart() {
    let notebook = parse(SAMPLE).unwrap();
    let ids: Vec<&str> = notebook
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Request { id, .. } => Some(id.as_str()),
            Block::Text { .. } => None,
        })
        .collect();
    // The third block has no id: it is `block-3`, and is written back without one.
    assert_eq!(ids, ["login", "list", "block-3"]);
    // The `ref:` inside the ```yaml fence is prose, not a block.
    let Block::Text { markdown } = &notebook.blocks[0] else {
        panic!("prose first")
    };
    assert!(markdown.contains("```yaml\n# not a request block\nref: nothing.yml\n```"));
    let Block::Request {
        embedded,
        set,
        show,
        ..
    } = &notebook.blocks[1]
    else {
        panic!()
    };
    let spec = embedded.as_ref().unwrap();
    assert_eq!(
        (spec.method.as_str(), spec.url.as_str()),
        ("POST", "{{base}}/login")
    );
    assert_eq!(spec.headers, [KeyValue::new("Accept", "application/json")]);
    assert_eq!(
        set,
        &[
            ("token".into(), "$.access_token".into()),
            ("userId".into(), "$.user.id".into())
        ]
    );
    assert!(show.is_empty());
    let Block::Request {
        show, request_ref, ..
    } = &notebook.blocks[3]
    else {
        panic!()
    };
    assert_eq!(request_ref, &None);
    assert_eq!(
        show,
        &[
            Show::Response,
            Show::Table {
                path: "$.items[*]".into()
            },
            Show::Chart {
                path: "$.items[*].total".into()
            }
        ]
    );
}

#[test]
fn text_without_blocks_and_odd_endings_survive() {
    for text in [
        "",
        "no newline at the end",
        "# Only a title\n\n\n",
        "a\r\nb\r\n",
    ] {
        assert_eq!(render(&parse(text).unwrap()).unwrap(), text, "{text:?}");
    }
    assert_eq!(parse("# Hello\n").unwrap().title, "Hello");
    assert_eq!(parse("x\n").unwrap().title, "");
}

#[test]
fn a_hand_written_block_is_normalised_once_and_stable() {
    let text = "```rt-request\n# a comment\nurl:   http://x/a\nid:  one\nmethod: post\nshow:\n- response\n```\n";
    let once = render(&parse(text).unwrap()).unwrap();
    assert_eq!(
        once,
        "```rt-request\nid: one\nmethod: POST\nurl: http://x/a\nshow:\n  - response\n```\n"
    );
    assert_eq!(render(&parse(&once).unwrap()).unwrap(), once);
}

#[test]
fn multi_line_bodies_round_trip() {
    let text =
        "```rt-request\nmethod: POST\nurl: http://x/a\nbody: |\n  {\n    \"a\": 1\n\n  }\n```\n";
    let notebook = parse(text).unwrap();
    let Block::Request { embedded, .. } = &notebook.blocks[0] else {
        panic!()
    };
    let rt_core::model::Body::Raw { data, .. } = &embedded.as_ref().unwrap().body else {
        panic!("raw")
    };
    assert_eq!(data, "{\n  \"a\": 1\n\n}");
    assert_eq!(render(&notebook).unwrap(), text);
}

#[test]
fn bad_blocks_say_where_and_why() {
    for (text, needle) in [
        ("x\n```rt-request\nurl: http://a\n", "line 2 is not closed"),
        ("```rt-request\nfoo: 1\n```\n", "unknown key \"foo\""),
        ("```rt-request\nmethod: GET\n```\n", "needs `ref:"),
        (
            "```rt-request\nref: a.yml\nurl: http://x\n```\n",
            "cannot be mixed",
        ),
        (
            "```rt-request\nid: a\nref: a.yml\n```\n```rt-request\nid: a\nref: b.yml\n```\n",
            "used twice",
        ),
        (
            "```rt-request\nref: a.yml\nshow:\n  - pie\n```\n",
            "bad show item",
        ),
        (
            "```rt-request\nref: a.yml\nset:\n  x\n```\n",
            "bad set line",
        ),
    ] {
        let error = parse(text).unwrap_err().to_string();
        assert!(error.contains(needle), "{text:?}: {error}");
    }
}

#[test]
fn a_notebook_holds_at_most_500_request_blocks() {
    let block = "```rt-request\nref: a.yml\n```\n";
    assert!(parse(&block.repeat(500)).is_ok());
    assert!(parse(&block.repeat(501)).is_err());
}

// ---------------------------------------------------------------------------------------------
// Running

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

struct Api {
    seen: Mutex<Vec<HttpRequest>>,
}

impl Api {
    fn new() -> Self {
        Self {
            seen: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| format!("{} {}", r.method, r.url))
            .collect()
    }
    fn header(&self, call: usize, name: &str) -> Option<String> {
        self.seen.lock().unwrap()[call]
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }
}

impl Transport for Api {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push(request.clone());
        let rest = request.url.split_once("://").map_or("", |(_, r)| r);
        let path = rest.find('/').map_or("", |i| &rest[i..]);
        let auth = request
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let (status, body) = if path == "/login" {
            (
                200,
                format!(
                    "{{\"access_token\":\"{TOKEN}\",\"user\":{{\"id\":42,\"email\":\"{PERSONAL}\"}},\"echo\":\"{CANARY}\"}}"
                ),
            )
        } else if path == "/orders" {
            (
                200,
                format!(
                    "{{\"items\":[{{\"id\":1,\"total\":10.5}},{{\"id\":2,\"total\":4}}],\"auth\":\"{auth}\"}}"
                ),
            )
        } else if path == "/bad" {
            (500, "{\"error\":\"boom\"}".to_owned())
        } else if path.starts_with("/orders/") {
            (200, "{\"id\":7}".to_owned())
        } else {
            (404, "{\"error\":\"not found\"}".to_owned())
        };
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(body))));
        Ok(Response {
            head: ResponseHead {
                status,
                version: HttpVersion::Http11,
                headers: vec![("Content-Type".into(), "application/json".into())],
                url: request.url.clone(),
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

fn collection() -> Collection {
    let get_order = RequestSpec {
        name: "get-order".into(),
        method: "GET".into(),
        url: "{{base}}/orders/7".into(),
        headers: vec![KeyValue::new("x-secret", "{{apiKey}}")],
        ..RequestSpec::default()
    };
    let env = |name: &str| Environment {
        name: name.into(),
        variables: vec![
            Variable::new("base", "http://api.test"),
            Variable {
                name: "apiKey".into(),
                value: Some(CANARY.into()),
                enabled: true,
                secret: true,
            },
        ],
        client_certificates: vec![],
        external_secrets: None,
    };
    Collection {
        name: "shop".into(),
        items: vec![Item::Request(RequestEntry {
            path: "orders/get-order.yml".into(),
            spec: get_order,
        })],
        environments: vec![env("Test"), env("PRD")],
        ..Collection::default()
    }
}

struct Ran {
    outcome: rt_app::notebook::NotebookOutcome,
    audit: Vec<AuditEntry>,
}

fn run_with(
    api: &Api,
    text: &str,
    environment: &str,
    run: NotebookRun,
    services: &Services<'_>,
) -> Result<Ran, rt_app::AppError> {
    let notebook = parse(text)?;
    let snapshot = RunSnapshot {
        collection: collection(),
        environment: Some(environment.into()),
        vars: Vec::new(),
        files: FileSet::default(),
    };
    let env_run = EnvRun {
        snapshot,
        services: Services { ..*services },
    };
    let audit: Mutex<Vec<AuditEntry>> = Mutex::new(Vec::new());
    let sink = |e: &AuditEntry| audit.lock().unwrap().push(e.clone());
    let env = NotebookEnv {
        run: &env_run,
        transport: api,
        options: &SendOptions::default(),
        caller: "test",
        audit: &sink,
        cancelled: &|| false,
    };
    let outcome = block_on(rt_app::notebook::run(&env, &notebook, &run))?;
    Ok(Ran {
        outcome,
        audit: audit.into_inner().unwrap(),
    })
}

fn run_all(api: &Api, text: &str) -> Ran {
    run_with(
        api,
        text,
        "Test",
        NotebookRun {
            path: "a.notebook.md".into(),
            block: None,
            environment: Some("Test".into()),
            allow_writes: false,
            vars: Vec::new(),
        },
        &Services::none(),
    )
    .unwrap()
}

#[test]
fn blocks_run_in_order_and_pass_variables_on() {
    let api = Api::new();
    // The login block is a POST: read it as a reading block by using GET in this test.
    let text = SAMPLE.replace("method: POST", "method: GET");
    let ran = run_all(&api, &text);
    assert_eq!(
        api.calls(),
        [
            "GET http://api.test/login",
            "GET http://api.test/orders",
            "GET http://api.test/orders/7"
        ]
    );
    // `token` of the first block is the bearer of the second one.
    assert_eq!(
        api.header(1, "authorization").as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    let results = &ran.outcome.results;
    assert_eq!(
        results.iter().map(|r| r.block.as_str()).collect::<Vec<_>>(),
        ["login", "list", "block-3"]
    );
    // `userId` is shown, `token` looks like a credential: carried, shown as the mask.
    assert_eq!(
        results[0].set,
        [
            ("token".into(), MASK.into()),
            ("userId".into(), "42".into())
        ]
    );
    assert!(results.iter().all(|r| r.skipped.is_none()));
}

#[test]
fn show_lines_become_tables_and_chart_values_in_rust() {
    let api = Api::new();
    let ran = run_all(&api, &SAMPLE.replace("method: POST", "method: GET"));
    let list = &ran.outcome.results[1];
    assert_eq!(list.views.len(), 2, "response is the report itself");
    let table = &list.views[0].result;
    assert_eq!(table.columns, ["id", "total"]);
    assert_eq!(table.rows.len(), 2);
    let chart = &list.views[1].result;
    assert_eq!(
        chart.values,
        [serde_json::json!(10.5), serde_json::json!(4)]
    );
}

#[test]
fn a_single_block_runs_with_the_variables_the_view_holds() {
    let api = Api::new();
    let ran = run_with(
        &api,
        &SAMPLE.replace("method: POST", "method: GET"),
        "Test",
        NotebookRun {
            path: "a.notebook.md".into(),
            block: Some("list".into()),
            environment: Some("Test".into()),
            allow_writes: false,
            vars: vec![
                ("token".into(), "abc".into()),
                ("masked".into(), MASK.into()),
            ],
        },
        &Services::none(),
    )
    .unwrap();
    assert_eq!(api.calls(), ["GET http://api.test/orders"]);
    assert_eq!(
        api.header(0, "authorization").as_deref(),
        Some("Bearer abc")
    );
    assert_eq!(ran.outcome.results.len(), 1);
    let error = run_with(
        &api,
        SAMPLE,
        "Test",
        NotebookRun {
            path: "a".into(),
            block: Some("nope".into()),
            environment: None,
            allow_writes: false,
            vars: Vec::new(),
        },
        &Services::none(),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("no block \"nope\""));
}

#[test]
fn a_writing_block_needs_permission_and_never_runs_against_production() {
    let api = Api::new();
    // The sample's login is a POST.
    let ran = run_all(&api, SAMPLE);
    let results = &ran.outcome.results;
    assert_eq!(results[0].skipped.as_deref(), Some(SKIPPED_WRITES));
    assert!(results[0].report.is_none());
    // The later blocks still run: they do not depend on the skipped block in this file.
    assert_eq!(api.calls().len(), 2);
    assert_eq!(ran.audit[0].outcome, AuditOutcome::Sent);

    let api = Api::new();
    let allowed = |env: &str| {
        run_with(
            &api,
            SAMPLE,
            env,
            NotebookRun {
                path: "a.notebook.md".into(),
                block: Some("login".into()),
                environment: Some(env.into()),
                allow_writes: true,
                vars: Vec::new(),
            },
            &Services::none(),
        )
        .unwrap()
    };
    let test = allowed("Test");
    assert!(test.outcome.results[0].report.is_some());
    assert_eq!(api.calls(), ["POST http://api.test/login"]);
    assert_eq!(test.audit[0].approved_by.as_deref(), Some("test"));
    let prd = allowed("PRD");
    assert_eq!(
        prd.outcome.results[0].skipped.as_deref(),
        Some(SKIPPED_PRODUCTION)
    );
    assert_eq!(api.calls().len(), 1, "nothing was sent to production");
    assert_eq!(prd.audit[0].outcome, AuditOutcome::Denied);
}

#[test]
fn a_block_that_fails_to_send_skips_the_rest() {
    let api = Api::new();
    let text = "```rt-request\nid: a\nref: nothing.yml\n```\n```rt-request\nid: b\nref: orders/get-order.yml\n```\n";
    let ran = run_all(&api, text);
    assert!(
        ran.outcome.results[0]
            .skipped
            .as_deref()
            .unwrap()
            .contains("no request nothing.yml")
    );
    assert_eq!(
        ran.outcome.results[1].skipped.as_deref(),
        Some(SKIPPED_EARLIER)
    );
    assert!(api.calls().is_empty());
}

#[test]
fn a_set_path_that_matches_nothing_is_said_not_guessed() {
    let api = Api::new();
    let text =
        "```rt-request\nmethod: GET\nurl: {{base}}/orders\nset:\n  gone: $.nothing.here\n```\n";
    let ran = run_all(&api, text);
    let result = &ran.outcome.results[0];
    assert!(result.set.is_empty());
    assert!(
        result.notes[0].contains("matched nothing"),
        "{:?}",
        result.notes
    );
}

struct Deny;
impl SendGuard for Deny {
    fn check(&self, _: &SendCheck) -> GuardVerdict {
        GuardVerdict::Refuse {
            rule: "no-notebooks".into(),
            reason: "not today".into(),
        }
    }
}

#[test]
fn the_send_guard_is_asked_and_its_refusal_is_audited() {
    let api = Api::new();
    let services = Services {
        guard: Some(&Deny),
        ..Services::none()
    };
    let ran = run_with(
        &api,
        "```rt-request\nref: orders/get-order.yml\n```\n",
        "Test",
        NotebookRun {
            path: "a.notebook.md".into(),
            block: None,
            environment: None,
            allow_writes: false,
            vars: Vec::new(),
        },
        &services,
    )
    .unwrap();
    assert!(api.calls().is_empty());
    assert_eq!(ran.audit[0].outcome, AuditOutcome::Refused);
    assert_eq!(ran.audit[0].rule.as_deref(), Some("no-notebooks"));
}

#[test]
fn no_secret_value_reaches_a_result_or_the_audit_log() {
    let api = Api::new();
    let text = "```rt-request\nid: login\nmethod: GET\nurl: {{base}}/login\nheaders:\n  x-key: {{apiKey}}\nset:\n  token: $.access_token\n  echo: $.echo\n  mail: $.user.email\n```\n```rt-request\nid: list\nmethod: GET\nurl: {{base}}/orders\nheaders:\n  Authorization: Bearer {{token}}\nshow:\n  - response\n  - chart: $.auth\n```\n";
    let ran = run_all(&api, text);
    // The secret really went out, and the carried token too.
    assert_eq!(api.header(0, "x-key").as_deref(), Some(CANARY));
    assert_eq!(
        api.header(1, "authorization").as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    let json =
        serde_json::to_string(&ran.outcome).unwrap() + &serde_json::to_string(&ran.audit).unwrap();
    for canary in [CANARY, TOKEN] {
        assert!(!json.contains(canary), "{canary} leaked");
    }
    // The echoed secret and the token (in the second answer) are masked; masking is the mask.
    assert!(json.contains(MASK));
    // Personal data of the API's answer is the user's own response, shown as received.
    let first = ran.outcome.results[0].report.as_ref().unwrap();
    let ReportBody::Utf8(body) = &first.response.as_ref().unwrap().body else {
        panic!()
    };
    assert!(body.contains(PERSONAL));
}

#[test]
fn running_a_notebook_stores_nothing() {
    // The run takes a parsed notebook and a snapshot and has no way to write: the text of the
    // notebook is the same before and after, and the module has no function that saves a result.
    let api = Api::new();
    let before = parse(SAMPLE).unwrap();
    let _ = run_all(&api, &SAMPLE.replace("method: POST", "method: GET"));
    assert_eq!(before, parse(SAMPLE).unwrap());
    assert_eq!(render(&before).unwrap(), SAMPLE);
}

#[test]
fn the_workspace_lists_reads_and_stages_notebooks() {
    use rt_app::Workspace;
    let files = [
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: C\n".to_owned(),
        ),
        ("docs/orders.notebook.md", SAMPLE.to_owned()),
        (
            "broken.notebook.md",
            "```rt-request\nfoo: 1\n```\n".to_owned(),
        ),
        ("README.md", "not a notebook".to_owned()),
    ];
    let mut workspace = Workspace::open(FileSet {
        files: files
            .into_iter()
            .map(|(p, t)| (p.to_owned(), t.into_bytes()))
            .collect(),
    })
    .unwrap();
    let list = rt_app::notebook::list(&workspace);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].path, "broken.notebook.md");
    assert!(list[0].title.contains("unreadable"));
    assert_eq!(list[1].path, "docs/orders.notebook.md");
    assert_eq!(
        (list[1].title.as_str(), list[1].blocks),
        ("Orders walkthrough", 3)
    );
    let mut notebook: Notebook =
        rt_app::notebook::read(&workspace, "docs/orders.notebook.md").unwrap();
    assert!(rt_app::notebook::read(&workspace, "README.md").is_err());
    // Unchanged: staging writes the same bytes, so a view that only opens a notebook changes nothing.
    rt_app::notebook::stage(&mut workspace, "docs/orders.notebook.md", &notebook).unwrap();
    assert_eq!(
        rt_app::companions::read(&workspace, "docs/orders.notebook.md").as_deref(),
        Some(SAMPLE)
    );
    notebook.blocks.push(Block::Text {
        markdown: "\nThe end.\n".into(),
    });
    rt_app::notebook::stage(&mut workspace, "docs/orders.notebook.md", &notebook).unwrap();
    assert!(
        rt_app::companions::read(&workspace, "docs/orders.notebook.md")
            .unwrap()
            .ends_with("The end.\n")
    );
}
