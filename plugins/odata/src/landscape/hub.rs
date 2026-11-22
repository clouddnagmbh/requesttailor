// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer `sap-hub`: one API specification as downloaded from the SAP Business
//! Accelerator Hub, as a new collection with the sandbox and a DEV/QAS/PRD template.
//!
//! Detection is a heuristic, written against our own synthetic files in
//! `testdata/hub/` (no Hub data is in the repo), not validated against real downloads:
//! - **OpenAPI**: a JSON object with `"openapi": "3…"` and at least one `x-sap-*` key at the
//!   top level or in `info` (the Hub's specifications carry `x-sap-api-type`,
//!   `x-sap-shortText` and others), or a server on `sandbox.api.sap.com`. YAML downloads are
//!   not recognised; the built-in OpenAPI importer takes them.
//! - **EDMX**: an `Edmx` document that mentions `sandbox.api.sap.com` (in SAP Gateway style
//!   `$metadata` the `atom:link rel="self"` carries the URL it was read from). An EDMX
//!   without it is an ordinary `$metadata` for the `edmx` importer.
//!
//! Exactly one file per import.
//!
//! Result:
//! - OpenAPI: requests per path and method ([`super::openapi`]); EDMX: the requests of
//!   [`crate::metadata::requests_for`] for the model of [`crate::metadata::parse`].
//! - Collection: header `APIKey: {{apiKey}}` when the specification has the sandbox (the
//!   plugin's `prepare` drops it where no key is set), auth Basic `{{user}}`/`{{password}}`
//!   for the own systems (dropped in the sandbox by `sapSandbox=true`).
//! - Environments: `API Hub Sandbox` (`baseUrl` of the sandbox server, `apiKey` as a secret)
//!   and the DEV/QAS/PRD template ([`super::template`]) with `baseUrl` =
//!   `{{baseHost}}/sap/opu/…` and empty hosts to fill in.
//! - A loss report for everything not converted.

use rt_plugin_sdk::api::{ImportReply, PluginFile};
use rt_plugin_sdk::model::{Auth, KeyValue};
use rt_plugin_sdk::{ImportFormat, Loss, LossEffect, LossKind, LossReport, Result, bail};
use serde_json::Value;

use super::files::{CollectionFiles, RequestFile, unsupported};
use super::openapi::{self, foreign_refs, loss};
use super::params::API_KEY_HEADER;
use super::template::{Landscape, STAGES, sandbox_environment};

const SANDBOX_HOST: &str = "sandbox.api.sap.com";

enum HubFile {
    OpenApi(Value),
    Edmx(String),
}

fn classify(file: &PluginFile) -> Option<HubFile> {
    let text = rt_plugin_sdk::file_text(file).ok()?;
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('{') {
        let root: Value = serde_json::from_str(trimmed).ok()?;
        let openapi3 = root["openapi"].as_str().is_some_and(|v| v.starts_with('3'));
        let sap_keys = [&root, &root["info"]]
            .into_iter()
            .filter_map(Value::as_object)
            .any(|m| m.keys().any(|k| k.starts_with("x-sap-")));
        let sandbox = servers(&root)
            .iter()
            .any(|u| host_of(u) == Some(SANDBOX_HOST));
        return (openapi3 && (sap_keys || sandbox)).then_some(HubFile::OpenApi(root));
    }
    let is_edmx =
        trimmed.starts_with('<') && (trimmed.contains("<edmx:Edmx") || trimmed.contains("<Edmx"));
    (is_edmx && text.contains(SANDBOX_HOST)).then_some(HubFile::Edmx(text))
}

pub fn detect(files: &[PluginFile]) -> Result<bool> {
    Ok(files.len() == 1 && classify(&files[0]).is_some())
}

pub fn import(files: &[PluginFile]) -> Result<ImportReply> {
    let [file] = files else {
        bail!("the SAP Business Accelerator Hub importer takes exactly one file");
    };
    let Some(kind) = classify(file) else {
        bail!(
            "{} is not an API specification of the SAP Business Accelerator Hub (OpenAPI 3 JSON with x-sap-* keys, or EDMX from the sandbox)",
            file.name
        );
    };
    let (mut collection, mut losses) = match kind {
        HubFile::OpenApi(root) => from_openapi(&root, &file.name),
        HubFile::Edmx(xml) => from_edmx(&xml, &file.name)?,
    };
    collection.auth = Auth::Basic {
        username: "{{user}}".to_owned(),
        password: "{{password}}".to_owned(),
    };
    let (files, counts) = collection.write();
    losses.sort_by(|a, b| {
        (a.location != "collection", &a.location, a.kind).cmp(&(
            b.location != "collection",
            &b.location,
            b.kind,
        ))
    });
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

fn from_openapi(root: &Value, file_name: &str) -> (CollectionFiles, Vec<Loss>) {
    let name = root["info"]["title"]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map_or_else(|| stem(file_name).to_owned(), str::to_owned);

    let mut docs = Vec::new();
    let used = ["x-sap-api-type", "x-sap-shortText"];
    if let Some(short) = root["x-sap-shortText"].as_str() {
        docs.push(short.to_owned());
    }
    if let Some(d) = root["info"]["description"].as_str() {
        docs.push(d.to_owned());
    }
    let mut facts = Vec::new();
    if let Some(t) = root["x-sap-api-type"].as_str() {
        facts.push(format!("API type: {t}"));
    }
    if let Some(v) = root["info"]["version"].as_str() {
        facts.push(format!("Version: {v}"));
    }
    if let Some(u) = root["externalDocs"]["url"].as_str() {
        facts.push(format!("Documentation: {u}"));
    }
    if !facts.is_empty() {
        docs.push(facts.join("\n"));
    }

    let converted = openapi::requests(root);
    let mut losses = converted.losses;

    let urls = servers(root);
    let sandbox = urls
        .iter()
        .find(|u| host_of(u) == Some(SANDBOX_HOST))
        .map(|u| u.trim_end_matches('/').to_owned());
    let own = urls
        .iter()
        .find(|u| host_of(u) != Some(SANDBOX_HOST))
        .cloned();
    let service_path = sandbox
        .as_deref()
        .and_then(sap_path)
        .or_else(|| own.as_deref().and_then(path_after_origin))
        .unwrap_or_default();
    if urls.is_empty() {
        losses.push(loss(
            "collection",
            LossKind::Variable,
            LossEffect::Approximated,
            "the specification has no servers: set baseUrl in the environments".to_owned(),
        ));
    }
    for url in urls
        .iter()
        .filter(|u| Some(u.trim_end_matches('/')) != sandbox.as_deref() && Some(*u) != own.as_ref())
    {
        losses.push(loss(
            "collection",
            LossKind::Variable,
            LossEffect::Dropped,
            format!("server {url} not imported as an environment"),
        ));
    }

    let mut collection = CollectionFiles {
        name,
        docs: Some(docs.join("\n\n")).filter(|d| !d.is_empty()),
        folder_docs: converted.folder_docs,
        requests: converted.requests,
        ..CollectionFiles::default()
    };
    environments(
        &mut collection,
        &mut losses,
        sandbox.as_deref(),
        &service_path,
        own.as_deref(),
    );
    security(root, &mut losses);

    let mut extensions: Vec<&str> = [root, &root["info"]]
        .into_iter()
        .filter_map(Value::as_object)
        .flat_map(|m| m.keys())
        .map(String::as_str)
        .filter(|k| k.starts_with("x-sap-") && !used.contains(k))
        .collect();
    extensions.sort_unstable();
    if !extensions.is_empty() {
        losses.push(loss(
            "collection",
            LossKind::Documentation,
            LossEffect::Dropped,
            format!("SAP extensions not imported: {}", extensions.join(", ")),
        ));
    }
    let mut refs = Vec::new();
    foreign_refs(root, &mut refs);
    if !refs.is_empty() {
        losses.push(loss(
            "collection",
            LossKind::Other,
            LossEffect::Dropped,
            format!(
                "references to other files not resolved: {}",
                refs.join(", ")
            ),
        ));
    }
    (collection, losses)
}

fn from_edmx(xml: &str, file_name: &str) -> Result<(CollectionFiles, Vec<Loss>)> {
    let Some(metadata_url) = sandbox_url(xml) else {
        bail!("{file_name}: no {SANDBOX_HOST} URL found");
    };
    let root = metadata_url
        .split('?')
        .next()
        .unwrap_or_default()
        .trim_end_matches("$metadata")
        .trim_end_matches('/')
        .to_owned();
    let model = crate::metadata::parse(xml, &format!("{root}/"))?;
    let mut losses = Vec::new();
    let mut requests = Vec::new();
    for new in crate::metadata::requests_for(&model) {
        let folder: Vec<String> = new
            .folder
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let mut spec = new.spec;
        spec.name = new.name;
        let location = folder
            .iter()
            .cloned()
            .chain([spec.name.clone()])
            .collect::<Vec<_>>()
            .join(" / ");
        for detail in unsupported(&spec) {
            losses.push(loss(
                &location,
                LossKind::Other,
                LossEffect::Dropped,
                detail,
            ));
        }
        requests.push(RequestFile {
            folder,
            spec,
            docs: None,
        });
    }
    if requests.is_empty() {
        losses.push(loss(
            "collection",
            LossKind::Other,
            LossEffect::Dropped,
            "the metadata has no entity sets or operations: no requests".to_owned(),
        ));
    }
    let name = if model.namespace.is_empty() {
        stem(file_name).to_owned()
    } else {
        model.namespace.clone()
    };
    let mut collection = CollectionFiles {
        name,
        docs: Some(format!(
            "OData service from the SAP Business Accelerator Hub ($metadata of {root})."
        )),
        requests,
        ..CollectionFiles::default()
    };
    let service_path = sap_path(&root).unwrap_or_default();
    environments(
        &mut collection,
        &mut losses,
        Some(&root),
        &service_path,
        None,
    );
    Ok((collection, losses))
}

/// Sandbox environment, `APIKey` header and the DEV/QAS/PRD template.
fn environments(
    collection: &mut CollectionFiles,
    losses: &mut Vec<Loss>,
    sandbox: Option<&str>,
    service_path: &str,
    own: Option<&str>,
) {
    let template = Landscape {
        service_path: service_path.to_owned(),
        ..Landscape::default()
    };
    collection.environments = template.environments(true);
    if let Some(url) = sandbox {
        collection.environments.push(sandbox_environment(url));
        collection
            .headers
            .push(KeyValue::new(API_KEY_HEADER, "{{apiKey}}"));
    }
    let from = match own {
        Some(url) => format!("server {url}"),
        None => "the service path".to_owned(),
    };
    losses.push(loss(
        &format!("environments {}", STAGES.join(", ")),
        LossKind::Variable,
        LossEffect::Approximated,
        format!(
            "template from {from}: set baseHost (e.g. https://s4d.example.corp:44300), sapClient and user, and the secret password (or use the plugin's SAP systems tab)"
        ),
    ));
}

/// Security schemes other than the sandbox key and Basic are reported.
fn security(root: &Value, losses: &mut Vec<Loss>) {
    for (name, scheme) in root["components"]["securitySchemes"]
        .as_object()
        .into_iter()
        .flatten()
    {
        let kind = scheme["type"].as_str().unwrap_or_default();
        let api_key = kind == "apiKey"
            && scheme["in"] == "header"
            && scheme["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(API_KEY_HEADER));
        let basic = kind == "http"
            && scheme["scheme"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("basic"));
        if !(api_key || basic) {
            losses.push(loss(
                "collection",
                LossKind::Auth,
                LossEffect::Dropped,
                format!(
                    "security scheme `{name}` ({kind}) not imported; the collection uses Basic auth with {{{{user}}}} and {{{{password}}}}"
                ),
            ));
        }
    }
}

fn servers(root: &Value) -> Vec<String> {
    root["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["url"].as_str().map(str::to_owned))
        .collect()
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    authority.rsplit('@').next()?.split(':').next()
}

/// `/sap/opu/…` of a URL, without trailing `/`.
fn sap_path(url: &str) -> Option<String> {
    let start = url.to_ascii_lowercase().find("/sap/opu/")?;
    Some(url[start..].trim_end_matches('/').to_owned())
}

/// The path of `https://{host}:{port}/path`, without trailing `/`.
fn path_after_origin(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let start = rest.find('/')?;
    let path = rest[start..].trim_end_matches('/');
    (!path.is_empty()).then(|| path.to_owned())
}

/// The first `https://sandbox.api.sap.com/…` URL in the text, up to a quote or `<`.
fn sandbox_url(text: &str) -> Option<String> {
    let start = text.find(&format!("https://{SANDBOX_HOST}/"))?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| c == '"' || c == '\'' || c == '<' || c.is_whitespace())
        .unwrap_or(rest.len());
    Some(rest[..end].to_owned())
}

fn stem(name: &str) -> &str {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    base.rsplit_once('.').map_or(base, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            host_of("https://sandbox.api.sap.com/s4hanacloud/x"),
            Some(SANDBOX_HOST)
        );
        assert_eq!(
            sap_path("https://sandbox.api.sap.com/s4hanacloud/sap/opu/odata/sap/API_X/").as_deref(),
            Some("/sap/opu/odata/sap/API_X")
        );
        assert_eq!(
            path_after_origin("https://{host}:{port}/api/v1/").as_deref(),
            Some("/api/v1")
        );
        assert_eq!(
            sandbox_url("<a href=\"https://sandbox.api.sap.com/s/$metadata\"/>").as_deref(),
            Some("https://sandbox.api.sap.com/s/$metadata")
        );
    }
}
