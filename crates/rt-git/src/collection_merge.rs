// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! RequestTailor's file merger (seam N1): every API collection of the repository (a folder
//! with `opencollection.yml`, outside hidden folders and `node_modules`) merges on the model
//! with `rt_format::merge` (the same merge as `rt merge-driver` and the web app). Its files
//! are those of `FileSet::from_dir`; nested collections merge as groups of their own.

use std::collections::{BTreeMap, BTreeSet};

use rt_app::merges::FileSet;
use tk_git_types::{FileMerger, GroupFiles, GroupMerge};

/// The file that makes a folder a collection.
const COLLECTION_FILE: &str = "opencollection.yml";

/// Merges each collection on the model.
#[derive(Debug, Clone, Copy, Default)]
pub struct CollectionMerger;

/// A group's files as `FileSet::from_dir` reads them: text only, without nested collections.
fn file_set(files: &GroupFiles) -> FileSet {
    let mut text: BTreeMap<String, String> = files
        .iter()
        .filter_map(|(p, b)| Some((p.clone(), String::from_utf8(b.clone()).ok()?)))
        .collect();
    rt_app::git::drop_nested(&mut text);
    FileSet {
        files: text.into_iter().map(|(p, t)| (p, t.into_bytes())).collect(),
    }
}

impl FileMerger for CollectionMerger {
    fn groups(&self, paths: &[&str]) -> BTreeSet<String> {
        let suffix = format!("/{COLLECTION_FILE}");
        paths
            .iter()
            .filter_map(|&path| {
                if path == COLLECTION_FILE {
                    Some("")
                } else {
                    path.strip_suffix(&suffix)
                }
            })
            .filter(|dir| {
                !dir.split('/')
                    .any(|part| part.starts_with('.') || part == "node_modules")
            })
            .map(str::to_owned)
            .collect()
    }

    fn claims(&self, rel: &str) -> bool {
        rt_app::git::tree_path(rel)
    }

    fn merge(
        &self,
        base: &GroupFiles,
        ours: &GroupFiles,
        theirs: &GroupFiles,
    ) -> Option<GroupMerge> {
        let (b, o, t) = (file_set(base), file_set(ours), file_set(theirs));
        let (result, marked) = rt_app::merges::pull_merge(&b, &o, &t).ok()?;
        let paths = b
            .files
            .keys()
            .chain(o.files.keys())
            .chain(t.files.keys())
            .cloned()
            .collect();
        let mut conflicts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for c in &result.conflicts {
            let part = match c.field.join(".") {
                f if f.is_empty() => "file".to_owned(),
                f => f,
            };
            conflicts.entry(c.file.clone()).or_default().push(part);
        }
        Some(GroupMerge {
            paths,
            files: result.files.files,
            marked,
            conflicts,
            whole_file: result.text_conflicts.into_iter().collect(),
        })
    }

    fn changed_parts(&self, base: &GroupFiles, theirs: &GroupFiles) -> BTreeMap<String, u32> {
        let mut out: BTreeMap<String, u32> = BTreeMap::new();
        if let Ok(diff) = rt_app::diffs::diff(&file_set(base), &file_set(theirs)) {
            for change in diff.changes {
                let n = u32::try_from(change.fields.len().max(1)).unwrap_or(u32::MAX);
                *out.entry(change.file).or_default() += n;
            }
        }
        out
    }
}
