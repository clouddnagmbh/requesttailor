// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The script debugger (wave 19, strand Y): breakpoints, stepping (over, into, out),
//! looking at variables and the `rt.*` objects, evaluating an expression, for pre-request,
//! post-response and test scripts and for the scripts of folders and the collection.
//!
//! **Method.** For a debug run only, the source is instrumented: a call to a host function
//! `__rt_break(line, column)` before each statement. The host function traps *resumably*;
//! the sandbox's wasmi (2.0, `Func::call_resumable` / `ResumableCall::resume`, verified to
//! exist in the version of the lock file) keeps the instance paused at that point, the host
//! looks at the state (variables are read by evaluating inside the paused instance), waits
//! for the next [`DebugCommand`] and resumes. Fuel and time limits stop a paused script from
//! counting while the user looks (the clock is suspended); the memory limit stays. **A normal
//! run contains no debug code at all**: no instrumentation, no host function, the same cost
//! as before (the script-execution budget does not change).
//!
//! Secret values in the variable inspector are masked like everywhere. Interface: types
//! only; the sandbox does not debug until strand Y.

use serde::{Deserialize, Serialize};

/// A line the script stops at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    /// 1-based line in the script as the user sees it (instrumentation keeps lines).
    pub line: u32,
    /// Stop only when this expression is truthy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// Which script to debug (`DebuggerApi.start`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugTarget {
    /// The request whose send runs the script.
    pub request_path: String,
    /// `before-request`, `after-response` or `tests`.
    pub phase: String,
    /// `None`: the request's own script; `Some("")`: the collection's; `Some(folder)`: a
    /// folder's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    pub breakpoints: Vec<Breakpoint>,
    /// Stop at the first statement.
    #[serde(default)]
    pub stop_on_entry: bool,
}

/// What the user asks the paused script to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DebugCommand {
    Continue,
    StepOver,
    StepInto,
    StepOut,
    /// Evaluate an expression in the paused frame; the answer is masked.
    Evaluate {
        expression: String,
    },
    /// Replace the breakpoints.
    SetBreakpoints {
        breakpoints: Vec<Breakpoint>,
    },
    /// End the run; the script is cancelled.
    Abort,
}

/// One value in the inspector. Secrets are masked; `children` names how many entries a
/// structure has, they are fetched by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugValue {
    pub name: String,
    /// `string`, `number`, `object`, `array`, `function`, …
    pub kind: String,
    /// A short text, at most 200 characters.
    pub preview: String,
    #[serde(default)]
    pub children: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugFrame {
    pub function: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugScope {
    /// `local`, `closure`, `global`, `rt` (the `rt.*` objects).
    pub name: String,
    pub values: Vec<DebugValue>,
}

/// Where and why the script is stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugPause {
    pub line: u32,
    /// `breakpoint`, `step`, `entry`.
    pub reason: String,
    pub frames: Vec<DebugFrame>,
    pub scopes: Vec<DebugScope>,
    /// The answer to the last `Evaluate`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluated: Option<DebugValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebugStatus {
    Running,
    Paused,
    Finished,
    Aborted,
}

/// The state of a debug session (`DebuggerApi.command` and `state` answer it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugState {
    pub session: u32,
    pub status: DebugStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause: Option<DebugPause>,
    /// `console` output so far (masked).
    #[serde(default)]
    pub output: Vec<String>,
    /// The script's error, if it failed (masked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
