// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The script debugger (wave 19, strand Y): breakpoints, stepping, the inspector, expressions,
//! abort, secrets, the time limit, and that a normal run has no debug code in it.

use std::cell::RefCell;

use rt_core::model::ScriptPhase;
use rt_core::script::{
    ScriptCall, ScriptInfo, ScriptRequest, ScriptResponse, ScriptSource, ScriptVar,
};
use rt_core::services::ScriptRunner;
use rt_core::vars::VarSource;
use rt_script::debug::{
    Breakpoint, DebugCommand, DebugInput, DebugRunner, DebugSession, DebugState, DebugStatus,
    DebugTerminal, DebugValue,
};
use rt_script::{Limits, Sandbox, block_on};
use web_time::{Duration, Instant};

const CANARY: &str = "RT-CANARY-secret-7f3a91c2";

fn var(name: &str, value: &str, secret: bool) -> ScriptVar {
    ScriptVar {
        name: name.into(),
        value: value.into(),
        source: VarSource::Runtime,
        secret,
    }
}

fn input(code: &str) -> DebugInput {
    DebugInput {
        phase: ScriptPhase::BeforeRequest,
        scripts: vec![ScriptSource {
            origin: "request".into(),
            code: code.into(),
        }],
        request: ScriptRequest {
            method: "GET".into(),
            url: "http://h/{{id}}".into(),
            headers: vec![("X-A".into(), "1".into())],
            body: None,
        },
        response: None,
        variables: vec![var("id", "7", false), var("token", CANARY, true)],
        info: ScriptInfo::default(),
        origin: "request".into(),
        breakpoints: Vec::new(),
        stop_on_entry: false,
    }
}

fn bp(line: u32) -> Breakpoint {
    Breakpoint {
        line,
        condition: None,
    }
}

fn session(input: DebugInput) -> DebugSession {
    DebugSession::new(1, Limits::default(), input).expect("session")
}

fn at(state: &DebugState) -> u32 {
    assert_eq!(state.status, DebugStatus::Paused, "{state:?}");
    state.pause.as_ref().expect("a pause").line
}

fn reason(state: &DebugState) -> &str {
    &state.pause.as_ref().expect("a pause").reason
}

fn scope<'a>(state: &'a DebugState, name: &str) -> &'a [DebugValue] {
    &state
        .pause
        .as_ref()
        .expect("a pause")
        .scopes
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no scope {name}: {state:?}"))
        .values
}

fn value<'a>(values: &'a [DebugValue], name: &str) -> &'a DebugValue {
    values
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("no value {name}: {values:?}"))
}

const SCRIPT: &str = "\
let a = 1;
const b = a + 1;
function twice(x) {
  const y = x * 2;
  return y;
}
const c = twice(b);
console.log('c', c);
rt.vars.set('c', c);
";

#[test]
fn a_breakpoint_stops_the_script_with_its_variables() {
    block_on(async {
        let mut input = input(SCRIPT);
        input.breakpoints = vec![bp(7)];
        let mut s = session(input);
        let state = s.start().await;
        assert_eq!(at(&state), 7);
        assert_eq!(reason(&state), "breakpoint");
        let globals = scope(&state, "global");
        assert_eq!(value(globals, "a").preview, "1");
        assert_eq!(value(globals, "b").preview, "2");
        assert_eq!(value(globals, "b").kind, "number");
        // `c` is declared on this line and not initialised yet.
        assert_eq!(value(globals, "c").kind, "uninitialized");
        let frames = &state.pause.as_ref().unwrap().frames;
        assert_eq!(frames.len(), 1);
        assert_eq!(
            (frames[0].function.as_str(), frames[0].line),
            ("request", 7)
        );

        let done = s.command(DebugCommand::Continue).await;
        assert_eq!(done.status, DebugStatus::Finished, "{done:?}");
        assert_eq!(done.output, ["c 4"]);
        assert_eq!(done.error, None);
        let outcome = s.take_outcome().expect("outcome");
        assert_eq!(outcome.var_changes.len(), 1);
        assert_eq!(outcome.var_changes[0].value.as_deref(), Some("4"));
    });
}

#[test]
fn without_breakpoints_a_script_runs_to_the_end() {
    block_on(async {
        let mut s = session(input(SCRIPT));
        let state = s.start().await;
        assert_eq!(state.status, DebugStatus::Finished);
        assert_eq!(state.output, ["c 4"]);
    });
}

#[test]
fn stop_on_entry_stops_at_the_first_statement() {
    block_on(async {
        let mut input = input("\n\nlet a = 1;\nlet b = 2;\n");
        input.stop_on_entry = true;
        let mut s = session(input);
        let state = s.start().await;
        assert_eq!(at(&state), 3);
        assert_eq!(reason(&state), "entry");
        let state = s.command(DebugCommand::StepOver).await;
        assert_eq!(at(&state), 4);
        assert_eq!(reason(&state), "step");
        let state = s.command(DebugCommand::StepOver).await;
        assert_eq!(state.status, DebugStatus::Finished);
    });
}

#[test]
fn step_over_goes_past_a_call_and_step_into_enters_it() {
    block_on(async {
        let mut input = input(SCRIPT);
        input.breakpoints = vec![bp(7)];
        let mut s = session(input);
        assert_eq!(at(&s.start().await), 7);
        // Over: the call runs, the next stop is line 8.
        let over = s.command(DebugCommand::StepOver).await;
        assert_eq!(at(&over), 8);

        // Into: the body of `twice`, with its own frame and locals.
        let mut input = self::input(SCRIPT);
        input.breakpoints = vec![bp(7)];
        let mut s = session(input);
        assert_eq!(at(&s.start().await), 7);
        let into = s.command(DebugCommand::StepInto).await;
        assert_eq!(at(&into), 4);
        let frames = &into.pause.as_ref().unwrap().frames;
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].function, "twice");
        assert_eq!(frames[1].function, "request");
        assert_eq!(frames[1].line, 7);
        let local = scope(&into, "local");
        assert_eq!(value(local, "x").preview, "2");
        // Out: back in the caller, after the call returned.
        let into = s.command(DebugCommand::StepInto).await;
        assert_eq!(at(&into), 5);
        assert_eq!(value(scope(&into, "local"), "y").preview, "4");
        let out = s.command(DebugCommand::StepOut).await;
        assert_eq!(at(&out), 8);
        assert_eq!(out.pause.as_ref().unwrap().frames.len(), 1);
    });
}

#[test]
fn closures_show_the_scopes_around_a_function() {
    block_on(async {
        let code = "\
function outer() {
  const o = 'outer';
  function inner(p) {
    const i = p + 1;
    return i;
  }
  return inner(1);
}
const g = outer();
";
        let mut input = input(code);
        input.breakpoints = vec![bp(5)];
        let mut s = session(input);
        let state = s.start().await;
        assert_eq!(at(&state), 5);
        assert_eq!(value(scope(&state, "local"), "i").preview, "2");
        assert_eq!(value(scope(&state, "local"), "p").preview, "1");
        assert_eq!(value(scope(&state, "closure"), "o").preview, "\"outer\"");
        let frames: Vec<&str> = state
            .pause
            .as_ref()
            .unwrap()
            .frames
            .iter()
            .map(|f| f.function.as_str())
            .collect();
        assert_eq!(frames, ["inner", "outer", "request"]);
    });
}

#[test]
fn the_rt_objects_are_in_the_inspector() {
    block_on(async {
        let mut input = input("rt.vars.set('k', 'v');\nconsole.log(1);\n");
        input.breakpoints = vec![bp(2)];
        let mut s = session(input);
        let state = s.start().await;
        assert_eq!(at(&state), 2);
        let rt = scope(&state, "rt");
        let request = value(rt, "request");
        assert!(
            request.preview.contains("\"method\":\"GET\""),
            "{request:?}"
        );
        assert!(request.preview.contains("http://h/{{id}}"), "{request:?}");
        assert!(request.preview.contains("X-A"));
        assert!(value(rt, "vars").preview.contains("\"k\":\"v\""));
        assert!(value(rt, "vars").preview.contains("\"id\":\"7\""));
        assert!(value(rt, "info").children > 0);
    });
}

#[test]
fn an_expression_is_evaluated_in_the_paused_scope() {
    block_on(async {
        let mut input = input(SCRIPT);
        input.breakpoints = vec![bp(4)];
        let mut s = session(input);
        let state = s.start().await;
        assert_eq!(at(&state), 4);
        let state = s
            .command(DebugCommand::Evaluate {
                expression: "x * 10 + b".into(),
            })
            .await;
        // Still stopped, in the same place, with the answer.
        assert_eq!(at(&state), 4);
        let evaluated = state.pause.as_ref().unwrap().evaluated.clone().unwrap();
        assert_eq!(evaluated.name, "x * 10 + b");
        assert_eq!(evaluated.preview, "22");
        // An error is an answer, not the end of the session.
        let state = s
            .command(DebugCommand::Evaluate {
                expression: "nothing.here".into(),
            })
            .await;
        let evaluated = state.pause.as_ref().unwrap().evaluated.clone().unwrap();
        assert_eq!(evaluated.kind, "error");
        assert!(
            evaluated.preview.contains("ReferenceError"),
            "{evaluated:?}"
        );
        // The script can be changed from here, like in a browser's console.
        let state = s
            .command(DebugCommand::Evaluate {
                expression: "rt.vars.set('from-console', 'yes')".into(),
            })
            .await;
        assert!(state.pause.as_ref().unwrap().evaluated.is_some());
        let state = s.command(DebugCommand::Continue).await;
        assert_eq!(state.status, DebugStatus::Finished);
        let outcome = s.take_outcome().unwrap();
        assert!(outcome.var_changes.iter().any(|c| c.name == "from-console"));
    });
}

#[test]
fn abort_ends_the_session_without_running_the_rest() {
    block_on(async {
        let mut input = input("console.log('first');\nconsole.log('second');\n");
        input.breakpoints = vec![bp(2)];
        let mut s = session(input);
        assert_eq!(at(&s.start().await), 2);
        let state = s.command(DebugCommand::Abort).await;
        assert_eq!(state.status, DebugStatus::Aborted);
        assert!(state.pause.is_none());
        assert!(s.take_outcome().is_none());
        // Nothing runs after an abort.
        let again = s.command(DebugCommand::Continue).await;
        assert_eq!(again.status, DebugStatus::Aborted);
    });
}

#[test]
fn breakpoints_can_change_while_stopped_and_move_to_a_statement() {
    block_on(async {
        let code = "let a = 1;\n\n// a comment\nlet b = 2;\nlet c = 3;\nlet d = 4;\n";
        let mut input = input(code);
        input.breakpoints = vec![bp(1)];
        let mut s = session(input);
        assert_eq!(at(&s.start().await), 1);
        // Line 3 is a comment: the breakpoint sits on the next statement, line 4.
        let state = s
            .command(DebugCommand::SetBreakpoints {
                breakpoints: vec![bp(3), bp(6)],
            })
            .await;
        assert_eq!(at(&state), 1);
        let state = s.command(DebugCommand::Continue).await;
        assert_eq!(at(&state), 4);
        let state = s.command(DebugCommand::Continue).await;
        assert_eq!(at(&state), 6);
        let state = s
            .command(DebugCommand::SetBreakpoints {
                breakpoints: Vec::new(),
            })
            .await;
        assert_eq!(at(&state), 6);
        assert_eq!(
            s.command(DebugCommand::Continue).await.status,
            DebugStatus::Finished
        );
    });
}

#[test]
fn a_condition_decides_whether_to_stop() {
    block_on(async {
        let code =
            "let sum = 0;\nfor (let i = 0; i < 5; i++) {\n  sum += i;\n}\nconsole.log(sum);\n";
        let mut input = input(code);
        input.breakpoints = vec![Breakpoint {
            line: 3,
            condition: Some("i === 3".into()),
        }];
        let mut s = session(input);
        let state = s.start().await;
        assert_eq!(at(&state), 3);
        assert_eq!(value(scope(&state, "local"), "i").preview, "3");
        assert_eq!(value(scope(&state, "global"), "sum").preview, "3");
        let state = s.command(DebugCommand::Continue).await;
        assert_eq!(state.status, DebugStatus::Finished);
        assert_eq!(state.output, ["10"]);
    });
}

#[test]
fn loops_stop_every_time_around() {
    block_on(async {
        let code = "const out = [];\nfor (const n of [1, 2, 3]) {\n  out.push(n);\n}\nconsole.log(out.join());\n";
        let mut input = input(code);
        input.breakpoints = vec![bp(3)];
        let mut s = session(input);
        let mut seen = Vec::new();
        let mut state = s.start().await;
        while state.status == DebugStatus::Paused {
            seen.push(value(scope(&state, "local"), "n").preview.clone());
            state = s.command(DebugCommand::Continue).await;
        }
        assert_eq!(seen, ["1", "2", "3"]);
        assert_eq!(state.output, ["1,2,3"]);
    });
}

#[test]
fn an_error_is_the_end_with_its_line() {
    block_on(async {
        let mut input = input("let a = 1;\nlet b = 2;\nthrow new Error('boom');\n");
        input.breakpoints = vec![bp(2)];
        let mut s = session(input);
        assert_eq!(at(&s.start().await), 2);
        let state = s.command(DebugCommand::Continue).await;
        assert_eq!(state.status, DebugStatus::Finished);
        let error = state.error.expect("an error");
        assert!(error.contains("boom"), "{error}");
        // Instrumenting keeps the lines: the error is on line 3 as the user wrote it.
        assert!(error.contains("line 3"), "{error}");
    });
}

#[test]
fn the_phases_with_a_response_and_the_test_results() {
    block_on(async {
        let code = "\
test('status is 200', () => {
  const s = rt.response.status;
  if (s !== 200) throw new Error('status ' + s);
});
test('has a body', () => {
  if (rt.response.json().ok !== false) throw new Error('not false');
});
";
        let mut input = input(code);
        input.phase = ScriptPhase::Tests;
        input.response = Some(ScriptResponse {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: br#"{"ok":true}"#.to_vec(),
            time_ms: 5,
            url: "http://h/7".into(),
        });
        input.breakpoints = vec![bp(2)];
        let mut s = session(input);
        let state = s.start().await;
        assert_eq!(at(&state), 2);
        let rt = scope(&state, "rt");
        assert!(value(rt, "response").preview.contains("\"status\":200"));
        // Stopped inside the callback of `test`: two frames.
        assert_eq!(state.pause.as_ref().unwrap().frames.len(), 2);
        let state = s.command(DebugCommand::Continue).await;
        assert_eq!(state.status, DebugStatus::Finished, "{state:?}");
        assert!(
            state
                .output
                .contains(&"test passed: status is 200".to_string()),
            "{:?}",
            state.output
        );
        assert!(
            state
                .output
                .iter()
                .any(|l| l.starts_with("test failed: has a body")),
            "{:?}",
            state.output
        );
    });
}

#[test]
fn only_the_chosen_script_is_stopped_in() {
    block_on(async {
        let mut input = input("console.log('request');\n");
        input.scripts.insert(
            0,
            ScriptSource {
                origin: "collection".into(),
                code: "console.log('collection');\nconsole.log('collection 2');\n".into(),
            },
        );
        input.origin = "request".into();
        input.breakpoints = vec![bp(1)];
        let mut s = session(input);
        let state = s.start().await;
        // The collection script ran on without stopping (its lines are not the target's).
        assert_eq!(at(&state), 1);
        assert_eq!(state.output, ["collection", "collection 2"]);
        let done = s.command(DebugCommand::Continue).await;
        assert_eq!(done.output, ["collection", "collection 2", "request"]);
    });
}

#[test]
fn a_script_that_is_not_there_is_refused() {
    let mut input = input("let a = 1;");
    input.origin = "folder:users".into();
    assert!(DebugSession::new(1, Limits::default(), input).is_err());
}

#[test]
fn secret_values_never_leave_a_pause() {
    block_on(async {
        let code = "\
const token = rt.vars.get('token');
const copy = token;
const obj = { auth: 'Bearer ' + token };
console.log('using', token);
throw new Error('failed with ' + token);
";
        let mut input = input(code);
        input.breakpoints = vec![bp(4)];
        let mut s = session(input);
        let state = s.start().await;
        assert_eq!(at(&state), 4);
        let mut states = vec![state];
        for expression in [
            "token",
            "copy",
            "obj",
            "rt.vars.get('token')",
            "JSON.stringify(rt.vars.toObject())",
            "[token]",
        ] {
            let state = s
                .command(DebugCommand::Evaluate {
                    expression: expression.into(),
                })
                .await;
            states.push(state);
        }
        states.push(s.command(DebugCommand::StepOver).await);
        states.push(s.command(DebugCommand::Continue).await);
        let last = states.last().unwrap();
        assert_eq!(last.status, DebugStatus::Finished);
        assert!(last.error.as_deref().unwrap().contains("***"));
        for state in &states {
            let text = serde_json::to_string(state).unwrap();
            assert!(!text.contains(CANARY), "the secret leaked: {text}");
        }
        // It is shown as masked, not left out.
        let first = serde_json::to_string(&states[0]).unwrap();
        assert!(first.contains("***"), "{first}");
        let outcome = s.take_outcome().unwrap();
        // (The runner's caller masks the outcome as for any send; the session's own state
        // is what the debugger shows.)
        assert!(outcome.error.is_some());
    });
}

#[test]
fn a_pause_does_not_use_up_the_time_limit() {
    block_on(async {
        let limits = Limits {
            time_ms: 700,
            ..Limits::default()
        };
        let mut input = input("let a = 1;\nlet b = 2;\nconsole.log('done');\n");
        input.breakpoints = vec![bp(2)];
        let mut s = DebugSession::new(1, limits, input).unwrap();
        assert_eq!(at(&s.start().await), 2);
        // Longer than the whole limit.
        rt_script_sleep(Duration::from_millis(1500));
        let state = s.command(DebugCommand::Continue).await;
        assert_eq!(state.status, DebugStatus::Finished);
        assert_eq!(state.error, None, "{state:?}");
        assert_eq!(state.output, ["done"]);
    });
}

fn rt_script_sleep(d: Duration) {
    std::thread::sleep(d);
}

#[test]
fn the_limits_still_stop_a_script_that_runs_away() {
    block_on(async {
        let limits = Limits {
            time_ms: 300,
            ..Limits::default()
        };
        let mut input = input("let n = 0;\nwhile (true) { n++; }\n");
        input.breakpoints = Vec::new();
        let mut s = DebugSession::new(1, limits, input).unwrap();
        let state = s.start().await;
        assert_eq!(state.status, DebugStatus::Finished);
        assert!(
            state.error.as_deref().unwrap_or("").contains("time limit"),
            "{state:?}"
        );
    });
}

#[test]
fn a_normal_run_has_no_debug_code() {
    let call = |code: &str| ScriptCall {
        phase: ScriptPhase::BeforeRequest,
        scripts: vec![ScriptSource {
            origin: "request".into(),
            code: code.into(),
        }],
        request: ScriptRequest {
            method: "GET".into(),
            url: "http://h/".into(),
            headers: Vec::new(),
            body: None,
        },
        response: None,
        variables: Vec::new(),
        info: ScriptInfo::default(),
        sender: None,
    };
    let outcome = block_on(Sandbox::default().run(call(
        "console.log(typeof __rt_b, typeof __rt_e, typeof __rt_x, typeof __rt_internal);",
    )));
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.logs[0].text,
        "undefined undefined undefined undefined"
    );
}

/// The bundled libraries are the biggest real JavaScript in the tree: instrumenting them must
/// keep them valid, and running them under the debugger must give what a normal run gives.
#[test]
fn instrumented_libraries_still_run() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("js");
    let files = ["modules", "shims"].into_iter().flat_map(|dir| {
        std::fs::read_dir(root.join(dir))
            .unwrap()
            .map(|e| e.unwrap().path())
    });
    for path in files {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let source = std::fs::read_to_string(&path).unwrap();
        let wrap = |body: &str| {
            format!(
                "const __m = {{ exports: {{}} }};\n(function (exports, require, module, __filename, __dirname) {{{body}\n}}).call(__m.exports, __m.exports, require, __m, 'x', '');\nconsole.log(typeof __m.exports, Object.keys(__m.exports).length > 0);\n"
            )
        };
        let normal = block_on(Sandbox::default().run(ScriptCall {
            phase: ScriptPhase::BeforeRequest,
            scripts: vec![ScriptSource {
                origin: "request".into(),
                code: wrap(&source),
            }],
            request: ScriptRequest {
                method: "GET".into(),
                url: "http://h/".into(),
                headers: Vec::new(),
                body: None,
            },
            response: None,
            variables: Vec::new(),
            info: ScriptInfo::default(),
            sender: None,
        }));
        // The wrapper itself is the script; only its body is instrumented.
        let inner = rt_script::debug::instrument::instrument(&source).code;
        let debugged = block_on(async {
            let mut s = DebugSession::new(
                1,
                Limits {
                    time_ms: 60_000,
                    ..Limits::default()
                },
                input(&wrap(&inner)),
            )
            .unwrap();
            let state = s.start().await;
            state
        });
        assert_eq!(normal.error, None, "{name}: normal run");
        assert_eq!(debugged.error, None, "{name}: debug run: {debugged:?}");
        assert_eq!(
            normal
                .logs
                .iter()
                .map(|l| l.text.clone())
                .collect::<Vec<_>>(),
            debugged.output,
            "{name}"
        );
    }
}

struct Scripted {
    commands: RefCell<Vec<DebugCommand>>,
    stops: RefCell<Vec<(u32, String)>>,
    ended: RefCell<Option<DebugState>>,
}

impl DebugTerminal for &Scripted {
    fn paused(&self, state: &DebugState) -> Option<DebugCommand> {
        let pause = state.pause.as_ref().unwrap();
        self.stops
            .borrow_mut()
            .push((pause.line, pause.reason.clone()));
        let mut commands = self.commands.borrow_mut();
        if commands.is_empty() {
            None
        } else {
            Some(commands.remove(0))
        }
    }

    fn finished(&self, state: &DebugState) {
        *self.ended.borrow_mut() = Some(state.clone());
    }
}

#[test]
fn the_terminal_runner_debugs_the_request_script_of_a_call() {
    let terminal = Scripted {
        commands: RefCell::new(vec![DebugCommand::StepOver, DebugCommand::Continue]),
        stops: RefCell::new(Vec::new()),
        ended: RefCell::new(None),
    };
    let runner = DebugRunner::new(&terminal, vec![1]);
    let outcome = block_on(runner.run(ScriptCall {
        phase: ScriptPhase::BeforeRequest,
        scripts: vec![ScriptSource {
            origin: "request".into(),
            code: "rt.request.headers.set('X-Debugged', 'yes');\nrt.vars.set('n', '1');\n".into(),
        }],
        request: ScriptRequest {
            method: "GET".into(),
            url: "http://h/".into(),
            headers: Vec::new(),
            body: None,
        },
        response: None,
        variables: Vec::new(),
        info: ScriptInfo::default(),
        sender: None,
    }));
    assert_eq!(outcome.error, None);
    // The real outcome comes back: the header the script set, the variable.
    let request = outcome.request.expect("a changed request");
    assert!(request.headers.iter().any(|(k, _)| k == "X-Debugged"));
    assert_eq!(outcome.var_changes.len(), 1);
    assert_eq!(
        *terminal.stops.borrow(),
        [(1, "breakpoint".to_string()), (2, "step".to_string())]
    );
    assert_eq!(
        terminal.ended.borrow().as_ref().unwrap().status,
        DebugStatus::Finished
    );
}

/// `debug_step` (perf/README.md): one step, from a stop to the next stop, median of 100.
/// `RT_PERF_OUT=<dir> cargo test --release -p rt-script --test debug -- debug_step_is_measured --exact --nocapture`
#[test]
fn debug_step_is_measured() {
    let code: String = (1..=120)
        .map(|n| format!("let v{n} = {n};\n"))
        .collect::<String>();
    let mut times = Vec::new();
    block_on(async {
        let mut input = input(&code);
        input.stop_on_entry = true;
        let mut s = session(input);
        let mut state = s.start().await;
        // A warm engine first: the first run compiles the module.
        for _ in 0..100 {
            assert_eq!(state.status, DebugStatus::Paused, "{state:?}");
            let started = Instant::now();
            state = s.command(DebugCommand::StepOver).await;
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        s.command(DebugCommand::Abort).await;
    });
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    println!("debug_step: median {median:.3} ms");
    if !cfg!(debug_assertions) {
        assert!(median < 20.0, "{median} ms");
    }
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({ "debug_step": median, "max_ms": times.last() });
        std::fs::write(
            std::path::Path::new(&dir).join("debugger.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}
