// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Collection lint and the health score (wave 18, strand F): four rules, a number from 0 to 100
//! and `rt lint`. It reads the model in memory, sends nothing and works in the browser.
//! Quieter is better than wrong: a dynamic script access to a variable never reports it as
//! unused. The score is a plain number with an open formula (weights in [`LintRule::weight`]);
//! it is never a secret and never stored in a file.
//!
//! The formula, open: for each rule, the satisfaction is `ok / total` over the items the rule
//! checks (requests for tests and hosts, variables, requests and folders for names). The score
//! is the weighted mean of those over the rules that have something to check (tests 40, hosts
//! 25, variables 15, names 20), rounded, 0 to 100; with nothing to check it is 100. A loopback
//! URL is reported (info) but counts as satisfied: local development URLs are normal. Items
//! left out by `ignore` are not checked. The folder score uses the items below the folder.
//!
//! Settings live in `extensions.requesttailor.lint` of `opencollection.yml`
//! (`rt_format::lint_config`): a level per rule id, `ignore` patterns (`*` matches any
//! characters, `<rule id>:<pattern>` limits one to a rule; a pattern ending in `/` takes the
//! folder and everything below), and `min_score`. There are no comment markers in request
//! files: ignoring is only done through the settings.

mod check;
mod score;
mod usage;

use std::collections::BTreeMap;

use rt_core::model::Collection;
use rt_format::lint_config::LintConfig;
use serde::{Deserialize, Serialize};

use crate::{AppError, Workspace};

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

    /// The id without the `lint/` prefix is accepted too (`no-tests`).
    pub fn from_id(id: &str) -> Option<LintRule> {
        let id = id.strip_prefix("lint/").unwrap_or(id);
        LintRule::ALL
            .into_iter()
            .find(|r| r.id().strip_prefix("lint/") == Some(id))
    }

    /// The level when the settings say nothing.
    pub fn default_level(self) -> LintLevel {
        match self {
            LintRule::NoTests | LintRule::HardCodedHost => LintLevel::Warn,
            LintRule::OrphanVariable => LintLevel::Info,
            LintRule::DuplicateName => LintLevel::Error,
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

impl LintLevel {
    pub fn word(self) -> &'static str {
        match self {
            LintLevel::Off => "off",
            LintLevel::Info => "info",
            LintLevel::Warn => "warn",
            LintLevel::Error => "error",
        }
    }

    pub fn from_word(word: &str) -> Option<LintLevel> {
        match word.trim() {
            "off" => Some(LintLevel::Off),
            "info" => Some(LintLevel::Info),
            "warn" => Some(LintLevel::Warn),
            "error" => Some(LintLevel::Error),
            _ => None,
        }
    }
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

impl LintSettings {
    /// The level of `rule`: the setting, else the default.
    pub fn level(&self, rule: LintRule) -> LintLevel {
        self.rules
            .get(rule.id())
            .copied()
            .unwrap_or_else(|| rule.default_level())
    }

    /// Sets the level of `rule` (the default level removes the entry).
    pub fn set_level(&mut self, rule: LintRule, level: LintLevel) {
        if level == rule.default_level() {
            self.rules.remove(rule.id());
        } else {
            self.rules.insert(rule.id().to_owned(), level);
        }
    }
}

fn settings_of(config: &LintConfig) -> LintSettings {
    LintSettings {
        rules: config
            .rules
            .iter()
            .filter_map(|(id, word)| {
                let rule = LintRule::from_id(id)?;
                Some((rule.id().to_owned(), LintLevel::from_word(word)?))
            })
            .collect(),
        ignore: config.ignore.clone(),
        min_score: config.min_score,
    }
}

fn config_of(settings: &LintSettings) -> LintConfig {
    LintConfig {
        rules: settings
            .rules
            .iter()
            .filter_map(|(id, level)| {
                let rule = LintRule::from_id(id)?;
                Some((rule.id().to_owned(), level.word().to_owned()))
            })
            .collect(),
        ignore: settings.ignore.clone(),
        min_score: settings.min_score,
    }
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

/// The settings of the workspace's collection (unsaved edits included).
pub fn settings(ws: &Workspace) -> LintSettings {
    settings_of(&ws.doc().lint_config())
}

/// Replaces the settings (unsaved; `opencollection.yml` is staged like any edit). Only the
/// `lint` block of the file changes. Refused if `min_score` is above 100.
pub fn set_settings(ws: &mut Workspace, settings: &LintSettings) -> Result<(), AppError> {
    ws.doc_mut().set_lint_config(&config_of(settings))?;
    ws.refresh();
    Ok(())
}

/// Leaves the findings of `rule` at `path` out from now on (`<rule id>:<path>` in `ignore`).
pub fn ignore(ws: &mut Workspace, path: &str, rule: LintRule) -> Result<(), AppError> {
    let mut settings = settings(ws);
    let entry = format!("{}:{path}", rule.id());
    if !settings.ignore.contains(&entry) {
        settings.ignore.push(entry);
        set_settings(ws, &settings)?;
    }
    Ok(())
}

/// Lints the workspace's collection with its settings.
pub fn run(ws: &Workspace) -> LintReport {
    lint_with(ws, &settings(ws))
}

/// Lints the workspace's collection with `settings` instead of the file's (the CLI's
/// `--rule`). Non-YAML files of the collection (body files, ...) only serve to see which
/// variables are used.
pub fn lint_with(ws: &Workspace, settings: &LintSettings) -> LintReport {
    let extra: Vec<&str> = ws
        .files()
        .files
        .iter()
        .filter(|(path, bytes)| !path.ends_with(".yml") && bytes.len() <= EXTRA_FILE_LIMIT)
        .filter_map(|(_, bytes)| std::str::from_utf8(bytes).ok())
        .collect();
    lint(ws.collection(), &extra, settings)
}

/// Non-YAML files larger than this are not read for variable use.
const EXTRA_FILE_LIMIT: usize = 1024 * 1024;

/// The lint of a model in memory: nothing is read or sent. `extra` are texts of files that are
/// not part of the model (body files), searched for `{{variables}}`.
pub fn lint(collection: &Collection, extra: &[&str], settings: &LintSettings) -> LintReport {
    check::run(collection, extra, settings)
}
