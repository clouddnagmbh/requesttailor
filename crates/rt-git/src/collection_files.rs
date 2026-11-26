// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! RequestTailor's files of a scope (seam N3): the YAML files of the collection outside
//! hidden folders and `node_modules` (`rt_app::git::tree_path`), without the files of nested
//! collections (`rt_app::git::drop_nested`), as `rt_format::FileSet::from_dir` reads them.

use std::collections::BTreeMap;

use tk_git_types::ScopeFiles;

/// The files of a collection.
#[derive(Debug, Clone, Copy, Default)]
pub struct CollectionFiles;

impl ScopeFiles for CollectionFiles {
    fn includes(&self, rel: &str) -> bool {
        rt_app::git::tree_path(rel)
    }

    fn drop_nested(&self, files: &mut BTreeMap<String, String>) {
        rt_app::git::drop_nested(files);
    }
}
