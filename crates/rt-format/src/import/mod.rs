// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importers: other tools' files → a new OpenCollection collection, plus a loss report.
//!
//! Every importer is its own module and turns its source into a complete collection
//! ([`Import::files`], written by this crate's writer, so it reads back with
//! [`crate::read_collection`]). Whatever it cannot carry over goes into the [`LossReport`]:
//! nothing is dropped silently (CLAUDE.md, "Importer").
//!
//! Like the rest of the crate this is in memory and I/O-free, so it runs in the browser too.
//! Input is bounded ([`MAX_INPUT_BYTES`], nesting depth) and never panics, whatever it holds.
//! Test data: `testdata/import/<source>/`, every file with a snapshot test of the result and
//! of its loss report.
//!
//! Secrets (product principle 3): values of secret variables (Postman `secret`, Insomnia
//! private environments, JetBrains private env files) are never written; the variable is
//! imported as `secret: true` and the report says to set the value in Environments.
//!
//! Owner: wave 4, strand B. The public types in this file are interface (working rule 1).

use serde::{Deserialize, Serialize};

use crate::FileSet;

mod auth;
mod common;
mod curl;
mod har;
mod http;
mod insomnia;
mod openapi;
mod out;
mod postman;
mod value;

use value::Value;

/// The formats RequestTailor imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportFormat {
    /// Postman collection v2.0 / v2.1 (JSON), with environment and globals files.
    Postman,
    /// Insomnia export v4 (JSON or YAML) and v5 (YAML).
    Insomnia,
    /// OpenAPI 3.0 / 3.1 (JSON or YAML).
    Openapi,
    /// One or more `curl` command lines.
    Curl,
    /// HTTP Archive 1.2 (JSON).
    Har,
    /// `.http` / `.rest` files (VS Code REST Client, JetBrains HTTP Client).
    Http,
    /// Converted by a plugin's importer (wave 5). Not in [`ImportFormat::ALL`]: never detected
    /// and never passed to [`import`]; the plugin host produces the [`Import`] itself.
    Plugin,
}

impl ImportFormat {
    pub const ALL: [ImportFormat; 6] = [
        ImportFormat::Postman,
        ImportFormat::Insomnia,
        ImportFormat::Openapi,
        ImportFormat::Curl,
        ImportFormat::Har,
        ImportFormat::Http,
    ];

    /// The name users know the format by.
    pub fn label(self) -> &'static str {
        match self {
            ImportFormat::Postman => "Postman",
            ImportFormat::Insomnia => "Insomnia",
            ImportFormat::Openapi => "OpenAPI",
            ImportFormat::Curl => "cURL",
            ImportFormat::Har => "HAR",
            ImportFormat::Http => ".http",
            ImportFormat::Plugin => "plugin",
        }
    }
}

impl std::fmt::Display for ImportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A converted collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// Name for the collection (and the default folder name).
    pub collection_name: String,
    /// The complete collection: `opencollection.yml`, folders, requests, environments.
    pub files: FileSet,
    pub report: LossReport,
}

/// What an import could not carry over, item by item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossReport {
    pub format: ImportFormat,
    /// Requests written to the collection.
    pub requests: u32,
    /// Folders written to the collection.
    pub folders: u32,
    /// Environments written to the collection.
    pub environments: u32,
    /// `collection` first, then sorted by `location`, then `kind`.
    pub losses: Vec<Loss>,
}

impl LossReport {
    /// `true` if nothing was lost.
    pub fn is_complete(&self) -> bool {
        self.losses.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loss {
    /// Where in the source, as the user knows it: `Users / Get user` for an item,
    /// `environment Staging`, `collection` for collection-level settings.
    pub location: String,
    pub kind: LossKind,
    pub effect: LossEffect,
    /// One readable sentence, e.g. `pre-request script (12 lines) not imported`.
    pub detail: String,
}

/// What was lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LossKind {
    /// Pre-request or post-response script.
    Script,
    /// Tests and assertions.
    Test,
    /// An auth scheme the core does not support yet (OAuth 2.0, Digest, AWS, ...), or
    /// credentials typed into a request.
    Auth,
    /// A body type or part that could not be represented (GraphQL variables, binary, ...).
    Body,
    /// Variables, dynamic values (`{{$guid}}`) and their scopes; secret values.
    Variable,
    /// Request settings (proxy, certificates, timeouts, cookie handling, ...).
    Setting,
    /// Descriptions and documentation.
    Documentation,
    /// Saved example responses.
    Example,
    /// Items of another protocol (WebSocket, gRPC, GraphQL subscriptions, ...).
    Protocol,
    Other,
}

impl LossKind {
    pub const ALL: [LossKind; 10] = [
        LossKind::Script,
        LossKind::Test,
        LossKind::Auth,
        LossKind::Body,
        LossKind::Variable,
        LossKind::Setting,
        LossKind::Documentation,
        LossKind::Example,
        LossKind::Protocol,
        LossKind::Other,
    ];

    /// Heading for a group of losses of this kind.
    pub fn label(self) -> &'static str {
        match self {
            LossKind::Script => "Scripts",
            LossKind::Test => "Tests",
            LossKind::Auth => "Auth and credentials",
            LossKind::Body => "Bodies",
            LossKind::Variable => "Variables and secrets",
            LossKind::Setting => "Settings",
            LossKind::Documentation => "Documentation",
            LossKind::Example => "Examples",
            LossKind::Protocol => "Other protocols",
            LossKind::Other => "Other",
        }
    }
}

/// How much of it was lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LossEffect {
    /// Not in the collection at all.
    Dropped,
    /// In the collection, but not exactly as in the source (the detail says how).
    Approximated,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("unknown file format; choose the format explicitly")]
    UnknownFormat,
    /// The file looks like JSON or YAML but does not parse, so its format is unknown.
    #[error("{name} is not valid {syntax}: {message}")]
    Malformed {
        name: String,
        syntax: String,
        message: String,
    },
    #[error("not a valid {format} file: {message}")]
    Invalid {
        format: ImportFormat,
        message: String,
    },
    #[error("{format} version {version} is not supported")]
    UnsupportedVersion {
        format: ImportFormat,
        version: String,
    },
    /// The files cannot be imported together (two collections, only environments, ...).
    #[error("{message}")]
    Files { message: String },
    #[error("the files are larger than {limit_mb} MB together")]
    TooLarge { limit_mb: u32 },
}

/// One input file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFile<'a> {
    /// File name as the user picked it (only the last path component matters).
    pub name: &'a str,
    pub data: &'a [u8],
}

/// Largest input accepted, all files together.
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

/// What a file is, as far as combining files goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A collection-like file of this format.
    Main(ImportFormat),
    /// A Postman environment or globals export (`*.postman_environment.json`).
    PostmanEnvironment,
    /// JetBrains `http-client.env.json` / `http-client.private.env.json`.
    HttpEnvironment,
}

pub(crate) fn base_name(file_name: &str) -> &str {
    file_name.rsplit(['/', '\\']).next().unwrap_or(file_name)
}

fn is_http_env_file(file_name: &str) -> bool {
    let lower = base_name(file_name).to_ascii_lowercase();
    lower.ends_with(".env.json") && lower.contains("http-client")
}

/// What a parsed JSON/YAML document is.
fn classify(v: &Value) -> Option<Role> {
    if v.get("_postman_variable_scope").is_some()
        || (v.get("values").is_some_and(Value::is_arr)
            && v.get("name").is_some()
            && v.get("info").is_none())
    {
        return Some(Role::PostmanEnvironment);
    }
    let info = v.get("info");
    let schema = info.map(|i| i.string("schema")).unwrap_or_default();
    if schema.contains("getpostman.com") || schema.contains("schema.postman.com") {
        return Some(Role::Main(ImportFormat::Postman));
    }
    if info.is_some_and(|i| i.get("_postman_id").is_some()) && v.get("item").is_some() {
        return Some(Role::Main(ImportFormat::Postman));
    }
    // Postman collection v1 (rejected by the importer with a clear message).
    if v.get("requests").is_some_and(Value::is_arr) && v.get("order").is_some() {
        return Some(Role::Main(ImportFormat::Postman));
    }
    let kind = v.string("type");
    if v.get("__export_format").is_some()
        || v.str_of("_type").as_deref() == Some("export")
        || kind.starts_with("collection.insomnia.rest/")
        || kind.starts_with("spec.insomnia.rest/")
        || kind.starts_with("environment.insomnia.rest/")
    {
        return Some(Role::Main(ImportFormat::Insomnia));
    }
    if v.get("openapi").is_some() || v.get("swagger").is_some() {
        return Some(Role::Main(ImportFormat::Openapi));
    }
    if v.get("log").is_some_and(|l| l.get("entries").is_some()) {
        return Some(Role::Main(ImportFormat::Har));
    }
    None
}

/// The first line that is not blank or a comment.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .unwrap_or("")
}

pub(crate) fn looks_like_curl(text: &str) -> bool {
    // `curl` as a command of the first line, also inside a pipeline (`echo x | curl ...`).
    first_line(text).split(['|', ';', '&']).any(|cmd| {
        let cmd = cmd.trim();
        let cmd = cmd.strip_prefix('$').map(str::trim_start).unwrap_or(cmd);
        cmd == "curl" || cmd.starts_with("curl ") || cmd.starts_with("curl.exe ")
    })
}

fn looks_like_http(text: &str) -> bool {
    text.lines().take(2000).any(|l| {
        let l = l.trim_end();
        if l.starts_with("###") {
            return true;
        }
        let mut words = l.split_whitespace();
        let (Some(method), Some(target)) = (words.next(), words.next()) else {
            return false;
        };
        http::METHODS.contains(&method)
            && (target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("{{")
                || target.starts_with('/'))
    })
}

fn role(file_name: &str, data: &[u8]) -> Option<Role> {
    if is_http_env_file(file_name) {
        return Some(Role::HttpEnvironment);
    }
    let lower = base_name(file_name).to_ascii_lowercase();
    if lower.ends_with(".http") || lower.ends_with(".rest") {
        return Some(Role::Main(ImportFormat::Http));
    }
    let text = common::text(data)?;
    let text: &str = &text;
    if looks_like_curl(text) {
        return Some(Role::Main(ImportFormat::Curl));
    }
    if data.len() <= MAX_INPUT_BYTES
        && let Ok(v) = value::parse_any(text)
        && v.is_obj()
        && let Some(role) = classify(&v)
    {
        return Some(role);
    }
    if lower.ends_with(".har") {
        return Some(Role::Main(ImportFormat::Har));
    }
    if lower.ends_with(".postman_collection.json") {
        return Some(Role::Main(ImportFormat::Postman));
    }
    if lower.ends_with(".postman_environment.json") || lower.ends_with(".postman_globals.json") {
        return Some(Role::PostmanEnvironment);
    }
    if looks_like_http(text) {
        return Some(Role::Main(ImportFormat::Http));
    }
    None
}

/// Why a file that looks like JSON or YAML cannot be read.
fn malformed(f: &SourceFile<'_>) -> Option<ImportError> {
    let text = common::text(f.data)?;
    let lower = base_name(f.name).to_ascii_lowercase();
    let syntax = if text.trim_start().starts_with(['{', '[']) {
        "JSON"
    } else if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        "YAML"
    } else {
        return None;
    };
    let message = value::parse_any(&text).err()?;
    Some(ImportError::Malformed {
        name: base_name(f.name).to_owned(),
        syntax: syntax.to_owned(),
        message,
    })
}

/// Guesses the format from the content and the file name; `None` if it is none of them.
/// Postman environment files count as Postman, `http-client.env.json` as `.http`.
pub fn detect(file_name: &str, data: &[u8]) -> Option<ImportFormat> {
    match role(file_name, data)? {
        Role::Main(format) => Some(format),
        Role::PostmanEnvironment => Some(ImportFormat::Postman),
        Role::HttpEnvironment => Some(ImportFormat::Http),
    }
}

/// Imports `data` (the file `file_name`) as `format`, or as the detected format.
pub fn import(
    format: Option<ImportFormat>,
    file_name: &str,
    data: &[u8],
) -> Result<Import, ImportError> {
    import_files(
        format,
        &[SourceFile {
            name: file_name,
            data,
        }],
    )
}

/// Imports several files as one collection: a Postman collection with its environment (and
/// globals) files; `.http` files (one folder each if there are several) with
/// `http-client.env.json` / `http-client.private.env.json`; several cURL files. Any other
/// format is one file at a time. `format` applies to the files that are not environment
/// files; `None` detects each file's format.
pub fn import_files(
    format: Option<ImportFormat>,
    files: &[SourceFile<'_>],
) -> Result<Import, ImportError> {
    if files.is_empty() {
        return Err(ImportError::Files {
            message: "no file to import".into(),
        });
    }
    let total: usize = files.iter().map(|f| f.data.len()).sum();
    if total > MAX_INPUT_BYTES {
        return Err(ImportError::TooLarge {
            limit_mb: (MAX_INPUT_BYTES / (1024 * 1024)) as u32,
        });
    }
    if let Some(f) = files.iter().find(|f| f.data.trim_ascii().is_empty()) {
        return Err(ImportError::Files {
            message: format!("{} is empty", base_name(f.name)),
        });
    }
    let mut mains: Vec<(ImportFormat, SourceFile<'_>)> = Vec::new();
    let mut postman_envs = Vec::new();
    let mut http_envs = Vec::new();
    for f in files {
        match (role(f.name, f.data), format) {
            (Some(Role::PostmanEnvironment), None | Some(ImportFormat::Postman)) => {
                postman_envs.push(*f)
            }
            (Some(Role::HttpEnvironment), None | Some(ImportFormat::Http)) => http_envs.push(*f),
            (_, Some(format)) | (Some(Role::Main(format)), None) => mains.push((format, *f)),
            (None, None) if files.len() == 1 => {
                return Err(malformed(f).unwrap_or(ImportError::UnknownFormat));
            }
            (None, None) => {
                return Err(ImportError::Files {
                    message: format!(
                        "the format of {} is unknown; choose the format explicitly",
                        base_name(f.name)
                    ),
                });
            }
        }
    }
    let Some(&(main_format, first)) = mains.first() else {
        return Err(ImportError::Files {
            message: "only environment files were given; add the collection file too".into(),
        });
    };
    if let Some((other, f)) = mains.iter().find(|(fmt, _)| *fmt != main_format) {
        return Err(ImportError::Files {
            message: format!(
                "{} is a {other} file and {} a {main_format} file; import one format at a time",
                base_name(f.name),
                base_name(first.name)
            ),
        });
    }
    let multi = matches!(main_format, ImportFormat::Http | ImportFormat::Curl);
    if mains.len() > 1 && !multi {
        return Err(ImportError::Files {
            message: format!(
                "several {main_format} files were given; import one collection at a time"
            ),
        });
    }
    if !postman_envs.is_empty() && main_format != ImportFormat::Postman {
        return Err(ImportError::Files {
            message: format!(
                "Postman environment files can only be imported with a Postman collection, \
                 not with a {main_format} file"
            ),
        });
    }
    if !http_envs.is_empty() && main_format != ImportFormat::Http {
        return Err(ImportError::Files {
            message: format!(
                "http-client environment files can only be imported with .http files, not \
                 with a {main_format} file"
            ),
        });
    }
    let mains: Vec<SourceFile<'_>> = mains.into_iter().map(|(_, f)| f).collect();
    match main_format {
        ImportFormat::Postman => postman::import(&first, &postman_envs),
        ImportFormat::Insomnia => insomnia::import(&first),
        ImportFormat::Openapi => openapi::import(&first),
        ImportFormat::Curl => curl::import(&mains),
        ImportFormat::Har => har::import(&first),
        ImportFormat::Http => http::import(&mains, &http_envs),
        ImportFormat::Plugin => Err(ImportError::Files {
            message: "plugin importers run in the plugin host, not here".to_owned(),
        }),
    }
}

/// The file name without directories and without the first of `extensions` it ends with
/// (ignoring case), or else without its last extension.
pub(crate) fn stem<'a>(file_name: &'a str, extensions: &[&str]) -> &'a str {
    let name = base_name(file_name);
    let lower = name.to_ascii_lowercase();
    for ext in extensions {
        if lower.ends_with(ext) && lower.len() > ext.len() {
            return &name[..name.len() - ext.len()];
        }
    }
    match name.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => name,
    }
}
