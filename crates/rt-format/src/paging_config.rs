// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Paging rules in collection files (wave 18, strand C). The rule types are
//! `rt_core::paging`; this module reads and writes them in two places:
//!
//! - the companion `<request>.paging.json` next to a request file (JSON with fixed keys in a
//!   stable order, so a diff of it is a diff of the rule): `version`, `rule`, `merge`;
//! - the collection default `extensions.requesttailor.paging` of `opencollection.yml`:
//!
//!   ```yaml
//!   extensions:
//!     requesttailor:
//!       paging:
//!         rule: cursor
//!         next: $.meta.cursor
//!         param:
//!           place: query
//!           name: after
//!         merge: $.items
//!   ```
//!
//! `extensions` is the one place the OpenCollection schema allows unknown keys, and the
//! companion is a file the document does not own (the reader looks at `*.yml` only), so no
//! schema change is needed. Reading never changes a file; [`CollectionDoc::set_paging_config`]
//! replaces only the `paging` block (missing parents are added, `None` removes the block and
//! parents that become empty), the rest of the file stays byte for byte, and the edit is read
//! back and refused if it does not give the rule.
//!
//! The paths are only text here: `rt_app::paging` checks that they are JSONPath.

use rt_core::paging::{CursorParam, CursorPlace, PagingRule, PagingSpec};
use serde::{Deserialize, Serialize};

use crate::yaml::{Map, Node, Val};
use crate::{COLLECTION_FILE, CollectionDoc, FileSet, FormatError, Source};

/// The version of the companion file.
pub const COMPANION_VERSION: u32 = 1;

/// The suffix of a request's paging companion.
pub const COMPANION_SUFFIX: &str = ".paging.json";

/// The companion of the request file `request_path`: `users/list.yml` has
/// `users/list.paging.json`.
pub fn companion_path(request_path: &str) -> String {
    let stem = request_path
        .strip_suffix(".yml")
        .or_else(|| request_path.strip_suffix(".yaml"))
        .unwrap_or(request_path);
    format!("{stem}{COMPANION_SUFFIX}")
}

/// Whether `path` is a paging companion.
pub fn is_companion(path: &str) -> bool {
    path.ends_with(COMPANION_SUFFIX)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompanionFile {
    version: u32,
    rule: PagingRule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    merge: Option<String>,
}

fn invalid(path: &str, message: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        path: path.to_owned(),
        message: message.into(),
    }
}

/// The text of a companion: the rule, or `{}` for "no rule here".
pub fn write_companion(spec: Option<&PagingSpec>) -> String {
    let mut text = match spec {
        None => "{}".to_owned(),
        Some(spec) => serde_json::to_string_pretty(&CompanionFile {
            version: COMPANION_VERSION,
            rule: spec.rule.clone(),
            merge: spec.merge.clone(),
        })
        .expect("a rule serialises"),
    };
    text.push('\n');
    text
}

/// Reads a companion. `{}` means "no rule here" (`Ok(None)`); anything that is not a rule of
/// this version is refused with the reason.
pub fn read_companion(text: &str) -> Result<Option<PagingSpec>, FormatError> {
    const PATH: &str = "paging companion";
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| invalid(PATH, format!("not JSON: {e}")))?;
    let Some(object) = value.as_object() else {
        return Err(invalid(PATH, "expected a JSON object"));
    };
    if object.is_empty() {
        return Ok(None);
    }
    let file: CompanionFile =
        serde_json::from_value(value).map_err(|e| invalid(PATH, e.to_string()))?;
    if file.version != COMPANION_VERSION {
        return Err(invalid(
            PATH,
            format!(
                "unsupported version {} (this build reads {COMPANION_VERSION})",
                file.version
            ),
        ));
    }
    Ok(Some(PagingSpec {
        rule: file.rule,
        merge: file.merge,
    }))
}

/// Top-level keys of `opencollection.yml` in Bruno's order (as in `lint_config.rs`).
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
    "auth", "agents", "http3", "contract", "ai", "lint", "paging",
];

fn paging_node(root: &Node) -> Option<&Node> {
    root.get("extensions")?.get("requesttailor")?.get("paging")
}

fn text_of(node: Option<&Node>) -> Option<String> {
    node.and_then(Node::text).map(str::to_owned)
}

fn read(root: &Node) -> Option<PagingSpec> {
    let node = paging_node(root).filter(|n| n.is_map())?;
    let rule = match node.get("rule").and_then(Node::text)? {
        "link-header" => PagingRule::LinkHeader,
        "json-url" => PagingRule::JsonUrl {
            next: text_of(node.get("next"))?,
        },
        "cursor" => {
            let param = node.get("param").filter(|p| p.is_map())?;
            let place = match param.get("place").and_then(Node::text)? {
                "query" => CursorPlace::Query,
                "body" => CursorPlace::Body,
                "header" => CursorPlace::Header,
                _ => return None,
            };
            PagingRule::Cursor {
                next: text_of(node.get("next"))?,
                param: CursorParam {
                    place,
                    name: text_of(param.get("name")).filter(|n| !n.is_empty())?,
                },
            }
        }
        _ => return None,
    };
    Some(PagingSpec {
        rule,
        merge: text_of(node.get("merge")),
    })
}

/// The default of the collection file in `files` (`None` if there is none or it is odd).
pub fn configured(files: &FileSet) -> Option<PagingSpec> {
    let bytes = files.files.get(COLLECTION_FILE)?;
    let text = std::str::from_utf8(bytes).ok()?;
    crate::yaml::parse(text)
        .ok()
        .and_then(|doc| read(&doc.root))
}

fn block(spec: &PagingSpec) -> Val {
    let mut entries: Vec<(&'static str, Val)> = Vec::new();
    match &spec.rule {
        PagingRule::LinkHeader => entries.push(("rule", Val::str("link-header"))),
        PagingRule::JsonUrl { next } => {
            entries.push(("rule", Val::str("json-url")));
            entries.push(("next", Val::str(next)));
        }
        PagingRule::Cursor { next, param } => {
            entries.push(("rule", Val::str("cursor")));
            entries.push(("next", Val::str(next)));
            let place = match param.place {
                CursorPlace::Query => "query",
                CursorPlace::Body => "body",
                CursorPlace::Header => "header",
            };
            entries.push((
                "param",
                Val::Map(vec![
                    ("place", Val::str(place)),
                    ("name", Val::str(&param.name)),
                ]),
            ));
        }
    }
    if let Some(merge) = &spec.merge {
        entries.push(("merge", Val::str(merge)));
    }
    Val::Map(entries)
}

/// Removes `extensions.requesttailor.paging` and parents that become empty. `Err` names a
/// parent that is not a block mapping.
fn remove_block(root: &mut Map) -> Result<(), &'static str> {
    let Some(ext) = root.get_mut("extensions") else {
        return Ok(());
    };
    let ext = ext.block_map_mut().ok_or("extensions")?;
    let Some(rt) = ext.get_mut("requesttailor") else {
        return Ok(());
    };
    let rt = rt.block_map_mut().ok_or("extensions.requesttailor")?;
    rt.remove("paging");
    if rt.is_empty() {
        ext.remove("requesttailor");
    }
    if ext.is_empty() {
        root.remove("extensions");
    }
    Ok(())
}

impl CollectionDoc {
    /// The paging rule of `opencollection.yml` (`extensions.requesttailor.paging`).
    pub fn paging_config(&self) -> Option<PagingSpec> {
        match self.files.get(COLLECTION_FILE) {
            Some(Source::Yaml(doc)) => read(&doc.root),
            _ => None,
        }
    }

    /// Replaces the collection default; `None` removes it. Only that block is written.
    /// Refused if a line break is in a text, if a parent is not a block mapping, or if the
    /// edit does not read back.
    pub fn set_paging_config(&mut self, spec: Option<&PagingSpec>) -> Result<(), FormatError> {
        let invalid = |message: String| FormatError::Invalid {
            path: COLLECTION_FILE.to_owned(),
            message,
        };
        if let Some(spec) = spec {
            let mut texts = vec![spec.merge.as_deref().unwrap_or("")];
            match &spec.rule {
                PagingRule::LinkHeader => {}
                PagingRule::JsonUrl { next } => texts.push(next),
                PagingRule::Cursor { next, param } => {
                    texts.push(next);
                    texts.push(&param.name);
                    if param.name.is_empty() {
                        return Err(invalid("a cursor needs a parameter name".into()));
                    }
                }
            }
            if texts.iter().any(|t| t.contains(['\n', '\r'])) {
                return Err(invalid("a paging path is one line".into()));
            }
        }
        let Some(Source::Yaml(doc)) = self.files.get(COLLECTION_FILE) else {
            return Err(invalid("the collection file cannot be edited".into()));
        };
        if read(&doc.root).as_ref() == spec {
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
            match spec {
                None => remove_block(root).map_err(not_block)?,
                Some(spec) => {
                    let value = block(spec);
                    match root.get_mut("extensions") {
                        None => root.put(
                            "extensions",
                            &Val::Map(vec![("requesttailor", Val::Map(vec![("paging", value)]))]),
                            ROOT_ORDER,
                            true,
                            nl,
                        ),
                        Some(ext) => {
                            let ext = ext.block_map_mut().ok_or_else(|| not_block("extensions"))?;
                            match ext.get_mut("requesttailor") {
                                None => ext.put(
                                    "requesttailor",
                                    &Val::Map(vec![("paging", value)]),
                                    &[],
                                    false,
                                    nl,
                                ),
                                Some(rt) => {
                                    let rt = rt
                                        .block_map_mut()
                                        .ok_or_else(|| not_block("extensions.requesttailor"))?;
                                    rt.put("paging", &value, EXTENSION_ORDER, false, nl);
                                }
                            }
                        }
                    }
                }
            }
        }
        let doc = crate::yaml::reparse(&doc).map_err(invalid)?;
        let got = read(&doc.root);
        if got.as_ref() != spec {
            return Err(invalid(format!(
                "internal error: the paging rule does not read back as written \
                 (wanted {spec:?}, got {got:?})"
            )));
        }
        self.files
            .insert(COLLECTION_FILE.to_owned(), Source::Yaml(doc));
        Ok(())
    }
}
