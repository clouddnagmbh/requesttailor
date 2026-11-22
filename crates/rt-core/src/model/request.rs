// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A request as the user wrote it: templates with `{{variables}}`, not yet resolved.

use serde::{Deserialize, Serialize};

use super::env::Variable;

/// A header or form entry. `enabled = false` keeps it in the file but does not send it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValue {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

impl KeyValue {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    /// Mirrors a pair of the URL's query string for display. The URL is the truth: query
    /// params are never appended when sending (same as Bruno).
    Query,
    /// Replaces `:name` in the URL path.
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub value: String,
    pub kind: ParamKind,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RawKind {
    Json,
    Text,
    Xml,
    Sparql,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartKind {
    Text,
    File,
}

/// One part of a multipart body. For `PartKind::File`, `values` are file paths relative to the
/// collection root; for `Text` it holds exactly one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultipartPart {
    pub name: String,
    pub kind: PartKind,
    pub values: Vec<String>,
    pub content_type: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Body {
    #[default]
    None,
    Raw {
        kind: RawKind,
        data: String,
    },
    FormUrlEncoded {
        fields: Vec<KeyValue>,
    },
    Multipart {
        parts: Vec<MultipartPart>,
    },
    /// Request body read from a file, path relative to the collection root.
    File {
        path: String,
        content_type: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyPlacement {
    Header,
    Query,
}

/// Authentication as declared on a request, folder or collection. Values are templates.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Auth {
    /// Use the nearest ancestor (folder, then collection) that declares something else.
    #[default]
    Inherit,
    /// Explicitly no auth; stops inheritance.
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    ApiKey {
        key: String,
        value: String,
        placement: ApiKeyPlacement,
    },
    /// OAuth 2.0 (wave 5, strand C): the token is fetched, cached and refreshed by
    /// `rt_core::auth::oauth2` when sending.
    #[serde(rename = "oauth2")]
    OAuth2(Box<super::auth::OAuth2>),
    /// AWS Signature V4 (wave 5, strand D): signed by `rt_core::auth::sigv4` as the last
    /// step before the wire.
    #[serde(rename = "awsv4")]
    AwsSigV4(super::auth::AwsSigV4),
    /// Declared in the file but not executable (Digest, NTLM, WSSE, OAuth 1.0, ...).
    /// `kind` is the type name from the file, e.g. "oauth2". Sending fails with a clear error.
    Unsupported {
        kind: String,
    },
}

/// Per-request transport settings. `None` means "inherit" (folder, collection, then default).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Milliseconds; `Some(0)` means no timeout.
    pub timeout_ms: Option<u64>,
    pub follow_redirects: Option<bool>,
    pub max_redirects: Option<u32>,
    pub encode_url: Option<bool>,
}

/// An HTTP request as declared in a request file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RequestSpec {
    pub name: String,
    /// Sort order inside the parent folder.
    pub seq: Option<f64>,
    pub method: String,
    /// URL template, e.g. `{{baseUrl}}/users/:id`.
    pub url: String,
    pub headers: Vec<KeyValue>,
    pub params: Vec<Param>,
    pub body: Body,
    pub auth: Auth,
    /// Request-level variables (highest file-based precedence).
    pub vars: Vec<Variable>,
    pub settings: Settings,
    /// Scripts in file order (wave 7).
    #[serde(default)]
    pub scripts: Vec<super::Script>,
    /// Declarative assertions (wave 7).
    #[serde(default)]
    pub assertions: Vec<super::Assertion>,
    /// Variable actions such as "set variable from the response" (wave 7).
    #[serde(default)]
    pub actions: Vec<super::VarAction>,
}
