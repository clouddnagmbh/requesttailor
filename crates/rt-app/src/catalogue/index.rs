// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reading a catalogue index and finding where a source lives. Pure: no network.

use super::{
    CatalogueEntry, CatalogueIndex, INDEX_VERSION, MAX_ENTRIES, MAX_INDEX_BYTES, MAX_WASM_BYTES,
};
use crate::AppError;

fn err(message: impl Into<String>) -> AppError {
    AppError::Plugin(message.into())
}

/// Reads `plugins.json`: at most [`MAX_INDEX_BYTES`] and [`MAX_ENTRIES`], version 1, plain ids,
/// a 64-digit hex SHA-256 per module (stored in lower case), unique ids.
pub fn parse_index(bytes: &[u8]) -> Result<CatalogueIndex, AppError> {
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(err(format!(
            "the index is larger than {} MiB",
            MAX_INDEX_BYTES / (1024 * 1024)
        )));
    }
    let mut index: CatalogueIndex = serde_json::from_slice(bytes)
        .map_err(|e| err(format!("this is not a plugin index (plugins.json): {e}")))?;
    if index.version != INDEX_VERSION {
        return Err(err(format!(
            "the index has version {}; this app reads version {INDEX_VERSION}",
            index.version
        )));
    }
    if index.plugins.len() > MAX_ENTRIES {
        return Err(err(format!(
            "the index lists {} plugins; at most {MAX_ENTRIES} are read",
            index.plugins.len()
        )));
    }
    let mut seen = std::collections::HashSet::new();
    for entry in &mut index.plugins {
        let id = &entry.id;
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || id.starts_with('-')
        {
            return Err(err(format!(
                "the index names a plugin {id:?}: not a plugin id"
            )));
        }
        if !seen.insert(id.clone()) {
            return Err(err(format!("the index lists {id} twice")));
        }
        let sha = entry.wasm.sha256.trim().to_ascii_lowercase();
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err(format!(
                "{id}: the checksum is not a SHA-256 (64 hexadecimal digits)"
            )));
        }
        entry.wasm.sha256 = sha;
        if entry.wasm.size > MAX_WASM_BYTES as u64 {
            return Err(err(format!(
                "{id}: the module is larger than {} MiB",
                MAX_WASM_BYTES / (1024 * 1024)
            )));
        }
        if entry.wasm.url.trim().is_empty() {
            return Err(err(format!("{id}: no address for the module")));
        }
    }
    Ok(index)
}

/// Whether `entry` matches the search text (every word in the id, name or description).
pub fn matches(entry: &CatalogueEntry, text: &str) -> bool {
    let hay = format!("{} {} {}", entry.id, entry.name, entry.description).to_lowercase();
    text.to_lowercase()
        .split_whitespace()
        .all(|word| hay.contains(word))
}

/// A Git repository that has `plugins.json` in its root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSource {
    /// The address to clone (no fragment).
    pub url: String,
    /// Branch, tag or commit from the `#ref` of the source.
    pub git_ref: Option<String>,
}

/// Where a source is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// Addresses of the index file, tried in order (the host's raw-file address).
    pub index_urls: Vec<String>,
    /// For a Git source: the repository, for a clone when no raw address answers (native only).
    pub git: Option<GitSource>,
    /// The commit the `#ref` names, when it is one (a hex id): "index at 3fa1c9e".
    pub commit: Option<String>,
}

/// Whether `url` may be requested: `https`, or `http` for a loopback address.
pub fn allowed_url(url: &url::Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => url.host().is_some(),
        "http" => match url.host() {
            Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(a)) => a.is_loopback(),
            Some(url::Host::Ipv6(a)) => a.is_loopback(),
            None => false,
        },
        _ => false,
    }
}

/// Checks what the user typed and says where to read it. No network.
pub fn locate(source: &str) -> Result<Located, AppError> {
    let source = source.trim();
    let url = url::Url::parse(source)
        .map_err(|_| err(format!("{source:?} is not a web address (use https://…)")))?;
    if !allowed_url(&url) {
        return Err(err(
            "a source is an https:// address without a password (http only for localhost)",
        ));
    }
    let git_ref = url.fragment().map(str::to_owned).filter(|r| !r.is_empty());
    let mut bare = url.clone();
    bare.set_fragment(None);
    if git_ref.is_none() && bare.path().to_ascii_lowercase().ends_with(".json") {
        return Ok(Located {
            index_urls: vec![bare.to_string()],
            git: None,
            commit: None,
        });
    }
    if let Some(r) = &git_ref
        && (r.contains(char::is_whitespace) || r.contains("..") || r.starts_with('-'))
    {
        return Err(err(format!("{r:?} is not a branch, tag or commit")));
    }
    let path = bare
        .path()
        .trim_matches('/')
        .trim_end_matches(".git")
        .to_owned();
    if path.split('/').filter(|p| !p.is_empty()).count() < 2 {
        return Err(err(
            "a Git source looks like https://host/org/repo (or a link to plugins.json)",
        ));
    }
    let host = bare.host_str().unwrap_or_default().to_ascii_lowercase();
    let origin = {
        let mut o = bare.clone();
        o.set_path("");
        o.set_query(None);
        o.to_string().trim_end_matches('/').to_owned()
    };
    let at = git_ref.clone();
    let mut index_urls = Vec::new();
    if host == "github.com" || host == "www.github.com" {
        index_urls.push(format!(
            "https://raw.githubusercontent.com/{path}/{}/plugins.json",
            at.as_deref().unwrap_or("HEAD")
        ));
    } else if host.contains("gitlab") {
        index_urls.push(format!(
            "{origin}/{path}/-/raw/{}/plugins.json",
            at.as_deref().unwrap_or("HEAD")
        ));
    } else {
        // Gitea and Forgejo; a commit id has its own route.
        match at.as_deref() {
            Some(r) if is_commit(r) => {
                index_urls.push(format!("{origin}/{path}/raw/commit/{r}/plugins.json"));
            }
            Some(r) => {
                index_urls.push(format!("{origin}/{path}/raw/branch/{r}/plugins.json"));
                index_urls.push(format!("{origin}/{path}/raw/tag/{r}/plugins.json"));
            }
            None => {
                index_urls.push(format!("{origin}/{path}/raw/branch/main/plugins.json"));
                index_urls.push(format!("{origin}/{path}/raw/branch/master/plugins.json"));
            }
        }
        index_urls.push(format!(
            "{origin}/{path}/-/raw/{}/plugins.json",
            at.as_deref().unwrap_or("HEAD")
        ));
    }
    let commit = at.as_deref().filter(|r| is_commit(r)).map(short);
    Ok(Located {
        index_urls,
        git: Some(GitSource {
            url: bare.to_string(),
            git_ref,
        }),
        commit,
    })
}

fn is_commit(r: &str) -> bool {
    (7..=40).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The short form of a commit id, as shown ("3fa1c9e").
pub fn short(commit: &str) -> String {
    commit.chars().take(7).collect()
}

/// `entry.wasm.url` against the address the index was read from (`index_url`).
pub fn resolve_wasm_url(index_url: &str, entry: &CatalogueEntry) -> Result<String, AppError> {
    let base = url::Url::parse(index_url)
        .map_err(|_| err(format!("{index_url:?} is not a web address")))?;
    let url = base.join(entry.wasm.url.trim()).map_err(|_| {
        err(format!(
            "{}: {:?} is not an address",
            entry.id, entry.wasm.url
        ))
    })?;
    if !allowed_url(&url) {
        return Err(err(format!(
            "{}: the module must come from an https:// address ({url})",
            entry.id
        )));
    }
    Ok(url.to_string())
}
