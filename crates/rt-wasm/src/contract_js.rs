// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand F: the contract check, spec changes and changelog (`rt_app::contract`).
//!
//! The web shell reads the files and the spec's bytes itself (`GitApi.treeAt`/`show`, or the
//! open folder) and hands them over; these functions are the same core steps the desktop and
//! the CLI run. `files` are `[path, text][]` of the collection, a spec is its bytes or `null`
//! if there is none. The web shell's `contract.ts` declares the TypeScript types of these
//! functions itself, so `apps/web/src/wasm.ts` stays untouched.

use js_sys::Uint8Array;
use rt_format::FileSet;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};

fn file_set(files: &JsValue) -> Result<FileSet, JsValue> {
    let list: Vec<(String, String)> = from_js(files, "files")?;
    Ok(FileSet {
        files: list.into_iter().map(|(p, t)| (p, t.into_bytes())).collect(),
    })
}

fn bytes(spec: Option<Uint8Array>) -> Option<Vec<u8>> {
    spec.map(|a| a.to_vec())
}

fn fail(e: rt_format::contract::ContractError) -> JsValue {
    app_error(e.into())
}

/// The spec path `opencollection.yml` of `files` names, or `null`.
#[wasm_bindgen(js_name = contractConfigured)]
pub fn contract_configured(files: JsValue) -> Result<Option<String>, JsValue> {
    Ok(rt_format::contract::configured_spec(&file_set(&files)?))
}

/// `{ title, version }` of a spec.
#[wasm_bindgen(js_name = contractSpecInfo)]
pub fn contract_spec_info(spec: Uint8Array) -> Result<JsValue, JsValue> {
    let (title, version) = rt_format::contract::spec_info(&spec.to_vec()).map_err(fail)?;
    to_js(&serde_json::json!({ "title": title, "version": version }))
}

/// `ContractReport` of the collection `files` against `spec`.
#[wasm_bindgen(js_name = contractCheck)]
pub fn contract_check(files: JsValue, spec: Uint8Array) -> Result<JsValue, JsValue> {
    let report = rt_format::contract::check(&file_set(&files)?, &spec.to_vec()).map_err(fail)?;
    to_js(&report)
}

/// `SpecChanges` from `old` to `new` (`null`: no spec on that side), with the requests of the
/// new collection `files`.
#[wasm_bindgen(js_name = contractChanges)]
pub fn contract_changes(
    old: Option<Uint8Array>,
    new: Option<Uint8Array>,
    files: JsValue,
) -> Result<JsValue, JsValue> {
    let changes = rt_format::contract::changes(
        bytes(old).as_deref(),
        bytes(new).as_deref(),
        &file_set(&files)?,
    )
    .map_err(fail)?;
    to_js(&changes)
}

/// The changelog (Markdown) of a range; the same text as `rt changelog`. Each side is its
/// files and the bytes of its spec; `from` and `to` label the revisions.
#[wasm_bindgen(js_name = contractChangelog)]
pub fn contract_changelog(
    old_files: JsValue,
    old_spec: Option<Uint8Array>,
    new_files: JsValue,
    new_spec: Option<Uint8Array>,
    from: &str,
    to: &str,
) -> Result<String, JsValue> {
    let (old_files, new_files) = (file_set(&old_files)?, file_set(&new_files)?);
    let (old_spec, new_spec) = (bytes(old_spec), bytes(new_spec));
    // The spec's bytes are all there is: whatever path `load` asks for, it is this file.
    let read_old = |_: &str| Ok(old_spec.clone());
    let read_new = |_: &str| Ok(new_spec.clone());
    rt_app::contract::changelog(
        &rt_app::contract::Side {
            files: &old_files,
            read: &read_old,
        },
        &rt_app::contract::Side {
            files: &new_files,
            read: &read_new,
        },
        from,
        to,
    )
    .map_err(app_error)
}

/// A commit of the range as the shell reads it from Git: its full message is all the facts
/// need besides the id and the author.
#[derive(serde::Deserialize)]
struct RawCommit {
    oid: String,
    message: String,
    author: String,
}

/// `ChangelogFacts` of a range: the same data `rt changelog` hands to a writer, with `commits`
/// (newest first, the first-parent chain of the range) given as `{ oid, message, author }`.
/// The web app adds no sections.
#[wasm_bindgen(js_name = contractChangelogFacts)]
pub fn contract_changelog_facts(
    old_files: JsValue,
    old_spec: Option<Uint8Array>,
    new_files: JsValue,
    new_spec: Option<Uint8Array>,
    from: &str,
    to: &str,
    commits: JsValue,
) -> Result<JsValue, JsValue> {
    let (old_files, new_files) = (file_set(&old_files)?, file_set(&new_files)?);
    let (old_spec, new_spec) = (bytes(old_spec), bytes(new_spec));
    let raw: Vec<RawCommit> = from_js(&commits, "commits")?;
    let commits = raw
        .iter()
        .map(|c| rt_app::contract::ChangelogCommit::from_message(&c.oid, &c.message, &c.author))
        .collect();
    let read_old = |_: &str| Ok(old_spec.clone());
    let read_new = |_: &str| Ok(new_spec.clone());
    let facts = rt_app::contract::changelog_facts(
        &rt_app::contract::Side {
            files: &old_files,
            read: &read_old,
        },
        &rt_app::contract::Side {
            files: &new_files,
            read: &read_new,
        },
        from,
        to,
        &[],
        commits,
    )
    .map_err(app_error)?;
    to_js(&facts)
}

// ---- GraphQL and Protobuf schemas (wave 18, strand H) ----

/// What the shell has read of the schema files so far: `[path, text | null][]`, `null` for a
/// path that does not exist.
type Probe = Vec<(String, Option<String>)>;

fn schema_kind(kind: &str) -> Result<rt_format::contract::SchemaKind, JsValue> {
    match kind {
        "graphql" => Ok(rt_format::contract::SchemaKind::Graphql),
        "protobuf" => Ok(rt_format::contract::SchemaKind::Protobuf),
        other => Err(JsValue::from_str(&format!("unknown schema kind {other:?}"))),
    }
}

fn probe(value: &JsValue) -> Result<rt_format::contract::SchemaProbe, JsValue> {
    let list: Probe = from_js(value, "probe")?;
    Ok(list
        .into_iter()
        .map(|(p, t)| (p, t.map(String::into_bytes)))
        .collect())
}

/// The schema files of `kind` the shell still has to read, given what it has read so far
/// (`[path, text | null][]`); empty when it has them all. Paths are collection-relative, or
/// repository-relative with a leading `/`.
#[wasm_bindgen(js_name = contractSchemaWanted)]
pub fn contract_schema_wanted(
    kind: &str,
    files: JsValue,
    read: JsValue,
) -> Result<JsValue, JsValue> {
    let wanted =
        rt_format::contract::schema_wanted(schema_kind(kind)?, &file_set(&files)?, &probe(&read)?)
            .map_err(fail)?;
    to_js(&wanted)
}

type Sources = Option<rt_format::contract::SchemaSources>;

fn sources(
    kind: rt_format::contract::SchemaKind,
    files: &JsValue,
    read: &JsValue,
) -> Result<Sources, JsValue> {
    rt_format::contract::schema_sources(kind, &file_set(files)?, &probe(read)?).map_err(fail)
}

/// `SpecChanges` of the GraphQL or Protobuf schema from one side to the other; each side is its
/// collection files and what was read of its schema files.
#[wasm_bindgen(js_name = contractSchemaChanges)]
pub fn contract_schema_changes(
    kind: &str,
    old_files: JsValue,
    old_read: JsValue,
    new_files: JsValue,
    new_read: JsValue,
) -> Result<JsValue, JsValue> {
    let kind = schema_kind(kind)?;
    let (old, new) = (
        sources(kind, &old_files, &old_read)?,
        sources(kind, &new_files, &new_read)?,
    );
    let changes = rt_format::contract::schema_changes(
        kind,
        old.as_ref(),
        new.as_ref(),
        &file_set(&new_files)?,
    )
    .map_err(fail)?;
    to_js(&changes)
}

/// The changelog section (`## GraphQL` or `## Protobuf`) of a range, or `undefined` when the
/// schema did not change; the same text `rt changelog` appends.
#[wasm_bindgen(js_name = contractSchemaSection)]
pub fn contract_schema_section(
    kind: &str,
    old_files: JsValue,
    old_read: JsValue,
    new_files: JsValue,
    new_read: JsValue,
) -> Result<Option<String>, JsValue> {
    let kind = schema_kind(kind)?;
    let (old, new) = (
        sources(kind, &old_files, &old_read)?,
        sources(kind, &new_files, &new_read)?,
    );
    let changes = rt_format::contract::schema_changes(
        kind,
        old.as_ref(),
        new.as_ref(),
        &file_set(&new_files)?,
    )
    .map_err(fail)?;
    Ok(rt_format::contract::schema_section(kind, &changes))
}

/// Wave 17: stages `text` as the contract's spec at `path` (the spec the shell found:
/// `opencollection.yml`'s, else `openapi.{yaml,yml,json}`), unsaved. The core refuses any other
/// path and a text that is no spec; `prepareSave` then returns the file like a request edit.
#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = contractStageSpec)]
    pub fn contract_stage_spec(&self, path: &str, text: &str) -> Result<String, JsValue> {
        rt_app::contract::stage_spec(&mut self.workspace_mut(), path, text).map_err(app_error)
    }
}
