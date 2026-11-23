// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Import for the web shell (`rt_app::Imports`): convert, report, then hand the files to the
//! shell, which writes them with the File System Access API. Owner: wave 4, strand B.
//!
//! `analyze(files, format)`: `files` is `{ name: string, data: Uint8Array }[]`, imported
//! together as one collection (a collection with its environment files, several `.http` or
//! cURL files); `format` is an `ImportFormat` string, or `null` to detect it.

use js_sys::{Array, Reflect, Uint8Array};
use rt_app::{FileChange, Imports};
use rt_format::import::{ImportFormat, SourceFile};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, int, to_js};

#[wasm_bindgen]
pub struct Importer {
    pub(crate) imports: Imports,
}

impl Default for Importer {
    fn default() -> Self {
        Self::new()
    }
}

fn type_error(message: &str) -> JsValue {
    js_sys::TypeError::new(message).into()
}

/// `(name, bytes)` of each `{ name, data }` in `files`.
fn read_files(files: &Array) -> Result<Vec<(String, Vec<u8>)>, JsValue> {
    let mut out = Vec::with_capacity(files.length() as usize);
    for entry in files.iter() {
        let name = Reflect::get(&entry, &"name".into())?
            .as_string()
            .ok_or_else(|| type_error("files[].name must be a string"))?;
        let data = Reflect::get(&entry, &"data".into())?
            .dyn_into::<Uint8Array>()
            .map_err(|_| type_error("files[].data must be a Uint8Array"))?;
        out.push((name, data.to_vec()));
    }
    Ok(out)
}

#[wasm_bindgen]
impl Importer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Importer {
        Importer {
            imports: Imports::new(),
        }
    }

    /// `ImportPlan` for `files` (see the module documentation).
    pub fn analyze(&mut self, files: Array, format: JsValue) -> Result<JsValue, JsValue> {
        let format: Option<ImportFormat> = from_js(&format, "format")?;
        let owned = read_files(&files)?;
        let sources: Vec<SourceFile<'_>> = owned
            .iter()
            .map(|(name, data)| SourceFile { name, data })
            .collect();
        let plan = self.imports.analyze(&sources, format).map_err(app_error)?;
        to_js(&plan)
    }

    /// `ImportPlan` for what a plugin importer returned (`Plugins.importFiles`, wave 5
    /// strand A): checked like a built-in import, then written with `take` like any other.
    #[wasm_bindgen(js_name = addPluginImport)]
    pub fn add_plugin_import(&mut self, reply: JsValue) -> Result<JsValue, JsValue> {
        let reply: rt_app::plugins::ImportReply = from_js(&reply, "reply")?;
        let import = rt_app::plugins::plugin_import(reply).map_err(app_error)?;
        to_js(&self.imports.add(import))
    }

    /// The files of plan `id` as `FileChange[]` (paths relative to the new collection's
    /// folder); the plan is gone afterwards.
    pub fn take(&mut self, id: f64) -> Result<JsValue, JsValue> {
        let files = self.imports.take(int(id)?).map_err(app_error)?;
        let changes: Vec<FileChange> = files
            .files
            .into_iter()
            .map(|(path, bytes)| FileChange {
                path,
                text: String::from_utf8_lossy(&bytes).into_owned(),
                deleted: false,
            })
            .collect();
        to_js(&changes)
    }

    pub fn discard(&mut self, id: f64) -> Result<(), JsValue> {
        self.imports.discard(int(id)?);
        Ok(())
    }
}

// ---- wave 13: extension imports, into the open collection ----

#[wasm_bindgen]
impl Importer {
    /// `ImportPlan` for an `ExternalImport` (a collection converted by an extension), checked
    /// like a plugin import, then written with `take` like any other.
    pub fn adopt(&mut self, import: JsValue) -> Result<JsValue, JsValue> {
        let import: rt_app::import::ExternalImport = from_js(&import, "import")?;
        to_js(&self.imports.adopt(import).map_err(app_error)?)
    }
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Adds the files of a plan (`Importer.take`) to the open collection as a new folder
    /// `name` (`rt_app::import::add_to_workspace`); returns `FolderImport`. Unsaved.
    #[wasm_bindgen(js_name = addImport)]
    pub fn add_import(&self, files: JsValue, name: &str) -> Result<JsValue, JsValue> {
        let files: Vec<FileChange> = from_js(&files, "files")?;
        let set = rt_format::FileSet {
            files: files
                .into_iter()
                .map(|f| (f.path, f.text.into_bytes()))
                .collect(),
        };
        let result = rt_app::import::add_to_workspace(&mut self.workspace_mut(), &set, name)
            .map_err(app_error)?;
        to_js(&result)
    }
}
