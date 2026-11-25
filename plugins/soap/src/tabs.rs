// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The two tabs: `soap` in the request editor (version, `SOAPAction`, WS-Security) and
//! `soap-response` in the response view (readable envelope, faults as plain text, MTOM
//! attachments).

use rt_plugin_sdk::NoteLevel;
use rt_plugin_sdk::api::{
    ActionButton, Block, FieldKind, FormField, PluginBody, PluginResponse, View, ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::{Action, Error, Result, Tab};

use crate::edit::{self, Settings};
use crate::envelope::{self, Envelope, SoapVersion};
use crate::mtom;
use crate::pretty::pretty;
use crate::security::{self, Security};

pub const REQUEST_TAB: &str = "soap";
pub const RESPONSE_TAB: &str = "soap-response";

fn now() -> u64 {
    rt_plugin_sdk::host::now().unwrap_or(0)
}

fn view(blocks: Vec<Block>) -> ViewReply {
    ViewReply {
        view: View { blocks },
        ..ViewReply::default()
    }
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    match tab.tab.as_str() {
        REQUEST_TAB => Ok(request_view(&tab.spec, None)),
        RESPONSE_TAB => Ok(response_view(tab.response.as_ref())),
        other => Err(Error::new(format!("unknown tab {other}"))),
    }
}

pub fn action(action: Action) -> Result<ViewReply> {
    if action.tab != REQUEST_TAB {
        return Err(Error::new(format!("tab {} has no actions", action.tab)));
    }
    let current = edit::read(&action.spec);
    let mut settings = match &current {
        Some(s) => s.clone(),
        None => Settings {
            version: SoapVersion::V11,
            action: edit::action_of(&action.spec),
            security: None,
        },
    };
    match action.action.as_str() {
        "apply" => {
            settings = from_form(&action, &settings)?;
        }
        "refresh" => {
            let Some(sec) = &mut settings.security else {
                return Ok(request_view(
                    &action.spec,
                    Some((
                        NoteLevel::Warning,
                        "There is no WS-Security header to refresh.".into(),
                    )),
                ));
            };
            let ttl = sec.timestamp_ttl.unwrap_or(security::DEFAULT_TTL);
            sec.timestamp_ttl = Some(ttl);
        }
        other => return Err(Error::new(format!("unknown action {other}"))),
    }
    let mut spec = action.spec.clone();
    edit::apply(&mut spec, &settings, now()).map_err(Error::new)?;
    let mut reply = request_view(
        &spec,
        Some((
            NoteLevel::Info,
            "The envelope and headers were updated (unsaved).".into(),
        )),
    );
    reply.update_request = Some(spec);
    Ok(reply)
}

fn from_form(action: &Action, base: &Settings) -> Result<Settings> {
    let version = action
        .input("version")
        .and_then(SoapVersion::from_label)
        .unwrap_or(base.version);
    let flag = |name: &str| action.input(name) == Some("true");
    let var = |name: &str, default: &str| {
        let v = action.input(name).unwrap_or("").trim();
        let v = v
            .strip_prefix("{{")
            .and_then(|r| r.strip_suffix("}}"))
            .unwrap_or(v);
        if v.is_empty() {
            default.to_owned()
        } else {
            v.to_owned()
        }
    };
    for name in ["user_var", "password_var"] {
        let v = var(name, "x");
        if !v
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return Err(Error::new(format!("{v:?} is not a usable variable name")));
        }
    }
    let ttl: Option<u64> = if flag("timestamp") {
        let t = action.input("ttl").unwrap_or("").trim();
        Some(if t.is_empty() {
            security::DEFAULT_TTL
        } else {
            t.parse::<u64>()
                .ok()
                .filter(|n| (1..=86_400).contains(n))
                .ok_or_else(|| {
                    Error::new("the time to live is a number of seconds from 1 to 86400")
                })?
        })
    } else {
        None
    };
    let username_token = flag("username_token");
    Ok(Settings {
        version,
        action: action.input("action").unwrap_or("").trim().to_owned(),
        security: (username_token || ttl.is_some()).then(|| Security {
            username_token,
            user_var: var("user_var", security::USER_VAR),
            password_var: var("password_var", security::PASSWORD_VAR),
            timestamp_ttl: ttl,
        }),
    })
}

fn request_view(spec: &RequestSpec, note: Option<(NoteLevel, String)>) -> ViewReply {
    let mut blocks = Vec::new();
    if let Some((level, text)) = note {
        blocks.push(Block::Notice { level, text });
    }
    let read = edit::read(spec);
    if read.is_none() {
        blocks.push(Block::Notice {
            level: NoteLevel::Info,
            text: "The body is not a SOAP envelope. Apply puts it into one (or starts an empty envelope).".into(),
        });
    }
    let settings = read.unwrap_or(Settings {
        version: SoapVersion::V11,
        action: edit::action_of(spec),
        security: None,
    });
    let sec = settings.security.clone();
    let has_sec = sec.is_some();
    let sec = sec.unwrap_or_default();
    blocks.push(Block::Heading {
        text: "SOAP".into(),
    });
    blocks.push(Block::Text {
        text: "Credentials stay placeholders ({{name}}): the app fills them in when sending, and this plugin never sees their values. Set the password as a secret variable. Only the clear-text password type is possible; use https.".into(),
    });
    blocks.push(Block::Form {
        id: "apply".into(),
        submit: "Apply".into(),
        fields: vec![
            FormField {
                name: "version".into(),
                label: "SOAP version".into(),
                kind: FieldKind::Select,
                value: settings.version.label().into(),
                options: vec!["1.1".into(), "1.2".into()],
            },
            FormField {
                name: "action".into(),
                label: "SOAPAction".into(),
                kind: FieldKind::Text,
                value: settings.action.clone(),
                options: vec![],
            },
            FormField {
                name: "username_token".into(),
                label: "WS-Security UsernameToken".into(),
                kind: FieldKind::Checkbox,
                value: (has_sec && sec.username_token).to_string(),
                options: vec![],
            },
            FormField {
                name: "user_var".into(),
                label: "Username variable".into(),
                kind: FieldKind::Text,
                value: sec.user_var.clone(),
                options: vec![],
            },
            FormField {
                name: "password_var".into(),
                label: "Password variable (secret)".into(),
                kind: FieldKind::Text,
                value: sec.password_var.clone(),
                options: vec![],
            },
            FormField {
                name: "timestamp".into(),
                label: "WS-Security Timestamp".into(),
                kind: FieldKind::Checkbox,
                value: sec.timestamp_ttl.is_some().to_string(),
                options: vec![],
            },
            FormField {
                name: "ttl".into(),
                label: "Timestamp lifetime (seconds)".into(),
                kind: FieldKind::Text,
                value: sec
                    .timestamp_ttl
                    .unwrap_or(security::DEFAULT_TTL)
                    .to_string(),
                options: vec![],
            },
        ],
    });
    if sec.timestamp_ttl.is_some() {
        blocks.push(Block::Actions {
            actions: vec![ActionButton {
                id: "refresh".into(),
                label: "Refresh timestamp".into(),
            }],
        });
        blocks.push(Block::Text {
            text: "The timestamp is also renewed when sending, as long as the envelope holds no secret value. With a password placeholder the plugin cannot rewrite the body (the app refuses patches that contain secrets): refresh it here when the lifetime has passed.".into(),
        });
    }
    view(blocks)
}

/// The text of a response body, and the attachments if it is an MTOM package.
fn envelope_of(response: &PluginResponse) -> (Option<String>, Vec<mtom::Attachment>) {
    let bytes = rt_plugin_sdk::body_bytes(&response.body).unwrap_or_default();
    let content_type = content_type(response);
    if let Some(pkg) = mtom::split(&content_type, &bytes) {
        return (Some(pkg.root), pkg.attachments);
    }
    (
        Some(String::from_utf8_lossy(&bytes).into_owned()),
        Vec::new(),
    )
}

fn content_type(response: &PluginResponse) -> String {
    rt_plugin_sdk::header(&response.headers, "Content-Type")
        .map(str::to_owned)
        .or_else(|| response.body.content_type.clone())
        .unwrap_or_default()
}

/// A parsed response for the view and the hook.
pub struct Parsed {
    pub envelope: Option<Envelope>,
    pub text: String,
    pub attachments: Vec<mtom::Attachment>,
}

pub fn parse_response(response: &PluginResponse) -> Parsed {
    let (text, attachments) = envelope_of(response);
    let text = text.unwrap_or_default();
    Parsed {
        envelope: envelope::parse(&text),
        text,
        attachments,
    }
}

fn response_view(response: Option<&PluginResponse>) -> ViewReply {
    let Some(response) = response else {
        return view(vec![Block::Text {
            text: "No response yet.".into(),
        }]);
    };
    let parsed = parse_response(response);
    let mut blocks = Vec::new();
    let Some(env) = &parsed.envelope else {
        blocks.push(Block::Notice {
            level: NoteLevel::Info,
            text: "The response is not a SOAP envelope.".into(),
        });
        blocks.push(Block::Code {
            language: Some("xml".into()),
            text: pretty(&parsed.text),
        });
        return view(blocks);
    };
    if let Some(f) = &env.fault {
        blocks.push(Block::Notice {
            level: NoteLevel::Error,
            text: format!("SOAP fault: {}", f.sentence()),
        });
        let mut rows = vec![
            ("Code".to_owned(), f.code.clone()),
            ("Reason".to_owned(), f.reason.clone()),
        ];
        if let Some(a) = &f.actor {
            rows.push(("Role".into(), a.clone()));
        }
        if let Some(d) = &f.detail {
            rows.push(("Detail".into(), d.clone()));
        }
        blocks.push(Block::KeyValue { rows });
    }
    blocks.push(Block::Heading {
        text: format!("SOAP {} response", env.version.label()),
    });
    let mut rows = vec![("HTTP status".to_owned(), response.status.to_string())];
    if let Some(op) = envelope::operation_name(env) {
        rows.push(("Result element".into(), op.to_owned()));
    }
    if !env.header.is_empty() {
        rows.push((
            "Header blocks".into(),
            env.header
                .iter()
                .map(|h| h.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    blocks.push(Block::KeyValue { rows });
    if !parsed.attachments.is_empty() {
        blocks.push(Block::Heading {
            text: "Attachments (MTOM, read only)".into(),
        });
        blocks.push(Block::Table {
            columns: vec!["Content-ID".into(), "Type".into(), "Bytes".into()],
            rows: parsed
                .attachments
                .iter()
                .map(|a| {
                    vec![
                        a.content_id.clone(),
                        a.content_type.clone(),
                        a.size.to_string(),
                    ]
                })
                .collect(),
        });
    }
    blocks.push(Block::Code {
        language: Some("xml".into()),
        text: pretty(&parsed.text),
    });
    view(blocks)
}

/// A body as text, for the hooks.
pub fn body_text(body: &PluginBody) -> Option<String> {
    rt_plugin_sdk::body_bytes(body)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}
