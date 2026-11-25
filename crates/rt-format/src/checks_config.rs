// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.checks` of `opencollection.yml` (wave 18, strand B): the passive
//! response checks (`rt_app::inspect`) this collection switches off.
//!
//! ```yaml
//! extensions:
//!   requesttailor:
//!     checks:
//!       ignore:
//!         - chk/hsts-missing
//!         - chk/version-disclosure
//! ```
//!
//! `extensions` is the one place the OpenCollection schema allows unknown keys. Reading never
//! changes the file; [`CollectionDoc::set_checks_config`] replaces only the `checks` block
//! (missing parents are added, an empty list removes the block and parents that become empty),
//! the rest of the file stays byte for byte. The edit is read back and refused if it does not
//! give the list.

use crate::yaml::{Map, Node, Val};
use crate::{COLLECTION_FILE, CollectionDoc, FileSet, FormatError, Source};

/// The settings as the file says them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChecksConfig {
    /// Rule ids (`chk/…`) that are off in this collection.
    pub ignore: Vec<String>,
}

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
const EXTENSION_ORDER: &[&str] = &[
    "auth", "agents", "http3", "contract", "ai", "lint", "checks",
];

fn read(root: &Node) -> ChecksConfig {
    let ignore = root
        .get("extensions")
        .and_then(|n| n.get("requesttailor"))
        .and_then(|n| n.get("checks"))
        .filter(|n| n.is_map())
        .and_then(|n| n.get("ignore"))
        .map(|n| {
            n.items()
                .filter_map(|i| i.text().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    ChecksConfig { ignore }
}

/// The settings of the collection file in `files` (empty if there is none or it is odd).
pub fn configured(files: &FileSet) -> ChecksConfig {
    let Some(bytes) = files.files.get(COLLECTION_FILE) else {
        return ChecksConfig::default();
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return ChecksConfig::default();
    };
    crate::yaml::parse(text)
        .map(|doc| read(&doc.root))
        .unwrap_or_default()
}

fn block(config: &ChecksConfig) -> Val {
    Val::Map(vec![(
        "ignore",
        Val::Seq(config.ignore.iter().map(Val::str).collect()),
    )])
}

/// Removes `extensions.requesttailor.checks` and parents that become empty. `Err` names a parent
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
    rt.remove("checks");
    if rt.is_empty() {
        ext.remove("requesttailor");
    }
    if ext.is_empty() {
        root.remove("extensions");
    }
    Ok(())
}

impl CollectionDoc {
    /// The checks settings of `opencollection.yml`.
    pub fn checks_config(&self) -> ChecksConfig {
        match self.files.get(COLLECTION_FILE) {
            Some(Source::Yaml(doc)) => read(&doc.root),
            _ => ChecksConfig::default(),
        }
    }

    /// Replaces the checks settings (`extensions.requesttailor.checks`); an empty list removes
    /// them. Only that block is written. Refused for an id that is empty or has a line break,
    /// or if a parent is not a block mapping.
    pub fn set_checks_config(&mut self, config: &ChecksConfig) -> Result<(), FormatError> {
        let invalid = |message: String| FormatError::Invalid {
            path: COLLECTION_FILE.to_owned(),
            message,
        };
        if config
            .ignore
            .iter()
            .any(|id| id.trim().is_empty() || id.contains(['\n', '\r']))
        {
            return Err(invalid("a rule id is one non-empty line".into()));
        }
        let want = config.clone();
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
            if want.ignore.is_empty() {
                remove_block(root).map_err(not_block)?;
            } else {
                let value = block(&want);
                match root.get_mut("extensions") {
                    None => root.put(
                        "extensions",
                        &Val::Map(vec![("requesttailor", Val::Map(vec![("checks", value)]))]),
                        ROOT_ORDER,
                        true,
                        nl,
                    ),
                    Some(ext) => {
                        let ext = ext.block_map_mut().ok_or_else(|| not_block("extensions"))?;
                        match ext.get_mut("requesttailor") {
                            None => ext.put(
                                "requesttailor",
                                &Val::Map(vec![("checks", value)]),
                                &[],
                                false,
                                nl,
                            ),
                            Some(rt) => {
                                let rt = rt
                                    .block_map_mut()
                                    .ok_or_else(|| not_block("extensions.requesttailor"))?;
                                rt.put("checks", &value, EXTENSION_ORDER, false, nl);
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
                "internal error: the checks settings do not read back as written \
                 (wanted {want:?}, got {got:?})"
            )));
        }
        self.files
            .insert(COLLECTION_FILE.to_owned(), Source::Yaml(doc));
        Ok(())
    }
}
