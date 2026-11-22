// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Editing a [`CollectionDoc`]: requests and environments. The YAML edits themselves are in
//! the `write` module; this module finds the files, names new ones and keeps the model in
//! step with the files.
//!
//! Owner: wave 3, strand F.

use std::collections::BTreeMap;

use rt_core::model::{Environment, Item, RequestSpec};

use crate::yaml::{self, ItemOp, Node, Val};
use crate::{COLLECTION_FILE, CollectionDoc, FOLDER_FILE, FormatError, Source, read, write};

/// Where an environment is declared.
enum EnvLocation {
    /// `environments/<name>.yml`.
    File(String),
    /// Index in `config.environments` of `opencollection.yml`.
    Inline(usize),
}

fn invalid(path: &str, message: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        path: path.to_owned(),
        message: message.into(),
    }
}

impl CollectionDoc {
    pub(crate) fn edit_request(
        &mut self,
        path: &str,
        spec: &RequestSpec,
    ) -> Result<(), FormatError> {
        if self.model.request(path).is_none() {
            return Err(invalid(path, "no such request"));
        }
        let Some(Source::Yaml(doc)) = self.files.get(path) else {
            return Err(invalid(path, "the file could not be parsed"));
        };
        let edited = write::apply_request(doc, path, spec).map_err(|e| invalid(path, e))?;
        self.files.insert(path.to_owned(), Source::Yaml(edited));
        self.rebuild();
        Ok(())
    }

    pub(crate) fn new_request(
        &mut self,
        folder: &str,
        spec: &RequestSpec,
    ) -> Result<String, FormatError> {
        if !folder.is_empty() && !has_folder(&self.model.items, folder) {
            return Err(invalid(folder, "folder not found"));
        }
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{folder}/")
        };
        let path = unique_path(&prefix, &slug(&spec.name), |p| self.taken(p));
        let mut spec = spec.clone();
        if spec.seq.is_none() {
            spec.seq = Some(self.next_seq(folder));
        }
        let initial = yaml::parse(&write::new_request_text(&spec))
            .map_err(|e| invalid(&path, format!("internal error: {e}")))?;
        let doc = write::apply_request(&initial, &path, &spec).map_err(|e| invalid(&path, e))?;
        self.files.insert(path.clone(), Source::Yaml(doc));
        self.rebuild();
        Ok(path)
    }

    pub(crate) fn new_folder(&mut self, parent: &str, name: &str) -> Result<String, FormatError> {
        if !parent.is_empty() && !has_folder(&self.model.items, parent) {
            return Err(invalid(parent, "folder not found"));
        }
        if name.trim().is_empty() || name.chars().any(char::is_control) {
            return Err(invalid(parent, format!("invalid folder name {name:?}")));
        }
        let prefix = if parent.is_empty() {
            String::new()
        } else {
            format!("{parent}/")
        };
        let stem = file_name(name);
        let dir = (1..)
            .map(|n| match n {
                1 => format!("{prefix}{stem}"),
                n => format!("{prefix}{stem}-{n}"),
            })
            .find(|d| {
                let reserved = parent.is_empty() && d.eq_ignore_ascii_case(read::ENVIRONMENTS_DIR);
                !reserved && !self.dir_taken(d)
            })
            .expect("some suffix is free");
        let path = format!("{dir}/{FOLDER_FILE}");
        let text = write::new_folder_text(name, self.next_seq(parent));
        let doc = yaml::parse(&text).map_err(|e| invalid(&path, format!("internal error: {e}")))?;
        self.files.insert(path.clone(), Source::Yaml(doc));
        self.rebuild();
        let read_back = find_folder(&self.model.items, &dir).map(|f| f.name.clone());
        if read_back.as_deref() != Some(name) {
            self.files.remove(&path);
            self.rebuild();
            return Err(invalid(&path, "the new folder does not read back"));
        }
        Ok(dir)
    }

    /// Whether a file lies in the folder `dir` already, ignoring case.
    fn dir_taken(&self, dir: &str) -> bool {
        let prefix = format!("{}/", dir.to_lowercase());
        self.files
            .keys()
            .any(|k| k.to_lowercase().starts_with(&prefix))
    }

    pub(crate) fn edit_environment(&mut self, env: &Environment) -> Result<String, FormatError> {
        if env.name.trim().is_empty() || env.name.chars().any(char::is_control) {
            return Err(invalid(
                read::ENVIRONMENTS_DIR,
                format!("invalid environment name {:?}", env.name),
            ));
        }
        let (path, doc) = match self.find_environment(&env.name) {
            Some(EnvLocation::Inline(index)) => {
                let Some(Source::Yaml(doc)) = self.files.get(COLLECTION_FILE) else {
                    unreachable!("inline environments come from a parsed collection file");
                };
                let mut doc = doc.clone();
                let nl = doc.newline;
                let item = inline_environments(&mut doc)
                    .and_then(|seq| seq.items.get_mut(index))
                    .and_then(|i| i.value.block_map_mut())
                    .ok_or_else(|| {
                        invalid(
                            COLLECTION_FILE,
                            format!("environment {:?} cannot be edited here", env.name),
                        )
                    })?;
                write::apply_environment(item, env, "", nl);
                let doc = yaml::reparse(&doc).map_err(|e| invalid(COLLECTION_FILE, e))?;
                let node = doc
                    .root
                    .get("config")
                    .and_then(|c| c.get("environments"))
                    .and_then(|l| l.items().nth(index))
                    .ok_or_else(|| invalid(COLLECTION_FILE, "internal error: environment lost"))?;
                write::check_environment(node, env, "").map_err(|e| invalid(COLLECTION_FILE, e))?;
                (COLLECTION_FILE.to_owned(), doc)
            }
            Some(EnvLocation::File(path)) => {
                let Some(Source::Yaml(doc)) = self.files.get(&path) else {
                    unreachable!("environment files are found among parsed files");
                };
                let stem = read::file_stem(&path).to_owned();
                let doc =
                    write::apply_environment_doc(doc, env, &stem).map_err(|e| invalid(&path, e))?;
                (path, doc)
            }
            None => {
                let prefix = format!("{}/", read::ENVIRONMENTS_DIR);
                let path = unique_path(&prefix, &file_name(&env.name), |p| self.taken(p));
                let stem = read::file_stem(&path).to_owned();
                let initial = yaml::parse(&write::new_environment_text(&env.name))
                    .map_err(|e| invalid(&path, format!("internal error: {e}")))?;
                let doc = write::apply_environment_doc(&initial, env, &stem)
                    .map_err(|e| invalid(&path, e))?;
                (path, doc)
            }
        };
        self.files.insert(path.clone(), Source::Yaml(doc));
        self.rebuild();
        Ok(path)
    }

    pub(crate) fn delete_environment(&mut self, name: &str) -> Result<(), FormatError> {
        match self.find_environment(name) {
            Some(EnvLocation::File(path)) => {
                self.files.remove(&path);
            }
            Some(EnvLocation::Inline(index)) => {
                let Some(Source::Yaml(doc)) = self.files.get(COLLECTION_FILE) else {
                    unreachable!("inline environments come from a parsed collection file");
                };
                let mut doc = doc.clone();
                let nl = doc.newline;
                let cannot = || {
                    invalid(
                        COLLECTION_FILE,
                        format!("environment {name:?} cannot be removed here"),
                    )
                };
                let config = doc
                    .root
                    .block_map_mut()
                    .and_then(|r| r.get_mut("config"))
                    .and_then(Node::block_map_mut)
                    .ok_or_else(cannot)?;
                let count = config.get("environments").map_or(0, |l| l.items().count());
                if count <= 1 {
                    config.put("environments", &Val::Seq(Vec::new()), &[], false, nl);
                } else {
                    let seq = config
                        .get_mut("environments")
                        .and_then(Node::block_seq_mut)
                        .ok_or_else(cannot)?;
                    let keep: Vec<ItemOp<'_>> = (0..count)
                        .filter(|i| *i != index)
                        .map(ItemOp::Keep)
                        .collect();
                    seq.rebuild(&keep, nl);
                }
                let doc = yaml::reparse(&doc).map_err(|e| invalid(COLLECTION_FILE, e))?;
                self.files
                    .insert(COLLECTION_FILE.to_owned(), Source::Yaml(doc));
            }
            None => {
                return Err(invalid(
                    &format!("{}/{name}.yml", read::ENVIRONMENTS_DIR),
                    "environment not found",
                ));
            }
        }
        self.rebuild();
        Ok(())
    }

    /// Where the environment `name` is declared. Inline environments come first, as in the
    /// model, so this finds the one `Collection::environment` finds.
    fn find_environment(&self, name: &str) -> Option<EnvLocation> {
        if let Some(Source::Yaml(doc)) = self.files.get(COLLECTION_FILE)
            && let Some(list) = doc.root.get("config").and_then(|c| c.get("environments"))
            && let Some(i) = list
                .items()
                .position(|e| read::environment(e, "").name == name)
        {
            return Some(EnvLocation::Inline(i));
        }
        let prefix = format!("{}/", read::ENVIRONMENTS_DIR);
        self.files.iter().find_map(|(path, source)| {
            let rest = path.strip_prefix(&prefix)?;
            let Source::Yaml(doc) = source else {
                return None;
            };
            let found = !rest.contains('/')
                && read::environment(&doc.root, read::file_stem(path)).name == name;
            found.then(|| EnvLocation::File(path.clone()))
        })
    }

    /// The `seq` after the last item (request, other item or folder) directly in `folder`.
    fn next_seq(&self, folder: &str) -> f64 {
        let mut max: Option<f64> = None;
        for (path, source) in &self.files {
            let Source::Yaml(doc) = source else { continue };
            let dir = read::dir_of(path);
            let file = path.rsplit('/').next().unwrap_or(path);
            let direct = dir == folder
                && file != COLLECTION_FILE
                && file != FOLDER_FILE
                && !(folder.is_empty() && dir == read::ENVIRONMENTS_DIR);
            let subfolder = file == FOLDER_FILE && dir != folder && read::dir_of(dir) == folder;
            if !(direct || subfolder) {
                continue;
            }
            if let Some(seq) = doc
                .root
                .get("info")
                .and_then(|i| i.get("seq"))
                .and_then(Node::as_f64)
                .filter(|s| s.is_finite())
            {
                max = Some(max.map_or(seq, |m| m.max(seq)));
            }
        }
        max.map_or(1.0, |m| m.floor() + 1.0)
    }

    /// Whether a file of this path exists, ignoring case (see [`unique_path`]).
    fn taken(&self, path: &str) -> bool {
        let path = path.to_lowercase();
        self.files.keys().any(|k| k.to_lowercase() == path)
    }

    /// Rebuilds the model (and the issues) from the parsed files after an edit.
    fn rebuild(&mut self) {
        let docs: BTreeMap<&str, &yaml::Document> = self
            .files
            .iter()
            .filter_map(|(path, source)| match source {
                Source::Yaml(doc) => Some((path.as_str(), doc)),
                Source::Unparsed(_) => None,
            })
            .collect();
        let mut builder = read::Builder::new(&docs);
        let model = builder.collection();
        let mut issues: Vec<FormatError> = self
            .parse_issues
            .iter()
            .filter(|e| match e {
                FormatError::Invalid { path, .. } => self.files.contains_key(path),
                _ => true,
            })
            .cloned()
            .collect();
        issues.extend(builder.issues);
        self.model = model;
        self.issues = issues;
    }
}

/// The `config.environments` list of a collection file, if it is a block sequence.
fn inline_environments(doc: &mut yaml::Document) -> Option<&mut yaml::Seq> {
    doc.root
        .block_map_mut()?
        .get_mut("config")?
        .block_map_mut()?
        .get_mut("environments")?
        .block_seq_mut()
}

fn find_folder<'a>(items: &'a [Item], path: &str) -> Option<&'a rt_core::model::Folder> {
    items.iter().find_map(|i| match i {
        Item::Folder(f) if f.path == path => Some(f),
        Item::Folder(f) => find_folder(&f.items, path),
        Item::Request(_) => None,
    })
}

fn has_folder(items: &[Item], path: &str) -> bool {
    items.iter().any(|i| match i {
        Item::Folder(f) => f.path == path || has_folder(&f.items, path),
        Item::Request(_) => false,
    })
}

/// Names Windows does not allow as file names (with any extension).
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Maximum length of a generated file stem, in characters.
const MAX_STEM: usize = 64;

/// A file stem for a request name: lower-case letters and digits, `-` for everything else.
pub(crate) fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out: String = out.trim_matches('-').chars().take(MAX_STEM).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("request");
    }
    not_reserved(out)
}

/// A file stem for an environment: its name, with characters that are not allowed in file
/// names (on any platform) replaced by `-`.
pub(crate) fn file_name(name: &str) -> String {
    let out: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .take(MAX_STEM)
        .collect();
    let out = out.trim_end_matches(['.', ' ']);
    if out.is_empty() || out.starts_with('.') {
        return not_reserved(format!("env{out}"));
    }
    not_reserved(out.to_owned())
}

fn not_reserved(stem: String) -> String {
    let lower = stem.to_lowercase();
    if RESERVED_NAMES.contains(&lower.as_str()) || lower == "opencollection" || lower == "folder" {
        format!("{stem}-1")
    } else {
        stem
    }
}

/// `<prefix><stem>.yml`, or with `-2`, `-3`, ... before `.yml`, whichever is not `taken`.
/// Case-insensitive file systems (macOS, Windows) make `A.yml` and `a.yml` the same file, so
/// names are compared ignoring case.
fn unique_path(prefix: &str, stem: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                format!("{prefix}{stem}.yml")
            } else {
                format!("{prefix}{stem}-{n}.yml")
            }
        })
        .find(|p| !taken(p))
        .expect("some suffix is free")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Get User"), "get-user");
        assert_eq!(slug("  Create  user (v2)! "), "create-user-v2");
        assert_eq!(slug("Über"), "über");
        assert_eq!(slug("???"), "request");
        assert_eq!(slug("CON"), "con-1");
        assert_eq!(slug("folder"), "folder-1");
        assert_eq!(slug(&"x".repeat(100)).len(), MAX_STEM);
    }

    #[test]
    fn environment_file_names() {
        assert_eq!(file_name("Local"), "Local");
        assert_eq!(file_name("a/b:c"), "a-b-c");
        assert_eq!(file_name(".hidden"), "env.hidden");
        assert_eq!(file_name("nul"), "nul-1");
    }

    #[test]
    fn unique_paths() {
        let taken = ["a.yml", "a-2.yml"];
        assert_eq!(unique_path("", "a", |p| taken.contains(&p)), "a-3.yml");
        assert_eq!(unique_path("x/", "a", |p| taken.contains(&p)), "x/a.yml");
    }
}
