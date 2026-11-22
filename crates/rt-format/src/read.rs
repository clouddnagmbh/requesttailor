// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Parsed OpenCollection files → `rt_core::model`.
//!
//! Mapping decisions (see also the crate docs):
//! - No auth is written as the absence of `auth` (that is what Bruno writes); the strings
//!   `none` and `{type: none}` found in hand-written files mean the same. A request or the
//!   collection without `auth` therefore has [`Auth::None`]; a folder without `auth` has
//!   [`Auth::Inherit`], because a folder cannot say "no auth" differently from "nothing set"
//!   and Bruno's CLI lets such folders pass the parent's auth through.
//! - Only `http` items become requests. GraphQL, gRPC, WebSocket, script and app items stay
//!   in the document (and are written back) but are not part of the model.
//! - Body and variable variants (lists with `selected`) map to the selected entry, or the
//!   first one if none is selected.

use std::collections::BTreeMap;

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Collection, CollectionConfig, Environment, Folder, Item, KeyValue,
    MultipartPart, Param, ParamKind, PartKind, RawKind, RequestDefaults, RequestEntry, RequestSpec,
    Settings, Variable,
};

use crate::yaml::{Document, Node};
use crate::{COLLECTION_FILE, FOLDER_FILE, FormatError};

/// Directory of environment files, relative to the collection root.
pub(crate) const ENVIRONMENTS_DIR: &str = "environments";

/// Item types that are valid OpenCollection items but not HTTP requests.
const OTHER_ITEM_TYPES: &[&str] = &["graphql", "grpc", "websocket", "ws", "script", "app"];

pub(crate) struct Builder<'a> {
    docs: &'a BTreeMap<&'a str, &'a Document>,
    pub issues: Vec<FormatError>,
}

pub(crate) fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".yml").unwrap_or(name)
}

pub(crate) fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

pub(crate) fn text(node: Option<&Node>) -> String {
    node.and_then(Node::text).unwrap_or_default().to_owned()
}

pub(crate) fn flag(node: Option<&Node>) -> Option<bool> {
    node.and_then(Node::as_bool)
}

/// `enabled` from `disabled: true` (the spec) or `enabled: false` (seen in the wild).
pub(crate) fn enabled(node: &Node) -> bool {
    flag(node.get("disabled")) != Some(true) && flag(node.get("enabled")) != Some(false)
}

/// The selected entry of a variant list, or the first one.
pub(crate) fn selected(list: &Node) -> Option<&Node> {
    list.items()
        .find(|v| flag(v.get("selected")) == Some(true))
        .or_else(|| list.items().next())
}

impl<'a> Builder<'a> {
    pub fn new(docs: &'a BTreeMap<&'a str, &'a Document>) -> Self {
        Self {
            docs,
            issues: Vec::new(),
        }
    }

    fn issue(&mut self, path: &str, message: impl Into<String>) {
        self.issues.push(FormatError::Invalid {
            path: path.to_owned(),
            message: message.into(),
        });
    }

    pub fn collection(&mut self) -> Collection {
        let root = &self.docs[COLLECTION_FILE].root;
        let mut environments: Vec<Environment> = root
            .get("config")
            .and_then(|c| c.get("environments"))
            .map(|list| list.items().map(|e| environment(e, "")).collect())
            .unwrap_or_default();
        let prefix = format!("{ENVIRONMENTS_DIR}/");
        for (path, doc) in self.docs {
            if let Some(name) = path.strip_prefix(&prefix)
                && !name.contains('/')
            {
                environments.push(environment(&doc.root, file_stem(path)));
            }
        }
        let mut defaults = self.defaults(COLLECTION_FILE, root.get("request"), Auth::None);
        if let Some(auth) = crate::auth::oauth2::read_extension(root) {
            defaults.auth = auth;
        }
        let mut col = Collection {
            name: text(root.get("info").and_then(|i| i.get("name"))),
            defaults,
            items: Vec::new(),
            environments,
            config: CollectionConfig {
                // Wave 8, strand C: `extensions.requesttailor.agents`.
                agents: crate::agents::read(root),
                http3: false,
                protobuf: Default::default(),
                client_certificates: crate::auth::certs::read(
                    root.get("config").and_then(|c| c.get("clientCertificates")),
                ),
            },
        };
        if flag(root.get("bundled")) == Some(true) && root.get("items").is_some() {
            self.issue(
                COLLECTION_FILE,
                "bundled collections (items inside opencollection.yml) are not supported yet",
            );
        }
        col.items = self.items("");
        col
    }

    /// Items of the folder at `dir` (`""` for the collection root), sorted.
    fn items(&mut self, dir: &str) -> Vec<Item> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut subdirs: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        for path in self.docs.keys() {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((sub, _)) => {
                    if !(dir.is_empty() && sub == ENVIRONMENTS_DIR)
                        && subdirs.last().is_none_or(|s| s != sub)
                    {
                        subdirs.push(sub.to_owned());
                    }
                }
                None if rest == FOLDER_FILE || (dir.is_empty() && rest == COLLECTION_FILE) => {}
                None => files.push((*path).to_owned()),
            }
        }
        let mut items = Vec::new();
        for sub in subdirs {
            items.push(Item::Folder(self.folder(&format!("{prefix}{sub}"))));
        }
        for path in files {
            if let Some(entry) = self.request(&path) {
                items.push(Item::Request(entry));
            }
        }
        // Folders and requests share one sequence; without `seq` an item goes last.
        let key = |i: &Item| match i {
            Item::Folder(f) => (f.seq, 0, f.name.clone(), f.path.clone()),
            Item::Request(r) => (r.spec.seq, 1, r.spec.name.clone(), r.path.clone()),
        };
        items.sort_by(|a, b| {
            let (sa, ka, na, pa) = key(a);
            let (sb, kb, nb, pb) = key(b);
            let seq = match (sa, sb) {
                (Some(x), Some(y)) => x.total_cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            };
            seq.then(ka.cmp(&kb)).then(na.cmp(&nb)).then(pa.cmp(&pb))
        });
        items
    }

    fn folder(&mut self, dir: &str) -> Folder {
        let file = format!("{dir}/{FOLDER_FILE}");
        let root = self.docs.get(file.as_str()).map(|d| &d.root);
        let info = root.and_then(|r| r.get("info"));
        let name = info
            .and_then(|i| i.get("name"))
            .and_then(Node::text)
            .unwrap_or_else(|| file_stem(dir))
            .to_owned();
        let seq = info.and_then(|i| i.get("seq")).and_then(Node::as_f64);
        let defaults = self.defaults(&file, root.and_then(|r| r.get("request")), Auth::Inherit);
        Folder {
            name,
            seq,
            path: dir.to_owned(),
            defaults,
            items: self.items(dir),
        }
    }

    fn defaults(&mut self, path: &str, request: Option<&Node>, no_auth: Auth) -> RequestDefaults {
        let Some(r) = request else {
            return RequestDefaults {
                auth: no_auth,
                ..RequestDefaults::default()
            };
        };
        RequestDefaults {
            scripts: crate::runtime::scripts(r.get("scripts")),
            headers: key_values(r.get("headers")),
            auth: self.auth(path, r.get("auth"), no_auth),
            vars: variables(r.get("variables")),
            settings: settings(r.get("settings").and_then(|s| s.get("http"))),
        }
    }

    pub(crate) fn request(&mut self, path: &str) -> Option<RequestEntry> {
        let root = &self.docs.get(path)?.root;
        let info = root.get("info");
        let kind = info.and_then(|i| i.get("type")).and_then(Node::text);
        match kind {
            Some("http") => {}
            None if root.get("http").is_some() => {}
            Some(k) if OTHER_ITEM_TYPES.contains(&k) => {
                return crate::protocols::read(self, path, k);
            }
            Some(k) => {
                self.issue(path, format!("unknown item type `{k}`"));
                return None;
            }
            None => {
                self.issue(path, "not an OpenCollection item (no `info.type`)");
                return None;
            }
        }
        let http = root.get("http");
        let field = |k: &str| http.and_then(|h| h.get(k));
        let name = info
            .and_then(|i| i.get("name"))
            .and_then(Node::text)
            .unwrap_or_else(|| file_stem(path))
            .to_owned();
        let runtime = root.get("runtime");
        let spec = RequestSpec {
            protocol: Default::default(),
            scripts: crate::runtime::scripts(runtime.and_then(|r| r.get("scripts"))),
            assertions: crate::runtime::assertions(runtime.and_then(|r| r.get("assertions"))),
            actions: crate::runtime::actions(runtime.and_then(|r| r.get("actions"))),
            name,
            seq: info.and_then(|i| i.get("seq")).and_then(Node::as_f64),
            method: field("method")
                .and_then(Node::text)
                .unwrap_or("GET")
                .to_owned(),
            url: text(field("url")),
            headers: key_values(field("headers")),
            params: params(field("params")),
            body: self.body(path, field("body")),
            auth: self.auth(path, field("auth"), Auth::None),
            vars: variables(runtime.and_then(|r| r.get("variables"))),
            settings: settings(root.get("settings")),
        };
        Some(RequestEntry {
            path: path.to_owned(),
            spec,
        })
    }

    pub(crate) fn auth(&mut self, path: &str, node: Option<&Node>, missing: Auth) -> Auth {
        let Some(node) = node.filter(|n| !n.is_null()) else {
            return missing;
        };
        let kind = if node.is_map() {
            // `mode` is how older Bruno versions named the type.
            node.get("type")
                .or_else(|| node.get("mode"))
                .and_then(Node::text)
        } else {
            node.text()
        };
        let Some(kind) = kind else {
            self.issue(path, "auth without a type");
            return Auth::Unsupported {
                kind: String::new(),
            };
        };
        let field = |k: &str| text(node.get(k));
        match kind {
            "inherit" => Auth::Inherit,
            "none" => Auth::None,
            "basic" if node.is_map() => Auth::Basic {
                username: field("username"),
                password: field("password"),
            },
            "bearer" if node.is_map() => Auth::Bearer {
                token: field("token"),
            },
            "apikey" if node.is_map() => Auth::ApiKey {
                key: field("key"),
                value: field("value"),
                placement: match field("placement").as_str() {
                    "query" | "queryparams" => ApiKeyPlacement::Query,
                    _ => ApiKeyPlacement::Header,
                },
            },
            "oauth2" if node.is_map() => crate::auth::oauth2::read(node),
            "awsv4" if node.is_map() => Auth::AwsSigV4(crate::auth::awsv4::read(node)),
            other => Auth::Unsupported {
                kind: other.to_owned(),
            },
        }
    }

    pub(crate) fn body(&mut self, path: &str, node: Option<&Node>) -> Body {
        let Some(node) = node.filter(|n| !n.is_null()) else {
            return Body::None;
        };
        if node.is_seq() {
            return match selected(node) {
                Some(v) => self.body(path, v.get("body")),
                None => Body::None,
            };
        }
        let data = node.get("data");
        let raw = |kind| Body::Raw {
            kind,
            data: text(data),
        };
        match node.get("type").and_then(Node::text).unwrap_or("none") {
            "none" => Body::None,
            "json" => raw(RawKind::Json),
            "text" => raw(RawKind::Text),
            "xml" => raw(RawKind::Xml),
            "sparql" => raw(RawKind::Sparql),
            "form-urlencoded" => Body::FormUrlEncoded {
                fields: key_values(data),
            },
            "multipart-form" => Body::Multipart {
                parts: data
                    .map(|d| d.items().map(part).collect())
                    .unwrap_or_default(),
            },
            "file" => match data.and_then(selected) {
                Some(f) => Body::File {
                    path: text(f.get("filePath")),
                    content_type: f.get("contentType").and_then(Node::text).map(str::to_owned),
                },
                None => Body::None,
            },
            other => {
                self.issue(path, format!("unsupported body type `{other}`"));
                Body::None
            }
        }
    }
}

pub(crate) fn key_values(list: Option<&Node>) -> Vec<KeyValue> {
    list.map(|l| l.items().map(key_value).collect())
        .unwrap_or_default()
}

pub(crate) fn key_value(h: &Node) -> KeyValue {
    KeyValue {
        name: text(h.get("name")),
        value: text(h.get("value")),
        enabled: enabled(h),
    }
}

pub(crate) fn params(list: Option<&Node>) -> Vec<Param> {
    list.map(|l| l.items().map(param).collect())
        .unwrap_or_default()
}

pub(crate) fn param(p: &Node) -> Param {
    Param {
        name: text(p.get("name")),
        value: text(p.get("value")),
        kind: match p.get("type").and_then(Node::text) {
            Some("path") => ParamKind::Path,
            _ => ParamKind::Query,
        },
        enabled: enabled(p),
    }
}

pub(crate) fn part(p: &Node) -> MultipartPart {
    let kind = match p.get("type").and_then(Node::text) {
        Some("file") => PartKind::File,
        _ => PartKind::Text,
    };
    let value = p.get("value");
    let values = match value {
        Some(v) if v.is_seq() => v.items().map(|x| text(Some(x))).collect(),
        Some(v) if !v.is_null() => vec![text(Some(v))],
        _ if kind == PartKind::Text => vec![String::new()],
        _ => Vec::new(),
    };
    MultipartPart {
        name: text(p.get("name")),
        kind,
        values,
        content_type: p.get("contentType").and_then(Node::text).map(str::to_owned),
        enabled: enabled(p),
    }
}

/// A variable value: a string, a typed value (`{type, data}`) or a list of variants.
fn variable_value(node: Option<&Node>) -> Option<String> {
    let node = node.filter(|n| !n.is_null())?;
    if node.is_map() {
        return Some(text(node.get("data")));
    }
    if node.is_seq() {
        return variable_value(selected(node)?.get("value"));
    }
    node.text().map(str::to_owned)
}

pub(crate) fn variables(list: Option<&Node>) -> Vec<Variable> {
    list.map(|l| l.items().map(variable).collect())
        .unwrap_or_default()
}

pub(crate) fn variable(v: &Node) -> Variable {
    let secret = flag(v.get("secret")) == Some(true);
    Variable {
        name: text(v.get("name")),
        // Secret values never live in the file (product principle 3).
        value: if secret {
            None
        } else {
            variable_value(v.get("value"))
        },
        enabled: enabled(v),
        secret,
    }
}

pub(crate) fn environment(e: &Node, fallback_name: &str) -> Environment {
    Environment {
        name: e
            .get("name")
            .and_then(Node::text)
            .unwrap_or(fallback_name)
            .to_owned(),
        variables: variables(e.get("variables")),
        client_certificates: crate::auth::certs::read(e.get("clientCertificates")),
        external_secrets: crate::auth::secrets::read(e.get("externalSecrets")),
    }
}

pub(crate) fn settings(node: Option<&Node>) -> Settings {
    let Some(s) = node else {
        return Settings::default();
    };
    // Numbers only; `inherit` (or anything else) leaves the setting to the parent.
    let number = |k: &str| {
        s.get(k)
            .and_then(Node::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
    };
    Settings {
        timeout_ms: number("timeout").map(|n| n as u64),
        follow_redirects: flag(s.get("followRedirects")),
        max_redirects: number("maxRedirects").map(|n| n.min(u32::MAX as f64) as u32),
        encode_url: flag(s.get("encodeUrl")),
    }
}

/// Checks the `opencollection` version: 1.0.x, and the short forms `1` and `1.0` found in
/// files written by hand.
pub(crate) fn check_version(root: &Node) -> Result<(), FormatError> {
    let Some(version) = root.get("opencollection").and_then(Node::text) else {
        // Bruno treats a missing version as 1.0.0.
        return Ok(());
    };
    let ok = match version.split('.').collect::<Vec<_>>()[..] {
        ["1"] | ["1", "0"] => true,
        ["1", "0", patch] => !patch.is_empty() && patch.bytes().all(|c| c.is_ascii_digit()),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(FormatError::UnsupportedVersion {
            path: COLLECTION_FILE.to_owned(),
            version: version.to_owned(),
        })
    }
}

/// Directory of an item path; used to skip nested collections.
pub(crate) fn dir_of(path: &str) -> &str {
    parent_dir(path)
}
