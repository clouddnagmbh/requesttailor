// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.consumers` of `opencollection.yml` (wave 19, strand F): the
//! consumer collections a provider's repository lists (`rt_app::consumers`,
//! `rt verify-consumers`).
//!
//! ```yaml
//! extensions:
//!   requesttailor:
//!     consumers:
//!       - name: billing
//!         source: ../billing-collection   # a local path or a Git URL
//!         ref: main                       # optional branch, tag or commit
//!         path: api-tests                 # optional folder of the collection in the repo
//!         environment: ci                 # an environment of the consumer collection
//!         vars: { baseUrl: "{{provider.baseUrl}}" }
//! ```
//!
//! Only reading lives here: the list is the team's decision and the app never writes it, so
//! every writer of the file leaves the block byte for byte as it is. `extensions` is the one
//! place the OpenCollection schema allows unknown keys, so the key needs no schema change.
//! An entry that cannot be used (no name, no source, no environment, a name twice, more than
//! [`MAX_CONSUMERS`] entries) is an error naming the entry, never silently skipped.

use crate::yaml::Node;
use crate::{COLLECTION_FILE, FileSet, FormatError};

/// The most consumers one provider lists.
pub const MAX_CONSUMERS: usize = 50;

/// One entry of the list, as the file says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerEntry {
    pub name: String,
    pub source: String,
    pub git_ref: Option<String>,
    pub path: Option<String>,
    pub environment: String,
    /// In the order of the file.
    pub vars: Vec<(String, String)>,
}

fn invalid(message: String) -> FormatError {
    FormatError::Invalid {
        path: COLLECTION_FILE.to_owned(),
        message,
    }
}

fn read(root: &Node) -> Result<Vec<ConsumerEntry>, FormatError> {
    let Some(list) = root
        .get("extensions")
        .and_then(|e| e.get("requesttailor"))
        .and_then(|r| r.get("consumers"))
    else {
        return Ok(Vec::new());
    };
    if list.is_null() {
        return Ok(Vec::new());
    }
    if !list.is_seq() {
        return Err(invalid("consumers must be a list".into()));
    }
    let mut out: Vec<ConsumerEntry> = Vec::new();
    for (i, node) in list.items().enumerate() {
        let at = |what: &str| invalid(format!("consumers[{i}]: {what}"));
        if !node.is_map() {
            return Err(at("must be a mapping with name, source and environment"));
        }
        let field = |key: &str| {
            node.get(key)
                .and_then(Node::text)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let name = field("name").ok_or_else(|| at("name is missing"))?;
        let source = field("source").ok_or_else(|| at(&format!("{name}: source is missing")))?;
        let environment =
            field("environment").ok_or_else(|| at(&format!("{name}: environment is missing")))?;
        if out.iter().any(|c| c.name == name) {
            return Err(at(&format!("the name {name:?} is used twice")));
        }
        let mut vars = Vec::new();
        match node.get("vars") {
            Some(Node::Map(m)) => {
                for e in &m.entries {
                    let value = e.value.text().ok_or_else(|| {
                        at(&format!(
                            "{name}: variable {} has no text value",
                            e.key.value
                        ))
                    })?;
                    vars.push((e.key.value.clone(), value.to_owned()));
                }
            }
            Some(n) if !n.is_null() => return Err(at(&format!("{name}: vars must be a mapping"))),
            _ => {}
        }
        out.push(ConsumerEntry {
            name,
            source,
            git_ref: field("ref"),
            path: field("path"),
            environment,
            vars,
        });
    }
    if out.len() > MAX_CONSUMERS {
        return Err(invalid(format!(
            "{} consumers are listed, at most {MAX_CONSUMERS} are supported",
            out.len()
        )));
    }
    Ok(out)
}

/// The consumers the collection file in `files` lists (empty without a file or a list).
pub fn configured(files: &FileSet) -> Result<Vec<ConsumerEntry>, FormatError> {
    let Some(bytes) = files.files.get(COLLECTION_FILE) else {
        return Ok(Vec::new());
    };
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("not UTF-8".into()))?;
    let doc = crate::yaml::parse(text).map_err(|e| invalid(e.to_string()))?;
    read(&doc.root)
}
