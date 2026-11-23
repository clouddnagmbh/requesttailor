// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.contract.spec` of `opencollection.yml`: the OpenAPI spec the
//! collection is checked against (read by [`crate::contract::configured_spec`]). Written when
//! an OpenAPI import that kept its spec goes into an open collection as a folder (OpenAPI
//! import wizard; `rt_app::import::add_to_workspace`).
//!
//! ```yaml
//! extensions:
//!   requesttailor:
//!     contract:
//!       spec: Petstore/openapi.yaml
//! ```
//!
//! [`CollectionDoc::set_contract_spec`] changes only that value: missing parents are added
//! (`extensions` at the end of the file, as RequestTailor writes it), everything else stays
//! byte for byte. The edit is read back and refused if it does not give the path.

use crate::yaml::{Node, Val};
use crate::{COLLECTION_FILE, CollectionDoc, FormatError, Source};

/// Top-level keys of `opencollection.yml` in Bruno's order (as in `agents.rs`).
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
const EXTENSION_ORDER: &[&str] = &["auth", "agents", "http3", "contract"];

/// The value in a parsed `opencollection.yml`, as written.
fn read(root: &Node) -> Option<&str> {
    root.get("extensions")?
        .get("requesttailor")?
        .get("contract")?
        .get("spec")?
        .text()
}

impl CollectionDoc {
    /// Names the collection's OpenAPI spec (`extensions.requesttailor.contract.spec` of
    /// `opencollection.yml`): a path relative to the collection root, or with a leading `/`
    /// relative to the repository root (see [`crate::contract`]). Only that value is written;
    /// the edit is read back and refused if it does not give `path`.
    pub fn set_contract_spec(&mut self, path: &str) -> Result<(), FormatError> {
        let invalid = |message: String| FormatError::Invalid {
            path: COLLECTION_FILE.to_owned(),
            message,
        };
        let path = path.trim();
        if path.is_empty() || path.contains(['\n', '\r']) {
            return Err(invalid(format!("invalid spec path {path:?}")));
        }
        let Some(Source::Yaml(doc)) = self.files.get(COLLECTION_FILE) else {
            return Err(invalid("the collection file cannot be edited".into()));
        };
        if read(&doc.root) == Some(path) {
            return Ok(());
        }
        let mut doc = doc.clone();
        let nl = doc.newline;
        let not_block = |key: &str| {
            invalid(format!(
                "`{key}` is not a block mapping and cannot be edited"
            ))
        };
        let spec = || Val::Map(vec![("spec", Val::str(path))]);
        {
            let root = doc
                .root
                .block_map_mut()
                .ok_or_else(|| invalid("the file is not a YAML mapping".into()))?;
            match root.get_mut("extensions") {
                None => root.put(
                    "extensions",
                    &Val::Map(vec![(
                        "requesttailor",
                        Val::Map(vec![("contract", spec())]),
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
                            &Val::Map(vec![("contract", spec())]),
                            &[],
                            false,
                            nl,
                        ),
                        Some(rt) => {
                            let rt = rt
                                .block_map_mut()
                                .ok_or_else(|| not_block("extensions.requesttailor"))?;
                            match rt.get_mut("contract").and_then(Node::block_map_mut) {
                                Some(contract) => {
                                    contract.put("spec", &Val::str(path), &["spec"], false, nl)
                                }
                                // Missing, or a flow mapping or scalar: written anew.
                                None => rt.put("contract", &spec(), EXTENSION_ORDER, false, nl),
                            }
                        }
                    }
                }
            }
        }
        let doc = crate::yaml::reparse(&doc).map_err(invalid)?;
        if read(&doc.root) != Some(path) {
            return Err(invalid(format!(
                "internal error: the contract spec does not read back as {path:?}"
            )));
        }
        self.files
            .insert(COLLECTION_FILE.to_owned(), Source::Yaml(doc));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{COLLECTION_FILE, FileSet, read_collection};

    fn set(collection: &str, path: &str) -> String {
        let files = FileSet {
            files: [(COLLECTION_FILE.to_owned(), collection.as_bytes().to_vec())]
                .into_iter()
                .collect(),
        };
        let mut doc = read_collection(&files).unwrap();
        doc.set_contract_spec(path).unwrap();
        let written = doc.write();
        assert_eq!(
            crate::contract::configured_spec(&written).as_deref(),
            Some(path)
        );
        // Reads back cleanly and writes the same bytes again.
        let again = read_collection(&written).unwrap();
        assert!(again.issues().is_empty(), "{:?}", again.issues());
        assert_eq!(again.write(), written);
        String::from_utf8(written.files[COLLECTION_FILE].clone()).unwrap()
    }

    const HEAD: &str = "opencollection: \"1.0.0\"\n\ninfo:\n  name: Shop\n\nbundled: false\n";

    #[test]
    fn adds_the_extension_and_keeps_the_rest() {
        assert_eq!(
            set(HEAD, "Petstore/openapi.yaml"),
            format!(
                "{HEAD}\nextensions:\n  requesttailor:\n    contract:\n      spec: Petstore/openapi.yaml\n"
            )
        );
        // Next to other RequestTailor settings, with comments kept.
        let agents = format!(
            "{HEAD}\nextensions:\n  other: 1 # theirs\n  requesttailor:\n    agents:\n      access: write\n"
        );
        assert_eq!(
            set(&agents, "api/openapi.json"),
            format!("{agents}    contract:\n      spec: api/openapi.json\n")
        );
        // An existing value is replaced, nothing else changes.
        let existing = format!(
            "{HEAD}\nextensions:\n  requesttailor:\n    contract:\n      spec: old.yaml # the old one\n"
        );
        assert_eq!(
            set(&existing, "new folder/openapi.yaml"),
            existing.replace("old.yaml", "new folder/openapi.yaml")
        );
    }

    #[test]
    fn unchanged_and_invalid_paths() {
        let existing =
            format!("{HEAD}\nextensions:\n  requesttailor:\n    contract:\n      spec: a.yaml\n");
        assert_eq!(set(&existing, "a.yaml"), existing);
        let files = FileSet {
            files: [(COLLECTION_FILE.to_owned(), HEAD.as_bytes().to_vec())]
                .into_iter()
                .collect(),
        };
        let mut doc = read_collection(&files).unwrap();
        assert!(doc.set_contract_spec(" ").is_err());
        assert!(doc.set_contract_spec("a\nb").is_err());
        assert_eq!(doc.write(), files);
    }
}
