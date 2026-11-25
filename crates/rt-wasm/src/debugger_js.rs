// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand Y: the web shell's `Workspace` methods for `Backend.debugger`, over
//! `rt_script::debug`. The same engine as natively (QuickJS in wasmi); the paused script is a
//! suspended wasmi call, kept in a session here until the next command.
//!
//! ```text
//! debuggerStart(target: DebugTarget, input: SendInput) -> Promise<DebugState>
//! debuggerCommand(session: number, command: DebugCommand) -> Promise<DebugState>
//! debuggerState(session: number) -> DebugState
//! debuggerStop(session: number) -> void
//! ```
//!
//! `input` is what a send gets (environment, session variables, secret values read by the
//! shell): the scripts see the secrets, nothing the debugger shows does (masked). The shell
//! checks that the collection is trusted before it calls `debuggerStart`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use rt_app::SendInput;
use rt_core::script::ScriptResponse;
use rt_script::Limits;
use rt_script::debug::{
    DebugCommand, DebugSession, DebugState, DebugStatus, DebugTarget, PrepareArgs, prepare,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, int, to_js};

thread_local! {
    /// The sessions; a session is out of the map while a command runs.
    static SESSIONS: RefCell<HashMap<u32, DebugSession>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u32> = const { Cell::new(0) };
}

fn error(message: String) -> JsValue {
    app_error(rt_app::AppError::Shell(message))
}

fn put_back(id: u32, session: DebugSession, state: &DebugState) {
    // A finished session keeps its last state until the shell stops it.
    let _ = state;
    SESSIONS.with(|s| s.borrow_mut().insert(id, session));
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = debuggerStart)]
    pub fn debugger_start(
        &self,
        target: JsValue,
        input: JsValue,
    ) -> Result<js_sys::Promise, JsValue> {
        let target: DebugTarget = from_js(&target, "target")?;
        let input: SendInput = from_js(&input, "input")?;
        let snapshot = self
            .workspace()
            .snapshot(&target.request_path, input)
            .map_err(app_error)?;
        let response = match target.response_id {
            None => None,
            Some(id) => {
                let responses = self.responses();
                let report = responses.report(id).map_err(app_error)?;
                let received = report
                    .response
                    .as_ref()
                    .ok_or_else(|| app_error(rt_app::AppError::ResponseNotFound(id)))?;
                Some(ScriptResponse {
                    status: received.status,
                    headers: received.headers.clone(),
                    body: responses.body(id).map_err(app_error)?.to_vec(),
                    time_ms: u64::try_from(received.total.as_millis()).unwrap_or(u64::MAX),
                    url: received.url.clone(),
                })
            }
        };
        let prepared = prepare(
            PrepareArgs {
                collection: &snapshot.collection,
                runtime: snapshot.vars.clone(),
                response,
            },
            &target,
        )
        .map_err(error)?;
        let id = NEXT.with(|n| {
            n.set(n.get() + 1);
            n.get()
        });
        let mut session = DebugSession::new(id, Limits::default(), prepared).map_err(error)?;
        Ok(future_to_promise(async move {
            let state = session.start().await;
            put_back(id, session, &state);
            to_js(&state)
        }))
    }

    #[wasm_bindgen(js_name = debuggerCommand)]
    pub fn debugger_command(&self, session: f64, command: JsValue) -> Result<js_sys::Promise, JsValue> {
        let id = u32::try_from(int(session)?).map_err(|_| error("invalid session".into()))?;
        let command: DebugCommand = from_js(&command, "command")?;
        let Some(mut session) = SESSIONS.with(|s| s.borrow_mut().remove(&id)) else {
            return Err(error(format!(
                "there is no debug session {id}, or it is busy"
            )));
        };
        Ok(future_to_promise(async move {
            let state = session.command(command).await;
            put_back(id, session, &state);
            to_js(&state)
        }))
    }

    #[wasm_bindgen(js_name = debuggerState)]
    pub fn debugger_state(&self, session: f64) -> Result<JsValue, JsValue> {
        let id = u32::try_from(int(session)?).map_err(|_| error("invalid session".into()))?;
        SESSIONS.with(|s| match s.borrow().get(&id) {
            Some(session) => to_js(&session.state()),
            None => to_js(&DebugState {
                session: id,
                status: DebugStatus::Running,
                pause: None,
                output: Vec::new(),
                error: None,
            }),
        })
    }

    #[wasm_bindgen(js_name = debuggerStop)]
    pub fn debugger_stop(&self, session: f64) -> Result<(), JsValue> {
        let id = u32::try_from(int(session)?).map_err(|_| error("invalid session".into()))?;
        SESSIONS.with(|s| s.borrow_mut().remove(&id));
        Ok(())
    }
}
