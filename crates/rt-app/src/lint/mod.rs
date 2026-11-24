// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Collection lint and the health score (wave 18, strand F): four rules, a number from 0 to 100
//! and `rt lint`. It reads the model in memory, sends nothing and works in the browser.
//! Quieter is better than wrong: a dynamic script access to a variable never reports it as
//! unused. The score is a plain number with an open formula (weights in [`LintRule::weight`]);
//! it is never a secret and never stored in a file. Interface: types only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The rules of wave 18. More rules are "later".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintRule {
    /// A request without a test, assertion or snapshot (default `warn`).
    NoTests,
    /// A URL, token URL or auth endpoint with a fixed scheme and host instead of a variable
    /// (loopback is `info`; default `warn`).
    HardCodedHost,
    /// A variable nobody uses (default `info`).
    OrphanVariable,
    /// The same name twice in a folder, or the same file name (default `error`).
    DuplicateName,
}

impl LintRule {
    pub const ALL: [LintRule; 4] = [
        LintRule::NoTests,
        LintRule::HardCodedHost,
        LintRule::OrphanVariable,
        LintRule::DuplicateName,
    ];

    /// `lint/no-tests`, … (the id in settings, reports and `--rule id=level`).
    pub fn id(self) -> &'static str {
        match self {
            LintRule::NoTests => "lint/no-tests",
            LintRule::HardCodedHost => "lint/hard-coded-host",
            LintRule::OrphanVariable => "lint/orphan-variable",
            LintRule::DuplicateName => "lint/duplicate-name",
        }
    }

    /// Weight in the health score: tests 40, hosts 25, variables 15, names 20.
    pub fn weight(self) -> u32 {
        match self {
            LintRule::NoTests => 40,
            LintRule::HardCodedHost => 25,
            LintRule::OrphanVariable => 15,
            LintRule::DuplicateName => 20,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintLevel {
    Off,
    Info,
    Warn,
    Error,
}

/// `extensions.requesttailor.lint` of `opencollection.yml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LintSettings {
    /// Level per rule id; absent rules have their default.
    #[serde(default)]
    pub rules: BTreeMap<String, LintLevel>,
    /// Path patterns whose findings are left out (`*` for any characters).
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub min_score: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintFinding {
    pub rule: LintRule,
    pub level: LintLevel,
    /// The request (`users/get.yml`) or folder (`users/`) the finding is about.
    pub path: String,
    pub message: String,
    /// Where in the request, e.g. `url`, `auth.token_url`; or the variable's scope.
    #[serde(default)]
    pub location: Option<String>,
}

/// How well a rule is satisfied: `ok` of `total` checked items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleScore {
    pub rule: LintRule,
    pub ok: u32,
    pub total: u32,
}

/// The weighted mean of the satisfaction per rule, 0 to 100.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthScore {
    pub score: u8,
    pub rules: Vec<RuleScore>,
}

/// The score of one folder (`""`: the collection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderScore {
    pub folder: String,
    pub score: HealthScore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintReport {
    pub collection: String,
    pub score: HealthScore,
    pub folders: Vec<FolderScore>,
    pub findings: Vec<LintFinding>,
    /// Findings left out by `ignore`.
    pub ignored: u32,
}
