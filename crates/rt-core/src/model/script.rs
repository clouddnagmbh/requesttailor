// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts, declarative assertions and variable actions of a request (wave 7). OpenCollection
//! keeps them under `runtime` in request files (`scripts`, `assertions`, `actions`) and under
//! `request.scripts` in `opencollection.yml` and `folder.yml`.
//!
//! The code is kept as written; what runs it is `rt-script` (behind
//! [`crate::services::ScriptRunner`]). Assertions and actions are evaluated by the core itself
//! (`crate::assert`), without JavaScript.

use serde::{Deserialize, Serialize};

/// When a script runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptPhase {
    /// Before variables are resolved: may set variables and change the request (templates).
    BeforeRequest,
    /// After the response: may set variables.
    AfterResponse,
    /// After `AfterResponse`: `test(...)` results go into the report.
    Tests,
}

/// One script. Phases OpenCollection knows but RequestTailor does not run (e.g. the gRPC
/// ones) are kept in the file untouched and are not part of the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Script {
    pub phase: ScriptPhase,
    pub code: String,
}

/// A declarative check of the response, e.g. `res.status eq 200`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertion {
    /// What to check: `res.status`, `res.body.items[0].id`, `res.headers.content-type`,
    /// `res.responseTime`, or a variable name.
    pub expression: String,
    /// `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, `in`, `notIn`, `contains`, `notContains`,
    /// `length`, `matches`, `notMatches`, `startsWith`, `endsWith`, `between`, `isEmpty`,
    /// `isNotEmpty`, `isNull`, `isUndefined`, `isDefined`, `isTruthy`, `isFalsy`, `isJson`,
    /// `isNumber`, `isString`, `isBoolean`, `isArray` (Bruno's set). Unknown operators fail
    /// the assertion with a message, they are not dropped.
    pub operator: String,
    /// The expected value as written; may contain `{{variables}}`. Empty for unary operators.
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled: bool,
}

/// Where a variable action writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VarScope {
    /// The runtime layer: lives for the session (app) or the run (runner, CLI).
    #[default]
    Runtime,
    /// The selected environment, in memory for the session; files are never written.
    Environment,
    Collection,
    Global,
}

/// "Take this value from the response into a variable" (OpenCollection `set-variable`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarAction {
    /// `AfterResponse` in practice; kept for the file format.
    pub phase: ScriptPhase,
    /// Selector expression, e.g. `res.body.token` (method `jsonq`), or a header name
    /// (method `header`).
    pub expression: String,
    /// `jsonq` (path into the JSON body), `header`, `status`, … as written.
    pub method: String,
    pub variable: String,
    pub scope: VarScope,
    pub enabled: bool,
}
