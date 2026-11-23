// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OpenAPI wizard (`OpenApiApi`): fetch a spec from a URL with its referenced files
//! (`rt_app::openapi::fetch_spec` over the native transport), the overview and the plan of
//! the import wizard (`rt_app::openapi`), and the run wizard's checks against the open
//! collection's spec (`rt_app::contract::{requirements, validate_response}`) with the parsed
//! spec cached between calls ([`SpecCache`]).
//!
//! `openapi_inspect` and `openapi_plan` take the files as the raw body `import.ts` packs
//! (a little-endian u32 header length, a JSON header `{ options?, files: [{ name, size }] }`,
//! then the bytes), like `import_analyze`.

use std::sync::{LazyLock, Mutex};

use rt_app::contract::{ResponseCheck, SendRequirements, SpecCache};
use rt_app::import::SourceFile;
use rt_app::openapi::{FetchedSpec, OpenApiOptions, SpecOverview};
use rt_app::{AppError, ImportPlan, SendInput, contract};
use rt_core::transport::native::NativeTransport;
use rt_format::import::MAX_INPUT_BYTES;
use serde::Deserialize;
use tauri::State;
use tauri::ipc::{InvokeBody, Request};

use crate::commands::{AppCore, CommandError};
use crate::contract::{read_disk, work_dir};

/// Most files of one spec (a main file and the files its `$ref`s name).
const MAX_FILES: usize = 256;

/// The parsed spec of the open collection, kept while its bytes stay the same.
static CACHE: LazyLock<Mutex<SpecCache>> = LazyLock::new(|| Mutex::new(SpecCache::default()));

#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    options: Option<OpenApiOptions>,
    files: Vec<FileHeader>,
}

#[derive(Deserialize)]
struct FileHeader {
    name: String,
    size: usize,
}

type Unpacked<'a> = (Option<OpenApiOptions>, Vec<(String, &'a [u8])>);

fn unpack(body: &[u8]) -> Result<Unpacked<'_>, AppError> {
    let bad = |why: &str| AppError::Shell(format!("malformed OpenAPI request: {why}"));
    let len_bytes: [u8; 4] = body
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| bad("no header"))?;
    let header_len = u32::from_le_bytes(len_bytes) as usize;
    let header_bytes = body
        .get(4..4usize.saturating_add(header_len))
        .ok_or_else(|| bad("header is cut off"))?;
    let header: Header = serde_json::from_slice(header_bytes).map_err(|e| bad(&e.to_string()))?;
    if header.files.len() > MAX_FILES {
        return Err(AppError::Import(format!(
            "too many files ({}); a spec may have at most {MAX_FILES}",
            header.files.len()
        )));
    }
    let mut rest = &body[4 + header_len..];
    let mut files = Vec::with_capacity(header.files.len());
    for f in header.files {
        if f.size > MAX_INPUT_BYTES || f.size > rest.len() {
            return Err(bad("file data is cut off"));
        }
        let (data, tail) = rest.split_at(f.size);
        files.push((f.name, data));
        rest = tail;
    }
    if !rest.is_empty() {
        return Err(bad("unexpected data after the files"));
    }
    Ok((header.options, files))
}

fn raw<'a>(request: &'a Request<'_>, command: &str) -> Result<&'a [u8], AppError> {
    match request.body() {
        InvokeBody::Raw(body) => Ok(body),
        _ => Err(AppError::Shell(format!(
            "{command} expects the files as raw bytes"
        ))),
    }
}

fn sources<'a>(files: &'a [(String, &'a [u8])]) -> Vec<SourceFile<'a>> {
    files
        .iter()
        .map(|(name, data)| SourceFile { name, data })
        .collect()
}

#[tauri::command]
pub async fn openapi_fetch(url: String) -> Result<FetchedSpec, CommandError> {
    let runtime = tokio::runtime::Handle::current();
    let fetched = tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        runtime.block_on(rt_app::openapi::fetch_spec(&transport, &url))
    })
    .await
    .map_err(|e| AppError::Shell(format!("fetching the spec failed: {e}")))?;
    Ok(fetched?)
}

#[tauri::command]
pub async fn openapi_inspect(request: Request<'_>) -> Result<SpecOverview, CommandError> {
    let (_, files) = unpack(raw(&request, "openapi_inspect")?)?;
    Ok(rt_app::openapi::inspect(&sources(&files))?)
}

#[tauri::command]
pub async fn openapi_plan(
    core: State<'_, AppCore>,
    request: Request<'_>,
) -> Result<ImportPlan, CommandError> {
    let (options, files) = unpack(raw(&request, "openapi_plan")?)?;
    let options = options.unwrap_or_default();
    let mut imports = core.imports.lock().unwrap_or_else(|p| p.into_inner());
    Ok(imports.analyze_openapi(&sources(&files), &options)?)
}

#[tauri::command]
pub async fn openapi_requirements(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
) -> Result<Option<SendRequirements>, CommandError> {
    // Secret values never come from the webview; the core reads them as for a send.
    let input = SendInput {
        secrets: Vec::new(),
        ..input
    };
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    let work = work_dir(&root);
    let read = |p: &str| read_disk(&root, &work, p);
    let found = core
        .with_send_input(input, |ws, input, _| {
            let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
            contract::requirements(ws, &path, &input, &mut cache, &read)
        })
        .await?;
    Ok(found)
}

#[tauri::command]
pub async fn openapi_validate_response(
    core: State<'_, AppCore>,
    path: String,
    response_id: u64,
) -> Result<Option<ResponseCheck>, CommandError> {
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    let work = work_dir(&root);
    let read = |p: &str| read_disk(&root, &work, p);
    Ok(core.with_responses(|ws, responses| {
        let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        contract::validate_response(ws, &path, responses, response_id, &mut cache, &read)
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(options: Option<&str>, files: &[(&str, &[u8])]) -> Vec<u8> {
        let header = serde_json::json!({
            "options": options.map(|o| serde_json::from_str::<serde_json::Value>(o).unwrap()),
            "files": files.iter().map(|(n, d)| serde_json::json!({ "name": n, "size": d.len() })).collect::<Vec<_>>(),
        })
        .to_string();
        let mut body = (header.len() as u32).to_le_bytes().to_vec();
        body.extend_from_slice(header.as_bytes());
        for (_, d) in files {
            body.extend_from_slice(d);
        }
        body
    }

    const SPEC: &[u8] = br#"{"openapi":"3.0.3","info":{"title":"Pets","version":"1"},"servers":[{"url":"https://api.example.com"}],"paths":{"/pets":{"get":{"tags":["pets"],"responses":{"200":{"description":"ok"}}}},"/users":{"get":{"tags":["users"],"responses":{"200":{"description":"ok"}}}}}}"#;

    #[test]
    fn unpacks_options_and_files() {
        let body = pack(
            Some(r#"{"operations":["GET /pets"],"grouping":"flat","keep_spec":true}"#),
            &[("pets.json", SPEC), ("common.yaml", b"a: 1")],
        );
        let (options, files) = unpack(&body).unwrap();
        let options = options.unwrap();
        assert_eq!(options.operations, Some(vec!["GET /pets".to_owned()]));
        assert!(options.keep_spec);
        assert_eq!(files.len(), 2);
        assert_eq!(files[1], ("common.yaml".to_owned(), &b"a: 1"[..]));
    }

    #[test]
    fn inspects_and_plans_from_the_packed_files() {
        let body = pack(
            Some(r#"{"operations":["GET /pets"]}"#),
            &[("pets.json", SPEC)],
        );
        let (options, files) = unpack(&body).unwrap();
        let overview = rt_app::openapi::inspect(&sources(&files)).unwrap();
        assert_eq!(overview.title, "Pets");
        assert_eq!(overview.operations.len(), 2);
        let mut imports = rt_app::Imports::default();
        let plan = imports
            .analyze_openapi(&sources(&files), &options.unwrap())
            .unwrap();
        assert_eq!(plan.report.requests, 1);
    }

    #[test]
    fn refuses_cut_bodies() {
        let mut body = pack(None, &[("pets.json", SPEC)]);
        body.truncate(body.len() - 3);
        assert!(unpack(&body).is_err());
        assert!(unpack(&[1, 0]).is_err());
    }
}
