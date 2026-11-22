// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer `edmx`: a `$metadata` document (`.edmx` or `.xml` whose root is `edmx:Edmx`) into
//! an OpenCollection collection: the requests of [`requests_for`] (a folder per entity set),
//! an environment `Default` with `baseUrl` (the service root of the document's
//! `atom:link rel="self"` if it has one, else [`FALLBACK_BASE_URL`]), and a loss report.
//!
//! [`collection`] builds the files from a model and is shared with the `sap-hub` importer.

use rt_plugin_sdk::api::{ImportReply, NewRequest, PluginEnvironment, PluginFile};
use rt_plugin_sdk::model::{Body, RawKind, RequestSpec};
use rt_plugin_sdk::{Error, ImportFormat, Loss, LossEffect, LossKind, LossReport, Result};

use crate::edm::Model;

use super::edmx::{self, Parsed};
use super::{cache, requests_for};

/// `baseUrl` when the document does not say where the service lives.
pub const FALLBACK_BASE_URL: &str = "https://example.com/odata/";

const EXTENSIONS: [&str; 2] = ["edmx", "xml"];

/// A file with an `.edmx`/`.xml` name whose root element is `Edmx`.
fn is_edmx(file: &PluginFile) -> Option<String> {
    let ext_ok = file
        .name
        .rsplit_once('.')
        .is_some_and(|(_, e)| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    if !ext_ok {
        return None;
    }
    let text = rt_plugin_sdk::file_text(file).ok()?;
    if !text.contains("Edmx") {
        return None;
    }
    let root = super::xml::parse(&text).ok()?;
    (root.name == "Edmx").then_some(text)
}

pub fn detect(files: &[PluginFile]) -> Result<bool> {
    Ok(files.iter().any(|f| is_edmx(f).is_some()))
}

pub fn import(files: &[PluginFile]) -> Result<ImportReply> {
    let Some((index, text)) = files
        .iter()
        .enumerate()
        .find_map(|(i, f)| is_edmx(f).map(|t| (i, t)))
    else {
        return Err(Error::new("no EDMX ($metadata) file among the files"));
    };
    let file = &files[index];
    let probe = edmx::parse(&text, "").map_err(|e| Error::new(format!("{}: {e}", file.name)))?;
    let root = probe
        .self_root
        .clone()
        .unwrap_or_else(|| FALLBACK_BASE_URL.to_owned());
    let mut parsed = probe;
    parsed.model.service_root = root.clone();

    // With a real service root the cache serves the query builder right away. Best effort:
    // an importer without storage (or over the limit) still imports.
    if parsed.self_root.is_some() {
        let _ = cache::store(&parsed.model);
    }

    let mut losses = losses(&parsed);
    for (i, other) in files.iter().enumerate() {
        if i != index {
            losses.push(Loss {
                location: other.name.clone(),
                kind: LossKind::Other,
                effect: LossEffect::Dropped,
                detail: "only one EDMX document is imported at a time".into(),
            });
        }
    }
    let name = if parsed.model.namespace.is_empty() {
        stem(&file.name).to_owned()
    } else {
        parsed.model.namespace.clone()
    };
    let environments = vec![PluginEnvironment {
        name: super::browser::DEFAULT_ENVIRONMENT.into(),
        variables: vec![rt_plugin_sdk::api::PluginVariable {
            name: "baseUrl".into(),
            value: root,
            secret: false,
        }],
    }];
    Ok(collection(&parsed.model, &name, &environments, losses))
}

/// What a model cannot carry into requests.
fn losses(parsed: &Parsed) -> Vec<Loss> {
    let mut out = Vec::new();
    let mut loss = |location: String, effect, detail: String| {
        out.push(Loss {
            location,
            kind: LossKind::Other,
            effect,
            detail,
        })
    };
    for s in &parsed.singletons {
        loss(
            format!("singleton {s}"),
            LossEffect::Dropped,
            "singletons get no requests".into(),
        );
    }
    for o in &parsed.without_import {
        loss(
            format!("operation {o}"),
            LossEffect::Dropped,
            "unbound operation without an action or function import is not callable".into(),
        );
    }
    for (o, method) in &parsed.other_methods {
        loss(
            format!("operation {o}"),
            LossEffect::Approximated,
            format!("HTTP method {method} written as POST"),
        );
    }
    let model = &parsed.model;
    for op in &model.operations {
        if let Some(bound) = &op.bound_to {
            let ty = bound
                .strip_prefix("Collection(")
                .and_then(|r| r.strip_suffix(')'))
                .unwrap_or(bound);
            if !model.entity_sets.iter().any(|s| s.entity_type == ty) {
                loss(
                    format!("operation {}", op.name),
                    LossEffect::Dropped,
                    format!("bound to {ty}, which no entity set exposes"),
                );
            }
        }
    }
    for set in &model.entity_sets {
        if model.entity_type(&set.entity_type).is_some_and(|t| t.media) {
            loss(
                set.name.clone(),
                LossEffect::Approximated,
                "media entities: no request for the media resource ($value)".into(),
            );
        }
    }
    out
}

/// An OpenCollection collection from a model: `opencollection.yml`, a `folder.yml` per folder,
/// a request file per request of [`requests_for`], `environments/<name>.yml` per environment
/// (secret variables without value). `losses` go into the report as they are, sorted.
pub fn collection(
    model: &Model,
    name: &str,
    environments: &[PluginEnvironment],
    mut losses: Vec<Loss>,
) -> ImportReply {
    let requests = requests_for(model);
    let mut files = vec![(
        "opencollection.yml".to_owned(),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: {}\n", quote(name)),
    )];
    let mut folders: Vec<String> = Vec::new();
    for r in &requests {
        if !r.folder.is_empty() && !folders.contains(&r.folder) {
            folders.push(r.folder.clone());
        }
    }
    let folder_dirs: Vec<String> = {
        let mut taken: Vec<String> = Vec::new();
        folders
            .iter()
            .map(|f| {
                let dir = unique(&file_stem(f), &taken);
                taken.push(dir.clone());
                dir
            })
            .collect()
    };
    for (i, (folder, dir)) in folders.iter().zip(&folder_dirs).enumerate() {
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
    let root_offset = folders.len();
    for r in &requests {
        let dir = folders
            .iter()
            .position(|f| *f == r.folder)
            .map(|i| format!("{}/", folder_dirs[i]))
            .unwrap_or_default();
        let stem = unique(&format!("{dir}{}", slug(&r.name)), &taken);
        taken.push(stem.clone());
        files.push((format!("{stem}.yml"), request_file(r, root_offset)));
    }
    for env in environments {
        files.push((
            format!("environments/{}.yml", file_stem(&env.name)),
            environment_file(env),
        ));
    }
    losses.sort_by(|a, b| (&a.location, a.kind).cmp(&(&b.location, b.kind)));
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

/// `stem`, or `stem-2`, `stem-3`, ... if taken (ignoring case).
fn unique(stem: &str, taken: &[String]) -> String {
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

/// A request file. Requests in the collection root come after the folders (`seq` offset).
fn request_file(r: &NewRequest, root_offset: usize) -> String {
    let spec: &RequestSpec = &r.spec;
    let seq = spec.seq.unwrap_or(1.0) as usize + if r.folder.is_empty() { root_offset } else { 0 };
    let mut out = format!(
        "info:\n  name: {}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {}\n  url: {}\n",
        quote(&r.name),
        spec.method,
        quote(&spec.url)
    );
    if !spec.params.is_empty() {
        out.push_str("  params:\n");
        for p in &spec.params {
            out.push_str(&format!(
                "    - name: {}\n      value: {}\n      type: query\n",
                quote(&p.name),
                quote(&p.value)
            ));
        }
    }
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
    if let Body::Raw {
        kind: RawKind::Json,
        data,
    } = &spec.body
    {
        out.push_str("  body:\n    type: json\n    data: |-\n");
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

fn environment_file(env: &PluginEnvironment) -> String {
    let mut out = format!("name: {}\n", quote(&env.name));
    if env.variables.is_empty() {
        out.push_str("variables: []\n");
        return out;
    }
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

/// A YAML double-quoted scalar.
fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A file name stem: lowercase letters, digits, `_` and `-` (as `rt-format` names new
/// requests).
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

/// A folder or environment file name: the name with characters no file system allows
/// replaced by `-`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(slug("List SalesOrderSet"), "list-salesorderset");
        assert_eq!(slug("$metadata"), "metadata");
        assert_eq!(slug("folder"), "folder-1");
        assert_eq!(file_stem("a/b"), "a-b");
        assert_eq!(file_stem("environments"), "_environments");
        assert_eq!(unique("a", &["A".into(), "a-2".into()]), "a-3");
        assert_eq!(stem("dir/x.edmx"), "x");
        assert_eq!(quote("a\"\\\n"), "\"a\\\"\\\\\\x0A\"");
    }
}
