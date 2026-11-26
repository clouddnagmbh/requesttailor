// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The commit graph of a whole repository (wave 14, strand F): every local and remote branch,
//! tag and `HEAD`, with the commits behind them, paged for the lanes view in the main area.
//!
//! Generic on purpose: nothing here assumes the repository is a collection (wave 15 opens any
//! repository). Paths are repository-relative with a leading `/` (the wave-12 convention for
//! "relative to the repository root"). A changed file that is a request of a collection in the
//! set names that collection ([`GitChangedFile::collection`]), so the UI shows the semantic
//! diff for it and the line diff for every other file.
//!
//! The lane layout is computed once in the UI (`apps/ui/src/git/graph/layout.ts`), the same for
//! every shell; the shells only list commits and refs (natively `rt-git/src/graph.rs`, in the
//! browser `apps/web/src/git/graph.ts`). Additions are additive (new fields default).

//!
//! The types live in the Tailor Kit (`tk_git_types::graph`); RequestTailor adds the collection
//! of each changed file ([`InCollection`]).

pub use tk_git_types::graph::*;

use super::InCollection;

/// A file a commit (or a range) changed, with the collection of the set it belongs to: a request
/// (or environment, folder, collection file) of `CollectionEntry::id`; `None`: any other file,
/// shown as a line diff.
pub type GitChangedFile = tk_git_types::graph::GitChangedFile<InCollection>;

/// A collection of the set by where it sits in the repository, for [`assign_collections`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionAt {
    /// [`CollectionEntry::id`](crate::collections::CollectionEntry::id).
    pub id: String,
    /// Its folder relative to the repository root: `""` at the root, else `a/b` (no slashes
    /// around).
    pub dir: String,
}

/// The collection whose file `path` (repository-relative, leading `/` optional) is: the
/// deepest collection folder that holds it, if the rest of the path is a collection file
/// ([`super::tree_path`]). Nested collections win over their parents, as they are separate
/// collections.
pub fn collection_of<'a>(path: &str, collections: &'a [CollectionAt]) -> Option<&'a str> {
    let path = path.strip_prefix('/').unwrap_or(path);
    collections
        .iter()
        .filter_map(|c| {
            let rest = if c.dir.is_empty() {
                Some(path)
            } else {
                path.strip_prefix(c.dir.as_str())
                    .and_then(|r| r.strip_prefix('/'))
            }?;
            Some((c, rest))
        })
        .max_by_key(|(c, _)| c.dir.len() + usize::from(!c.dir.is_empty()))
        .filter(|(_, rest)| super::tree_path(rest))
        .map(|(c, _)| c.id.as_str())
}

/// Fills [`InCollection::collection`] for every file that belongs to one of `collections`.
pub fn assign_collections(files: &mut [GitChangedFile], collections: &[CollectionAt]) {
    for f in files {
        f.extra.collection = collection_of(&f.path, collections).map(str::to_owned);
    }
}

/// The files the Git core lists ([`tk_git_types::graph::GitChangedFile`]) with the collection
/// each belongs to ([`assign_collections`]).
pub fn in_collections(
    files: Vec<tk_git_types::graph::GitChangedFile>,
    collections: &[CollectionAt],
) -> Vec<GitChangedFile> {
    let mut files: Vec<GitChangedFile> = files
        .into_iter()
        .map(|f| f.with_extra(InCollection::default()))
        .collect();
    assign_collections(&mut files, collections);
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(id: &str, dir: &str) -> CollectionAt {
        CollectionAt {
            id: id.into(),
            dir: dir.into(),
        }
    }

    #[test]
    fn files_belong_to_the_deepest_collection_that_holds_them() {
        let set = [
            at("root", ""),
            at("api", "apis/billing"),
            at("inner", "apis/billing/v2"),
        ];
        assert_eq!(collection_of("/users/get.yml", &set), Some("root"));
        assert_eq!(collection_of("/apis/billing/pay.yml", &set), Some("api"));
        assert_eq!(
            collection_of("apis/billing/v2/x/a.yml", &set),
            Some("inner")
        );
        // Not collection files: line diffs.
        assert_eq!(collection_of("/README.md", &set), None);
        assert_eq!(collection_of("/apis/billing/.github/ci.yml", &set), None);
        assert_eq!(
            collection_of("/apis/billing/node_modules/x.yml", &set),
            None
        );
        // `apis/billingx` is not inside `apis/billing`.
        let only = [at("api", "apis/billing")];
        assert_eq!(collection_of("/apis/billingx/a.yml", &only), None);
        assert_eq!(collection_of("/top.yml", &only), None);
    }

    #[test]
    fn assign_fills_the_field() {
        let mut files = vec![
            GitChangedFile {
                path: "/a.yml".into(),
                change: crate::git::GitFileChange::Added,
                from: None,
                binary: false,
                extra: InCollection { collection: None },
            },
            GitChangedFile {
                path: "/logo.png".into(),
                change: crate::git::GitFileChange::Added,
                from: None,
                binary: true,
                extra: InCollection {
                    collection: Some("stale".into()),
                },
            },
        ];
        assign_collections(&mut files, &[at("c", "")]);
        assert_eq!(files[0].extra.collection.as_deref(), Some("c"));
        assert_eq!(files[1].extra.collection, None);
    }
}
