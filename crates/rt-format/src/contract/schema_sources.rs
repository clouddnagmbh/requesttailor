// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Where the schema files of a collection are (wave 18, strand H), so that every shell reads
//! the same ones. The core does not read files: the shell asks [`schema_wanted`] which paths it
//! still has to read, reads them (the work tree, or `GitApi.show` at a revision), notes what it
//! found in a [`SchemaProbe`] and asks again until nothing is wanted; then [`schema_sources`]
//! says which of them are the schema.
//!
//! GraphQL: `extensions.requesttailor.contract.graphql` in `opencollection.yml` (a path relative
//! to the collection; a leading `/` is relative to the repository root), else the first of
//! [`GRAPHQL_CANDIDATES`] that exists, next to the collection and then at the repository root.
//!
//! Protobuf: the `.proto` files of `config.protobuf` and the `protoFilePath` of gRPC requests,
//! and the files they import (looked up in the import paths and in the folder of the importing
//! file, as the gRPC requests do).

use std::collections::BTreeMap;

use rt_core::model::{Item, Protocol};
use rt_grpc::{join_path, normalize_path};

use super::{ContractError, SchemaKind, SchemaSources};
use crate::{COLLECTION_FILE, FileSet};

/// Looked for, in this order, when `opencollection.yml` names no GraphQL schema.
pub const GRAPHQL_CANDIDATES: &[&str] = &[
    "schema.graphql",
    "schema.gql",
    "schema.graphqls",
    "/schema.graphql",
    "/schema.gql",
    "/schema.graphqls",
];

/// What the shell has read so far: path (as asked for) to its bytes, `None` for "does not
/// exist".
pub type SchemaProbe = BTreeMap<String, Option<Vec<u8>>>;

fn check_path(path: &str) -> Result<(), ContractError> {
    let bad = path.is_empty()
        || path.contains('\\')
        || path.contains(':')
        || path.split('/').any(|s| s == "..");
    if bad {
        return Err(ContractError::Spec(format!(
            "the schema path {path:?} must stay inside the collection or the repository"
        )));
    }
    Ok(())
}

/// The GraphQL schema path `opencollection.yml` names, if any.
pub fn configured_graphql(files: &FileSet) -> Option<String> {
    let bytes = files.files.get(COLLECTION_FILE)?;
    let doc = crate::yaml::parse(std::str::from_utf8(bytes).ok()?).ok()?;
    let path = doc
        .root
        .get("extensions")?
        .get("requesttailor")?
        .get("contract")?
        .get("graphql")?
        .text()?
        .trim()
        .to_owned();
    (!path.is_empty()).then_some(path)
}

/// `/` at the start is kept (it means the repository root), the rest is joined and cleaned.
fn join(dir: &str, name: &str) -> String {
    let root = dir.starts_with('/') || name.starts_with('/');
    let joined = join_path(dir.trim_start_matches('/'), name.trim_start_matches('/'));
    if root { format!("/{joined}") } else { joined }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The proto files the collection names: `config.protobuf` and its import paths, and the
/// `protoFilePath` of gRPC requests.
fn proto_inputs(files: &FileSet) -> Result<(Vec<String>, Vec<String>), ContractError> {
    let doc = crate::read_collection(files)
        .map_err(|e| ContractError::Unsupported(format!("cannot read the collection: {e}")))?;
    let model = doc.model();
    let mut inputs: Vec<String> = Vec::new();
    let mut add = |p: String| {
        if !inputs.contains(&p) {
            inputs.push(p);
        }
    };
    for f in &model.config.protobuf.proto_files {
        check_path(f)?;
        add(join("", f));
    }
    fn walk(items: &[Item], out: &mut Vec<String>) {
        for item in items {
            match item {
                Item::Request(r) => {
                    if let Protocol::Grpc(g) = &r.spec.protocol
                        && let Some(f) = g.proto_file.as_deref().filter(|f| !f.trim().is_empty())
                    {
                        out.push(join(parent(&r.path), f));
                    }
                }
                Item::Folder(f) => walk(&f.items, out),
            }
        }
    }
    let mut from_requests = Vec::new();
    walk(&model.items, &mut from_requests);
    for f in from_requests {
        if check_path(&f).is_ok() {
            add(f);
        }
    }
    let mut paths = Vec::new();
    for p in &model.config.protobuf.import_paths {
        check_path(p)?;
        paths.push(join("", p));
    }
    Ok((inputs, paths))
}

/// The imports of the files found so far that no known file satisfies yet.
fn proto_wanted(inputs: &[String], import_paths: &[String], probe: &SchemaProbe) -> Vec<String> {
    let mut roots: Vec<String> = import_paths.to_vec();
    for f in inputs {
        let dir = parent(f).to_owned();
        if !roots.contains(&dir) {
            roots.push(dir);
        }
    }
    let mut wanted: Vec<String> = inputs
        .iter()
        .filter(|p| !probe.contains_key(*p))
        .cloned()
        .collect();
    for (path, bytes) in probe {
        let Some(bytes) = bytes else { continue };
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        for import in super::protobuf::imports_of(text) {
            if import.starts_with("google/protobuf/") {
                continue;
            }
            let candidates: Vec<String> = roots.iter().map(|r| join(r, &import)).collect();
            if candidates
                .iter()
                .any(|c| matches!(probe.get(c), Some(Some(_))))
            {
                continue;
            }
            for c in candidates {
                if !probe.contains_key(&c) && !wanted.contains(&c) {
                    wanted.push(c);
                }
            }
        }
        let _ = path;
    }
    wanted
}

/// The paths still to read for the schema of `kind`; empty when `probe` has everything.
pub fn schema_wanted(
    kind: SchemaKind,
    files: &FileSet,
    probe: &SchemaProbe,
) -> Result<Vec<String>, ContractError> {
    match kind {
        SchemaKind::Graphql => {
            if let Some(path) = configured_graphql(files) {
                check_path(&path)?;
                return Ok(if probe.contains_key(&path) {
                    vec![]
                } else {
                    vec![path]
                });
            }
            // next to the collection first, the repository root only when that has none
            for group in GRAPHQL_CANDIDATES.chunks(3) {
                if group.iter().any(|c| matches!(probe.get(*c), Some(Some(_)))) {
                    return Ok(Vec::new());
                }
                let unread: Vec<String> = group
                    .iter()
                    .filter(|c| !probe.contains_key(**c))
                    .map(|c| (*c).to_owned())
                    .collect();
                if !unread.is_empty() {
                    return Ok(unread);
                }
            }
            Ok(Vec::new())
        }
        SchemaKind::Protobuf => {
            let (inputs, paths) = proto_inputs(files)?;
            Ok(proto_wanted(&inputs, &paths, probe))
        }
    }
}

/// The schema files of one side, from what [`schema_wanted`] had read; `None` if the side has
/// no schema of this kind. A GraphQL schema `opencollection.yml` names but that does not exist
/// is an error (the configuration is wrong).
pub fn schema_sources(
    kind: SchemaKind,
    files: &FileSet,
    probe: &SchemaProbe,
) -> Result<Option<SchemaSources>, ContractError> {
    let mut found = SchemaSources::new();
    match kind {
        SchemaKind::Graphql => {
            if let Some(path) = configured_graphql(files) {
                check_path(&path)?;
                match probe.get(&path) {
                    Some(Some(bytes)) => {
                        found.insert(path, bytes.clone());
                    }
                    _ => {
                        return Err(ContractError::Spec(format!(
                            "the schema {path} named in opencollection.yml does not exist"
                        )));
                    }
                }
            } else if let Some((path, bytes)) = GRAPHQL_CANDIDATES
                .iter()
                .find_map(|c| probe.get(*c).and_then(|b| b.as_ref()).map(|b| (*c, b)))
            {
                found.insert(path.to_owned(), bytes.clone());
            }
        }
        SchemaKind::Protobuf => {
            let (inputs, import_paths) = proto_inputs(files)?;
            // The inputs, then each import the way the compiler finds it.
            let mut queue: Vec<String> = inputs.clone();
            let mut roots: Vec<String> = import_paths;
            for f in &inputs {
                let dir = parent(f).to_owned();
                if !roots.contains(&dir) {
                    roots.push(dir);
                }
            }
            while let Some(path) = queue.pop() {
                let Some(Some(bytes)) = probe.get(&path) else {
                    continue;
                };
                if found.contains_key(&path) {
                    continue;
                }
                found.insert(path.clone(), bytes.clone());
                let Ok(text) = std::str::from_utf8(bytes) else {
                    continue;
                };
                for import in super::protobuf::imports_of(text) {
                    if import.starts_with("google/protobuf/") {
                        continue;
                    }
                    let import = normalize_path(&import);
                    if let Some(hit) = roots
                        .iter()
                        .map(|r| join(r, &import))
                        .find(|c| matches!(probe.get(c), Some(Some(_))))
                    {
                        queue.push(hit);
                    }
                }
            }
        }
    }
    Ok((!found.is_empty()).then_some(found))
}
