// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Hooks of `rt mock` (wave 18): an extension point that lets a distribution (or a test)
//! decide per request what the mock does instead of, or before, answering from the saved
//! examples: wait, drop the connection, answer something else, or deliver the body slowly.
//! `rt mock --hook NAME[=ARG]` switches a registered hook on; the builds of this repository
//! register none, so any `--hook` there is an error (`unknown hook`).
//!
//! A hook runs on every request, so it must be cheap: with nothing to do it answers
//! [`HookAction::Pass`]. Several hooks run in the order given; the first that answers anything
//! but `Pass` decides.

use std::sync::Arc;
use std::time::Duration;

use super::router::{Answer, MockRequest};

/// What a hook decides for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookAction {
    /// Answer from the examples as if there were no hook.
    Pass,
    /// Wait, then answer from the examples.
    Delay(Duration),
    /// Wait, then do the other action (a delay with an error, for instance).
    After(Duration, Box<HookAction>),
    /// End the connection without a (complete) answer.
    Abort(AbortKind),
    /// Answer this instead of the examples.
    Respond(Answer),
    /// Answer from the examples but deliver the body at about this many bytes per second.
    SlowBody(u64),
}

/// How [`HookAction::Abort`] ends the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortKind {
    /// Close at once, before any answer.
    Reset,
    /// Close after the headers were sent (the body never comes).
    CloseAfterHeaders,
    /// Hold the connection without answering for this long, then close.
    Stall(Duration),
}

/// A hook of `rt mock`. `Send + Sync`: the server calls it from its connection tasks.
pub trait MockHook: Send + Sync {
    /// Decides what happens to `request`. Cheap, never blocks.
    fn on_request(&self, request: &MockRequest<'_>) -> HookAction;

    /// An endpoint of the hook itself (e.g. statistics), answered before the examples are
    /// looked at. `None`: not this hook's. The server only offers it to loopback clients.
    fn endpoint(&self, request: &MockRequest<'_>) -> Option<Answer> {
        let _ = request;
        None
    }

    /// Called once when `rt mock` ends (Ctrl-C), e.g. for a summary on stderr.
    fn finish(&self, stderr: &mut dyn std::io::Write) {
        let _ = stderr;
    }
}

/// Builds a hook from the text after `=` of `--hook NAME=ARG` (`None` without one).
pub type MockHookFactory = fn(Option<&str>) -> Result<Arc<dyn MockHook>, String>;

/// A hook a distribution registers (`Distribution::mock_hooks`) under the name `--hook` uses.
#[derive(Clone, Copy)]
pub struct MockHookEntry {
    pub name: &'static str,
    /// One line for `rt mock --help`.
    pub summary: &'static str,
    /// Builds the hook from the text after `=` in `--hook NAME=ARG` (`None` without one).
    pub create: MockHookFactory,
}
