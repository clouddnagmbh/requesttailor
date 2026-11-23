// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Folder and collection scripts in the web shell (wave 16, `Backend.folderScripts`):
//!
//! ```text
//! folderScripts(folder: string) -> Script[]
//! setFolderScripts(folder: string, scripts: Script[])
//! ```

use rt_core::model::Script;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = folderScripts)]
    pub fn folder_scripts(&self, folder: &str) -> Result<JsValue, JsValue> {
        let scripts = self.workspace().folder_scripts(folder).map_err(app_error)?;
        to_js(&scripts)
    }

    #[wasm_bindgen(js_name = setFolderScripts)]
    pub fn set_folder_scripts(&self, folder: &str, scripts: JsValue) -> Result<(), JsValue> {
        let scripts: Vec<Script> = from_js(&scripts, "scripts")?;
        self.workspace_mut()
            .set_folder_scripts(folder, &scripts)
            .map_err(app_error)
    }
}
