// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! One debug run. The engine runs the instrumented script; at a stop the guest makes one host
//! call (`debug-pause`) that this module does not answer until the user's next
//! [`DebugCommand`] arrives, so the engine instance stays suspended in the middle of the
//! script (wasmi keeps it: the host call is a resumable trap). The session owns the run as a
//! future and polls it only as far as the next stop.

use std::cell::RefCell;
use std::rc::Rc;
use std::task::Poll;

use rt_core::model::ScriptPhase;
use rt_core::script::{
    ScriptCall, ScriptInfo, ScriptOutcome, ScriptRequest, ScriptResponse, ScriptSource, ScriptVar,
};
use rt_core::services::LocalBoxFuture;
use serde_json::{Value, json};
use web_time::{Duration, Instant};

use super::{
    Breakpoint, DebugCommand, DebugPause, DebugState, DebugStatus, DebugValue, instrument,
};
use crate::engine::{self, HostCalls};
use crate::{Limits, Sandbox};

/// Everything one debug run needs, owned.
#[derive(Debug, Clone)]
pub struct DebugInput {
    pub phase: ScriptPhase,
    /// All scripts of the phase (collection, folders, request), as `scripts_for` lists them;
    /// only the one with `origin` is stopped in, the others run as they are.
    pub scripts: Vec<ScriptSource>,
    pub request: ScriptRequest,
    pub response: Option<ScriptResponse>,
    pub variables: Vec<ScriptVar>,
    pub info: ScriptInfo,
    /// `collection`, `folder:<path>` or `request`.
    pub origin: String,
    pub breakpoints: Vec<Breakpoint>,
    pub stop_on_entry: bool,
}

impl DebugInput {
    /// The input for the scripts of a call (the terminal debugger).
    pub fn from_call(
        call: &ScriptCall<'_>,
        origin: &str,
        breakpoints: Vec<Breakpoint>,
        stop_on_entry: bool,
    ) -> Self {
        Self {
            phase: call.phase,
            scripts: call.scripts.clone(),
            request: call.request.clone(),
            response: call.response.clone(),
            variables: call.variables.clone(),
            info: call.info.clone(),
            origin: origin.to_owned(),
            breakpoints,
            stop_on_entry,
        }
    }

    /// Every secret value in scope, longest first (what is masked in all output).
    pub fn secrets(&self) -> Vec<String> {
        let mut values: Vec<String> = self
            .variables
            .iter()
            .filter(|v| v.secret && !v.value.is_empty())
            .map(|v| v.value.clone())
            .collect();
        values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        values.dedup();
        values
    }
}

/// `text` with every secret (raw, as in a Debug message, as inside a JSON string) replaced.
pub(crate) fn mask(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        let json = serde_json::to_string(secret)
            .map(|s| s[1..s.len() - 1].to_owned())
            .unwrap_or_default();
        for form in [secret.clone(), secret.escape_debug().to_string(), json] {
            if !form.is_empty() && out.contains(&form) {
                out = out.replace(&form, rt_core::send::MASK);
            }
        }
    }
    out
}

#[derive(Default)]
struct Shared {
    pause: Option<DebugPause>,
    output: Vec<String>,
    /// The guest is stopped and waits for `command`.
    waiting: bool,
    command: Option<DebugCommand>,
    paused_at: Option<Instant>,
    credit: Duration,
    /// The breakpoints, on lines that have a marker.
    breakpoints: Vec<Breakpoint>,
}

struct DebugHost {
    shared: Rc<RefCell<Shared>>,
    secrets: Vec<String>,
}

impl DebugHost {
    fn mask_value(&self, value: &mut DebugValue) {
        value.name = mask(&value.name, &self.secrets);
        value.preview = mask(&value.preview, &self.secrets);
    }

    fn mask_pause(&self, pause: &mut DebugPause) {
        for f in &mut pause.frames {
            f.function = mask(&f.function, &self.secrets);
        }
        for scope in &mut pause.scopes {
            for v in &mut scope.values {
                self.mask_value(v);
            }
        }
        if let Some(v) = &mut pause.evaluated {
            self.mask_value(v);
        }
    }

    async fn pause(&self, message: &Value) -> Vec<u8> {
        let mut pause: DebugPause =
            serde_json::from_value(message["pause"].clone()).unwrap_or(DebugPause {
                line: 0,
                reason: "step".into(),
                frames: Vec::new(),
                scopes: Vec::new(),
                evaluated: None,
            });
        pause.evaluated = serde_json::from_value(message["evaluated"].clone()).ok();
        self.mask_pause(&mut pause);
        let output: Vec<String> =
            serde_json::from_value(message["pause"]["output"].clone()).unwrap_or_default();
        {
            let mut shared = self.shared.borrow_mut();
            shared.pause = Some(pause);
            shared.output = output.iter().map(|l| mask(l, &self.secrets)).collect();
            shared.waiting = true;
            shared.paused_at = Some(Instant::now());
            // What the guest spent getting ready (reading the variables) is not the script's.
            let busy = message["busy"]
                .as_f64()
                .unwrap_or(0.0)
                .clamp(0.0, 3_600_000.0);
            shared.credit += Duration::from_secs_f64(busy / 1000.0);
        }
        let command = std::future::poll_fn(|_| {
            let mut shared = self.shared.borrow_mut();
            match shared.command.take() {
                Some(c) => {
                    shared.waiting = false;
                    if let Some(at) = shared.paused_at.take() {
                        shared.credit += at.elapsed();
                    }
                    Poll::Ready(c)
                }
                None => Poll::Pending,
            }
        })
        .await;
        let breakpoints = self.shared.borrow().breakpoints.clone();
        let mut reply = match command {
            DebugCommand::Continue => json!({ "cmd": "continue" }),
            DebugCommand::StepOver => json!({ "cmd": "step-over" }),
            DebugCommand::StepInto => json!({ "cmd": "step-into" }),
            DebugCommand::StepOut => json!({ "cmd": "step-out" }),
            DebugCommand::Evaluate { expression } => {
                json!({ "cmd": "evaluate", "expression": expression })
            }
            // Handled by the session; never reaches the guest.
            DebugCommand::SetBreakpoints { .. } | DebugCommand::Abort => json!({ "cmd": "none" }),
        };
        reply["breakpoints"] = json!(breakpoints);
        serde_json::to_vec(&reply).expect("JSON")
    }
}

impl HostCalls for DebugHost {
    fn call<'a>(&'a self, message: Vec<u8>, deadline: Instant) -> LocalBoxFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let value: Value = serde_json::from_slice(&message).unwrap_or(Value::Null);
            if value["op"] == "debug-pause" {
                return self.pause(&value).await;
            }
            // `rt.sleep` and the rest: as in a normal run (no `rt.sendRequest` here).
            let base = crate::Host { sender: None };
            base.call(message, deadline).await
        })
    }

    fn take_credit(&self) -> Duration {
        std::mem::take(&mut self.shared.borrow_mut().credit)
    }

    /// Looking at a script burns fuel too (reading values runs JavaScript): the fuel backstop
    /// is wide here, the wall clock (with the pause credited) is the limit.
    fn fuel_factor(&self) -> u64 {
        100
    }
}

/// A debug run: started, paused at a stop, resumed with commands.
pub struct DebugSession {
    id: u32,
    sandbox: Sandbox,
    run: Option<LocalBoxFuture<'static, Result<Vec<u8>, engine::Failure>>>,
    shared: Rc<RefCell<Shared>>,
    secrets: Vec<String>,
    marker_lines: Vec<u32>,
    state: DebugState,
    outcome: Option<ScriptOutcome>,
}

/// Breakpoints moved to the next line that has a marker (the lines a statement starts on).
fn resolve(lines: &[u32], list: &[Breakpoint]) -> Vec<Breakpoint> {
    list.iter()
        .filter_map(|b| {
            lines
                .iter()
                .copied()
                .find(|l| *l >= b.line)
                .map(|line| Breakpoint {
                    line,
                    condition: b.condition.clone(),
                })
        })
        .collect()
}

impl DebugSession {
    /// Prepares the run; nothing executes until [`DebugSession::start`].
    pub fn new(id: u32, limits: Limits, input: DebugInput) -> Result<Self, String> {
        let secrets = input.secrets();
        let mut scripts = input.scripts.clone();
        let mut marker_lines = Vec::new();
        let mut globals = String::new();
        let mut found = false;
        for script in &mut scripts {
            if script.origin == input.origin {
                let instrumented = instrument::instrument(&script.code);
                script.code = instrumented.code;
                marker_lines = instrumented.lines;
                globals = instrumented.globals;
                found = true;
            }
        }
        if !found {
            return Err(format!("there is no script for {}", input.origin));
        }
        let call = ScriptCall {
            phase: input.phase,
            scripts,
            request: input.request.clone(),
            response: input.response.clone(),
            variables: input.variables.clone(),
            info: input.info.clone(),
            sender: None,
        };
        let mut message: Value = serde_json::from_slice(&crate::input_of(&call))
            .map_err(|e| format!("invalid script input: {e}"))?;
        let breakpoints = resolve(&marker_lines, &input.breakpoints);
        message["debug"] = json!({
            "origin": input.origin,
            "breakpoints": breakpoints,
            "stop_on_entry": input.stop_on_entry,
            "globals": globals,
        });
        let bytes = serde_json::to_vec(&message).expect("JSON");
        let shared = Rc::new(RefCell::new(Shared {
            breakpoints,
            ..Shared::default()
        }));
        let host = DebugHost {
            shared: Rc::clone(&shared),
            secrets: secrets.clone(),
        };
        let (time_ms, memory_bytes) = (limits.time_ms, limits.memory_bytes);
        let run: LocalBoxFuture<'static, _> =
            Box::pin(async move { engine::run(&bytes, time_ms, memory_bytes, &host).await });
        Ok(Self {
            id,
            sandbox: Sandbox::new(limits),
            run: Some(run),
            shared,
            secrets,
            marker_lines,
            state: DebugState {
                session: id,
                status: DebugStatus::Running,
                pause: None,
                output: Vec::new(),
                error: None,
            },
            outcome: None,
        })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// The lines the script can stop on.
    pub fn marker_lines(&self) -> &[u32] {
        &self.marker_lines
    }

    /// The state as of the last stop or end.
    pub fn state(&self) -> DebugState {
        self.state.clone()
    }

    /// The outcome of the script once the run finished (`None` before, and after an abort).
    pub fn take_outcome(&mut self) -> Option<ScriptOutcome> {
        self.outcome.take()
    }

    /// Runs to the first stop (or the end).
    pub async fn start(&mut self) -> DebugState {
        self.advance().await
    }

    /// Hands the paused script a command and runs to the next stop (or the end).
    pub async fn command(&mut self, command: DebugCommand) -> DebugState {
        if self.run.is_none() || self.state.status != DebugStatus::Paused {
            return self.state();
        }
        match command {
            DebugCommand::Abort => {
                self.run = None;
                self.state.status = DebugStatus::Aborted;
                self.state.pause = None;
                self.state()
            }
            DebugCommand::SetBreakpoints { breakpoints } => {
                self.shared.borrow_mut().breakpoints = resolve(&self.marker_lines, &breakpoints);
                self.state()
            }
            other => {
                self.shared.borrow_mut().command = Some(other);
                self.state.status = DebugStatus::Running;
                self.advance().await
            }
        }
    }

    async fn advance(&mut self) -> DebugState {
        let Some(run) = self.run.as_mut() else {
            return self.state();
        };
        let shared = Rc::clone(&self.shared);
        let result = std::future::poll_fn(|cx| match run.as_mut().poll(cx) {
            Poll::Ready(r) => Poll::Ready(Some(r)),
            Poll::Pending => {
                let s = shared.borrow();
                if s.waiting && s.command.is_none() {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                }
            }
        })
        .await;
        match result {
            None => {
                let s = self.shared.borrow();
                self.state = DebugState {
                    session: self.id,
                    status: DebugStatus::Paused,
                    pause: s.pause.clone(),
                    output: s.output.clone(),
                    error: None,
                };
            }
            Some(result) => {
                self.run = None;
                let outcome = self.sandbox.outcome_of(result);
                let mut output: Vec<String> = outcome
                    .logs
                    .iter()
                    .map(|l| mask(&l.text, &self.secrets))
                    .collect();
                for t in &outcome.tests {
                    output.push(mask(
                        &match (&t.passed, &t.error) {
                            (true, _) => format!("test passed: {}", t.name),
                            (false, Some(e)) => format!("test failed: {}: {e}", t.name),
                            (false, None) => format!("test failed: {}", t.name),
                        },
                        &self.secrets,
                    ));
                }
                self.state = DebugState {
                    session: self.id,
                    status: DebugStatus::Finished,
                    pause: None,
                    output,
                    error: outcome.error.as_ref().map(|e| mask(e, &self.secrets)),
                };
                self.outcome = Some(outcome);
            }
        }
        self.state()
    }
}
