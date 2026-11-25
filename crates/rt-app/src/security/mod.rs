// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Passive API security checks (wave 19, strand R): `Backend.security`, `rt security audit` for CI.
//! Rules are pure functions over a masked [`rt_core::send::SendReport`] (and, natively, the
//! [`rt_core::transport::TlsInfo`] of its response). They run on responses that were sent
//! anyway: **no extra request**, ever. The findings are calm hints in the manner of the
//! response checks of wave 18 (`rt_app::inspect`), switchable per device and per collection
//! (`extensions.requesttailor.security`). A token's value never appears in a finding: only
//! its form and the verdict.
//!
//! Rules (ids are stable, listed by [`rules`]; the ids with `chk/` are rules of the response
//! checks of wave 18 that the audit lists too, the ones with `sec/` are new). Open CORS
//! (`chk/cors-credentials` for a wildcard or a reflected origin with credentials,
//! `sec/cors-reflected-origin` without), weak JWTs in requests and responses (`chk/jwt-alg-none`,
//! `sec/jwt-weak-hmac` as a hint without the secret, `sec/jwt-no-exp`, `sec/jwt-expired-in-use`),
//! missing security headers (`chk/hsts-missing`, `chk/nosniff-missing`,
//! `sec/cache-control-tokens`), no sign of rate limiting (`sec/no-rate-limit`, read like the
//! rate tracker of wave 18 reads a response), TLS (`sec/tls-old-protocol`,
//! `sec/tls-cert-expired`, `sec/tls-cert-expiring`, `sec/tls-weak-key`; native only, the
//! browser's `fetch` tells nothing), revealing error pages (`chk/stack-trace`,
//! `chk/version-disclosure`, `sec/error-page-version`). Every rule maps to a category of the
//! OWASP API Security Top 10 (2023).
//!
//! The `sec/` rules are [`ResponseCheck`]s ([`checks`]) that run in the same pass as the
//! checks of wave 18 (`rt_app::inspect::check_all`), so a finding shows in the response's
//! checks chip and tab, `rt send --checks` prints it, and it switches off per rule on this
//! device or in the collection like any check (`checks.ignore`). The device-wide switch and
//! the list of rules off on this device are [`SecuritySettings`] (a file in the data folder
//! on the desktop and in the CLI, browser storage on the web). A finding turns an
//! [`Observation`] into a [`SecurityFinding`] with the rule's OWASP category and severity
//! ([`findings`]).

use std::path::Path;

use serde::{Deserialize, Serialize};

use rt_core::send::SendReport;

use crate::inspect::{self, Observation, ResponseCheck};
use crate::{AppError, ResponseStore};

mod rules;
mod tokens;

/// The security rules that are new in wave 19, as response checks (the other rules of
/// [`rules`] are already in [`inspect::builtin`]).
pub fn checks() -> &'static [&'static dyn ResponseCheck] {
    rules::checks()
}

/// OWASP API Security Top 10, 2023.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwaspCategory {
    #[serde(rename = "API1:2023")]
    Api1,
    #[serde(rename = "API2:2023")]
    Api2,
    #[serde(rename = "API3:2023")]
    Api3,
    #[serde(rename = "API4:2023")]
    Api4,
    #[serde(rename = "API5:2023")]
    Api5,
    #[serde(rename = "API6:2023")]
    Api6,
    #[serde(rename = "API7:2023")]
    Api7,
    #[serde(rename = "API8:2023")]
    Api8,
    #[serde(rename = "API9:2023")]
    Api9,
    #[serde(rename = "API10:2023")]
    Api10,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// What a rule is, for the list in the settings and for reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityRule {
    /// Stable, `area/name`.
    pub id: String,
    pub title: String,
    pub category: OwaspCategory,
    pub severity: Severity,
    /// Needs the TLS details of the native transport.
    #[serde(default)]
    pub native_only: bool,
}

/// One observation about one response. Never contains a token or secret value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub rule: String,
    pub category: OwaspCategory,
    pub severity: Severity,
    /// Where: `response header access-control-allow-origin`, `request header authorization`.
    pub location: String,
    pub why: String,
    pub fix: String,
}

/// The response whose findings are wanted (`SecurityApi.check`): a stored response of the
/// active collection (as for the response checks).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityTarget {
    pub path: String,
    pub response_id: u32,
}

/// Per device. A collection switches single rules off in `checks.ignore` of `opencollection.yml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecuritySettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Rule ids switched off on this device.
    #[serde(default)]
    pub disabled: Vec<String>,
}

fn yes() -> bool {
    true
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            disabled: Vec::new(),
        }
    }
}

/// What [`rules`] and [`findings`] know about one rule.
struct Meta {
    id: &'static str,
    title: &'static str,
    category: OwaspCategory,
    severity: Severity,
    native_only: bool,
}

const fn meta(
    id: &'static str,
    title: &'static str,
    category: OwaspCategory,
    severity: Severity,
    native_only: bool,
) -> Meta {
    Meta {
        id,
        title,
        category,
        severity,
        native_only,
    }
}

use OwaspCategory::{Api2, Api4, Api8};

/// The rules of the passive audit, in the order they are listed. API2 is broken
/// authentication, API4 unrestricted resource consumption, API8 security misconfiguration.
const CATALOG: &[Meta] = &[
    meta(
        "chk/cors-credentials",
        "CORS allows any origin with credentials",
        Api8,
        Severity::Medium,
        false,
    ),
    meta(
        "sec/cors-reflected-origin",
        "CORS repeats the request's Origin",
        Api8,
        Severity::Low,
        false,
    ),
    meta(
        "chk/jwt-alg-none",
        "Token with alg none",
        Api2,
        Severity::High,
        false,
    ),
    meta(
        "sec/jwt-no-exp",
        "Token without an expiry",
        Api2,
        Severity::Low,
        false,
    ),
    meta(
        "sec/jwt-expired-in-use",
        "Expired token sent with the request",
        Api2,
        Severity::Medium,
        false,
    ),
    meta(
        "sec/jwt-weak-hmac",
        "Token signed with a guessable secret",
        Api2,
        Severity::High,
        false,
    ),
    meta(
        "chk/hsts-missing",
        "Strict-Transport-Security is missing",
        Api8,
        Severity::Low,
        false,
    ),
    meta(
        "chk/nosniff-missing",
        "X-Content-Type-Options is missing",
        Api8,
        Severity::Low,
        false,
    ),
    meta(
        "sec/cache-control-tokens",
        "Token in a response that may be cached",
        Api8,
        Severity::Medium,
        false,
    ),
    meta(
        "sec/no-rate-limit",
        "No sign of rate limiting",
        Api4,
        Severity::Low,
        false,
    ),
    meta(
        "sec/tls-old-protocol",
        "Outdated TLS protocol",
        Api8,
        Severity::High,
        true,
    ),
    meta(
        "sec/tls-cert-expired",
        "Certificate expired",
        Api8,
        Severity::High,
        true,
    ),
    meta(
        "sec/tls-cert-expiring",
        "Certificate expires soon",
        Api8,
        Severity::Low,
        true,
    ),
    meta(
        "sec/tls-weak-key",
        "Weak certificate key",
        Api8,
        Severity::High,
        true,
    ),
    meta(
        "chk/stack-trace",
        "Stack trace in an error response",
        Api8,
        Severity::Medium,
        false,
    ),
    meta(
        "chk/version-disclosure",
        "Server version in a header",
        Api8,
        Severity::Info,
        false,
    ),
    meta(
        "sec/error-page-version",
        "Software version on an error page",
        Api8,
        Severity::Info,
        false,
    ),
];

fn meta_of(id: &str) -> Option<&'static Meta> {
    CATALOG.iter().find(|m| m.id == id)
}

/// The rules of the passive audit with category and severity, for settings lists and reports.
pub fn rules() -> Vec<SecurityRule> {
    CATALOG
        .iter()
        .map(|m| SecurityRule {
            id: m.id.to_owned(),
            title: m.title.to_owned(),
            category: m.category,
            severity: m.severity,
            native_only: m.native_only,
        })
        .collect()
}

/// The findings among `observations`: those of rules in [`rules`], with the rule's OWASP
/// category and severity. Other observations (cookie flags, CSP, framing) are not security
/// findings of this audit and are left out.
pub fn findings(observations: &[Observation]) -> Vec<SecurityFinding> {
    observations
        .iter()
        .filter_map(|o| {
            let m = meta_of(&o.id)?;
            Some(SecurityFinding {
                rule: o.id.clone(),
                category: m.category,
                severity: m.severity,
                location: o.location.clone(),
                why: o.why.clone(),
                fix: o.fix.clone(),
            })
        })
        .collect()
}

/// The rule ids the device settings switch off: every new rule when the audit is off, else the
/// listed ones. The `chk/` rules of wave 18 follow their own lists.
pub fn ignored_by(settings: &SecuritySettings) -> Vec<String> {
    if settings.enabled {
        settings.disabled.clone()
    } else {
        checks().iter().map(|c| c.id().to_owned()).collect()
    }
}

/// All passive rules over one finished report (`rt send --checks`, `rt security audit`): the
/// findings of the response, minus what `settings` switch off. The report's body is read up to
/// [`inspect::BODY_LIMIT`].
pub fn passive(report: &SendReport, settings: &SecuritySettings) -> Vec<SecurityFinding> {
    let mut ignore = ignored_by(settings);
    if settings.enabled {
        // The `chk/` rules in the catalog switch off by id as well.
        ignore.extend(settings.disabled.iter().cloned());
    }
    findings(&inspect::check_report(report, &ignore))
}

/// The findings of a kept response (`SecurityApi.check`). `ignore` is the list of rule ids
/// switched off on this device and in the collection ([`inspect::all_ignored`]); `settings`
/// are added.
pub fn passive_of(
    responses: &ResponseStore,
    id: u64,
    ignore: &[String],
    settings: &SecuritySettings,
) -> Result<Vec<SecurityFinding>, AppError> {
    let mut all = ignore.to_vec();
    all.extend(ignored_by(settings));
    Ok(findings(&inspect::checks_of(responses, id, &all)?))
}

/// The file of [`SecuritySettings`] in the data folder (desktop and CLI share it).
pub const SETTINGS_FILE: &str = "security.json";

/// The settings of this device; the defaults when there is no file, no folder or the file is
/// odd.
pub fn read_settings(data_dir: Option<&Path>) -> SecuritySettings {
    data_dir
        .and_then(|d| std::fs::read(d.join(SETTINGS_FILE)).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Writes the settings of this device (rule ids and one flag; never a value).
pub fn write_settings(
    data_dir: Option<&Path>,
    settings: &SecuritySettings,
) -> Result<(), AppError> {
    let Some(dir) = data_dir else {
        return Err(AppError::Shell("this build has no data folder".into()));
    };
    let known = rules();
    if let Some(unknown) = settings
        .disabled
        .iter()
        .find(|id| !known.iter().any(|r| &r.id == *id))
    {
        return Err(AppError::Shell(format!("unknown security rule: {unknown}")));
    }
    std::fs::create_dir_all(dir).map_err(|e| AppError::Shell(e.to_string()))?;
    let text =
        serde_json::to_string_pretty(settings).map_err(|e| AppError::Shell(e.to_string()))?;
    std::fs::write(dir.join(SETTINGS_FILE), text).map_err(|e| AppError::Shell(e.to_string()))
}

/// The report formats of an audit (`rt security audit --out`), shared by the CLI and distributions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditFormat {
    Text,
    Markdown,
    Html,
    Json,
    /// SARIF 2.1.0 for code scanning in CI.
    Sarif,
}
