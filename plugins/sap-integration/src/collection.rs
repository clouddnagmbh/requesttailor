// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer `service-key`: one or two BTP service keys (API plan, runtime plan) into a
//! collection with an environment, OAuth client credentials per folder and the requests of
//! [`crate::requests`]. The client secrets are never read into the output: the environment
//! declares them as secret variables without a value, and the report says where to set them.

use rt_plugin_sdk::api::{ImportReply, PluginEnvironment, PluginFile, PluginVariable};
use rt_plugin_sdk::model::{Body, RequestSpec};
use rt_plugin_sdk::{Error, ImportFormat, Loss, LossEffect, LossKind, LossReport, Result};

use crate::requests::{self, FLOW_FOLDER, LOG_FOLDER};
use crate::servicekey::{self, Key, Side};
use crate::yaml::{file_stem, quote, slug};

fn keys(files: &[PluginFile]) -> Vec<Key> {
    files
        .iter()
        .filter(|f| f.name.to_ascii_lowercase().ends_with(".json"))
        .filter_map(|f| {
            let text = rt_plugin_sdk::file_text(f).ok()?;
            servicekey::parse(&f.name, &text)
        })
        .collect()
}

pub fn detect(files: &[PluginFile]) -> Result<bool> {
    Ok(!keys(files).is_empty())
}

pub fn import(files: &[PluginFile]) -> Result<ImportReply> {
    let found = keys(files);
    if found.is_empty() {
        return Err(Error::new(
            "no service key among the files (expected JSON with clientid, url and a token URL)",
        ));
    }
    let mut losses: Vec<Loss> = Vec::new();
    let mut api: Option<&Key> = None;
    let mut runtime: Option<&Key> = None;
    for key in &found {
        let slot = match key.side() {
            Side::Api => &mut api,
            Side::Runtime => &mut runtime,
        };
        if slot.is_some() {
            losses.push(loss(
                &key.file,
                LossKind::Setting,
                LossEffect::Dropped,
                "a second key for the same side; the first one was used",
            ));
        } else {
            *slot = Some(key);
        }
        if !key.has_secret {
            losses.push(loss(
                &key.file,
                LossKind::Variable,
                LossEffect::Approximated,
                "the key has no client secret; set the secret variable yourself",
            ));
        }
    }
    let tenant = api
        .or(runtime)
        .map(Key::tenant)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "tenant".into());
    let mut vars: Vec<PluginVariable> = Vec::new();
    let plain = |name: &str, value: &str| PluginVariable {
        name: name.into(),
        value: value.into(),
        secret: false,
    };
    let secret = |name: &str| PluginVariable {
        name: name.into(),
        value: String::new(),
        secret: true,
    };
    match api {
        Some(k) => {
            vars.push(plain("apiUrl", &k.url));
            vars.push(plain("tokenUrl", &k.token_url));
            vars.push(plain("clientId", &k.client_id));
        }
        None => {
            let guess = runtime.and_then(|k| k.other_url()).unwrap_or_default();
            losses.push(loss(
                "environment",
                LossKind::Variable,
                if guess.is_empty() { LossEffect::Dropped } else { LossEffect::Approximated },
                if guess.is_empty() {
                    "no API key among the files: apiUrl, tokenUrl and clientId are empty"
                } else {
                    "no API key among the files: apiUrl is derived from the runtime host (check it); tokenUrl and clientId are empty"
                },
            ));
            vars.push(plain("apiUrl", &guess));
            vars.push(plain("tokenUrl", ""));
            vars.push(plain("clientId", ""));
        }
    }
    vars.push(secret("clientSecret"));
    match runtime {
        Some(k) => {
            vars.push(plain("runtimeUrl", &k.url));
            vars.push(plain("runtimeTokenUrl", &k.token_url));
            vars.push(plain("runtimeClientId", &k.client_id));
        }
        None => {
            let guess = api.and_then(|k| k.other_url()).unwrap_or_default();
            losses.push(loss(
                "environment",
                LossKind::Variable,
                if guess.is_empty() { LossEffect::Dropped } else { LossEffect::Approximated },
                if guess.is_empty() {
                    "no runtime key among the files: runtimeUrl, runtimeTokenUrl and runtimeClientId are empty"
                } else {
                    "no runtime key among the files: runtimeUrl is derived from the API host (check it); runtimeTokenUrl and runtimeClientId are empty"
                },
            ));
            vars.push(plain("runtimeUrl", &guess));
            vars.push(plain("runtimeTokenUrl", ""));
            vars.push(plain("runtimeClientId", ""));
        }
    }
    vars.push(secret("runtimeClientSecret"));
    vars.push(plain("iflowPath", "your-endpoint"));
    vars.push(plain("iflowName", "Your_iFlow"));
    losses.push(loss(
        "environment",
        LossKind::Variable,
        LossEffect::Dropped,
        "client secrets are not imported: set clientSecret and runtimeClientSecret in the environment (they go to the key store, never into files)",
    ));
    losses.push(loss(
        "environment",
        LossKind::Setting,
        LossEffect::Approximated,
        "set iflowPath (the endpoint of your iFlow) and iflowName",
    ));
    let env = PluginEnvironment {
        name: tenant.clone(),
        variables: vars,
    };
    let name = format!("Integration Suite {tenant}");
    Ok(build(&name, env, losses))
}

fn loss(location: &str, kind: LossKind, effect: LossEffect, detail: &str) -> Loss {
    Loss {
        location: location.into(),
        kind,
        effect,
        detail: detail.into(),
    }
}

fn build(name: &str, env: PluginEnvironment, mut losses: Vec<Loss>) -> ImportReply {
    let mut files = vec![(
        "opencollection.yml".to_owned(),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: {}\n", quote(name)),
    )];
    files.push((
        format!("{FLOW_FOLDER}/folder.yml"),
        folder_file(
            "iFlow",
            1,
            "runtimeTokenUrl",
            "runtimeClientId",
            "runtimeClientSecret",
        ),
    ));
    files.push((
        format!("{LOG_FOLDER}/folder.yml"),
        folder_file(
            "Message processing log",
            2,
            "tokenUrl",
            "clientId",
            "clientSecret",
        ),
    ));
    let flow = [requests::trigger()];
    let log = [
        requests::log_of_last_trigger("COMPLETED"),
        requests::failed_logs(),
        requests::error_text_of_last_trigger(),
    ];
    for (folder, list) in [(FLOW_FOLDER, &flow[..]), (LOG_FOLDER, &log[..])] {
        for (i, spec) in list.iter().enumerate() {
            files.push((
                format!("{folder}/{}.yml", slug(&spec.name)),
                request_file(spec, i + 1),
            ));
        }
    }
    files.push((
        format!("environments/{}.yml", file_stem(&env.name)),
        environment_file(&env),
    ));
    losses.sort_by(|a, b| (&a.location, a.kind).cmp(&(&b.location, b.kind)));
    losses.dedup();
    ImportReply {
        collection_name: name.to_owned(),
        files,
        report: LossReport {
            format: ImportFormat::Plugin,
            requests: (flow.len() + log.len()) as u32,
            folders: 2,
            environments: 1,
            losses,
        },
    }
}

/// A folder whose requests get a token with OAuth client credentials from the variables.
fn folder_file(name: &str, seq: usize, token_url: &str, id: &str, secret: &str) -> String {
    format!(
        "info:\n  name: {}\n  type: folder\n  seq: {seq}\n\nrequest:\n  auth:\n    type: oauth2\n    flow: client_credentials\n    accessTokenUrl: '{{{{{token_url}}}}}'\n    credentials:\n      clientId: '{{{{{id}}}}}'\n      clientSecret: '{{{{{secret}}}}}'\n      placement: basic_auth_header\n",
        quote(name)
    )
}

fn environment_file(env: &PluginEnvironment) -> String {
    let mut out = format!("name: {}\n\nvariables:\n", quote(&env.name));
    for v in &env.variables {
        out.push_str(&format!("  - name: {}\n", quote(&v.name)));
        if v.secret {
            out.push_str("    secret: true\n");
        } else {
            out.push_str(&format!("    value: {}\n", quote(&v.value)));
        }
    }
    out
}

/// The request file for `spec`: method, URL, headers, a raw body, assertions and actions, auth
/// inherited from the folder.
pub fn request_file(spec: &RequestSpec, seq: usize) -> String {
    let mut out = format!(
        "info:\n  name: {}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {}\n  url: {}\n",
        quote(&spec.name),
        spec.method,
        quote(&spec.url)
    );
    if !spec.headers.is_empty() {
        out.push_str("  headers:\n");
        for h in &spec.headers {
            out.push_str(&format!(
                "    - name: {}\n      value: {}\n",
                quote(&h.name),
                quote(&h.value)
            ));
        }
    }
    if let Body::Raw { kind, data } = &spec.body {
        let kind = match kind {
            rt_plugin_sdk::model::RawKind::Json => "json",
            rt_plugin_sdk::model::RawKind::Xml => "xml",
            _ => "text",
        };
        out.push_str(&format!("  body:\n    type: {kind}\n    data: |-\n"));
        for line in data.lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                out.push_str("      ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out.push_str("  auth: inherit\n");
    if !spec.assertions.is_empty() || !spec.actions.is_empty() {
        out.push_str("\nruntime:\n");
    }
    if !spec.assertions.is_empty() {
        out.push_str("  assertions:\n");
        for a in &spec.assertions {
            out.push_str(&format!(
                "    - expression: {}\n      operator: {}\n",
                quote(&a.expression),
                a.operator
            ));
            if !a.value.is_empty() {
                out.push_str(&format!("      value: {}\n", quote(&a.value)));
            }
            if let Some(d) = &a.description {
                out.push_str(&format!("      description: {}\n", quote(d)));
            }
        }
    }
    if !spec.actions.is_empty() {
        out.push_str("  actions:\n");
        for a in &spec.actions {
            out.push_str(&format!(
                "    - type: set-variable\n      phase: after-response\n      selector:\n        expression: {}\n        method: {}\n      variable:\n        name: {}\n        scope: runtime\n",
                quote(&a.expression),
                a.method,
                quote(&a.variable)
            ));
        }
    }
    out
}
