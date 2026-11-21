// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-wasm`: the core as WebAssembly, for the web app and the browser conformance suite.
//!
//! # JavaScript API (wasm32 only)
//!
//! ```text
//! send(files, requestPath, options?) -> Promise<SendReport>
//! ```
//!
//! - `files`: the collection's files as an object or `Map` from path (relative to the
//!   collection root, `/`-separated, e.g. `"users/get-user.yml"`) to `string` (UTF-8) or
//!   `Uint8Array`. Body files referenced by requests are read from here too.
//! - `requestPath`: the request file's path in `files`.
//! - `options`: `{ environment?: string, vars?: Record<string, string>,
//!   globals?: Record<string, string>, bodyLimit?: number }`. `vars` are runtime variables
//!   (highest precedence, like `rt send --var`), `globals` the lowest layer.
//!
//! Resolves with the `SendReport` as a plain object: exactly the JSON `rt send` prints
//! (`JSON.parse` of the same serialization). Errors of the send itself (request not found,
//! preparation, transport) are in the report, as in the CLI. Rejects only when the input is
//! unusable: a `TypeError` for bad arguments, an `Error` named `FormatError` when the files
//! are not a valid collection.
//!
//! Requests go out through the browser's `fetch` (`FetchTransport`), so CORS applies.

use std::collections::BTreeMap;

use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::send::{SendOptions, SendReport, send};
use rt_core::transport::Transport;
use rt_format::{FileSet, FormatError, read_collection};
use serde::Deserialize;

/// The `options` argument of `send`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub environment: Option<String>,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    #[serde(default)]
    pub globals: BTreeMap<String, String>,
    pub body_limit: Option<usize>,
}

impl Options {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("invalid options: {e}"))
    }
}

/// Body files from the in-memory file set. Same rules as the CLI's file-system source:
/// relative, `/`-separated, no `..`, no absolute paths.
#[derive(Debug, Clone, Copy)]
pub struct MemFiles<'a>(pub &'a FileSet);

impl FileSource for MemFiles<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        if path.starts_with(['/', '\\']) || path.contains(':') {
            return Err("absolute paths are not allowed".into());
        }
        let mut parts = Vec::new();
        for part in path.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => return Err("path leaves the collection folder".into()),
                p => parts.push(p),
            }
        }
        let key = parts.join("/");
        self.0
            .files
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("no file {key}"))
    }
}

/// Reads the collection from `files` and sends one request. The Rust form of the JS `send`.
pub async fn send_files<T: Transport>(
    files: &FileSet,
    request_path: &str,
    options: &Options,
    transport: &T,
) -> Result<SendReport, FormatError> {
    let doc = read_collection(files)?;
    let input = PrepareInput {
        environment: options.environment.as_deref(),
        globals: options.globals.clone().into_iter().collect(),
        runtime: options.vars.clone().into_iter().collect(),
    };
    let send_options = SendOptions {
        body_limit: options
            .body_limit
            .unwrap_or(SendOptions::default().body_limit),
    };
    Ok(send(
        doc.model(),
        request_path,
        &input,
        &MemFiles(files),
        transport,
        &send_options,
    )
    .await)
}

#[cfg(target_arch = "wasm32")]
mod js {
    use js_sys::{Array, JSON, Map, Object, Uint8Array};
    use rt_core::transport::fetch::FetchTransport;
    use rt_format::FileSet;
    use wasm_bindgen::prelude::*;

    use super::{Options, send_files};

    fn type_error(msg: impl AsRef<str>) -> JsValue {
        js_sys::TypeError::new(msg.as_ref()).into()
    }

    fn file_set(files: &JsValue) -> Result<FileSet, JsValue> {
        let entries = if files.is_instance_of::<Map>() {
            Array::from(files)
        } else if files.is_object() {
            Object::entries(files.unchecked_ref())
        } else {
            return Err(type_error("files must be an object or a Map"));
        };
        let mut set = FileSet::default();
        for entry in entries.iter() {
            let entry: Array = entry.unchecked_into();
            let path = entry
                .get(0)
                .as_string()
                .ok_or_else(|| type_error("file paths must be strings"))?;
            let value = entry.get(1);
            let bytes = if let Some(text) = value.as_string() {
                text.into_bytes()
            } else if value.is_instance_of::<Uint8Array>() {
                value.unchecked_into::<Uint8Array>().to_vec()
            } else {
                return Err(type_error(format!(
                    "file {path}: expected a string or Uint8Array"
                )));
            };
            set.files.insert(path, bytes);
        }
        Ok(set)
    }

    fn options(options: &JsValue) -> Result<Options, JsValue> {
        if options.is_undefined() || options.is_null() {
            return Ok(Options::default());
        }
        let json = JSON::stringify(options)
            .map_err(|_| type_error("options must be JSON-serializable"))?;
        Options::from_json(&String::from(json)).map_err(type_error)
    }

    /// See the crate documentation.
    #[wasm_bindgen(js_name = send)]
    pub async fn send_js(
        files: JsValue,
        #[wasm_bindgen(js_name = requestPath)] request_path: String,
        options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let files = file_set(&files)?;
        let options = self::options(&options)?;
        let report = send_files(&files, &request_path, &options, &FetchTransport::new())
            .await
            .map_err(|e| {
                let err = js_sys::Error::new(&e.to_string());
                err.set_name("FormatError");
                JsValue::from(err)
            })?;
        let json = serde_json::to_string(&report).expect("report serializes");
        JSON::parse(&json)
    }
}

#[cfg(target_arch = "wasm32")]
pub use js::send_js;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_from_json() {
        let o = Options::from_json(
            r#"{"environment":"Local","vars":{"a":"1"},"globals":{"g":"2"},"bodyLimit":10}"#,
        )
        .unwrap();
        assert_eq!(o.environment.as_deref(), Some("Local"));
        assert_eq!(o.vars["a"], "1");
        assert_eq!(o.globals["g"], "2");
        assert_eq!(o.body_limit, Some(10));
        assert_eq!(Options::from_json("{}").unwrap(), Options::default());
        assert!(Options::from_json(r#"{"env":"x"}"#).is_err());
        assert!(Options::from_json(r#"{"vars":{"a":1}}"#).is_err());
    }

    #[test]
    fn mem_files_stay_inside_the_set() {
        let mut set = FileSet::default();
        set.files.insert("files/a.txt".into(), b"a".to_vec());
        let f = MemFiles(&set);
        assert_eq!(f.read("files/a.txt").unwrap(), b"a");
        assert_eq!(f.read("./files//a.txt").unwrap(), b"a");
        assert_eq!(f.read("files\\a.txt").unwrap(), b"a");
        for bad in [
            "/files/a.txt",
            "../a.txt",
            "files/../files/a.txt",
            "C:/a.txt",
            "b.txt",
        ] {
            assert!(f.read(bad).is_err(), "{bad}");
        }
    }
}
