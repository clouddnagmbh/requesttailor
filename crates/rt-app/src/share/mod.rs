// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sharing without a server and copying as code (wave 17, strand C): the types and the format
//! ([`SharePayload`], [`ShareLink`]), the secret rules (`sanitize`), the text of a link
//! (`codec`), the QR code (`qr`) and the generators of [`copy_as`] (`copy`).
//!
//! # The link
//!
//! `<base>#rt=<payload>`, with `payload` = `1.` + base64url(deflate(JSON of
//! [`SharePayload`])). Everything is in the fragment, which no browser sends to a server;
//! creating a link makes no network request and uploads nothing. On the web the base is the
//! app's own origin; on the desktop it is the constant `SHARE_BASE` of the shell
//! (`https://app.requesttailor.com/`), and with an empty base a shell copies only the payload
//! as text `rt:<payload>`. Pasting a link, a fragment or an
//! `rt:` payload (as [`ShareApi::open`]) opens the request as an **unsaved draft**; nothing
//! runs and nothing is sent.
//!
//! # Secrets (not negotiable)
//!
//! The payload is the definition with `{{variables}}` **unresolved** and the names of the
//! variables used. Values of secret variables, tokens and keys are never in it, auth blocks
//! carry only type and placeholder (`{{secret:<name>}}`). Values of non-secret variables
//! only with `include_values` (off by default). Headers `Authorization`, `Cookie`,
//! `Proxy-Authorization`, names containing `key`, `token`, `secret` or `password`, and
//! everything `crate::secret_scan` finds in the payload become placeholders; if a hit
//! remains, no link is made ([`ShareError::SecretsFound`]). The same holds for
//! [`CopyKind`] output: variables stay variables of the target tool (`$TOKEN`,
//! `process.env.TOKEN`, `os.environ["TOKEN"]`), secret values are never inserted.

use serde::{Deserialize, Serialize};

use rt_core::model::{Environment, RequestSpec, Variable};

use crate::{AppError, Workspace};

mod codec;
mod copy;
mod qr;
mod sanitize;

pub use codec::{decode, encode, find_payload, looks_shared};
pub use copy::copy_spec;
pub use qr::qr_svg;
pub use sanitize::{is_clean, is_sensitive_name, placeholder, sanitize};

/// The version of the payload.
pub const SHARE_VERSION: u32 = 1;

/// The fragment key of a link: `<base>#rt=<payload>`.
pub const FRAGMENT_KEY: &str = "rt";

/// The prefix of a payload copied without a base: `rt:<payload>`.
pub const TEXT_PREFIX: &str = "rt:";

/// The longest link made; a longer one is refused with [`ShareError::TooLong`].
pub const MAX_LINK_LEN: usize = 8_000;

/// The longest link a QR code carries; above it a link is fine but has no QR code.
pub const MAX_QR_LEN: usize = 2_000;

/// What a link carries (before encoding).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharePayload {
    pub version: u32,
    /// The request's name.
    pub name: String,
    /// The definition with `{{variables}}` unresolved and secrets replaced.
    pub request: RequestSpec,
    /// The variables the request uses.
    pub variables: Vec<ShareVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareVariable {
    pub name: String,
    /// Only for non-secret variables and only with `include_values`.
    pub value: Option<String>,
    pub secret: bool,
}

/// What the user chose for a link.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ShareOptions {
    /// "Include variable values": non-secret values of `environment`. Off by default.
    #[serde(default)]
    pub include_values: bool,
    /// The environment whose values and secret names count; `None`: none.
    #[serde(default)]
    pub environment: Option<String>,
    /// The base of the link (the shell's own origin or `SHARE_BASE`); empty or `None`: only
    /// the payload as `rt:<payload>`.
    #[serde(default)]
    pub base: Option<String>,
}

/// A link (or payload text) and what is in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShareLink {
    /// `<base>#rt=<payload>`, or `rt:<payload>` without a base.
    pub text: String,
    /// Exactly what the link carries, as text ("What is in this link").
    pub preview: String,
    pub payload: SharePayload,
    /// Length of `text` in characters.
    pub length: usize,
    /// A QR code can carry `text` (at most [`MAX_QR_LEN`] characters).
    pub fits_qr: bool,
}

/// What a request is copied as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CopyKind {
    Curl,
    /// JavaScript `fetch`.
    Fetch,
    /// Python with `requests`.
    Python,
}

/// What the user chose when copying.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CopyOptions {
    /// "Resolve variables": put the values of non-secret variables of `environment` into the
    /// snippet. Off by default; secret values are never put in, not even on request.
    #[serde(default)]
    pub resolve_variables: bool,
    #[serde(default)]
    pub environment: Option<String>,
}

/// The result of copying: the snippet, or why this request cannot be copied as that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopiedAs {
    pub kind: CopyKind,
    pub text: String,
    /// What the snippet leaves out (a GraphQL variable, a WebSocket message, ...).
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum ShareError {
    /// The scanner found credentials that could not be replaced; no link is made.
    #[error("{0}")]
    SecretsFound(String),
    /// The link would be longer than [`MAX_LINK_LEN`].
    #[error("this request is too large for a link ({0} characters); save it as a file instead")]
    TooLong(usize),
    /// The text is not a link of this app.
    #[error("{0}")]
    Invalid(String),
    /// The link was made by a newer version.
    #[error("this link needs a newer version of the app (format {0})")]
    Version(u32),
    /// The request cannot be written in that form ("not available for this request").
    #[error("{0}")]
    Unavailable(String),
}

impl From<ShareError> for AppError {
    fn from(e: ShareError) -> Self {
        match e {
            ShareError::SecretsFound(m) => AppError::SecretsFound(m),
            ShareError::Unavailable(m) => AppError::Unsupported(m),
            other => AppError::Format(other.to_string()),
        }
    }
}

/// The link for `spec` (named `name`), from the definition alone: `environment` supplies the
/// names (and, with `include_values`, the non-secret values) of the variables it uses.
pub fn build(
    name: &str,
    spec: &RequestSpec,
    environment: Option<&Environment>,
    options: &ShareOptions,
) -> Result<ShareLink, ShareError> {
    let clean = sanitize::sanitize(spec);
    let mut request = clean.spec;
    request.name = name.to_owned();
    let variables = variables_of(&request, &clean.placeholders, environment, options);
    let payload = SharePayload {
        version: SHARE_VERSION,
        name: name.to_owned(),
        request,
        variables,
    };
    let preview =
        serde_json::to_string_pretty(&payload).map_err(|e| ShareError::Invalid(e.to_string()))?;
    // The second net: whatever the rules above missed, the scanner of the commit sees.
    let findings = crate::secret_scan::scan(&[("request.json", &preview)]);
    if !findings.is_empty() {
        return Err(ShareError::SecretsFound(crate::secret_scan::refusal(
            &findings,
        )));
    }
    let text = match options
        .base
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        Some(base) => {
            let base = base.split('#').next().unwrap_or(base);
            format!("{base}#{FRAGMENT_KEY}={}", encode(&payload)?)
        }
        None => format!("{TEXT_PREFIX}{}", encode(&payload)?),
    };
    let length = text.chars().count();
    if length > MAX_LINK_LEN {
        return Err(ShareError::TooLong(length));
    }
    Ok(ShareLink {
        fits_qr: length <= MAX_QR_LEN,
        text,
        preview,
        payload,
        length,
    })
}

fn variables_of(
    request: &RequestSpec,
    placeholders: &[String],
    environment: Option<&Environment>,
    options: &ShareOptions,
) -> Vec<ShareVariable> {
    let own: Vec<&Variable> = request
        .vars
        .iter()
        .filter(|v| v.enabled && (v.secret || v.value.is_some()))
        .collect();
    let mut out: Vec<ShareVariable> = Vec::new();
    for used in sanitize::used_names(request) {
        let (name, placeholder) = match used.strip_prefix("secret:") {
            Some(n) => (n.trim().to_owned(), true),
            None => (used, false),
        };
        // The request carries its own definition of the variable.
        if own.iter().any(|v| v.name == name) || out.iter().any(|v| v.name == name) {
            continue;
        }
        let known =
            environment.and_then(|e| e.variables.iter().find(|v| v.enabled && v.name == name));
        let secret = placeholder
            || placeholders.contains(&name)
            || sanitize::is_sensitive_name(&name)
            || known.is_some_and(|v| v.secret);
        let value = match known {
            Some(v) if options.include_values && !secret => v.value.clone(),
            _ => None,
        };
        out.push(ShareVariable {
            name,
            value,
            secret,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The link for the request at `path` of `workspace` as it is in memory (unsaved edits
/// included). No request is made and nothing is uploaded.
pub fn link(
    workspace: &Workspace,
    path: &str,
    options: &ShareOptions,
) -> Result<ShareLink, AppError> {
    let spec = workspace.request(path)?;
    let environment = match options.environment.as_deref() {
        Some(name) => Some(
            workspace
                .environments()
                .into_iter()
                .find(|e| e.name == name)
                .ok_or_else(|| AppError::EnvironmentNotFound(name.to_owned()))?,
        ),
        None => None,
    };
    Ok(build(
        &spec.name.clone(),
        &spec,
        environment.as_ref(),
        options,
    )?)
}

/// Reads a pasted link, fragment or `rt:` payload. Nothing runs and nothing is created.
pub fn open(input: &str) -> Result<SharePayload, AppError> {
    Ok(decode(input)?)
}

/// The request at `path` as curl, `fetch` or Python.
pub fn copy_as(
    workspace: &Workspace,
    path: &str,
    kind: CopyKind,
    options: &CopyOptions,
) -> Result<CopiedAs, AppError> {
    let spec = workspace.request(path)?;
    let environment = match options.environment.as_deref() {
        Some(name) => Some(
            workspace
                .environments()
                .into_iter()
                .find(|e| e.name == name)
                .ok_or_else(|| AppError::EnvironmentNotFound(name.to_owned()))?,
        ),
        None => None,
    };
    Ok(copy_spec(&spec, kind, environment.as_ref(), options)?)
}

/// The QR code of a link as SVG. Rejects a link longer than [`MAX_QR_LEN`].
pub fn qr(link: &str) -> Result<String, AppError> {
    Ok(qr_svg(link)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_app_errors() {
        assert!(matches!(
            AppError::from(ShareError::SecretsFound("x".into())),
            AppError::SecretsFound(_)
        ));
        assert!(matches!(
            AppError::from(ShareError::TooLong(9000)),
            AppError::Format(_)
        ));
        const { assert!(MAX_QR_LEN < MAX_LINK_LEN) };
    }
}
