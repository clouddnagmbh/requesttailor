// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Passive API security checks (wave 19, strand R): `Backend.security`, `rt audit` for CI.
//! Rules are pure functions over a masked [`rt_core::send::SendReport`] (and, natively, the
//! [`rt_core::transport::TlsInfo`] of its response). They run on responses that were sent
//! anyway: **no extra request**, ever. The findings are calm hints in the manner of the
//! response checks of wave 18 (`rt_app::inspect`), switchable per device and per collection
//! (`extensions.requesttailor.security`). A token's value never appears in a finding: only
//! its form and the verdict.
//!
//! Rules (ids are stable, listed by [`SecurityApi::rules`]): open CORS (`cors/wildcard-credentials`,
//! `cors/reflected-origin`), weak JWTs in requests and responses (`jwt/alg-none`,
//! `jwt/weak-hmac` as a hint without a value, `jwt/no-exp`, `jwt/expired-in-use`), missing
//! security headers (`headers/hsts`, `headers/content-type-options`,
//! `headers/cache-control-tokens`), no sign of rate limiting (`rate/no-headers`, uses the
//! rate tracker of wave 18), TLS (`tls/old-protocol`, `tls/cert-expired`,
//! `tls/cert-expiring`, `tls/weak-key`, native only), revealing error pages
//! (`errors/stack-trace`, `errors/server-version`). Every rule maps to a category of the
//! OWASP API Security Top 10 (2023).
//!
//! Interface: types and the hook [`passive`] (answers nothing) until strand R.

use serde::{Deserialize, Serialize};

use rt_core::send::SendReport;

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

/// Per device; the collection's own switch is `extensions.requesttailor.security.enabled`.
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

/// The hook strand R fills: all passive rules over one masked report. Interface: nothing.
pub fn passive(report: &SendReport, settings: &SecuritySettings) -> Vec<SecurityFinding> {
    let _ = (report, settings);
    Vec::new()
}

/// The report formats of an audit (`rt audit --out`), shared by the CLI and distributions.
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
