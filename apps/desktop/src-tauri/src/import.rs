// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Import: convert files (`rt_app::Imports`), then write the new collection into a folder
//! the user picks and open it. Commands: `import_analyze`, `import_write`, `import_discard`.
//! Owner: wave 4, strand B.
//!
//! `import_analyze` takes its files as one raw IPC body (no JSON number arrays, so a
//! multi-megabyte export crosses the bridge as bytes): a little-endian `u32` with the length
//! of a JSON header `{ "format": ImportFormat | null, "files": [{ "name", "size" }] }`, the
//! header, then the files' bytes one after the other. `apps/desktop/src/import.ts` packs it.

use std::path::{Path, PathBuf};

use rt_app::import::SourceFile;
use rt_app::{AppError, FileChange, ImportPlan};
use rt_format::FileSet;
use rt_format::import::{ImportFormat, MAX_INPUT_BYTES};
use serde::Deserialize;
use tauri::State;
use tauri::ipc::{InvokeBody, Request};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{AppCore, CommandError};
use crate::core::CollectionInfo;
use crate::files;

#[derive(Debug, Deserialize)]
struct Header {
    format: Option<ImportFormat>,
    files: Vec<HeaderFile>,
}

#[derive(Debug, Deserialize)]
struct HeaderFile {
    name: String,
    size: usize,
}

/// Most files in one import (a collection with its environments, a folder of `.http` files).
const MAX_FILES: usize = 1000;

/// Format and `(name, bytes)` per file.
type Unpacked<'a> = (Option<ImportFormat>, Vec<(String, &'a [u8])>);

/// Splits the body `import.ts` packs into the format and the files.
fn unpack(body: &[u8]) -> Result<Unpacked<'_>, AppError> {
    let bad = |why: &str| AppError::Shell(format!("malformed import request: {why}"));
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
            "too many files ({}); import at most {MAX_FILES} at once",
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
    Ok((header.format, files))
}

#[tauri::command]
pub async fn import_analyze(
    core: State<'_, AppCore>,
    request: Request<'_>,
) -> Result<ImportPlan, CommandError> {
    let InvokeBody::Raw(body) = request.body() else {
        return Err(AppError::Shell("import_analyze expects the files as raw bytes".into()).into());
    };
    let (format, files) = unpack(body)?;
    let sources: Vec<SourceFile<'_>> = files
        .iter()
        .map(|(name, data)| SourceFile { name, data })
        .collect();
    let mut imports = core.imports.lock().unwrap_or_else(|p| p.into_inner());
    Ok(imports.analyze(&sources, format)?)
}

/// Writes `files` into the new folder `parent/folder_name` and returns its path. Refuses a
/// folder that already exists and is not empty, so an import never mixes with other files.
pub fn write_new_collection(
    parent: &Path,
    folder_name: &str,
    files: &FileSet,
) -> Result<PathBuf, AppError> {
    let target = parent.join(folder_name);
    if target.exists() {
        let empty = std::fs::read_dir(&target)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !empty {
            return Err(AppError::Import(format!(
                "{} already exists and is not empty; choose another folder",
                target.display()
            )));
        }
    }
    std::fs::create_dir_all(&target)
        .map_err(|e| AppError::Shell(format!("cannot create {}: {e}", target.display())))?;
    let changes: Vec<FileChange> = files
        .files
        .iter()
        .map(|(path, bytes)| FileChange {
            path: path.clone(),
            text: String::from_utf8_lossy(bytes).into_owned(),
            deleted: false,
        })
        .collect();
    files::write_changes(&target, &changes)
        .map_err(|e| AppError::Shell(format!("cannot write {}: {e}", target.display())))?;
    Ok(target)
}

/// Shows the folder picker, writes plan `id` into a new folder `plan.folder_name` inside the
/// picked one and opens it; `None` if cancelled (the plan stays available).
#[tauri::command]
pub async fn import_write(
    app: tauri::AppHandle,
    core: State<'_, AppCore>,
    id: u64,
) -> Result<Option<CollectionInfo>, CommandError> {
    let folder_name = {
        let imports = core.imports.lock().unwrap_or_else(|p| p.into_inner());
        imports.get(id)?.0.to_owned()
    };
    let title = format!("Choose where to create the folder \u{201c}{folder_name}\u{201d}");
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog().file().set_title(title).blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::Shell(e.to_string()))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let parent = picked
        .into_path()
        .map_err(|e| AppError::Shell(format!("unsupported folder: {e}")))?;
    let target = {
        let imports = core.imports.lock().unwrap_or_else(|p| p.into_inner());
        let (name, files) = imports.get(id)?;
        write_new_collection(&parent, name, files)?
    };
    let info = core.open(&target)?;
    core.imports
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .discard(id);
    Ok(Some(info))
}

#[tauri::command]
pub async fn import_discard(core: State<'_, AppCore>, id: u64) -> Result<(), CommandError> {
    core.imports
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .discard(id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(format: Option<&str>, files: &[(&str, &[u8])]) -> Vec<u8> {
        let header = serde_json::json!({
            "format": format,
            "files": files.iter().map(|(n, d)| serde_json::json!({"name": n, "size": d.len()})).collect::<Vec<_>>(),
        })
        .to_string();
        let mut body = (header.len() as u32).to_le_bytes().to_vec();
        body.extend_from_slice(header.as_bytes());
        for (_, d) in files {
            body.extend_from_slice(d);
        }
        body
    }

    #[test]
    fn unpacks_several_files() {
        let body = pack(
            Some("http"),
            &[
                ("a.http", b"GET https://a"),
                ("http-client.env.json", b"{}"),
            ],
        );
        let (format, files) = unpack(&body).unwrap();
        assert_eq!(format, Some(ImportFormat::Http));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], ("a.http".to_owned(), &b"GET https://a"[..]));
        assert_eq!(files[1].1, b"{}");
        let (format, _) = unpack(&pack(None, &[("x", b"1")])).unwrap();
        assert_eq!(format, None);
    }

    #[test]
    fn malformed_bodies_are_refused() {
        let good = pack(None, &[("a.sh", b"curl https://a")]);
        for bad in [
            &b""[..],
            &b"\x01\x00"[..],
            &b"\xff\xff\xff\xff{}"[..],
            &good[..good.len() - 1],
            &[good.as_slice(), b"x"].concat()[..],
        ] {
            assert!(unpack(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn writes_into_a_new_folder_and_refuses_a_used_one() {
        let dir = std::env::temp_dir().join(format!("rt-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut files = FileSet::default();
        files.files.insert(
            "opencollection.yml".into(),
            b"opencollection: \"1.0.0\"\n".to_vec(),
        );
        files
            .files
            .insert("users/get.yml".into(), b"info: {}\n".to_vec());
        let target = write_new_collection(&dir, "Shop", &files).unwrap();
        assert_eq!(target, dir.join("Shop"));
        assert!(target.join("users/get.yml").is_file());
        // Not empty any more: a second import there is refused, nothing is overwritten.
        let e = write_new_collection(&dir, "Shop", &files).unwrap_err();
        assert!(e.to_string().contains("already exists"), "{e}");
        // An existing empty folder is fine.
        std::fs::create_dir_all(dir.join("Empty")).unwrap();
        write_new_collection(&dir, "Empty", &files).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
