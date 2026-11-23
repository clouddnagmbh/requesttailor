// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The files of an OpenAPI or Swagger spec (OpenAPI import wizard): one file, a multi-file
//! spec (the main file and the files its `$ref`s name, by their relative paths) or the
//! entries of a `.zip`, read into one document.
//!
//! - Main file: a single file is the main file. Among several, the ones with a root
//!   `openapi` or `swagger` key; if several have one, `openapi.*`, `swagger.*` or `index.*`
//!   first, then the shallowest path, then the first given.
//! - Bundling (several files only): a `$ref` to `other.yaml#/ptr` is resolved relative to the
//!   referring file; the whole referenced file is hoisted once under the root object
//!   [`BUNDLED_KEY`] with a readable unique key (`schemas/pet.yaml` → `schemas_pet.yaml`), and
//!   the reference becomes the local pointer `#/x-requesttailor-bundled/schemas_pet.yaml/ptr`.
//!   Local `#/...` references inside a hoisted file point into its copy; references back to
//!   the main file become plain local pointers. So every `#/...` pointer of the result works
//!   on the one document, for the importer and the contract reader alike.
//! - A file that is referenced but not given stays an external reference and is reported
//!   (dropped, kind other) by name; so are URL references (`https://…`, never fetched here)
//!   and files that are no JSON or YAML. A path that is not found exactly is looked up by
//!   its file name if exactly one given file has that name (browsers hand over names only).
//!   Given files the spec does not reference are reported too.
//! - Bounds: the input size ([`MAX_INPUT_BYTES`]), at most [`MAX_FILES`] files hoisted; every
//!   file is hoisted once, so reference cycles end.

use std::collections::{BTreeSet, HashMap, VecDeque};

use super::common::{invalid, text, utf8};
use super::value::{Value, parse_any};
use super::{
    ImportError, ImportFormat, Loss, LossEffect, LossKind, MAX_INPUT_BYTES, SourceFile, base_name,
    zip,
};

const F: ImportFormat = ImportFormat::Openapi;

/// The root key the files of a multi-file spec are hoisted under.
pub(super) const BUNDLED_KEY: &str = "x-requesttailor-bundled";

/// Most files hoisted into one document.
pub(super) const MAX_FILES: usize = 256;

/// A spec read from its files.
pub(super) struct Loaded {
    /// Path of the main file (as given, `/`-separated).
    pub main: String,
    /// Its bytes as given.
    pub main_bytes: Vec<u8>,
    /// The main file is JSON (else YAML).
    pub main_json: bool,
    /// The main document, with the referenced files hoisted (see the module docs).
    pub root: Value,
    /// Files read: the main file and the hoisted ones.
    pub files: u32,
    /// Files were hoisted into `root`.
    pub bundled: bool,
    /// Several files were given (or a zip): the bundler reported unresolved references.
    pub multi: bool,
    /// What reading lost (several files only).
    pub losses: Vec<Loss>,
}

fn dropped(detail: String) -> Loss {
    Loss {
        location: "collection".into(),
        kind: LossKind::Other,
        effect: LossEffect::Dropped,
        detail,
    }
}

/// `a/./b/../c` → `a/c`; `\` as `/`. Leading `..` that cannot be resolved stay.
pub(super) fn clean(path: &str) -> String {
    let path = path.replace('\\', "/");
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." if parts.last().is_some_and(|l| *l != "..") => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// `target` relative to the file `from`.
fn join(from: &str, target: &str) -> String {
    if target.starts_with('/') {
        return clean(target);
    }
    let dir = dir_of(from);
    clean(&if dir.is_empty() {
        target.to_owned()
    } else {
        format!("{dir}/{target}")
    })
}

/// `%20` and friends decoded (references are URIs); invalid escapes stay.
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(h) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(h, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

pub(super) fn is_url(target: &str) -> bool {
    target.starts_with("//")
        || target
            .split_once("://")
            .is_some_and(|(scheme, _)| !scheme.is_empty() && !scheme.contains('/'))
}

/// `true` for a document with a root `openapi` or `swagger` key.
fn is_spec(v: &Value) -> bool {
    v.is_obj() && (v.get("openapi").is_some() || v.get("swagger").is_some())
}

/// The `$ref` targets of a document (the part before `#`), deduplicated, in order.
pub(super) fn references(v: &Value) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>, seen: &mut BTreeSet<String>) {
        match v {
            Value::Obj(entries) => {
                for (k, val) in entries {
                    if k == "$ref" {
                        if let Value::Str(r) = val {
                            let target = r.split('#').next().unwrap_or("");
                            if !target.is_empty() && seen.insert(target.to_owned()) {
                                out.push(target.to_owned());
                            }
                        }
                    } else {
                        walk(val, out, seen);
                    }
                }
            }
            Value::Arr(items) => items.iter().for_each(|i| walk(i, out, seen)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, &mut out, &mut BTreeSet::new());
    out
}

/// The given files with zips expanded, names cleaned.
fn expand(files: &[SourceFile<'_>]) -> Result<Vec<(String, Vec<u8>)>, ImportError> {
    if files.is_empty() {
        return Err(ImportError::Files {
            message: "no file to import".into(),
        });
    }
    let too_large = || ImportError::TooLarge {
        limit_mb: (MAX_INPUT_BYTES / (1024 * 1024)) as u32,
    };
    let mut out = Vec::new();
    let mut total = 0usize;
    for f in files {
        if f.data.trim_ascii().is_empty() {
            return Err(ImportError::Files {
                message: format!("{} is empty", base_name(f.name)),
            });
        }
        let entries = if zip::is_zip(f.data) || f.name.to_ascii_lowercase().ends_with(".zip") {
            zip::unzip(f.data)?
                .into_iter()
                .filter(|(_, d)| !d.trim_ascii().is_empty())
                .collect()
        } else {
            vec![(f.name.to_owned(), f.data.to_vec())]
        };
        for (name, data) in entries {
            total = total.saturating_add(data.len());
            if total > MAX_INPUT_BYTES {
                return Err(too_large());
            }
            out.push((clean(&name), data));
        }
    }
    if out.is_empty() {
        return Err(ImportError::Files {
            message: "the archive holds no files".into(),
        });
    }
    Ok(out)
}

/// The index of the main file among several.
fn pick_main(files: &[(String, Vec<u8>)], parsed: &[Option<Value>]) -> Option<usize> {
    let candidates: Vec<usize> = (0..files.len())
        .filter(|&i| parsed[i].as_ref().is_some_and(is_spec))
        .collect();
    let rank = |i: &usize| {
        let name = base_name(&files[*i].0).to_ascii_lowercase();
        let stem = name.split('.').next().unwrap_or("");
        let preferred = matches!(stem, "openapi" | "swagger" | "index");
        (
            !preferred,
            files[*i].0.matches('/').count(),
            *i, // the first given
        )
    };
    candidates.into_iter().min_by_key(rank)
}

/// Reads the spec in `files` (see the module docs). Errors are the importer's: a single file
/// that is not JSON or YAML, several files of which none is a spec, a damaged zip.
pub(super) fn load(files: &[SourceFile<'_>]) -> Result<Loaded, ImportError> {
    let files = expand(files)?;
    let multi = files.len() > 1;
    let (main, root) = if !multi {
        let t = utf8(F, &files[0].1)?;
        let root = parse_any(&t).map_err(|e| invalid(F, e))?;
        (0, root)
    } else {
        let parsed: Vec<Option<Value>> = files
            .iter()
            .map(|(_, d)| text(d).and_then(|t| parse_any(&t).ok()))
            .collect();
        let Some(main) = pick_main(&files, &parsed) else {
            return Err(ImportError::Files {
                message: "none of the files is an OpenAPI or Swagger document (a root \
                          `openapi` or `swagger` key)"
                    .into(),
            });
        };
        let root = parsed[main].clone().unwrap_or(Value::Null);
        (main, root)
    };
    let main_json = text(&files[main].1).is_some_and(|t| t.trim_start().starts_with('{'));
    let mut loaded = Loaded {
        main: files[main].0.clone(),
        main_bytes: files[main].1.clone(),
        main_json,
        root,
        files: 1,
        bundled: false,
        multi,
        losses: Vec::new(),
    };
    if multi && loaded.root.is_obj() {
        let mut b = Bundler {
            files: &files,
            main: loaded.main.clone(),
            keys: HashMap::new(),
            taken: BTreeSet::new(),
            pending: HashMap::new(),
            queue: VecDeque::new(),
            missing: Vec::new(),
            urls: Vec::new(),
            unreadable: Vec::new(),
            over_limit: false,
        };
        let mut root = std::mem::replace(&mut loaded.root, Value::Null);
        let mut existing: Vec<(String, Value)> = match root.get(BUNDLED_KEY) {
            Some(Value::Obj(entries)) => entries.clone(),
            _ => Vec::new(),
        };
        b.taken.extend(existing.iter().map(|(k, _)| k.clone()));
        let main_name = b.main.clone();
        b.rewrite(&mut root, &main_name);
        let mut hoisted = Vec::new();
        while let Some(path) = b.queue.pop_front() {
            let Some(mut v) = b.pending.remove(&path) else {
                continue;
            };
            b.rewrite(&mut v, &path);
            hoisted.push((b.keys[&path].clone(), v));
        }
        if !hoisted.is_empty()
            && let Value::Obj(entries) = &mut root
        {
            loaded.files += hoisted.len() as u32;
            loaded.bundled = true;
            existing.extend(hoisted);
            entries.retain(|(k, _)| k != BUNDLED_KEY);
            entries.push((BUNDLED_KEY.to_owned(), Value::Obj(existing)));
        }
        loaded.root = root;
        loaded.losses = b.losses();
        // Nothing is dropped silently: given files the spec does not use.
        let unused: Vec<String> = files
            .iter()
            .map(|(n, _)| n)
            .filter(|n| **n != b.main && !b.keys.contains_key(*n))
            .filter(|n| !b.unreadable.iter().any(|(p, _)| p == *n))
            .map(|n| format!("`{}`", b.shown(n)))
            .collect();
        if !unused.is_empty() {
            let more = unused.len() > 5;
            loaded.losses.push(dropped(format!(
                "{} not referenced by the spec; not imported",
                if unused.len() == 1 {
                    format!("{} is", unused[0])
                } else {
                    format!(
                        "{}{} are",
                        unused[..unused.len().min(5)].join(", "),
                        if more { ", ..." } else { "" }
                    )
                }
            )));
        }
    }
    Ok(loaded)
}

struct Bundler<'f> {
    files: &'f [(String, Vec<u8>)],
    main: String,
    /// File path → its key under [`BUNDLED_KEY`].
    keys: HashMap<String, String>,
    taken: BTreeSet<String>,
    /// Parsed files waiting for their references to be rewritten.
    pending: HashMap<String, Value>,
    queue: VecDeque<String>,
    missing: Vec<String>,
    urls: Vec<String>,
    unreadable: Vec<(String, String)>,
    over_limit: bool,
}

impl Bundler<'_> {
    /// The given file `path` names: exactly, else the only file with that file name.
    fn find(&self, path: &str) -> Option<String> {
        if self.files.iter().any(|(n, _)| n == path) {
            return Some(path.to_owned());
        }
        let name = base_name(path);
        let mut same = self.files.iter().filter(|(n, _)| base_name(n) == name);
        match (same.next(), same.next()) {
            (Some((n, _)), None) => Some(n.clone()),
            _ => None,
        }
    }

    /// `path` relative to the main file's folder, for messages and keys.
    fn shown(&self, path: &str) -> String {
        let dir = dir_of(&self.main);
        if dir.is_empty() {
            return path.to_owned();
        }
        match path.strip_prefix(&format!("{dir}/")) {
            Some(rest) => rest.to_owned(),
            None => {
                let ups = dir.split('/').count();
                format!("{}{path}", "../".repeat(ups))
            }
        }
    }

    fn key_for(&mut self, path: &str) -> String {
        let shown = self.shown(path);
        let mut key: String = shown
            .split('/')
            .map(|seg| if seg == ".." { "parent" } else { seg })
            .collect::<Vec<_>>()
            .join("_")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if key.is_empty() {
            key = "file".into();
        }
        let base = key.clone();
        let mut n = 2;
        while self.taken.contains(&key) {
            key = format!("{base}_{n}");
            n += 1;
        }
        self.taken.insert(key.clone());
        key
    }

    /// The key of the hoisted file `path`, hoisting it first; `None` if it cannot be read.
    fn hoist(&mut self, path: &str) -> Option<String> {
        if let Some(k) = self.keys.get(path) {
            return Some(k.clone());
        }
        if self.unreadable.iter().any(|(p, _)| p == path) {
            return None;
        }
        if self.keys.len() >= MAX_FILES {
            self.over_limit = true;
            return None;
        }
        let data = &self.files.iter().find(|(n, _)| n == path)?.1;
        let parsed = match text(data) {
            None => Err("not UTF-8 or UTF-16 text".to_owned()),
            Some(t) => parse_any(&t),
        };
        match parsed {
            Ok(v) => {
                let key = self.key_for(path);
                self.keys.insert(path.to_owned(), key.clone());
                self.pending.insert(path.to_owned(), v);
                self.queue.push_back(path.to_owned());
                Some(key)
            }
            Err(e) => {
                self.unreadable.push((path.to_owned(), e));
                None
            }
        }
    }

    /// The reference `r` of the file `file` as a pointer into the bundled document; `None`
    /// leaves it as it is.
    fn map_ref(&mut self, r: &str, file: &str) -> Option<String> {
        let (target, frag) = match r.split_once('#') {
            Some((t, f)) => (t, f),
            None => (r, ""),
        };
        if target.is_empty() {
            if file == self.main {
                return None;
            }
            let key = self.keys.get(file)?;
            return Some(format!("#/{BUNDLED_KEY}/{key}{frag}"));
        }
        if is_url(target) {
            if !self.urls.iter().any(|u| u == target) {
                self.urls.push(target.to_owned());
            }
            return None;
        }
        let path = join(file, &percent_decode(target));
        let Some(found) = self.find(&path) else {
            if !self.missing.contains(&path) {
                self.missing.push(path);
            }
            return None;
        };
        if found == self.main {
            return Some(format!("#{frag}"));
        }
        let key = self.hoist(&found)?;
        Some(format!("#/{BUNDLED_KEY}/{key}{frag}"))
    }

    fn rewrite(&mut self, v: &mut Value, file: &str) {
        match v {
            Value::Obj(entries) => {
                for (k, val) in entries.iter_mut() {
                    if k == "$ref"
                        && let Value::Str(r) = val
                    {
                        if let Some(new) = self.map_ref(r, file) {
                            *r = new;
                        }
                    } else {
                        self.rewrite(val, file);
                    }
                }
            }
            Value::Arr(items) => {
                for item in items {
                    self.rewrite(item, file);
                }
            }
            _ => {}
        }
    }

    fn losses(&self) -> Vec<Loss> {
        let mut out = Vec::new();
        for path in &self.missing {
            out.push(dropped(format!(
                "`{}` was not given; references to it are not resolved",
                self.shown(path)
            )));
        }
        for (path, e) in &self.unreadable {
            out.push(dropped(format!(
                "`{}` is not valid JSON or YAML ({e}); references to it are not resolved",
                self.shown(path)
            )));
        }
        if !self.urls.is_empty() {
            let shown: Vec<String> = self.urls.iter().take(5).map(|u| format!("`{u}`")).collect();
            out.push(dropped(format!(
                "references to URLs are not fetched ({}{}); download the files and add them",
                shown.join(", "),
                if self.urls.len() > 5 { ", ..." } else { "" }
            )));
        }
        if self.over_limit {
            out.push(dropped(format!(
                "more than {MAX_FILES} referenced files; references to the others are not \
                 resolved"
            )));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(clean("a/./b/../c"), "a/c");
        assert_eq!(clean("../x/../y"), "../y");
        assert_eq!(clean("a\\b"), "a/b");
        assert_eq!(
            join("api/openapi.yaml", "schemas/pet.yaml"),
            "api/schemas/pet.yaml"
        );
        assert_eq!(
            join("api/schemas/pet.yaml", "../common.yaml"),
            "api/common.yaml"
        );
        assert_eq!(join("openapi.yaml", "./a.yaml"), "a.yaml");
        assert_eq!(percent_decode("my%20file.yaml"), "my file.yaml");
        assert!(is_url("https://example.com/a.yaml"));
        assert!(!is_url("schemas/pet.yaml"));
    }
}
