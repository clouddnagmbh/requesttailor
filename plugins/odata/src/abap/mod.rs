// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! "Copy as ABAP" (wave 17, strand E): the request tab `abap` turns the OData request of the
//! editor into an ABAP snippet, in two flavors: ABAP Platform with `cl_http_client` and an RFC
//! destination, and ABAP Cloud with `cl_web_http_client_manager` and a communication
//! arrangement; responses are read with `/ui2/cl_json`.
//!
//! Supported: reads (also `$filter`/`$expand`), create, update, delete (CSRF token fetched at run
//! time, `If-Match` from a variable) and `$batch` with reads only. Never in a snippet: secret
//! values, tokens, cookies, passwords, logon data; they stay in the destination or the
//! arrangement, and the snippet has angle-bracket placeholders for what only the user knows.
//! The tab reads the request as edited; it never changes it. Tests: `tests/abap.rs` with golden
//! files in `testdata/abap/`.

mod emit;
mod request;

use rt_plugin_sdk::api::{Block, FieldKind, FormField, View, ViewReply};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::{Action, NoteLevel, Result, Tab};
use serde::{Deserialize, Serialize};

pub use request::Unavailable;

/// Which ABAP runtime the snippet is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flavor {
    /// ABAP Platform (on premise, private cloud): `cl_http_client`, RFC destination.
    Classic,
    /// ABAP Cloud (S/4HANA Cloud, BTP ABAP environment): `cl_web_http_client_manager`.
    Cloud,
}

impl Flavor {
    pub fn id(self) -> &'static str {
        match self {
            Flavor::Classic => "classic",
            Flavor::Cloud => "cloud",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Flavor::Classic => "ABAP Platform (cl_http_client)",
            Flavor::Cloud => "ABAP Cloud (cl_web_http_client_manager)",
        }
    }

    const ALL: [Flavor; 2] = [Flavor::Classic, Flavor::Cloud];

    /// The flavor a host name suggests: SAP's cloud hosts are ABAP Cloud, everything else
    /// is an ABAP Platform system.
    pub fn guess(url: &str) -> Flavor {
        let host = url
            .split_once("://")
            .map_or("", |(_, rest)| {
                rest.split(['/', ':', '?']).next().unwrap_or_default()
            })
            .to_ascii_lowercase();
        if host.ends_with(".s4hana.cloud.sap") || host.ends_with(".hana.ondemand.com") {
            Flavor::Cloud
        } else {
            Flavor::Classic
        }
    }
}

/// A generated snippet and what to tell the user about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub code: String,
    /// Plain sentences: placeholders to fill, what was left out and why.
    pub notes: Vec<String>,
}

/// The ABAP snippet for `spec` with the non-secret `variables` of the selected environment.
pub fn generate(
    spec: &RequestSpec,
    variables: &[(String, String)],
    flavor: Flavor,
) -> std::result::Result<Snippet, Unavailable> {
    let call = request::build(spec, variables)?;
    let code = emit::write(&call, flavor);

    let mut notes = Vec::new();
    match flavor {
        Flavor::Classic => notes.push(
            "Fill in the RFC destination (SM59) of the system; its logon data is not part of the snippet."
                .to_owned(),
        ),
        Flavor::Cloud => notes.push(
            "Fill in the communication scenario and outbound service of your communication arrangement; its logon data is not part of the snippet."
                .to_owned(),
        ),
    }
    if call.kind.needs_token() {
        notes.push(
            "The CSRF token is fetched when the code runs; no token value is copied.".to_owned(),
        );
    }
    if call.kind.needs_etag() && call.if_match.is_none() {
        notes.push("Set lv_etag to the ETag of the entity (header \"etag\" of a read).".to_owned());
    }
    if !call.omitted.is_empty() {
        notes.push(format!(
            "Left out because they carry secrets or sessions: {}.",
            call.omitted.join(", ")
        ));
    }
    if call.had_auth {
        notes.push(
            "The request's authentication is not copied; the destination or arrangement logs on."
                .to_owned(),
        );
    }
    if !call.unresolved.is_empty() {
        notes.push(format!(
            "No value for {}; replace the placeholders in angle brackets.",
            call.unresolved
                .iter()
                .map(|n| format!("{{{{{n}}}}}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(Snippet { code, notes })
}

// -------------------------------------------------------------------------------------------
// The tab

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    flavor: Option<Flavor>,
}

fn state_of(value: Option<&serde_json::Value>) -> State {
    value
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    let state = state_of(tab.state.as_ref());
    let variables = tab
        .context
        .as_ref()
        .map(|c| c.variables.as_slice())
        .unwrap_or_default();
    Ok(view(&tab.spec, variables, state))
}

pub fn action(action: Action) -> Result<ViewReply> {
    let mut state = state_of(action.state.as_ref());
    if action.action == "flavor"
        && let Some(choice) = action.input("flavor")
        && let Some(flavor) = Flavor::ALL.into_iter().find(|f| f.label() == choice)
    {
        state.flavor = Some(flavor);
    }
    let variables = action
        .context
        .as_ref()
        .map(|c| c.variables.as_slice())
        .unwrap_or_default();
    Ok(view(&action.spec, variables, state))
}

fn view(spec: &RequestSpec, variables: &[(String, String)], state: State) -> ViewReply {
    let flavor = state.flavor.unwrap_or_else(|| {
        let url = variables.iter().fold(spec.url.clone(), |u, (n, v)| {
            u.replace(&format!("{{{{{n}}}}}"), v)
        });
        Flavor::guess(&url)
    });
    let mut blocks = vec![Block::Heading {
        text: "Copy as ABAP".into(),
    }];
    blocks.push(Block::Form {
        id: "flavor".into(),
        fields: vec![FormField {
            name: "flavor".into(),
            label: "Target".into(),
            kind: FieldKind::Select,
            value: flavor.label().into(),
            options: Flavor::ALL.iter().map(|f| f.label().to_owned()).collect(),
        }],
        submit: "Show".into(),
    });
    match generate(spec, variables, flavor) {
        Ok(snippet) => {
            for note in snippet.notes {
                blocks.push(Block::Notice {
                    level: NoteLevel::Info,
                    text: note,
                });
            }
            blocks.push(Block::Code {
                language: Some("abap".into()),
                text: snippet.code,
            });
        }
        Err(e) => blocks.push(Block::Notice {
            level: NoteLevel::Warning,
            text: e.to_string(),
        }),
    }
    ViewReply {
        view: View { blocks },
        state: serde_json::to_value(State {
            flavor: Some(flavor),
        })
        .ok(),
        ..ViewReply::default()
    }
}
