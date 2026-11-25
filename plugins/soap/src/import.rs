// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer `wsdl`: WSDL 1.1 and 2.0 (with the XSD files it imports) into an OpenCollection
//! collection: a folder per SOAP binding, a request per operation with the envelope (SOAP 1.1
//! or 1.2), `Content-Type` and `SOAPAction`, sample values from the schema, and a loss report.
//! When the WSDL asks for a UsernameToken, the requests carry the WS-Security header with
//! `{{wsse_username}}`/`{{wsse_password}}` placeholders and an environment declares them (the
//! password as a secret without a value).

use rt_plugin_sdk::api::{ImportReply, PluginFile};
use rt_plugin_sdk::model::{Body, KeyValue, RawKind, RequestSpec};
use rt_plugin_sdk::{Error, ImportFormat, Loss, LossEffect, LossKind, LossReport, Result};

use crate::envelope::{self, Message};
use crate::node::{Child, Node};
use crate::schema::Sampler;
use crate::security::{self, Security};
use crate::wsdl::{self, Binding, Effect, Note, Operation, Part, Style};
use crate::xml::{self, Element};

/// URL when the WSDL names no endpoint for a binding.
pub const FALLBACK_BASE_URL: &str = "https://example.com/";
const ENV_NAME: &str = "Default";

struct Doc {
    name: String,
    root: Element,
}

fn read_docs(files: &[PluginFile]) -> Vec<Doc> {
    files
        .iter()
        .filter_map(|f| {
            let text = rt_plugin_sdk::file_text(f).ok()?;
            if !text.contains("schema")
                && !text.contains("definitions")
                && !text.contains("description")
            {
                return None;
            }
            let root = xml::parse(&text).ok()?;
            Some(Doc {
                name: f.name.clone(),
                root,
            })
        })
        .collect()
}

pub fn detect(files: &[PluginFile]) -> Result<bool> {
    Ok(read_docs(files).iter().any(|d| wsdl::is_wsdl(&d.root)))
}

pub fn import(files: &[PluginFile]) -> Result<ImportReply> {
    let docs = read_docs(files);
    let wsdls: Vec<&Doc> = docs.iter().filter(|d| wsdl::is_wsdl(&d.root)).collect();
    if wsdls.is_empty() {
        return Err(Error::new("no WSDL document among the files"));
    }
    let now = rt_plugin_sdk::host::now().unwrap_or(0);
    let mut losses: Vec<Loss> = Vec::new();
    let mut requests: Vec<Req> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let mut needs_security = false;
    let mut needs_base_url = false;
    let mut name = String::new();
    for doc in &wsdls {
        let others: Vec<(String, Element)> = docs
            .iter()
            .filter(|d| !std::ptr::eq(*d, *doc))
            .map(|d| (d.name.clone(), d.root.clone()))
            .collect();
        let parsed = wsdl::parse(&doc.root, &others)
            .map_err(|e| Error::new(format!("{}: {e}", doc.name)))?;
        if name.is_empty() {
            name = if parsed.name.is_empty() {
                stem(&doc.name).to_owned()
            } else {
                parsed.name.clone()
            };
        }
        for n in &parsed.notes {
            losses.push(loss_of(n));
        }
        let use_security = parsed.username_token;
        needs_security |= use_security;
        let timestamp = use_security
            && doc.root.descendants().iter().any(|e| {
                e.name == "IncludeTimestamp" || e.name == "Timestamp" && e.ns == security::WSU_NS
            });
        for binding in &parsed.bindings {
            let folder = unique_name(&binding.name, &folders);
            folders.push(folder.clone());
            if binding.endpoint.is_none() {
                needs_base_url = true;
                losses.push(Loss {
                    location: format!("binding {}", binding.name),
                    kind: LossKind::Setting,
                    effect: LossEffect::Approximated,
                    detail: "no service endpoint for this binding; the URL is {{baseUrl}} from the Default environment".into(),
                });
            }
            for op in &binding.operations {
                let mut sampler = Sampler::new(&parsed.schema);
                let sec = use_security.then(|| Security {
                    timestamp_ttl: timestamp.then_some(security::DEFAULT_TTL),
                    ..Security::default()
                });
                let text = build(binding, op, &mut sampler, sec.as_ref(), now);
                for gap in &sampler.gaps {
                    losses.push(Loss {
                        location: format!("{}/{}", binding.name, op.name),
                        kind: LossKind::Body,
                        effect: LossEffect::Approximated,
                        detail: gap.detail.clone(),
                    });
                }
                if op.one_way {
                    losses.push(Loss {
                        location: format!("{}/{}", binding.name, op.name),
                        kind: LossKind::Protocol,
                        effect: LossEffect::Approximated,
                        detail:
                            "one-way operation: the request is sent, a response is not expected"
                                .into(),
                    });
                }
                requests.push(Req {
                    folder: folder.clone(),
                    spec: spec_of(binding, op, text),
                });
            }
        }
    }
    if requests.is_empty() {
        return Err(Error::new("the WSDL has no SOAP operation to import"));
    }
    let environments = environment(needs_security, needs_base_url);
    Ok(collection(
        &name,
        &folders,
        &requests,
        &environments,
        losses,
    ))
}

struct Req {
    folder: String,
    spec: RequestSpec,
}

/// The envelope text of an operation.
pub fn build(
    binding: &Binding,
    op: &Operation,
    sampler: &mut Sampler,
    security: Option<&Security>,
    now: u64,
) -> String {
    let mut message = Message::default();
    match op.style {
        Style::Document => {
            for part in &op.input {
                message.body.push(part_node(sampler, part));
            }
        }
        Style::Rpc => {
            let mut wrapper = Node::new(&op.rpc_ns, &op.name);
            for part in &op.input {
                wrapper.children.push(Child::Node(part_node(sampler, part)));
            }
            message.body.push(wrapper);
        }
    }
    for part in &op.headers {
        message.header.push(part_node(sampler, part));
    }
    if let Some(sec) = security {
        message
            .raw_header
            .push(security::header(binding.version, "soapenv", sec, now));
    }
    envelope::render(binding.version, &message)
}

fn part_node(sampler: &mut Sampler, part: &Part) -> Node {
    match part {
        Part::Element { ns, name } => sampler.element(ns, name),
        Part::Typed {
            name,
            ns,
            type_name,
        } => sampler.typed(name, ns, type_name),
    }
}

/// The request of an operation: `POST`, the headers of the SOAP version, the envelope.
fn spec_of(binding: &Binding, op: &Operation, text: String) -> RequestSpec {
    let mut headers = vec![KeyValue::new(
        "Content-Type",
        binding.version.content_type(&op.soap_action),
    )];
    if let Some(a) = binding.version.action_header(&op.soap_action) {
        headers.push(KeyValue::new("SOAPAction", a));
    }
    let mut spec = RequestSpec {
        name: op.name.clone(),
        method: "POST".into(),
        url: binding
            .endpoint
            .clone()
            .unwrap_or_else(|| "{{baseUrl}}".into()),
        headers,
        ..RequestSpec::default()
    };
    spec.body = Body::Raw {
        kind: RawKind::Xml,
        data: text,
    };
    spec
}

fn environment(security: bool, base_url: bool) -> Vec<rt_plugin_sdk::api::PluginEnvironment> {
    use rt_plugin_sdk::api::{PluginEnvironment, PluginVariable};
    let mut variables = Vec::new();
    if base_url {
        variables.push(PluginVariable {
            name: "baseUrl".into(),
            value: FALLBACK_BASE_URL.into(),
            secret: false,
        });
    }
    if security {
        variables.push(PluginVariable {
            name: security::USER_VAR.into(),
            value: String::new(),
            secret: false,
        });
        variables.push(PluginVariable {
            name: security::PASSWORD_VAR.into(),
            value: String::new(),
            secret: true,
        });
    }
    if variables.is_empty() {
        Vec::new()
    } else {
        vec![PluginEnvironment {
            name: ENV_NAME.into(),
            variables,
        }]
    }
}

fn loss_of(n: &Note) -> Loss {
    let kind = if n.location.starts_with("WS-Policy") {
        LossKind::Auth
    } else if n.detail.contains("MIME")
        || n.detail.contains("encoded")
        || n.detail.contains("sample")
        || n.detail.contains("body")
    {
        LossKind::Body
    } else if n.detail.contains("binding")
        || n.detail.contains("transport")
        || n.detail.contains("protocol")
        || n.detail.contains("SOAP")
    {
        LossKind::Protocol
    } else {
        LossKind::Other
    };
    Loss {
        location: n.location.clone(),
        kind,
        effect: match n.effect {
            Effect::Dropped => LossEffect::Dropped,
            Effect::Approximated => LossEffect::Approximated,
        },
        detail: n.detail.clone(),
    }
}

// -------------------------------------------------------------------------------------------
// OpenCollection files

fn collection(
    name: &str,
    folders: &[String],
    requests: &[Req],
    environments: &[rt_plugin_sdk::api::PluginEnvironment],
    mut losses: Vec<Loss>,
) -> ImportReply {
    let mut files = vec![(
        "opencollection.yml".to_owned(),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: {}\n", quote(name)),
    )];
    let dirs: Vec<String> = {
        let mut taken: Vec<String> = Vec::new();
        folders
            .iter()
            .map(|f| {
                let dir = unique_name(&file_stem(f), &taken);
                taken.push(dir.clone());
                dir
            })
            .collect()
    };
    for (i, (folder, dir)) in folders.iter().zip(&dirs).enumerate() {
        files.push((
            format!("{dir}/folder.yml"),
            format!(
                "info:\n  name: {}\n  type: folder\n  seq: {}\n",
                quote(folder),
                i + 1
            ),
        ));
    }
    let mut taken: Vec<String> = Vec::new();
    let mut seq_in: Vec<usize> = vec![0; folders.len()];
    for r in requests {
        let fi = folders.iter().position(|f| *f == r.folder).unwrap_or(0);
        seq_in[fi] += 1;
        let stem = unique_name(&format!("{}/{}", dirs[fi], slug(&r.spec.name)), &taken);
        taken.push(stem.clone());
        files.push((format!("{stem}.yml"), request_file(&r.spec, seq_in[fi])));
    }
    for env in environments {
        files.push((
            format!("environments/{}.yml", file_stem(&env.name)),
            environment_file(env),
        ));
    }
    losses.sort_by(|a, b| (&a.location, a.kind).cmp(&(&b.location, b.kind)));
    losses.dedup();
    ImportReply {
        collection_name: name.to_owned(),
        files,
        report: LossReport {
            format: ImportFormat::Plugin,
            requests: requests.len() as u32,
            folders: folders.len() as u32,
            environments: environments.len() as u32,
            losses,
        },
    }
}

/// The request file as `rt-format` writes a new request (checked against `add_request` in the
/// tests).
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
    if let Body::Raw { data, .. } = &spec.body {
        out.push_str("  body:\n    type: xml\n    data: |-\n");
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
    out
}

fn environment_file(env: &rt_plugin_sdk::api::PluginEnvironment) -> String {
    let mut out = format!("name: {}\n", quote(&env.name));
    out.push_str("\nvariables:\n");
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

fn unique_name(stem: &str, taken: &[String]) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                stem.to_owned()
            } else {
                format!("{stem}-{n}")
            }
        })
        .find(|c| !taken.iter().any(|t| t.eq_ignore_ascii_case(c)))
        .expect("some suffix is free")
}

/// A YAML scalar the way `rt-format` writes it: plain when that reads back as the same string,
/// else single-quoted, else double-quoted (control characters).
fn quote(text: &str) -> String {
    if text.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}')
        || text.starts_with(['{', '[', '*', '&', '!', '|', '>', '%', '@', '`'])
    {
        return double_quoted(text);
    }
    if is_plain(text) {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', "''"))
}

fn is_plain(text: &str) -> bool {
    let first = text.chars().next();
    let Some(first) = first else { return false };
    if text.starts_with("- ") || text.starts_with("? ") || text.starts_with(": ") {
        return false;
    }
    if "-?:,[]{}#&*!|>'\"%@`".contains(first)
        && !(first == '-' && text.len() > 1 && !text.starts_with("- "))
    {
        return false;
    }
    if text != text.trim() || text.contains(": ") || text.contains(" #") || text.ends_with(':') {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
    ) {
        return false;
    }
    text.parse::<f64>().is_err()
}

/// A YAML double-quoted scalar.
fn double_quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(60).collect();
    let out = out.trim_end_matches('-');
    match out {
        "" => "request".to_owned(),
        "opencollection" | "folder" => format!("{out}-1"),
        o => o.to_owned(),
    }
}

fn file_stem(name: &str) -> String {
    let out: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .take(60)
        .collect();
    let out = out.trim_end_matches(['.', ' ']);
    if out.is_empty() || out.starts_with('.') || out == "environments" {
        format!("_{out}")
    } else {
        out.to_owned()
    }
}

fn stem(name: &str) -> &str {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    base.rsplit_once('.').map_or(base, |(s, _)| s)
}
