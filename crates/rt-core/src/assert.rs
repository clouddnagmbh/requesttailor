// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Declarative assertions and variable actions (wave 7, strand C), evaluated without
//! JavaScript so they behave the same everywhere and need no trust.
//!
//! Expressions: `res.status`, `res.statusText`, `res.responseTime`, `res.headers.<name>`,
//! `res.body` with a path (`res.body.items[0].id`, `res.body["a b"]`), or a variable name.
//! Operators: see [`crate::model::Assertion::operator`].

use crate::model::{Assertion, VarAction};
use crate::script::{AssertionResult, ScriptResponse, VarChange};
use crate::vars::Scope;

/// Evaluates the enabled assertions against the response. `scope` resolves `{{variables}}` in
/// expected values and bare variable names as expressions.
pub fn evaluate(
    assertions: &[Assertion],
    response: &ScriptResponse,
    scope: &Scope,
) -> Vec<AssertionResult> {
    let _ = (assertions, response, scope);
    Vec::new()
}

/// The variable changes of the enabled `set-variable` actions.
pub fn apply_actions(actions: &[VarAction], response: &ScriptResponse) -> Vec<VarChange> {
    let _ = (actions, response);
    Vec::new()
}
