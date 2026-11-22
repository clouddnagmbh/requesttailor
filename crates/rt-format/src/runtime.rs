// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts, assertions and actions (wave 7): `runtime.scripts`, `runtime.assertions` and
//! `runtime.actions` of request files, `request.scripts` of `opencollection.yml` and
//! `folder.yml`.
//!
//! Reading lives here (interface commit). Writing them back (edits from the app) is wave 7,
//! strand C; until then the writer keeps these blocks byte-identical and the re-read check
//! refuses edits to them.

use rt_core::model::{Assertion, Script, ScriptPhase, VarAction, VarScope};

use crate::read::{enabled, text};
use crate::yaml::Node;

/// The phase of an OpenCollection script `type`; `None` for types RequestTailor does not run
/// (gRPC phases), which stay in the file untouched.
pub(crate) fn phase(kind: &str) -> Option<ScriptPhase> {
    match kind {
        "before-request" => Some(ScriptPhase::BeforeRequest),
        "after-response" => Some(ScriptPhase::AfterResponse),
        "tests" => Some(ScriptPhase::Tests),
        _ => None,
    }
}

pub(crate) fn scripts(list: Option<&Node>) -> Vec<Script> {
    list.map(|l| {
        l.items()
            .filter_map(|s| {
                let phase = phase(s.get("type").and_then(Node::text)?)?;
                Some(Script {
                    phase,
                    code: text(s.get("code")),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

pub(crate) fn assertions(list: Option<&Node>) -> Vec<Assertion> {
    list.map(|l| {
        l.items()
            .map(|a| Assertion {
                expression: text(a.get("expression")),
                operator: text(a.get("operator")),
                value: text(a.get("value")),
                description: a.get("description").and_then(Node::text).map(str::to_owned),
                enabled: enabled(a),
            })
            .collect()
    })
    .unwrap_or_default()
}

/// `set-variable` actions; other action types are kept in the file, not modelled.
pub(crate) fn actions(list: Option<&Node>) -> Vec<VarAction> {
    list.map(|l| {
        l.items()
            .filter(|a| a.get("type").and_then(Node::text) == Some("set-variable"))
            .map(|a| {
                let selector = a.get("selector");
                let variable = a.get("variable");
                VarAction {
                    phase: a
                        .get("phase")
                        .and_then(Node::text)
                        .and_then(phase)
                        .unwrap_or(ScriptPhase::AfterResponse),
                    expression: text(selector.and_then(|s| s.get("expression"))),
                    method: text(selector.and_then(|s| s.get("method"))),
                    variable: text(variable.and_then(|v| v.get("name"))),
                    scope: match variable.and_then(|v| v.get("scope")).and_then(Node::text) {
                        Some("environment") => VarScope::Environment,
                        Some("collection") => VarScope::Collection,
                        Some("global") => VarScope::Global,
                        _ => VarScope::Runtime,
                    },
                    enabled: enabled(a),
                }
            })
            .collect()
    })
    .unwrap_or_default()
}
