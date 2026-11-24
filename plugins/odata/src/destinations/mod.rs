// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer `btp-destinations` (wave 18, strand K): exported SAP BTP destinations become
//! environments, without a network call and without an access key.
//!
//! One environment per destination, named like it:
//! - `baseUrl` from `URL`, `sapClient` from `sap-client` (when present);
//! - `BasicAuthentication`: `user` and the secret `password`;
//! - `OAuth2ClientCredentials`: `tokenUrl` (`tokenServiceURL`), `clientId` and the secret
//!   `clientSecret`;
//! - `NoAuthentication`: nothing more.
//!
//! These variable names are the shared interface with the live import of a distribution that
//! reads the destination service; nothing else is shared.
//!
//! **Secrets are references only**: a secret variable has no value, whatever the export
//! holds. No property value reaches a file or the loss report; the report names properties
//! and well-known type names (`Authentication`, `Type`, `ProxyType`), never values.
//!
//! Everything else (SAML bearer, user token exchange, principal propagation, client
//! certificate, on-premise destinations behind a Cloud Connector, non-HTTP destinations) gets
//! an environment with `baseUrl` and `sapClient` only and a loss with the reason.
//!
//! The result has no requests; the collection holds only `opencollection.yml` and
//! `environments/`. The export formats are assumptions (`testdata/destinations/SOURCES.md`).

mod parse;

use rt_plugin_sdk::api::{
    Contributions, ImportReply, ImporterContribution, PluginEnvironment, PluginFile, PluginVariable,
};
use rt_plugin_sdk::{ImportFormat, Loss, LossEffect, LossKind, LossReport, Result, bail};

use crate::landscape::files::CollectionFiles;
use parse::{Destination, parse_file};

/// The importer's id.
pub const IMPORTER: &str = "btp-destinations";

/// What this module adds to the manifest (the importer). Keep it in step with `plugin.json`.
pub fn contributions() -> Contributions {
    let mut c = Contributions::default();
    c.importers.push(ImporterContribution {
        id: IMPORTER.to_owned(),
        label: "SAP BTP destinations (exported JSON or .properties)".to_owned(),
        extensions: vec!["json".to_owned(), "properties".to_owned()],
    });
    c
}

/// True when every file is an exported destination (or several).
pub fn detect(files: &[PluginFile]) -> Result<bool> {
    Ok(!files.is_empty() && files.iter().all(|f| parse_file(f).is_ok()))
}

pub fn import(files: &[PluginFile]) -> Result<ImportReply> {
    if files.is_empty() {
        bail!("no destination file given");
    }
    let mut all: Vec<(Destination, String)> = Vec::new();
    for file in files {
        for destination in parse_file(file)? {
            all.push((destination, stem(&file.name)));
        }
    }

    let mut environments: Vec<PluginEnvironment> = Vec::new();
    let mut losses: Vec<Loss> = Vec::new();
    for (destination, file_stem) in &all {
        let wanted = destination.get("Name").unwrap_or(file_stem).to_owned();
        let name = unique(&wanted, &environments);
        let location = format!("environment {name}");
        if name != wanted {
            losses.push(loss(
                &location,
                LossKind::Other,
                LossEffect::Approximated,
                "destination name already used by another destination; renamed".to_owned(),
            ));
        }
        let (variables, mut found) = map(destination, &location);
        losses.append(&mut found);
        environments.push(PluginEnvironment { name, variables });
    }

    losses.sort_by(|a, b| {
        (a.location != "collection", &a.location, a.kind).cmp(&(
            b.location != "collection",
            &b.location,
            b.kind,
        ))
    });
    let collection = CollectionFiles {
        name: "BTP destinations".to_owned(),
        environments,
        ..CollectionFiles::default()
    };
    let (files, counts) = collection.write();
    Ok(ImportReply {
        collection_name: collection.name,
        files,
        report: LossReport {
            format: ImportFormat::Plugin,
            requests: counts.requests,
            folders: counts.folders,
            environments: counts.environments,
            losses,
        },
    })
}

const AUTH_TYPES: &[&str] = &[
    "NoAuthentication",
    "BasicAuthentication",
    "OAuth2ClientCredentials",
    "OAuth2SAMLBearerAssertion",
    "OAuth2UserTokenExchange",
    "OAuth2JWTBearer",
    "OAuth2Password",
    "OAuth2RefreshToken",
    "OAuth2TechnicalUserPropagation",
    "OAuth2AuthorizationCode",
    "PrincipalPropagation",
    "ClientCertificateAuthentication",
    "SAMLAssertion",
    "SAPAssertionSSO",
    "ApplicationToApplicationSSO",
    "BearerAssertion",
];
pub(crate) const TYPES: &[&str] = &["HTTP", "MAIL", "RFC", "LDAP", "TCP"];
const PROXY_TYPES: &[&str] = &["Internet", "OnPremise", "PrivateLink"];

/// Properties every destination has and that need no report.
const QUIET: &[&str] = &[
    "name",
    "type",
    "description",
    "proxytype",
    "authentication",
    "url",
    "sap-client",
    "tokenserviceurltype",
];

/// The variables of one destination and what was not carried over.
fn map(d: &Destination, location: &str) -> (Vec<PluginVariable>, Vec<Loss>) {
    let mut variables = Vec::new();
    let mut losses = Vec::new();
    let mut used: Vec<&str> = Vec::new();

    if let Some(url) = d.get("URL") {
        variables.push(plain("baseUrl", url));
    }
    if let Some(client) = d.get("sap-client") {
        variables.push(plain("sapClient", client));
    }

    let mut other = |kind: LossKind, text: String| {
        losses.push(loss(location, kind, LossEffect::Dropped, text));
    };

    // Not HTTP: only URL and client, if the destination has them.
    let kind = d.get("Type").unwrap_or("HTTP");
    if !kind.eq_ignore_ascii_case("HTTP") {
        let leftover = leftover(d, &used);
        let rest = names(&leftover);
        other(
            LossKind::Protocol,
            format!(
                "destination type {} is not HTTP; only the URL and sap-client are imported{rest}",
                known(kind, TYPES, "an unknown type"),
            ),
        );
        return (variables, losses);
    }

    // The proxy: an on-premise destination is a virtual host of the Cloud Connector, so
    // its credentials would not work from here either; only the URL and the client stay.
    let auth = d.get("Authentication").unwrap_or("NoAuthentication");
    let behind_proxy = d
        .get("ProxyType")
        .filter(|p| !p.eq_ignore_ascii_case("Internet"));
    let auth_kind = AUTH_TYPES
        .iter()
        .find(|k| k.eq_ignore_ascii_case(auth))
        .copied();
    let imported_auth = matches!(
        auth_kind,
        Some("NoAuthentication" | "BasicAuthentication" | "OAuth2ClientCredentials")
    );
    if behind_proxy.is_none() {
        match auth_kind {
            Some("BasicAuthentication") => {
                used.extend(["User", "Password"]);
                variables.push(plain("user", d.get("User").unwrap_or("")));
                variables.push(secret("password"));
            }
            Some("OAuth2ClientCredentials") => {
                used.extend(["clientId", "clientSecret", "tokenServiceURL"]);
                variables.push(plain("tokenUrl", d.get("tokenServiceURL").unwrap_or("")));
                variables.push(plain("clientId", d.get("clientId").unwrap_or("")));
                variables.push(secret("clientSecret"));
            }
            _ => {}
        }
    }
    let rest = leftover(d, &used);

    if let Some(proxy) = behind_proxy {
        let text = if proxy.eq_ignore_ascii_case("OnPremise") {
            "proxy type OnPremise (Cloud Connector) is not supported; the URL is a virtual host that is not reachable from here".to_owned()
        } else {
            format!(
                "proxy type {} is not supported",
                known(proxy, PROXY_TYPES, "an unknown proxy type")
            )
        };
        let shown = if imported_auth {
            names(&rest)
        } else {
            String::new()
        };
        other(
            LossKind::Setting,
            format!("{text}; only the URL and sap-client are imported{shown}"),
        );
    }
    if !imported_auth {
        other(
            LossKind::Auth,
            format!(
                "authentication {} is not supported; only the URL and sap-client are imported{}",
                known(auth, AUTH_TYPES, "of an unknown type"),
                names(&rest)
            ),
        );
    } else if behind_proxy.is_none() && !rest.is_empty() {
        let credential = rest.iter().any(|n| {
            let n = n.to_ascii_lowercase();
            [
                "password",
                "secret",
                "user",
                "key",
                "token",
                "cert",
                "assertion",
                "audience",
            ]
            .iter()
            .any(|w| n.contains(w))
        });
        other(
            if credential {
                LossKind::Auth
            } else {
                LossKind::Other
            },
            format!("properties not imported{}", names(&rest)),
        );
    }
    (variables, losses)
}

/// Property names that are neither quiet nor in `used`, in file order, without duplicates.
fn leftover(d: &Destination, used: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (key, _) in &d.props {
        let k = key.to_ascii_lowercase();
        let skip = QUIET.contains(&k.as_str())
            || k.starts_with("html5.")
            || k.starts_with("webide")
            || used.iter().any(|u| u.eq_ignore_ascii_case(key));
        if !skip && !out.iter().any(|o| o.eq_ignore_ascii_case(key)) {
            out.push(key.clone());
        }
    }
    out
}

/// `: a, b, c` (capped), or nothing for an empty list. Names are from the file, so they are
/// cleaned to plain characters and cut short.
fn names(list: &[String]) -> String {
    if list.is_empty() {
        return String::new();
    }
    const MAX: usize = 12;
    let mut shown: Vec<String> = list.iter().take(MAX).map(|n| clean(n)).collect();
    if list.len() > MAX {
        shown.push(format!("and {} more", list.len() - MAX));
    }
    format!(" (properties not imported: {})", shown.join(", "))
}

fn clean(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c))
        .take(48)
        .collect()
}

/// A well-known type name as written, anything else as `unknown`; never a free value.
fn known(value: &str, list: &[&str], unknown: &str) -> String {
    list.iter()
        .find(|k| k.eq_ignore_ascii_case(value))
        .map_or_else(|| unknown.to_owned(), |k| (*k).to_owned())
}

fn unique(wanted: &str, taken: &[PluginEnvironment]) -> String {
    let used = |n: &str| taken.iter().any(|e| e.name.eq_ignore_ascii_case(n));
    if !used(wanted) {
        return wanted.to_owned();
    }
    (2..)
        .map(|i| format!("{wanted} {i}"))
        .find(|n| !used(n))
        .unwrap_or_else(|| wanted.to_owned())
}

fn stem(file: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    if stem.is_empty() { "destination" } else { stem }.to_owned()
}

fn plain(name: &str, value: &str) -> PluginVariable {
    PluginVariable {
        name: name.to_owned(),
        value: value.to_owned(),
        secret: false,
    }
}

fn secret(name: &str) -> PluginVariable {
    PluginVariable {
        name: name.to_owned(),
        value: String::new(),
        secret: true,
    }
}

fn loss(location: &str, kind: LossKind, effect: LossEffect, detail: String) -> Loss {
    Loss {
        location: location.to_owned(),
        kind,
        effect,
        detail,
    }
}
