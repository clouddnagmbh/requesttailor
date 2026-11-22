// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Merging one file: on the model where the file is a collection file the writer can edit,
//! line by line otherwise.
//!
//! # On the model
//!
//! Both sides' models are merged field by field ([`super::fields`]). The result is written
//! into one side's file (the *host*) with the format-preserving writer, so whatever the model
//! does not hold (docs, examples, comments, unknown fields, layout) comes from the host. That
//! is only right if the *other* side changed nothing outside the model: writing the base's
//! model into the other side's file must give the base file byte for byte. If neither side
//! qualifies, or the writer cannot express the merged model (it reads every edit back), the
//! file is merged line by line instead.

use rt_core::model::{Environment, Folder, Item, RequestSpec, Script};
use serde_json::{Value, json};

use super::fields::{Merger, RawConflict};
use super::{ConflictChoice, Side, text};
use crate::diff::ChangeTarget;
use crate::read::{self, Builder};
use crate::yaml::{self, Document};
use crate::{COLLECTION_FILE, FOLDER_FILE};

/// One side's version of a file: its path (sides may differ after a rename) and bytes.
#[derive(Clone, Copy)]
pub(crate) struct Version<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    Collection,
    /// A folder's `folder.yml`; the folder's path.
    Folder(String),
    Environment,
    Request,
    /// Not a collection file (or not one of this collection): merged line by line.
    Other,
}

pub(crate) fn kind(path: &str) -> Kind {
    if path == COLLECTION_FILE {
        return Kind::Collection;
    }
    if !path.ends_with(".yml") {
        return Kind::Other;
    }
    if let Some(dir) = path.strip_suffix(&format!("/{FOLDER_FILE}")) {
        return Kind::Folder(dir.to_owned());
    }
    if path == FOLDER_FILE {
        return Kind::Other;
    }
    match path.strip_prefix(&format!("{}/", read::ENVIRONMENTS_DIR)) {
        Some(rest) if !rest.contains('/') => Kind::Environment,
        _ => Kind::Request,
    }
}

/// The outcome of merging one file.
#[derive(Debug, Clone, Default)]
pub(crate) struct FileMerge {
    /// The merged file (`None`: deleted). Conflicted fields hold ours; a text conflict holds
    /// ours as a whole.
    pub bytes: Option<Vec<u8>>,
    pub conflicts: Vec<RawConflict>,
    pub text_conflict: bool,
    /// A text conflict: the line merge with Git's conflict markers, for the working tree.
    pub marked: Option<String>,
}

impl FileMerge {
    fn take(bytes: Option<&[u8]>) -> Self {
        Self {
            bytes: bytes.map(<[u8]>::to_vec),
            ..Self::default()
        }
    }
}

fn parse(bytes: &[u8]) -> Option<Document> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| yaml::parse(s).ok())
}

/// A stand-in collection file, for reading a folder file on its own.
fn stand_in() -> Document {
    yaml::parse("opencollection: 1.0.0\ninfo:\n  name: merge\n").expect("valid YAML")
}

fn find_folder<'a>(items: &'a [Item], path: &str) -> Option<&'a Folder> {
    items.iter().find_map(|i| match i {
        Item::Folder(f) if f.path == path => Some(f),
        Item::Folder(f) => find_folder(&f.items, path),
        Item::Request(_) => None,
    })
}

/// The model the file at `path` holds, as JSON (see [`Merger`] for the shapes).
pub(crate) fn read_model(kind: &Kind, path: &str, doc: &Document) -> Option<Value> {
    match kind {
        Kind::Request => {
            let docs = std::collections::BTreeMap::from([(path, doc)]);
            let spec = Builder::new(&docs).request(path)?.spec;
            serde_json::to_value(spec).ok()
        }
        Kind::Environment => {
            if !doc.root.is_map() {
                return None;
            }
            serde_json::to_value(read::environment(&doc.root, read::file_stem(path))).ok()
        }
        Kind::Folder(dir) => {
            let col = stand_in();
            let docs = std::collections::BTreeMap::from([(COLLECTION_FILE, &col), (path, doc)]);
            let model = Builder::new(&docs).collection();
            let f = find_folder(&model.items, dir)?;
            Some(json!({"name": f.name, "seq": f.seq, "defaults": f.defaults}))
        }
        Kind::Collection => {
            if !doc.root.is_map() {
                return None;
            }
            let docs = std::collections::BTreeMap::from([(COLLECTION_FILE, doc)]);
            let c = Builder::new(&docs).collection();
            Some(json!({
                "name": c.name,
                "defaults": c.defaults,
                "config": c.config,
                "environments": c.environments,
            }))
        }
        Kind::Other => None,
    }
}

/// `doc` (the file at `path`) edited so it holds `value`; `None` if the writer cannot.
pub(crate) fn apply_model(
    kind: &Kind,
    path: &str,
    doc: &Document,
    value: &Value,
) -> Option<String> {
    match kind {
        Kind::Request => {
            let spec: RequestSpec = serde_json::from_value(value.clone()).ok()?;
            crate::write::apply_request(doc, path, &spec)
                .ok()
                .map(|d| d.to_text())
        }
        Kind::Environment => {
            let env: Environment = serde_json::from_value(value.clone()).ok()?;
            crate::write::apply_environment_doc(doc, &env, read::file_stem(path))
                .ok()
                .map(|d| d.to_text())
        }
        // The writer edits only the scripts of folder and collection files.
        Kind::Folder(_) | Kind::Collection => {
            let current = read_model(kind, path, doc)?;
            let without_scripts = |v: &Value| {
                let mut v = v.clone();
                if let Some(d) = v.get_mut("defaults").and_then(Value::as_object_mut) {
                    d.remove("scripts");
                }
                v
            };
            if without_scripts(&current) != without_scripts(value) {
                return None;
            }
            let scripts: Vec<Script> =
                serde_json::from_value(value.get("defaults")?.get("scripts")?.clone()).ok()?;
            crate::runtime::apply_default_scripts(doc, *kind == Kind::Collection, &scripts)
                .ok()
                .map(|d| d.to_text())
        }
        Kind::Other => None,
    }
}

/// What a conflict in this file is about.
pub(crate) fn target(kind: &Kind, path: &str, docs: &[Option<&Document>]) -> ChangeTarget {
    match kind {
        Kind::Collection => ChangeTarget::Collection,
        Kind::Folder(dir) => ChangeTarget::Folder { path: dir.clone() },
        Kind::Environment => {
            let name = docs
                .iter()
                .flatten()
                .find(|d| d.root.is_map())
                .map(|d| read::environment(&d.root, read::file_stem(path)).name)
                .unwrap_or_else(|| read::file_stem(path).to_owned());
            ChangeTarget::Environment { name }
        }
        Kind::Request | Kind::Other => ChangeTarget::Request {
            path: path.to_owned(),
        },
    }
}

fn file_choice(choices: &[ConflictChoice], path: &str) -> Option<Side> {
    choices.iter().find_map(|c| match c {
        ConflictChoice::File { path: p, side } if p == path => Some(*side),
        _ => None,
    })
}

/// Merges one collection file. `path` is where the result goes (ours' path, or the moved
/// side's).
pub(crate) fn merge_file(
    path: &str,
    base: Option<Version<'_>>,
    ours: Option<Version<'_>>,
    theirs: Option<Version<'_>>,
    choices: &[ConflictChoice],
) -> (ChangeTarget, FileMerge) {
    merge_inner(path, kind(path), base, ours, theirs, choices)
}

/// Merges a file that is not a collection file, line by line.
pub(crate) fn merge_text(
    path: &str,
    base: Option<Version<'_>>,
    ours: Option<Version<'_>>,
    theirs: Option<Version<'_>>,
    choices: &[ConflictChoice],
) -> (ChangeTarget, FileMerge) {
    merge_inner(path, Kind::Other, base, ours, theirs, choices)
}

fn merge_inner(
    path: &str,
    kind: Kind,
    base: Option<Version<'_>>,
    ours: Option<Version<'_>>,
    theirs: Option<Version<'_>>,
    choices: &[ConflictChoice],
) -> (ChangeTarget, FileMerge) {
    let (b, o, t) = (
        base.map(|v| v.bytes),
        ours.map(|v| v.bytes),
        theirs.map(|v| v.bytes),
    );
    let docs: Vec<Option<Document>> = [ours, theirs, base]
        .iter()
        .map(|v| v.and_then(|v| parse(v.bytes)))
        .collect();
    let refs: Vec<Option<&Document>> = docs.iter().map(Option::as_ref).collect();
    let target = target(&kind, path, &refs);
    if o == t || b == t {
        return (target, FileMerge::take(o));
    }
    if b == o {
        return (target, FileMerge::take(t));
    }
    match file_choice(choices, path) {
        Some(Side::Ours) => return (target, FileMerge::take(o)),
        Some(Side::Theirs) => return (target, FileMerge::take(t)),
        None => {}
    }
    let (Some(ov), Some(tv)) = (ours, theirs) else {
        // Deleted on one side, changed on the other: a decision about the whole file.
        let mut m = FileMerge::take(o);
        m.text_conflict = true;
        return (target, m);
    };
    if kind != Kind::Other
        && let Some((bytes, conflicts)) = model_merge(&kind, path, base, ov, tv, &target, choices)
    {
        let m = FileMerge {
            bytes: Some(bytes),
            conflicts,
            ..FileMerge::default()
        };
        return (target, m);
    }
    let texts = (
        b.map(std::str::from_utf8).unwrap_or(Ok("")),
        std::str::from_utf8(ov.bytes),
        std::str::from_utf8(tv.bytes),
    );
    let (Ok(bt), Ok(ot), Ok(tt)) = texts else {
        let mut m = FileMerge::take(o);
        m.text_conflict = true;
        return (target, m);
    };
    let merged = text::merge_lines(bt, ot, tt);
    if merged.clean {
        return (target, FileMerge::take(Some(merged.text.as_bytes())));
    }
    let mut m = FileMerge::take(o);
    m.text_conflict = true;
    m.marked = Some(merged.text);
    (target, m)
}

/// The merge on the model, or `None` where the file has to be merged line by line.
fn model_merge(
    kind: &Kind,
    path: &str,
    base: Option<Version<'_>>,
    ours: Version<'_>,
    theirs: Version<'_>,
    target: &ChangeTarget,
    choices: &[ConflictChoice],
) -> Option<(Vec<u8>, Vec<RawConflict>)> {
    let (od, td) = (parse(ours.bytes)?, parse(theirs.bytes)?);
    let (om, tm) = (
        read_model(kind, ours.path, &od)?,
        read_model(kind, theirs.path, &td)?,
    );
    let based = match base {
        Some(b) => {
            let d = parse(b.bytes)?;
            let m = read_model(kind, b.path, &d)?;
            Some((b, m))
        }
        None => None,
    };
    let mut merger = Merger::new(target, choices);
    let bm = based.as_ref().map(|(_, m)| m);
    let merged = match kind {
        Kind::Request => merger.request(bm, &om, &tm),
        Kind::Environment => merger.environment(bm, &om, &tm),
        Kind::Folder(_) | Kind::Collection => merger.container(bm, &om, &tm),
        Kind::Other => None,
    }?;
    // Everything the model does not hold (docs, comments, layout) comes from the files: the
    // merged model is written into all three versions, and those, which now differ only
    // outside the model, are merged line by line. The result must read as the merged model.
    let ro = apply_model(kind, path, &od, &merged);
    let rt = apply_model(kind, path, &td, &merged);
    if let (Some(ro), Some(rt)) = (ro, rt) {
        if ro == rt {
            return Some((ro.into_bytes(), merger.conflicts));
        }
        let want = read_model(kind, path, &yaml::parse(&ro).ok()?);
        if let Some((b, _)) = &based
            && let Some(bd) = parse(b.bytes)
            && let Some(rb) = apply_model(kind, path, &bd, &merged)
        {
            let lines = text::merge_lines(&rb, &ro, &rt);
            if lines.clean
                && let Ok(doc) = yaml::parse(&lines.text)
                && read_model(kind, path, &doc) == want
            {
                return Some((lines.text.into_bytes(), merger.conflicts));
            }
        }
    }
    // Otherwise one side's file can host the result if the other side changed nothing
    // outside the model: writing the base's model into it gives the base file.
    let same = |v: Version<'_>, d: &Document, model: &Value, want: &[u8]| {
        apply_model(kind, v.path, d, model).is_some_and(|t| t.as_bytes() == want)
    };
    let host = match &based {
        Some((b, bm)) if same(theirs, &td, bm, b.bytes) => &od,
        Some((b, bm)) if same(ours, &od, bm, b.bytes) => &td,
        Some(_) => return None,
        None if same(theirs, &td, &om, ours.bytes) => &od,
        None if same(ours, &od, &tm, theirs.bytes) => &td,
        None => return None,
    };
    let text = apply_model(kind, path, host, &merged)?;
    Some((text.into_bytes(), merger.conflicts))
}

/// Whether `b` (at `bp`) is `a` (at `ap`) moved: the same bytes, or for requests and
/// environments the same model apart from the name (and `seq`).
pub(crate) fn same_content(a: Version<'_>, b: Version<'_>) -> bool {
    if a.bytes == b.bytes {
        return true;
    }
    let ka = kind(a.path);
    if ka != kind(b.path) || !matches!(ka, Kind::Request | Kind::Environment) {
        return false;
    }
    let model = |v: Version<'_>| {
        let mut m = read_model(&ka, v.path, &parse(v.bytes)?)?;
        let o = m.as_object_mut()?;
        o.remove("name");
        o.remove("seq");
        Some(m)
    };
    matches!((model(a), model(b)), (Some(x), Some(y)) if x == y)
}
