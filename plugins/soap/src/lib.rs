// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! SOAP and WSDL (wave 19, strand T), built only on the public plugin API
//! (`crates/rt-plugin/API.md`). The core knows nothing about SOAP; what this plugin misses in
//! the API is listed in `API-WUENSCHE.md`.
//!
//! - [`import`]: the `wsdl` importer: WSDL 1.1 and 2.0 with their XSD files into requests
//!   (SOAP 1.1 and 1.2 envelopes, `SOAPAction`, sample values from the schema, a loss report).
//! - [`tabs`]: request tab `soap` (version, action, WS-Security) and response tab
//!   `soap-response` (readable envelope, faults as plain text, MTOM attachments read only).
//! - Hooks: `prepare` makes the headers fit the envelope's version and renews an expired
//!   WS-Security timestamp; `response` reports faults in plain words.
//!
//! WS-Security credentials are `{{placeholders}}`; the plugin never sees the values.

// Nested `if let` over XML attributes reads one step per line; let-chains would not be clearer.
#![allow(clippy::collapsible_if)]

pub mod edit;
pub mod envelope;
pub mod import;
pub mod mtom;
pub mod node;
pub mod pretty;
pub mod schema;
pub mod security;
pub mod tabs;
pub mod wsdl;
pub mod xml;

use rt_plugin_sdk::api::{
    ImportReply, NotesReply, PluginBody, PluginContext, PluginFile, PluginRequest, PluginResponse,
    PrepareReply, RequestPatch, ViewReply,
};
use rt_plugin_sdk::{Action, Plugin, Result, Tab, info, warning};

use envelope::SoapVersion;

/// The manifest, embedded into the `rt-plugin` custom section by [`rt_plugin_sdk::plugin!`].
pub const MANIFEST: &[u8] = include_bytes!("../plugin.json");

pub struct Soap;

// Library users (feature `plugin` off) get the parsers without the plugin's exports.
#[cfg(feature = "plugin")]
rt_plugin_sdk::plugin!(Soap, manifest = MANIFEST);

impl Plugin for Soap {
    fn prepare(&self, request: PluginRequest, _context: PluginContext) -> Result<PrepareReply> {
        Ok(prepare(&request))
    }

    fn response(
        &self,
        _request: PluginRequest,
        response: PluginResponse,
        _context: PluginContext,
    ) -> Result<NotesReply> {
        Ok(respond(&response))
    }

    fn render_tab(&self, tab: Tab) -> Result<ViewReply> {
        tabs::render(tab)
    }

    fn action(&self, action: Action) -> Result<ViewReply> {
        tabs::action(action)
    }

    fn detect(&self, importer: &str, files: Vec<PluginFile>) -> Result<bool> {
        match importer {
            "wsdl" => import::detect(&files),
            other => Err(rt_plugin_sdk::Error::new(format!(
                "unknown importer {other}"
            ))),
        }
    }

    fn import(&self, importer: &str, files: Vec<PluginFile>) -> Result<ImportReply> {
        match importer {
            "wsdl" => import::import(&files),
            other => Err(rt_plugin_sdk::Error::new(format!(
                "unknown importer {other}"
            ))),
        }
    }
}

fn is_loopback(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    host.starts_with("127.0.0.1") || host.starts_with("localhost") || host.starts_with("[::1]")
}

/// `prepare`: for a request whose body is a SOAP envelope, a `Content-Type` and `SOAPAction`
/// that fit its version, and a renewed timestamp where the body allows a patch.
pub fn prepare(request: &PluginRequest) -> PrepareReply {
    let mut reply = PrepareReply::default();
    let Some(body) = &request.body else {
        return reply;
    };
    let Some(text) = tabs::body_text(body) else {
        return reply;
    };
    let Some(env) = envelope::parse(&text) else {
        return reply;
    };
    let patch = &mut reply.patch;
    let content_type = rt_plugin_sdk::header(&request.headers, "Content-Type").unwrap_or("");
    let action_header = rt_plugin_sdk::header(&request.headers, "SOAPAction");
    let action = action_header
        .map(|a| a.trim().trim_matches('"').to_owned())
        .or_else(|| {
            content_type.split(';').skip(1).find_map(|p| {
                p.trim()
                    .strip_prefix("action=")
                    .map(|a| a.trim_matches('"').to_owned())
            })
        })
        .unwrap_or_default();
    let expected = match env.version {
        SoapVersion::V11 => "text/xml",
        SoapVersion::V12 => "application/soap+xml",
    };
    if envelope::media_type(content_type) != expected {
        patch
            .set_headers
            .push(("Content-Type".into(), env.version.content_type(&action)));
        reply.notes.push(info(format!(
            "SOAP {}: Content-Type set to {expected}",
            env.version.label()
        )));
    }
    if env.version == SoapVersion::V11 && action_header.is_none() {
        patch
            .set_headers
            .push(("SOAPAction".into(), format!("\"{action}\"")));
        reply
            .notes
            .push(info("SOAP 1.1: SOAPAction header added (it is required)"));
    }
    if let Some(expires) = security::expires(&text) {
        let now = rt_plugin_sdk::host::now().unwrap_or(0);
        if now > 0 && now >= expires {
            if text.contains("***") {
                reply.notes.push(warning(
                    "The WS-Security timestamp has expired. The body holds a secret, so it cannot be renewed here: open the SOAP tab and press \"Refresh timestamp\".",
                ));
            } else if let Some(fresh) = security::refresh_timestamp(&text, now) {
                patch.body = Some(PluginBody {
                    content_type: body.content_type.clone(),
                    text: Some(fresh),
                    base64: None,
                });
                reply.notes.push(info("WS-Security timestamp renewed"));
            }
        }
    }
    if text.contains("UsernameToken")
        && request.url.starts_with("http://")
        && !is_loopback(&request.url)
    {
        reply.notes.push(warning(
            "WS-Security sends the password in clear text; the URL is http, not https",
        ));
    }
    let _ = RequestPatch::default;
    reply
}

/// `response`: a fault becomes a warning in plain words.
pub fn respond(response: &PluginResponse) -> NotesReply {
    let mut notes = Vec::new();
    let parsed = tabs::parse_response(response);
    if let Some(env) = &parsed.envelope {
        if let Some(fault) = &env.fault {
            notes.push(warning(format!("SOAP fault: {}", fault.sentence())));
        }
        if !parsed.attachments.is_empty() {
            notes.push(info(format!(
                "{} MTOM attachment(s), shown read only in the SOAP tab",
                parsed.attachments.len()
            )));
        }
    }
    NotesReply {
        notes,
        ..NotesReply::default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_manifest_is_json_with_the_plugin_id() {
        let value: serde_json::Value = serde_json::from_slice(super::MANIFEST).unwrap();
        assert_eq!(value["api"], 1);
        assert_eq!(value["id"], "soap");
    }
}
