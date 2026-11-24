// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.lint` of `opencollection.yml` (wave 18, strand F): rule levels,
//! `ignore` patterns and `min_score` of the collection lint (`rt_app::lint`).
//!
//! ```yaml
//! extensions:
//!   requesttailor:
//!     lint:
//!       rules:
//!         lint/no-tests: off
//!       ignore:
//!         - "legacy/*"
//!         - lint/hard-coded-host:users/get.yml
//!       min_score: 80
//! ```
//!
//! `extensions` is the one place the OpenCollection schema allows unknown keys. Reading never
//! changes the file; [`CollectionDoc::set_lint_config`] replaces only the `lint` block (missing
//! parents are added, an empty config removes the block and parents that become empty), the
//! rest of the file stays byte for byte. The edit is read back and refused if it does not
//! give the config. Levels are kept as written here (`rt_app::lint` knows the words); rule
//! ids are those of `rt_app::lint::LintRule`, others are not written.

use std::collections::BTreeMap;

use crate::yaml::{Map, Node, Val};
use crate::{COLLECTION_FILE, CollectionDoc, FileSet, FormatError, Source};

/// The rule ids the lint knows, as they appear in the file (`rt_app::lint::LintRule::id`).
pub const RULE_IDS: [&str; 4] = [
    "lint/no-tests",
    "lint/hard-coded-host",
    "lint/orphan-variable",
    "lint/duplicate-name",
];

/// The settings as the file says them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LintConfig {
    /// Level word (`off`, `info`, `warn`, `error`) per rule id.
    pub rules: BTreeMap<String, String>,
    /// Path patterns (`*` for any characters), each optionally `<rule id>:<pattern>`.
    pub ignore: Vec<String>,
    /// 0 to 100.
    pub min_score: Option<u8>,
}

impl LintConfig {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.ignore.is_empty() && self.min_score.is_none()
    }
}

/// Top-level keys of `opencollection.yml` in Bruno's order (as in `contract_config.rs`).
const ROOT_ORDER: &[&str] = &[
    "opencollection",
    "info",
    "config",
    "request",
    "docs",
    "bundled",
    "extensions",
];

/// Keys of `extensions.requesttailor` in the order RequestTailor writes them.
const EXTENSION_ORDER: &[&str] = &["auth", "agents", "http3", "contract", "ai", "lint"];

fn lint_node(root: &Node) -> Option<&Node> {
    root.get("extensions")?.get("requesttailor")?.get("lint")
}

fn read(root: &Node) -> LintConfig {
    let Some(lint) = lint_node(root).filter(|n| n.is_map()) else {
        return LintConfig::default();
    };
    let mut rules = BTreeMap::new();
    if let Some(Node::Map(m)) = lint.get("rules") {
        for e in &m.entries {
            if let Some(level) = e.value.text() {
                rules.insert(e.key.value.clone(), level.to_owned());
            }
        }
    }
    let ignore = lint
        .get("ignore")
        .map(|n| {
            n.items()
                .filter_map(|i| i.text().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let min_score = lint
        .get("min_score")
        .and_then(Node::as_f64)
        .filter(|v| (0.0..=100.0).contains(v) && v.fract() == 0.0)
        .map(|v| v as u8);
    LintConfig {
        rules,
        ignore,
        min_score,
    }
}

/// The settings of the collection file in `files` (empty if there is none or it is odd).
pub fn configured(files: &FileSet) -> LintConfig {
    let Some(bytes) = files.files.get(COLLECTION_FILE) else {
        return LintConfig::default();
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return LintConfig::default();
    };
    crate::yaml::parse(text)
        .map(|doc| read(&doc.root))
        .unwrap_or_default()
}

fn block(config: &LintConfig) -> Val {
    let mut entries: Vec<(&'static str, Val)> = Vec::new();
    let rules: Vec<(&'static str, Val)> = RULE_IDS
        .iter()
        .filter_map(|id| {
            config
                .rules
                .get(*id)
                .map(|level| (*id, Val::Plain(level.clone())))
        })
        .collect();
    if !rules.is_empty() {
        entries.push(("rules", Val::Map(rules)));
    }
    if !config.ignore.is_empty() {
        entries.push((
            "ignore",
            Val::Seq(config.ignore.iter().map(Val::str).collect()),
        ));
    }
    if let Some(score) = config.min_score {
        entries.push(("min_score", Val::Plain(score.to_string())));
    }
    Val::Map(entries)
}

/// What the file must read back as: only known rule ids with a level word.
fn as_written(config: &LintConfig) -> LintConfig {
    LintConfig {
        rules: config
            .rules
            .iter()
            .filter(|(id, _)| RULE_IDS.contains(&id.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        ignore: config.ignore.clone(),
        min_score: config.min_score,
    }
}

/// Removes `extensions.requesttailor.lint` and parents that become empty. `Err` names a parent
/// that is not a block mapping.
fn remove_block(root: &mut Map) -> Result<(), &'static str> {
    let Some(ext) = root.get_mut("extensions") else {
        return Ok(());
    };
    let ext = ext.block_map_mut().ok_or("extensions")?;
    let Some(rt) = ext.get_mut("requesttailor") else {
        return Ok(());
    };
    let rt = rt.block_map_mut().ok_or("extensions.requesttailor")?;
    rt.remove("lint");
    if rt.is_empty() {
        ext.remove("requesttailor");
    }
    if ext.is_empty() {
        root.remove("extensions");
    }
    Ok(())
}

impl CollectionDoc {
    /// The lint settings of `opencollection.yml`.
    pub fn lint_config(&self) -> LintConfig {
        match self.files.get(COLLECTION_FILE) {
            Some(Source::Yaml(doc)) => read(&doc.root),
            _ => LintConfig::default(),
        }
    }

    /// Replaces the lint settings (`extensions.requesttailor.lint`); an empty config removes
    /// them. Only that block is written. Refused if `min_score` is above 100, if an entry
    /// would not read back, or if a parent is not a block mapping.
    pub fn set_lint_config(&mut self, config: &LintConfig) -> Result<(), FormatError> {
        let invalid = |message: String| FormatError::Invalid {
            path: COLLECTION_FILE.to_owned(),
            message,
        };
        if config.min_score.is_some_and(|s| s > 100) {
            return Err(invalid("min_score is at most 100".into()));
        }
        if config.ignore.iter().any(|p| p.contains(['\n', '\r'])) {
            return Err(invalid("an ignore pattern is one line".into()));
        }
        let want = as_written(config);
        let Some(Source::Yaml(doc)) = self.files.get(COLLECTION_FILE) else {
            return Err(invalid("the collection file cannot be edited".into()));
        };
        if read(&doc.root) == want {
            return Ok(());
        }
        let mut doc = doc.clone();
        let nl = doc.newline;
        let not_block = |key: &str| {
            invalid(format!(
                "`{key}` is not a block mapping and cannot be edited"
            ))
        };
        {
            let root = doc
                .root
                .block_map_mut()
                .ok_or_else(|| invalid("the file is not a YAML mapping".into()))?;
            if want.is_empty() {
                remove_block(root).map_err(not_block)?;
            } else {
                let value = block(&want);
                match root.get_mut("extensions") {
                    None => root.put(
                        "extensions",
                        &Val::Map(vec![(
                            "requesttailor",
                            Val::Map(vec![("lint", value)]),
                        )]),
                        ROOT_ORDER,
                        true,
                        nl,
                    ),
                    Some(ext) => {
                        let ext = ext.block_map_mut().ok_or_else(|| not_block("extensions"))?;
                        match ext.get_mut("requesttailor") {
                            None => ext.put(
                                "requesttailor",
                                &Val::Map(vec![("lint", value)]),
                                &[],
                                false,
                                nl,
                            ),
                            Some(rt) => {
                                let rt = rt
                                    .block_map_mut()
                                    .ok_or_else(|| not_block("extensions.requesttailor"))?;
                                rt.put("lint", &value, EXTENSION_ORDER, false, nl);
                            }
                        }
                    }
                }
            }
        }
        let doc = crate::yaml::reparse(&doc).map_err(invalid)?;
        let got = read(&doc.root);
        if got != want {
            return Err(invalid(format!(
                "internal error: the lint settings do not read back as written \
                 (wanted {want:?}, got {got:?})"
            )));
        }
        self.files
            .insert(COLLECTION_FILE.to_owned(), Source::Yaml(doc));
        Ok(())
    }
}
