// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Import for the web shell (`rt_app::Imports`): convert, report, then hand the files to the
//! shell, which writes them with the File System Access API. Owner: wave 4, strand B.

use js_sys::Uint8Array;
use rt_app::{FileChange, Imports};
use rt_format::import::ImportFormat;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{app_error, from_js, int, to_js};

#[wasm_bindgen]
pub struct Importer {
    imports: Imports,
}

impl Default for Importer {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Importer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Importer {
        Importer {
            imports: Imports::new(),
        }
    }

    /// `ImportPlan`. `format`: an `ImportFormat` string, or `null` to detect it.
    pub fn analyze(
        &mut self,
        #[wasm_bindgen(js_name = fileName)] file_name: &str,
        data: Uint8Array,
        format: JsValue,
    ) -> Result<JsValue, JsValue> {
        let format: Option<ImportFormat> = from_js(&format, "format")?;
        let plan = self
            .imports
            .analyze(file_name, &data.to_vec(), format)
            .map_err(app_error)?;
        to_js(&plan)
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
