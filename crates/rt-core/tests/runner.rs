// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection runner (wave 7, strand D): order, iterations, data rows, carrying variables,
//! runner control from scripts, bail, cancel, delay and the summary. A fake transport and a
//! fake script runner stand in for the network and `rt-script`.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_core::model::{
    Assertion, Collection, Environment, Folder, Item, RequestEntry, RequestSpec, Script,
    ScriptPhase, VarScope, Variable,
};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::runner::{
    DataRow, RunEvent, RunPlan, RunSummary, parse_data, plan, platform_sleep, run, run_with,
};
use rt_core::script::{Flow, Iteration, ScriptCall, ScriptOutcome, TestResult, VarChange};
use rt_core::send::{MASK, SendOptions};
use rt_core::services::{LocalBoxFuture, NoteLevel, ScriptRunner, Services};
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use serde_json::json;

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
// Fakes

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

/// Answers 200, or 500 for URLs with `/fail`. Records the URLs.
#[derive(Default)]
struct Wire {
    urls: RefCell<Vec<String>>,
}

impl Wire {
    fn urls(&self) -> Vec<String> {
        self.urls.borrow().clone()
    }
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let url = request.url.clone();
        self.urls.borrow_mut().push(url.clone());
        let status = if url.contains("/fail") { 500 } else { 200 };
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(format!("{{\"url\":\"{url}\"}}")))));
        Ok(Response {
            head: ResponseHead {
                status,
                version: HttpVersion::Http11,
                headers: vec![("content-type".into(), "application/json".into())],
                url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

struct NoFiles;

impl FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("{path}: not found"))
    }
}

/// What a script call saw.
#[derive(Debug, Clone)]
struct Seen {
    phase: ScriptPhase,
    path: String,
    iteration: Option<Iteration>,
    vars: Vec<(String, String)>,
}

impl Seen {
    fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

type Answer = Box<dyn Fn(&Seen) -> ScriptOutcome>;

/// Records every call and answers with `answer`.
struct Fake {
    seen: RefCell<Vec<Seen>>,
    answer: Answer,
}

impl Fake {
    fn new(answer: impl Fn(&Seen) -> ScriptOutcome + 'static) -> Self {
        Self {
            seen: RefCell::new(Vec::new()),
            answer: Box::new(answer),
        }
    }

    fn seen(&self, phase: ScriptPhase) -> Vec<Seen> {
        self.seen
            .borrow()
            .iter()
            .filter(|s| s.phase == phase)
            .cloned()
            .collect()
    }
}

impl ScriptRunner for Fake {
    fn run<'a>(&'a self, call: ScriptCall<'a>) -> LocalBoxFuture<'a, ScriptOutcome> {
        Box::pin(async move {
            let seen = Seen {
                phase: call.phase,
                path: call.info.request_path.clone(),
                iteration: call.info.iteration.clone(),
                vars: call
                    .variables
                    .iter()
                    .map(|v| (v.name.clone(), v.value.clone()))
                    .collect(),
            };
            self.seen.borrow_mut().push(seen.clone());
            (self.answer)(&seen)
        })
    }
}

fn nothing(_: &Seen) -> ScriptOutcome {
    ScriptOutcome::default()
}

// ---------------------------------------------------------------------------------------------
// Collection

fn all_phases() -> Vec<Script> {
    [
        ScriptPhase::BeforeRequest,
        ScriptPhase::AfterResponse,
        ScriptPhase::Tests,
    ]
    .into_iter()
    .map(|phase| Script {
        phase,
        code: "/* fake */".into(),
    })
    .collect()
}

fn request(path: &str, name: &str, url: &str) -> Item {
    Item::Request(RequestEntry {
        path: path.into(),
        spec: RequestSpec {
            name: name.into(),
            method: "GET".into(),
            url: url.into(),
            scripts: all_phases(),
            ..Default::default()
        },
    })
}

/// ```text
/// users/            (folder)
///   list.yml        List users     http://h/users?page={{page}}
///   admin/          (folder)
///     get.yml       Get admin      http://h/admin?t={{token}}
///   create.yml      Create user    http://h/users/{{name}}
/// health.yml        Health         http://h/health
/// ```
fn collection() -> Collection {
    Collection {
        name: "c".into(),
        items: vec![
            Item::Folder(Folder {
                name: "Users".into(),
                seq: Some(1.0),
                path: "users".into(),
                defaults: Default::default(),
                items: vec![
                    request(
                        "users/list.yml",
                        "List users",
                        "http://h/users?page={{page}}",
                    ),
                    Item::Folder(Folder {
                        name: "Admin".into(),
                        seq: Some(2.0),
                        path: "users/admin".into(),
                        defaults: Default::default(),
                        items: vec![request(
                            "users/admin/get.yml",
                            "Get admin",
                            "http://h/admin?t={{token}}",
                        )],
                    }),
                    request("users/create.yml", "Create user", "http://h/users/{{name}}"),
                ],
            }),
            request("health.yml", "Health", "http://h/health"),
        ],
        environments: vec![Environment {
            name: "E".into(),
            variables: vec![Variable {
                name: "key".into(),
                value: Some("s3cr3t".into()),
                enabled: true,
                secret: true,
            }],
            client_certificates: Vec::new(),
            external_secrets: None,
        }],
        ..Default::default()
    }
}

fn input() -> PrepareInput<'static> {
    PrepareInput {
        environment: Some("E"),
        globals: Vec::new(),
        runtime: Vec::new(),
    }
}

fn change(name: &str, value: &str) -> VarChange {
    VarChange {
        name: name.into(),
        value: Some(value.into()),
        scope: VarScope::Runtime,
    }
}

fn test_result(name: &str, passed: bool) -> TestResult {
    TestResult {
        name: name.into(),
        passed,
        error: (!passed).then(|| "expected true".to_owned()),
        origin: "request".into(),
    }
}

fn plan_of(requests: &[&str]) -> RunPlan {
    RunPlan {
        requests: requests.iter().map(|s| (*s).to_owned()).collect(),
        iterations: 1,
        ..Default::default()
    }
}

/// Everything one run produced.
struct Ran {
    events: Vec<RunEvent>,
    summary: RunSummary,
    sleeps: Vec<u64>,
}

impl Ran {
    fn steps(&self) -> Vec<&rt_core::runner::RunStep> {
        self.events
            .iter()
            .filter_map(|e| match e {
                RunEvent::Step { step } => Some(&**step),
                _ => None,
            })
            .collect()
    }

    fn step_paths(&self) -> Vec<(u32, String)> {
        self.steps()
            .iter()
            .map(|s| (s.iteration, s.request_path.clone()))
            .collect()
    }
}

/// Runs with a virtual sleep (recorded, never waits).
fn run_plan(
    c: &Collection,
    plan: &RunPlan,
    input: &PrepareInput<'_>,
    wire: &Wire,
    fake: Option<&Fake>,
    cancelled: &dyn Fn() -> bool,
) -> Ran {
    let services = Services {
        scripts: fake.map(|f| f as &dyn ScriptRunner),
        ..Services::none()
    };
    let mut events = Vec::new();
    let sleeps = RefCell::new(Vec::new());
    let sleep = |ms: u64| -> LocalBoxFuture<'static, ()> {
        sleeps.borrow_mut().push(ms);
        Box::pin(async {})
    };
    let summary = block_on(run_with(
        c,
        plan,
        input,
        &NoFiles,
        wire,
        &SendOptions::default(),
        &services,
        &mut |e| events.push(e),
        cancelled,
        &sleep,
    ));
    Ran {
        events,
        summary,
        sleeps: sleeps.into_inner(),
    }
}

fn never() -> bool {
    false
}

// ---------------------------------------------------------------------------------------------
// plan

#[test]
fn plan_lists_requests_depth_first_in_the_collection_order() {
    let c = collection();
    assert_eq!(
        plan(&c, None),
        [
            "users/list.yml",
            "users/admin/get.yml",
            "users/create.yml",
            "health.yml"
        ]
    );
    assert_eq!(plan(&c, Some("")), plan(&c, None));
    assert_eq!(
        plan(&c, Some("users")),
        ["users/list.yml", "users/admin/get.yml", "users/create.yml"]
    );
    assert_eq!(plan(&c, Some("users/admin")), ["users/admin/get.yml"]);
    assert_eq!(plan(&c, Some("users/admin/")), ["users/admin/get.yml"]);
    assert!(plan(&c, Some("nope")).is_empty());
    // A request path is not a folder.
    assert!(plan(&c, Some("health.yml")).is_empty());
}

// ---------------------------------------------------------------------------------------------
// run: order, events, summary

#[test]
fn runs_in_order_with_events_and_a_summary() {
    let c = collection();
    let wire = Wire::default();
    let fake = Fake::new(|s| match s.phase {
        ScriptPhase::Tests => ScriptOutcome {
            tests: vec![
                test_result("ok", true),
                test_result("health only", s.path != "health.yml"),
            ],
            ..Default::default()
        },
        _ => ScriptOutcome::default(),
    });
    let ran = run_plan(
        &c,
        &plan_of(&["users/list.yml", "health.yml"]),
        &input(),
        &wire,
        Some(&fake),
        &never,
    );
    let kinds: Vec<_> = ran
        .events
        .iter()
        .map(|e| match e {
            RunEvent::Started { total } => format!("started {total}"),
            RunEvent::Request {
                iteration,
                request_path,
            } => format!("request {iteration} {request_path}"),
            RunEvent::Step { step } => format!("step {} {}", step.iteration, step.request_path),
            RunEvent::Finished { .. } => "finished".into(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "started 2",
            "request 0 users/list.yml",
            "step 0 users/list.yml",
            "request 0 health.yml",
            "step 0 health.yml",
            "finished"
        ]
    );
    let steps = ran.steps();
    assert_eq!(steps[0].request_name, "List users");
    assert!(steps[0].passed && !steps[0].skipped);
    assert!(!steps[1].passed);
    assert_eq!(steps[1].report.tests.len(), 2);
    let s = &ran.summary;
    assert_eq!(
        (s.requests, s.passed, s.failed, s.skipped),
        (2, 1, 1, 0),
        "{s:?}"
    );
    assert_eq!((s.tests_passed, s.tests_failed), (3, 1));
    assert!(!s.stopped);
    let RunEvent::Finished { summary } = ran.events.last().unwrap() else {
        panic!("last event is finished")
    };
    assert_eq!(summary, &ran.summary);
    assert_eq!(
        wire.urls(),
        ["http://h/users?page=%7B%7Bpage%7D%7D", "http://h/health"]
    );
}

#[test]
fn a_failing_request_fails_its_step_and_a_missing_one_is_reported() {
    let mut c = collection();
    c.items
        .push(request("broken.yml", "Broken", "http://h/fail"));
    let wire = Wire::default();
    let ran = run_plan(
        &c,
        &plan_of(&["broken.yml", "gone.yml", "health.yml"]),
        &input(),
        &wire,
        None,
        &never,
    );
    let steps = ran.steps();
    // A 500 without tests or assertions is not a failure (as in Postman and Bruno) …
    assert!(steps[0].passed);
    // … a request that is not in the collection is.
    assert!(!steps[1].passed);
    assert_eq!(steps[1].request_name, "gone.yml");
    assert!(steps[1].report.error.is_some());
    assert!(steps[2].passed);
    assert_eq!(
        (ran.summary.requests, ran.summary.passed, ran.summary.failed),
        (3, 2, 1)
    );
}

// ---------------------------------------------------------------------------------------------
// iterations and data

#[test]
fn iterations_repeat_the_plan_and_scripts_see_the_iteration() {
    let c = collection();
    let wire = Wire::default();
    let fake = Fake::new(nothing);
    let plan = RunPlan {
        iterations: 3,
        ..plan_of(&["users/list.yml", "health.yml"])
    };
    let ran = run_plan(&c, &plan, &input(), &wire, Some(&fake), &never);
    assert_eq!(
        ran.step_paths(),
        [
            (0, "users/list.yml".into()),
            (0, "health.yml".into()),
            (1, "users/list.yml".into()),
            (1, "health.yml".into()),
            (2, "users/list.yml".into()),
            (2, "health.yml".into()),
        ]
    );
    assert!(matches!(ran.events[0], RunEvent::Started { total: 6 }));
    let iterations: Vec<_> = fake
        .seen(ScriptPhase::BeforeRequest)
        .iter()
        .map(|s| {
            let i = s.iteration.clone().expect("runner passes the iteration");
            (i.index, i.count, i.data.len())
        })
        .collect();
    assert_eq!(
        iterations,
        [
            (0, 3, 0),
            (0, 3, 0),
            (1, 3, 0),
            (1, 3, 0),
            (2, 3, 0),
            (2, 3, 0)
        ]
    );
    assert_eq!(ran.summary.requests, 6);
}

#[test]
fn zero_iterations_run_once() {
    let c = collection();
    let wire = Wire::default();
    let plan = RunPlan {
        iterations: 0,
        ..plan_of(&["health.yml"])
    };
    let ran = run_plan(&c, &plan, &input(), &wire, None, &never);
    assert_eq!(ran.summary.requests, 1);
}

#[test]
fn data_rows_drive_the_iterations_and_resolve_as_variables() {
    let c = collection();
    let wire = Wire::default();
    let fake = Fake::new(nothing);
    let rows: Vec<DataRow> = vec![
        vec![("name".into(), json!("ada")), ("page".into(), json!(1))],
        vec![
            ("name".into(), json!("bob")),
            ("page".into(), json!({"n": 2})),
        ],
    ];
    let plan = RunPlan {
        data: rows.clone(),
        // Ignored when there is data: one iteration per row.
        iterations: 5,
        ..plan_of(&["users/create.yml", "users/list.yml"])
    };
    let ran = run_plan(&c, &plan, &input(), &wire, Some(&fake), &never);
    assert_eq!(
        wire.urls(),
        [
            "http://h/users/ada",
            "http://h/users?page=1",
            "http://h/users/bob",
            // Non-string values resolve as their JSON text (URL-encoded by `prepare`).
            "http://h/users?page=%7B%22n%22%3A2%7D",
        ]
    );
    let seen = fake.seen(ScriptPhase::BeforeRequest);
    let i = seen[2].iteration.clone().unwrap();
    assert_eq!((i.index, i.count), (1, 2));
    assert_eq!(i.data, rows[1]);
    assert_eq!(seen[2].var("name"), Some("bob"));
    assert!(matches!(ran.events[0], RunEvent::Started { total: 4 }));
}

// ---------------------------------------------------------------------------------------------
// carrying variables

#[test]
fn variable_changes_carry_to_later_requests_and_iterations() {
    let c = collection();
    let wire = Wire::default();
    // The list request's after-response script sets a token (from the iteration); the admin
    // request uses it. `create` changes the name for every later request.
    let fake = Fake::new(|s| match (s.phase, s.path.as_str()) {
        (ScriptPhase::AfterResponse, "users/list.yml") => ScriptOutcome {
            var_changes: vec![change(
                "token",
                &format!("t{}", s.iteration.as_ref().unwrap().index),
            )],
            ..Default::default()
        },
        (ScriptPhase::AfterResponse, "users/create.yml") => ScriptOutcome {
            var_changes: vec![VarChange {
                name: "name".into(),
                value: Some("from-script".into()),
                scope: VarScope::Environment,
            }],
            ..Default::default()
        },
        _ => ScriptOutcome::default(),
    });
    let plan = RunPlan {
        data: vec![
            vec![("name".into(), json!("ada"))],
            vec![("name".into(), json!("bob"))],
        ],
        ..plan_of(&[
            "users/admin/get.yml",
            "users/list.yml",
            "users/admin/get.yml",
            "users/create.yml",
        ])
    };
    let input = PrepareInput {
        runtime: vec![("page".into(), "7".into())],
        ..input()
    };
    run_plan(&c, &plan, &input, &wire, Some(&fake), &never);
    assert_eq!(
        wire.urls(),
        [
            // Iteration 0: no token yet (left unresolved), then the one the list request set.
            "http://h/admin?t=%7B%7Btoken%7D%7D",
            "http://h/users?page=7",
            "http://h/admin?t=t0",
            "http://h/users/ada",
            // Iteration 1: changes are kept for the whole run and win over the data row.
            "http://h/admin?t=t0",
            "http://h/users?page=7",
            "http://h/admin?t=t1",
            "http://h/users/from-script",
        ]
    );
}

#[test]
fn precedence_data_row_below_input_below_run_changes() {
    let c = collection();
    let wire = Wire::default();
    let fake = Fake::new(|s| match (s.phase, s.path.as_str()) {
        (ScriptPhase::AfterResponse, "health.yml") => ScriptOutcome {
            var_changes: vec![change("page", "from-script")],
            ..Default::default()
        },
        _ => ScriptOutcome::default(),
    });
    let plan = RunPlan {
        data: vec![vec![
            ("page".into(), json!("from-data")),
            ("name".into(), json!("from-data")),
        ]],
        ..plan_of(&[
            "users/create.yml",
            "users/list.yml",
            "health.yml",
            "users/list.yml",
        ])
    };
    let input = PrepareInput {
        runtime: vec![("page".into(), "from-input".into())],
        ..input()
    };
    run_plan(&c, &plan, &input, &wire, Some(&fake), &never);
    assert_eq!(
        wire.urls(),
        [
            "http://h/users/from-data",
            "http://h/users?page=from-input",
            "http://h/health",
            "http://h/users?page=from-script",
        ]
    );
}

#[test]
fn unset_removes_a_run_variable_and_values_carry_unmasked() {
    let c = collection();
    let wire = Wire::default();
    let fake = Fake::new(|s| match (s.phase, s.path.as_str()) {
        // A value built from a secret: masked in the report, but the next request needs it.
        (ScriptPhase::AfterResponse, "users/list.yml") => ScriptOutcome {
            var_changes: vec![change(
                "token",
                &format!("Bearer-{}", s.var("key").unwrap_or_default()),
            )],
            ..Default::default()
        },
        (ScriptPhase::AfterResponse, "health.yml") => ScriptOutcome {
            var_changes: vec![VarChange {
                name: "token".into(),
                value: None,
                scope: VarScope::Runtime,
            }],
            ..Default::default()
        },
        _ => ScriptOutcome::default(),
    });
    let ran = run_plan(
        &c,
        &plan_of(&[
            "users/list.yml",
            "users/admin/get.yml",
            "health.yml",
            "users/admin/get.yml",
        ]),
        &input(),
        &wire,
        Some(&fake),
        &never,
    );
    assert_eq!(
        wire.urls(),
        [
            "http://h/users?page=%7B%7Bpage%7D%7D",
            "http://h/admin?t=Bearer-s3cr3t",
            "http://h/health",
            "http://h/admin?t=%7B%7Btoken%7D%7D",
        ]
    );
    let steps = ran.steps();
    assert_eq!(
        steps[0].report.var_changes[0].value.as_deref(),
        Some(format!("Bearer-{MASK}").as_str())
    );
}

// ---------------------------------------------------------------------------------------------
// runner control from scripts

#[test]
fn skip_does_not_send_and_counts_as_skipped() {
    let c = collection();
    let wire = Wire::default();
    let fake = Fake::new(|s| match (s.phase, s.path.as_str()) {
        (ScriptPhase::BeforeRequest, "users/list.yml") => ScriptOutcome {
            flow: Flow::Skip,
            ..Default::default()
        },
        _ => ScriptOutcome::default(),
    });
    let ran = run_plan(
        &c,
        &plan_of(&["users/list.yml", "health.yml"]),
        &input(),
        &wire,
        Some(&fake),
        &never,
    );
    let steps = ran.steps();
    assert!(steps[0].skipped && steps[0].passed);
    assert!(!steps[1].skipped);
    assert_eq!(wire.urls(), ["http://h/health"]);
    let s = &ran.summary;
    assert_eq!((s.requests, s.passed, s.failed, s.skipped), (2, 1, 0, 1));
    assert!(!s.stopped);
}

#[test]
fn set_next_jumps_by_name_or_path_within_the_iteration() {
    let c = collection();
    let all = plan(&c, None);
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    for target in ["Create user", "users/create.yml"] {
        let wire = Wire::default();
        let fake = Fake::new(move |s| match (s.phase, s.path.as_str()) {
            (ScriptPhase::Tests, "users/list.yml") => ScriptOutcome {
                flow: Flow::Next {
                    request: Some(target.to_owned()),
                },
                ..Default::default()
            },
            _ => ScriptOutcome::default(),
        });
        let plan = RunPlan {
            iterations: 2,
            ..plan_of(&all)
        };
        let ran = run_plan(&c, &plan, &input(), &wire, Some(&fake), &never);
        assert_eq!(
            ran.step_paths(),
            [
                (0, "users/list.yml".into()),
                (0, "users/create.yml".into()),
                (0, "health.yml".into()),
                (1, "users/list.yml".into()),
                (1, "users/create.yml".into()),
                (1, "health.yml".into()),
            ],
            "{target}"
        );
        assert!(!ran.summary.stopped);
    }
}

#[test]
fn set_next_can_jump_back() {
    let c = collection();
    let wire = Wire::default();
    let count = Cell::new(0);
    let fake = Fake::new(move |s| match (s.phase, s.path.as_str()) {
        // Loops back twice, then lets the run continue.
        (ScriptPhase::Tests, "health.yml") if count.get() < 2 => {
            count.set(count.get() + 1);
            ScriptOutcome {
                flow: Flow::Next {
                    request: Some("List users".into()),
                },
                ..Default::default()
            }
        }
        _ => ScriptOutcome::default(),
    });
    let ran = run_plan(
        &c,
        &plan_of(&["users/list.yml", "health.yml"]),
        &input(),
        &wire,
        Some(&fake),
        &never,
    );
    assert_eq!(ran.summary.requests, 6);
}

#[test]
fn set_next_null_stops_and_an_unknown_target_stops_with_a_note() {
    let c = collection();
    let all = plan(&c, None);
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    for (target, note) in [(None, false), (Some("No such request"), true)] {
        let wire = Wire::default();
        let fake = Fake::new(move |s| match (s.phase, s.path.as_str()) {
            (ScriptPhase::AfterResponse, "users/admin/get.yml") => ScriptOutcome {
                flow: Flow::Next {
                    request: target.map(str::to_owned),
                },
                ..Default::default()
            },
            _ => ScriptOutcome::default(),
        });
        let plan = RunPlan {
            iterations: 2,
            ..plan_of(&all)
        };
        let ran = run_plan(&c, &plan, &input(), &wire, Some(&fake), &never);
        assert_eq!(
            ran.step_paths(),
            [
                (0, "users/list.yml".into()),
                (0, "users/admin/get.yml".into())
            ]
        );
        assert!(ran.summary.stopped);
        let notes = &ran.steps()[1].report.notes;
        if note {
            assert_eq!(notes.len(), 1, "{notes:?}");
            assert_eq!(notes[0].source, "runner");
            assert_eq!(notes[0].level, NoteLevel::Error);
            assert!(
                notes[0].text.contains("No such request"),
                "{}",
                notes[0].text
            );
        } else {
            assert!(notes.is_empty());
        }
        assert!(matches!(ran.events.last(), Some(RunEvent::Finished { .. })));
    }
}

// ---------------------------------------------------------------------------------------------
// bail, cancel, delay

#[test]
fn bail_stops_at_the_first_failed_step() {
    let c = collection();
    let fails = |s: &Seen| match (s.phase, s.path.as_str()) {
        (ScriptPhase::Tests, "users/admin/get.yml") => ScriptOutcome {
            tests: vec![test_result("fails", false)],
            ..Default::default()
        },
        _ => ScriptOutcome::default(),
    };
    let all = plan(&c, None);
    let all: Vec<&str> = all.iter().map(String::as_str).collect();

    let wire = Wire::default();
    let fake = Fake::new(fails);
    let ran = run_plan(&c, &plan_of(&all), &input(), &wire, Some(&fake), &never);
    assert_eq!(ran.summary.requests, 4);
    assert_eq!(ran.summary.failed, 1);
    assert!(!ran.summary.stopped);

    let wire = Wire::default();
    let fake = Fake::new(fails);
    let plan = RunPlan {
        bail: true,
        iterations: 2,
        ..plan_of(&all)
    };
    let ran = run_plan(&c, &plan, &input(), &wire, Some(&fake), &never);
    assert_eq!(ran.summary.requests, 2);
    assert_eq!(ran.summary.failed, 1);
    assert!(ran.summary.stopped);
}

#[test]
fn cancel_is_polled_before_each_request() {
    let c = collection();
    let wire = Wire::default();
    let sent = std::rc::Rc::new(Cell::new(0));
    let counter = std::rc::Rc::clone(&sent);
    let fake = Fake::new(move |s| {
        if s.phase == ScriptPhase::AfterResponse {
            counter.set(counter.get() + 1);
        }
        ScriptOutcome::default()
    });
    let cancelled = || sent.get() >= 2;
    let all = plan(&c, None);
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    let ran = run_plan(&c, &plan_of(&all), &input(), &wire, Some(&fake), &cancelled);
    assert_eq!(ran.summary.requests, 2);
    assert!(ran.summary.stopped);
    assert_eq!(wire.urls().len(), 2);
    assert!(matches!(ran.events.last(), Some(RunEvent::Finished { .. })));

    // Cancelled before it starts: nothing is sent, the run still finishes.
    let wire = Wire::default();
    let ran = run_plan(&c, &plan_of(&all), &input(), &wire, None, &|| true);
    assert_eq!(ran.summary.requests, 0);
    assert!(ran.summary.stopped);
    assert!(wire.urls().is_empty());
}

#[test]
fn delay_sleeps_between_requests_in_slices() {
    let c = collection();
    let wire = Wire::default();
    let plan = RunPlan {
        delay_ms: 120,
        iterations: 2,
        ..plan_of(&["users/list.yml", "health.yml"])
    };
    let ran = run_plan(&c, &plan, &input(), &wire, None, &never);
    // Three pauses between four requests, none before the first or after the last; each
    // pause in slices so cancelling does not wait for the whole delay.
    assert_eq!(ran.sleeps.iter().sum::<u64>(), 360, "{:?}", ran.sleeps);
    assert!(ran.sleeps.iter().all(|ms| *ms <= 50), "{:?}", ran.sleeps);
    assert_eq!(ran.summary.requests, 4);
}

#[test]
fn cancel_during_the_delay_stops_without_sending_again() {
    let c = collection();
    let wire = Wire::default();
    let cancel = Cell::new(false);
    let services = Services::none();
    let sleeps = Cell::new(0u32);
    let sleep = |_ms: u64| -> LocalBoxFuture<'static, ()> {
        // The user cancels during the second slice of the first pause.
        sleeps.set(sleeps.get() + 1);
        if sleeps.get() == 2 {
            cancel.set(true);
        }
        Box::pin(async {})
    };
    let plan = RunPlan {
        delay_ms: 10_000,
        ..plan_of(&["users/list.yml", "health.yml"])
    };
    let mut events = Vec::new();
    let summary = block_on(run_with(
        &c,
        &plan,
        &input(),
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &services,
        &mut |e| events.push(e),
        &|| cancel.get(),
        &sleep,
    ));
    assert_eq!(sleeps.get(), 2);
    assert_eq!(summary.requests, 1);
    assert!(summary.stopped);
    assert_eq!(wire.urls().len(), 1);
}

#[test]
fn the_platform_sleep_waits_natively_without_a_runtime() {
    let started = web_time::Instant::now();
    block_on(platform_sleep(30));
    assert!(started.elapsed() >= std::time::Duration::from_millis(30));
}

#[test]
fn run_uses_the_platform_sleep_and_measures_the_duration() {
    let c = collection();
    let wire = Wire::default();
    let plan = RunPlan {
        delay_ms: 40,
        ..plan_of(&["users/list.yml", "health.yml"])
    };
    let mut events = Vec::new();
    let summary = block_on(run(
        &c,
        &plan,
        &input(),
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |e| events.push(e),
        &never,
    ));
    assert_eq!(summary.requests, 2);
    assert!(summary.duration_ms >= 40, "{summary:?}");
}

// ---------------------------------------------------------------------------------------------
// assertions (declarative, strand C's evaluator)

#[test]
fn assertions_count_in_the_summary_and_fail_the_step() {
    let mut c = collection();
    let assertion = |value: &str| Assertion {
        expression: "res.status".into(),
        operator: "eq".into(),
        value: value.into(),
        description: None,
        enabled: true,
    };
    if let Item::Request(r) = &mut c.items[1] {
        r.spec.assertions = vec![assertion("200"), assertion("201")];
    }
    let wire = Wire::default();
    let ran = run_plan(&c, &plan_of(&["health.yml"]), &input(), &wire, None, &never);
    let s = &ran.summary;
    assert_eq!((s.assertions_passed, s.assertions_failed), (1, 1));
    assert_eq!(s.failed, 1);
}

// ---------------------------------------------------------------------------------------------
// parse_data

fn row(pairs: &[(&str, serde_json::Value)]) -> DataRow {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

#[test]
fn csv_rows_with_quotes_crlf_and_a_bom() {
    let text = "\u{feff}name,note,n\r\n\
                ada,\"hello, world\",1\r\n\
                \"bob\",\"say \"\"hi\"\"\",\r\n\
                \"multi\nline\",x,3\r\n";
    let rows = parse_data("users.csv", text.as_bytes()).unwrap();
    assert_eq!(
        rows,
        vec![
            row(&[
                ("name", json!("ada")),
                ("note", json!("hello, world")),
                ("n", json!("1"))
            ]),
            row(&[
                ("name", json!("bob")),
                ("note", json!("say \"hi\"")),
                ("n", json!(""))
            ]),
            row(&[
                ("name", json!("multi\nline")),
                ("note", json!("x")),
                ("n", json!("3"))
            ]),
        ]
    );
    // LF line ends, no final newline, blank lines skipped, upper-case extension.
    let rows = parse_data("U.CSV", b"a,b\n1,2\n\n3,4").unwrap();
    assert_eq!(
        rows,
        vec![
            row(&[("a", json!("1")), ("b", json!("2"))]),
            row(&[("a", json!("3")), ("b", json!("4"))]),
        ]
    );
    // Only a header: no rows.
    assert!(parse_data("h.csv", b"a,b\r\n").unwrap().is_empty());
}

#[test]
fn csv_errors_name_the_line() {
    let err = parse_data("x.csv", b"a,b\n1,2\n1,2,3\n").unwrap_err();
    assert!(err.contains("line 3"), "{err}");
    let err = parse_data("x.csv", b"a,b\n1\n").unwrap_err();
    assert!(err.contains("line 2"), "{err}");
    let err = parse_data("x.csv", b"a,b\n1,\"open\n2,3\n").unwrap_err();
    assert!(err.contains("line 2") && err.contains("quote"), "{err}");
    let err = parse_data("x.csv", b"a,b\n1,x\"y\n").unwrap_err();
    assert!(err.contains("line 2"), "{err}");
    let err = parse_data("x.csv", b"a,,b\n1,2,3\n").unwrap_err();
    assert!(err.contains("line 1") && err.contains("empty"), "{err}");
    let err = parse_data("x.csv", b"a,a\n1,2\n").unwrap_err();
    assert!(err.contains("line 1") && err.contains("\"a\""), "{err}");
    let err = parse_data("x.csv", b"").unwrap_err();
    assert!(err.contains("header"), "{err}");
    let err = parse_data("x.csv", b"a\n\xff\n").unwrap_err();
    assert!(err.contains("UTF-8"), "{err}");
}

#[test]
fn json_rows_keep_their_values() {
    let rows = parse_data(
        "d.json",
        br#"[{"id": 1, "name": "ada", "tags": ["x"], "ok": true, "none": null}, {"id": 2}]"#,
    )
    .unwrap();
    assert_eq!(
        rows,
        vec![
            row(&[
                ("id", json!(1)),
                ("name", json!("ada")),
                ("tags", json!(["x"])),
                ("ok", json!(true)),
                ("none", json!(null)),
            ]),
            row(&[("id", json!(2))]),
        ]
    );
    // Keys keep their order in the file.
    let rows = parse_data("d.json", br#"[{"z": 1, "a": 2}]"#).unwrap();
    assert_eq!(rows[0][0].0, "z");
    // With a BOM.
    assert!(
        parse_data("d.json", "\u{feff}[]".as_bytes())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn json_errors_are_clear() {
    let err = parse_data("d.json", br#"{"a": 1}"#).unwrap_err();
    assert!(err.contains("array"), "{err}");
    let err = parse_data("d.json", br#"[{"a": 1}, 2]"#).unwrap_err();
    assert!(err.contains("item 2") && err.contains("object"), "{err}");
    let err = parse_data("d.json", b"[\n{\"a\": 1},\n{\"a\" 2}\n]").unwrap_err();
    assert!(err.contains("line 3"), "{err}");
    let err = parse_data("d.txt", b"a,b").unwrap_err();
    assert!(err.contains(".csv") && err.contains(".json"), "{err}");
}

const NEWMAN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../testdata/import/postman/newman-globals-env-data-files-v2.0.bundle/"
);

#[test]
fn newman_corpus_files_as_data() {
    // The legacy globals file is an array of objects: a valid data file, values kept.
    let bytes = std::fs::read(format!(
        "{NEWMAN}newman-globals-env-data-files.postman_globals.json"
    ))
    .unwrap();
    let rows = parse_data("globals.json", &bytes).unwrap();
    assert!(!rows.is_empty());
    let first = &rows[0];
    assert!(first.contains(&("key".into(), json!("full_global_url"))));
    assert!(first.contains(&("enabled".into(), json!(true))));
    // An environment file is one object, not rows.
    let bytes = std::fs::read(format!(
        "{NEWMAN}newman-globals-env-data-files.postman_environment.json"
    ))
    .unwrap();
    let err = parse_data("env.json", &bytes).unwrap_err();
    assert!(err.contains("array"), "{err}");
}

#[test]
fn data_files_drive_a_run() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/runner-data/");
    for file in ["users.csv", "users.json"] {
        let bytes = std::fs::read(format!("{dir}{file}")).unwrap();
        let rows = parse_data(file, &bytes).unwrap();
        let c = collection();
        let wire = Wire::default();
        let plan = RunPlan {
            data: rows,
            ..plan_of(&["users/create.yml"])
        };
        run_plan(&c, &plan, &input(), &wire, None, &never);
        assert_eq!(
            wire.urls(),
            ["http://h/users/ada", "http://h/users/grace%20hopper"],
            "{file}"
        );
    }
}
