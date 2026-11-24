// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Example plugin: uses every extension point of the plugin API (`crates/rt-plugin/API.md`)
//! and asks for no permissions.
//!
//! - `prepare`: sets an `X-Request-ID` header (unless the request has one) and notes it.
//! - `response`: notes status, size and content type.
//! - Response tab `summary`: key-value and table blocks, and an action that adds body
//!   statistics (kept in the tab state).
//! - Importer `url-list`: plain text files with one request per line (`[METHOD] URL`).
//!
//! Build: `node scripts/build-plugins.mjs` → `target/plugins/example.wasm`.

mod import;
mod tab;

use rt_plugin_sdk::api::{
    ImportReply, NotesReply, PluginContext, PluginFile, PluginRequest, PluginResponse,
    PrepareReply, ViewReply,
};
use rt_plugin_sdk::{Action, Plugin, Result, Tab, header, host, info, set_header};

/// The manifest, embedded into the `rt-plugin` custom section by [`rt_plugin_sdk::plugin!`].
pub const MANIFEST: &[u8] = include_bytes!("../plugin.json");

pub struct Example;

rt_plugin_sdk::plugin!(Example, manifest = MANIFEST);

const REQUEST_ID: &str = "X-Request-ID";

impl Plugin for Example {
    fn prepare(&self, request: PluginRequest, _: PluginContext) -> Result<PrepareReply> {
        if let Some(existing) = header(&request.headers, REQUEST_ID) {
            return Ok(PrepareReply {
                notes: vec![info(format!(
                    "{REQUEST_ID}: {existing} (kept from the request)"
                ))],
                ..PrepareReply::default()
            });
        }
        // The host's clock plus a hash of the request: unique enough for tracing, and built
        // only from hex digits, so it never carries a masked `***` value along.
        let id = format!(
            "rt-{:012x}-{:08x}",
            host::now()?,
            fnv1a(&format!("{} {}", request.method, request.url))
        );
        Ok(PrepareReply {
            notes: vec![info(format!("{REQUEST_ID}: {id}"))],
            patch: set_header(REQUEST_ID, id),
        })
    }

    fn response(
        &self,
        _: PluginRequest,
        response: PluginResponse,
        _: PluginContext,
    ) -> Result<NotesReply> {
        let size = rt_plugin_sdk::body_bytes(&response.body)?.len();
        let size = if response.truncated {
            format!("more than {} (cut by the host)", human_size(size))
        } else {
            human_size(size)
        };
        Ok(NotesReply {
            notes: vec![info(format!(
                "{} · {size} · {}",
                response.status,
                content_type(&response).unwrap_or("no content type")
            ))],
            segments: Vec::new(),
        })
    }

    fn render_tab(&self, tab: Tab) -> Result<ViewReply> {
        tab::render(tab)
    }

    fn action(&self, action: Action) -> Result<ViewReply> {
        tab::action(action)
    }

    fn detect(&self, importer: &str, files: Vec<PluginFile>) -> Result<bool> {
        import::detect(importer, &files)
    }

    fn import(&self, importer: &str, files: Vec<PluginFile>) -> Result<ImportReply> {
        import::import(importer, &files)
    }
}

/// The response's content type: the header, else what the host put on the body.
fn content_type(response: &PluginResponse) -> Option<&str> {
    header(&response.headers, "content-type").or(response.body.content_type.as_deref())
}

fn human_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    match bytes as f64 {
        b if b < KB => format!("{bytes} B"),
        b if b < KB * KB => format!("{:.1} KB", b / KB),
        b => format!("{:.1} MB", b / (KB * KB)),
    }
}

/// 32-bit FNV-1a.
fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_are_human() {
        assert_eq!(super::human_size(0), "0 B");
        assert_eq!(super::human_size(1023), "1023 B");
        assert_eq!(super::human_size(1536), "1.5 KB");
        assert_eq!(super::human_size(3 * 1024 * 1024), "3.0 MB");
    }
}
