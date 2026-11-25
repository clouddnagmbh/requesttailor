// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The terminal debugger (`rt send --debug`): a script runner that stops in the request's own
//! scripts and asks a [`DebugTerminal`] what to do at every stop. Scripts of the collection
//! and of folders run as usual. `rt.sendRequest` is not available in a debug run.

use std::cell::Cell;

use rt_core::script::{ScriptCall, ScriptOutcome};
use rt_core::services::{LocalBoxFuture, ScriptRunner};

use super::{Breakpoint, DebugCommand, DebugInput, DebugSession, DebugState, DebugStatus};
use crate::{Limits, Sandbox};

/// Where the stops are shown and the commands come from.
pub trait DebugTerminal {
    /// The script (its source as written) stopped. `None` (no more input): the script
    /// continues.
    fn paused(&self, source: &str, state: &DebugState) -> Option<DebugCommand>;
    /// The script ended (or the session did).
    fn finished(&self, state: &DebugState);
}

/// Debugs the `request` scripts of every phase it is asked to run.
pub struct DebugRunner<T> {
    pub terminal: T,
    /// Lines to stop at; without any the script stops at its first statement.
    pub breakpoints: Vec<u32>,
    pub limits: Limits,
    next_session: Cell<u32>,
}

impl<T: DebugTerminal> DebugRunner<T> {
    pub fn new(terminal: T, breakpoints: Vec<u32>) -> Self {
        Self {
            terminal,
            breakpoints,
            limits: Limits::default(),
            next_session: Cell::new(1),
        }
    }
}

impl<T: DebugTerminal> ScriptRunner for DebugRunner<T> {
    fn run<'a>(&'a self, call: ScriptCall<'a>) -> LocalBoxFuture<'a, ScriptOutcome> {
        Box::pin(async move {
            if !call.scripts.iter().any(|s| s.origin == "request") {
                return Sandbox::new(self.limits).run(call).await;
            }
            let source = call
                .scripts
                .iter()
                .find(|s| s.origin == "request")
                .map(|s| s.code.clone())
                .unwrap_or_default();
            let id = self.next_session.get();
            self.next_session.set(id + 1);
            let breakpoints = self
                .breakpoints
                .iter()
                .map(|line| Breakpoint {
                    line: *line,
                    condition: None,
                })
                .collect();
            let input =
                DebugInput::from_call(&call, "request", breakpoints, self.breakpoints.is_empty());
            let mut session = match DebugSession::new(id, self.limits, input) {
                Ok(s) => s,
                Err(e) => {
                    return ScriptOutcome {
                        error: Some(e),
                        ..ScriptOutcome::default()
                    };
                }
            };
            let mut state = session.start().await;
            while state.status == DebugStatus::Paused {
                let command = self
                    .terminal
                    .paused(&source, &state)
                    .unwrap_or(DebugCommand::Continue);
                state = session.command(command).await;
            }
            self.terminal.finished(&state);
            session.take_outcome().unwrap_or_else(|| ScriptOutcome {
                error: Some("Script stopped: the debug session was ended".into()),
                ..ScriptOutcome::default()
            })
        })
    }
}
