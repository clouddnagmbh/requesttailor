// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A small OpenCollection 1.0 writer for the `sap-hub` importer: a new collection
//! (`opencollection.yml`, `folder.yml` per folder, one file per request,
//! `environments/<name>.yml`) as text, in the layout the built-in importers write.
//!
//! The plugin API has no writer (an importer returns files as text, `API-WUENSCHE.md`), so
//! this covers exactly what the importer produces: method, URL, headers, params, raw and
//! form bodies, the auth types `inherit`, `none`, `basic`, `bearer` and `apikey`, docs.
//! What a [`RequestSpec`] holds beyond that is reported by [`unsupported`] and left out.
//! The tests read every result back with `rt-format`.

use std::collections::BTreeSet;

use rt_plugin_sdk::api::PluginEnvironment;
use rt_plugin_sdk::model::{ApiKeyPlacement, Auth, Body, KeyValue, Param, ParamKind, RawKind};
use rt_plugin_sdk::model::{RequestSpec, Settings};

/// One request of the new collection.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestFile {
    /// Folder names from the root, e.g. `["Product"]`; empty for the root.
    pub folder: Vec<String>,
    pub spec: RequestSpec,
    pub docs: Option<String>,
}

/// A folder's documentation, by folder names from the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderDoc {
    pub folder: Vec<String>,
    pub docs: String,
}

/// The collection as the importer built it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CollectionFiles {
    pub name: String,
    pub docs: Option<String>,
    /// Collection-wide headers (`request.headers` of `opencollection.yml`).
    pub headers: Vec<KeyValue>,
    /// Collection auth; `Auth::Inherit` or `Auth::None` write none.
    pub auth: Auth,
    pub folder_docs: Vec<FolderDoc>,
    pub requests: Vec<RequestFile>,
    pub environments: Vec<PluginEnvironment>,
}

/// What was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub requests: u32,
    pub folders: u32,
    pub environments: u32,
}

impl CollectionFiles {
    /// The files (path → text) and what they contain. Requests keep their order; folders
    /// appear in the order of their first request and get `seq` 1, 2, … among their
    /// siblings before the requests of the same parent.
    pub fn write(&self) -> (Vec<(String, String)>, Counts) {
        let mut out = vec![("opencollection.yml".to_owned(), self.collection_file())];
        let mut counts = Counts::default();
        let mut taken: BTreeSet<(String, String)> = BTreeSet::new();
        let mut folder_seq: Vec<(String, u32)> = Vec::new();

        // Folders first (so requests can follow their sibling folders in `seq`), each
        // directory unique per parent ignoring case.
        let mut dirs: Vec<(&[String], String)> = Vec::new();
        for request in &self.requests {
            for depth in 1..=request.folder.len() {
                let path = &request.folder[..depth];
                if dirs.iter().any(|(p, _)| *p == path) {
                    continue;
                }
                let parent = dirs
                    .iter()
                    .find(|(p, _)| *p == &path[..depth - 1])
                    .map_or(String::new(), |(_, d)| d.clone());
                let name = &path[depth - 1];
                let dir = join(&parent, &unique(&parent, &slug(name, "folder"), &mut taken));
                let seq = next(&mut folder_seq, &parent);
                let docs = self
                    .folder_docs
                    .iter()
                    .find(|d| d.folder == path)
                    .map(|d| d.docs.as_str());
                out.push((format!("{dir}/folder.yml"), folder_file(name, seq, docs)));
                counts.folders += 1;
                dirs.push((path, dir));
            }
        }

        let mut request_seq = folder_seq.clone();
        for request in &self.requests {
            let parent = dirs
                .iter()
                .find(|(p, _)| *p == request.folder.as_slice())
                .map_or(String::new(), |(_, d)| d.clone());
            let file = unique(&parent, &slug(&request.spec.name, "request"), &mut taken);
            let seq = next(&mut request_seq, &parent);
            out.push((
                format!("{}.yml", join(&parent, &file)),
                request_file(&request.spec, seq, request.docs.as_deref()),
            ));
            counts.requests += 1;
        }

        let mut env_names: BTreeSet<String> = BTreeSet::new();
        for env in &self.environments {
            let mut file = file_name(&env.name);
            while !env_names.insert(file.to_ascii_lowercase()) {
                file.push('_');
            }
            out.push((format!("environments/{file}.yml"), environment_file(env)));
            counts.environments += 1;
        }
        (out, counts)
    }

    fn collection_file(&self) -> String {
        let mut s = String::from("opencollection: \"1.0.0\"\n\ninfo:\n");
        s.push_str(&format!("  name: {}\n", scalar(&self.name)));
        let auth = !matches!(self.auth, Auth::Inherit | Auth::None);
        if !self.headers.is_empty() || auth {
            s.push_str("\nrequest:\n");
            if !self.headers.is_empty() {
                s.push_str("  headers:\n");
                key_values(&mut s, &self.headers, "    ");
            }
            if auth {
                auth_block(&mut s, &self.auth, "  ");
            }
        }
        if let Some(docs) = self.docs.as_deref().filter(|d| !d.is_empty()) {
            s.push_str(&format!("\ndocs: {}\n", block(docs, "  ")));
        }
        s.push_str("\nbundled: false\n");
        s
    }
}

/// What of `spec` this writer cannot write (for the loss report); empty if nothing.
pub fn unsupported(spec: &RequestSpec) -> Vec<String> {
    let mut lost = Vec::new();
    match &spec.body {
        Body::Multipart { .. } => lost.push("multipart body not imported".to_owned()),
        Body::File { .. } => lost.push("file body not imported".to_owned()),
        Body::Raw {
            kind: RawKind::Sparql,
            ..
        } => lost.push("SPARQL body not imported".to_owned()),
        _ => {}
    }
    match &spec.auth {
        Auth::Inherit | Auth::None | Auth::Basic { .. } | Auth::Bearer { .. } => {}
        Auth::ApiKey { .. } => {}
        _ => lost.push("auth not imported (the request inherits the collection's)".to_owned()),
    }
    if !spec.vars.is_empty() {
        lost.push(format!(
            "{} request variable(s) not imported",
            spec.vars.len()
        ));
    }
    if spec.settings != Settings::default() {
        lost.push("request settings not imported".to_owned());
    }
    lost
}

fn next(counters: &mut Vec<(String, u32)>, parent: &str) -> u32 {
    match counters.iter_mut().find(|(p, _)| p == parent) {
        Some((_, n)) => {
            *n += 1;
            *n
        }
        None => {
            counters.push((parent.to_owned(), 1));
            1
        }
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

fn unique(parent: &str, base: &str, taken: &mut BTreeSet<(String, String)>) -> String {
    let mut name = base.to_owned();
    let mut n = 2;
    while !taken.insert((parent.to_owned(), name.to_ascii_lowercase())) {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

/// A file name: lower case, `-` between words, at most 64 characters, `fallback` if empty.
pub fn slug(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(64).collect();
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out.to_owned()
    }
}

/// An environment file name: the name with characters not allowed in file names as `-`.
fn file_name(name: &str) -> String {
    let out: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let out = out.trim_matches([' ', '.']);
    if out.is_empty() {
        "environment".to_owned()
    } else {
        out.to_owned()
    }
}

fn folder_file(name: &str, seq: u32, docs: Option<&str>) -> String {
    let mut s = format!(
        "info:\n  name: {}\n  type: folder\n  seq: {seq}\n",
        scalar(name)
    );
    if let Some(docs) = docs.filter(|d| !d.is_empty()) {
        s.push_str(&format!("\ndocs: {}\n", block(docs, "  ")));
    }
    s
}

fn request_file(spec: &RequestSpec, seq: u32, docs: Option<&str>) -> String {
    let mut s = format!(
        "info:\n  name: {}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {}\n  url: {}\n",
        scalar(&spec.name),
        scalar(&spec.method),
        scalar(&spec.url)
    );
    if !spec.headers.is_empty() {
        s.push_str("  headers:\n");
        key_values(&mut s, &spec.headers, "    ");
    }
    if !spec.params.is_empty() {
        s.push_str("  params:\n");
        params(&mut s, &spec.params);
    }
    match &spec.body {
        Body::Raw { kind, data } if *kind != RawKind::Sparql => {
            let kind = match kind {
                RawKind::Json => "json",
                RawKind::Xml => "xml",
                _ => "text",
            };
            s.push_str(&format!(
                "  body:\n    type: {kind}\n    data: {}\n",
                block(data, "      ")
            ));
        }
        Body::FormUrlEncoded { fields } => {
            s.push_str("  body:\n    type: form-urlencoded\n    data:\n");
            key_values(&mut s, fields, "      ");
        }
        _ => {}
    }
    match &spec.auth {
        Auth::None => s.push_str("  auth: none\n"),
        a @ (Auth::Basic { .. } | Auth::Bearer { .. } | Auth::ApiKey { .. }) => {
            auth_block(&mut s, a, "  ")
        }
        _ => s.push_str("  auth: inherit\n"),
    }
    if let Some(docs) = docs.filter(|d| !d.is_empty()) {
        s.push_str(&format!("\ndocs: {}\n", block(docs, "  ")));
    }
    s
}

fn environment_file(env: &PluginEnvironment) -> String {
    let mut s = format!("name: {}\n", scalar(&env.name));
    if !env.variables.is_empty() {
        s.push_str("variables:\n");
        for v in &env.variables {
            if v.secret {
                // Secret values never go into files (product principle 3).
                s.push_str(&format!(
                    "  - secret: true\n    name: {}\n",
                    scalar(&v.name)
                ));
            } else {
                s.push_str(&format!(
                    "  - name: {}\n    value: {}\n",
                    scalar(&v.name),
                    scalar(&v.value)
                ));
            }
        }
    }
    s
}

fn key_values(s: &mut String, list: &[KeyValue], indent: &str) {
    for kv in list {
        s.push_str(&format!(
            "{indent}- name: {}\n{indent}  value: {}\n",
            scalar(&kv.name),
            scalar(&kv.value)
        ));
        if !kv.enabled {
            s.push_str(&format!("{indent}  disabled: true\n"));
        }
    }
}

fn params(s: &mut String, list: &[Param]) {
    for p in list {
        let kind = match p.kind {
            ParamKind::Query => "query",
            ParamKind::Path => "path",
        };
        s.push_str(&format!(
            "    - name: {}\n      value: {}\n      type: {kind}\n",
            scalar(&p.name),
            scalar(&p.value)
        ));
        if !p.enabled {
            s.push_str("      disabled: true\n");
        }
    }
}

fn auth_block(s: &mut String, auth: &Auth, indent: &str) {
    match auth {
        Auth::Basic { username, password } => s.push_str(&format!(
            "{indent}auth:\n{indent}  type: basic\n{indent}  username: {}\n{indent}  password: {}\n",
            scalar(username),
            scalar(password)
        )),
        Auth::Bearer { token } => s.push_str(&format!(
            "{indent}auth:\n{indent}  type: bearer\n{indent}  token: {}\n",
            scalar(token)
        )),
        Auth::ApiKey {
            key,
            value,
            placement,
        } => s.push_str(&format!(
            "{indent}auth:\n{indent}  type: apikey\n{indent}  key: {}\n{indent}  value: {}\n{indent}  placement: {}\n",
            scalar(key),
            scalar(value),
            match placement {
                ApiKeyPlacement::Header => "header",
                ApiKeyPlacement::Query => "query",
            }
        )),
        Auth::None => s.push_str(&format!("{indent}auth: none\n")),
        _ => s.push_str(&format!("{indent}auth: inherit\n")),
    }
}

/// A YAML scalar: plain when that is unambiguous, else double-quoted (JSON escapes, which
/// YAML's double-quoted style accepts).
pub fn scalar(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " _-./()$'".contains(c))
        && text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '$')
        && !text.ends_with(' ')
        && !text.contains(" #")
        && !matches!(
            text.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "y" | "n"
        );
    if plain {
        text.to_owned()
    } else {
        serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
    }
}

/// Multi-line text as a `|-` block scalar indented by `indent`, else [`scalar`].
fn block(text: &str, indent: &str) -> String {
    let blockable = text.contains('\n')
        && !text.contains('\r')
        && !text.ends_with(['\n', ' ', '\t'])
        && !text.starts_with([' ', '\t', '\n'])
        && text.chars().all(|c| c == '\n' || !c.is_control())
        && text.lines().all(|l| !l.ends_with([' ', '\t']));
    if !blockable {
        return scalar(text);
    }
    let mut out = String::from("|-");
    for line in text.split('\n') {
        out.push('\n');
        if !line.is_empty() {
            out.push_str(indent);
            out.push_str(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars() {
        assert_eq!(scalar("Get A_Product"), "Get A_Product");
        assert_eq!(scalar("{{baseUrl}}/x"), "\"{{baseUrl}}/x\"");
        assert_eq!(scalar("true"), "\"true\"");
        assert_eq!(scalar(""), "\"\"");
        assert_eq!(scalar("100"), "\"100\"");
        assert_eq!(scalar("a: b"), "\"a: b\"");
        assert_eq!(block("a\n\nb", "  "), "|-\n  a\n\n  b");
        assert_eq!(block("one line", "  "), "one line");
    }

    #[test]
    fn slugs_and_file_names() {
        assert_eq!(
            slug("Get entities from A_Product", "x"),
            "get-entities-from-a-product"
        );
        assert_eq!(slug("???", "request"), "request");
        assert_eq!(file_name("S4H PRD"), "S4H PRD");
        assert_eq!(file_name("a/b"), "a-b");
    }
}
