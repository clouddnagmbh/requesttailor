// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Proto files in memory to a [`ProtoSchema`].

use std::collections::BTreeMap;

use rt_grpc::{Descriptors, GrpcError, ProtoSchema, normalize_path};

use crate::contract::{ContractError, SchemaSources};

/// Largest single file read.
const MAX_BYTES: usize = 16 * 1024 * 1024;

/// The paths named by `import "…";` statements of `source` (comments and strings of other
/// statements are not told apart: this is only used to know which files to read).
pub(crate) fn imports_of(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let mut rest = line.trim_start();
        while let Some(after) = rest.strip_prefix("import") {
            let after = after.trim_start();
            let after = after
                .strip_prefix("public")
                .or_else(|| after.strip_prefix("weak"))
                .unwrap_or(after)
                .trim_start();
            let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                break;
            };
            let body = &after[1..];
            let Some(end) = body.find(quote) else {
                break;
            };
            out.push(body[..end].to_owned());
            rest = body[end + 1..]
                .trim_start()
                .trim_start_matches(';')
                .trim_start();
        }
    }
    out
}

pub(super) fn schema(sources: Option<&SchemaSources>) -> Result<ProtoSchema, ContractError> {
    let Some(sources) = sources.filter(|s| !s.is_empty()) else {
        return Ok(ProtoSchema::default());
    };
    let mut by_path: BTreeMap<String, String> = BTreeMap::new();
    for (name, bytes) in sources {
        if bytes.len() > MAX_BYTES {
            return Err(ContractError::Spec(format!(
                "{name}: the file is larger than {} MB",
                MAX_BYTES / (1024 * 1024)
            )));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| ContractError::Spec(format!("{name}: the file is not valid UTF-8")))?;
        by_path.insert(normalize_path(name), text.to_owned());
    }
    let keys: Vec<String> = by_path.keys().cloned().collect();
    // An import `a/b.proto` found as `x/a/b.proto` makes `x` an import path, and the file is
    // read through the import (compiling it again as an input would define it twice).
    let mut roots: Vec<String> = Vec::new();
    let mut imported: Vec<&String> = Vec::new();
    for text in by_path.values() {
        for import in imports_of(text) {
            let import = normalize_path(&import);
            for key in &keys {
                let root = if *key == import {
                    Some("")
                } else {
                    key.strip_suffix(&format!("/{import}"))
                };
                if let Some(root) = root {
                    imported.push(key);
                    if !roots.iter().any(|r| r == root) {
                        roots.push(root.to_owned());
                    }
                }
            }
        }
    }
    let mut inputs: Vec<String> = keys
        .iter()
        .filter(|k| !imported.contains(k))
        .cloned()
        .collect();
    if inputs.is_empty() {
        inputs = keys.clone();
    }
    let descriptors = Descriptors::from_loader(&inputs, &roots, &|p: &str| by_path.get(p).cloned())
        .map_err(|e| match e {
            GrpcError::Proto(m) => ContractError::Spec(m),
            other => ContractError::Spec(other.to_string()),
        })?;
    Ok(descriptors.schema())
}
