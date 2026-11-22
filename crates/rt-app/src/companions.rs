// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Companion files for extensions (since wave 13): files of the collection folder that the
//! OpenCollection document does not own (snapshots, recordings, notes next to the requests).
//! Every shell exposes these three functions as `Backend.companions`; writes are staged in the
//! workspace and saved with the next `save`, like a request edit. Text only.
//!
//! Collection files (`*.yml`) and paths that could leave the folder are refused, so an
//! extension cannot change requests behind the document's back. Secret values are not checked
//! here; the commit's secret scan covers companion files like any other.

use crate::AppError;
use crate::workspace::Workspace;

/// Paths of the companion files below `prefix` (`""`: all; else a folder path, with or
/// without the trailing `/`), unsaved ones included, sorted. `*.yml` files are not listed.
pub fn list(workspace: &Workspace, prefix: &str) -> Vec<String> {
    let prefix = prefix.trim_end_matches('/');
    workspace.companion_paths(|p| {
        !p.ends_with(".yml")
            && (prefix.is_empty()
                || p.strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/')))
    })
}

/// The text of a companion file (unsaved first); `None` if there is none. Bytes that are not
/// UTF-8 are replaced.
pub fn read(workspace: &Workspace, path: &str) -> Option<String> {
    if path.ends_with(".yml") {
        return None;
    }
    workspace
        .companion_file(path)
        .map(|b| String::from_utf8_lossy(b).into_owned())
}

/// Stages `text` as the companion file `path` for the next save (dirty until then).
pub fn stage(workspace: &mut Workspace, path: &str, text: &str) -> Result<(), AppError> {
    workspace.stage_companion(path, text.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_format::FileSet;

    fn workspace() -> Workspace {
        let files = [
            (
                "opencollection.yml",
                "opencollection: 1.0.0\ninfo:\n  name: C\n",
            ),
            ("orders/folder.yml", "info:\n  name: Orders\n"),
            ("orders/metadata.xml", "<edmx/>"),
            ("orders-old/notes.md", "x"),
            ("README.md", "readme"),
        ];
        Workspace::open(FileSet {
            files: files
                .iter()
                .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
                .collect(),
        })
        .unwrap()
    }

    #[test]
    fn lists_reads_and_stages_companion_files_only() {
        let mut ws = workspace();
        assert_eq!(
            list(&ws, ""),
            ["README.md", "orders-old/notes.md", "orders/metadata.xml"]
        );
        assert_eq!(list(&ws, "orders/"), ["orders/metadata.xml"]);
        assert_eq!(read(&ws, "orders/metadata.xml").as_deref(), Some("<edmx/>"));
        assert_eq!(read(&ws, "orders/folder.yml"), None);
        assert_eq!(read(&ws, "missing.xml"), None);

        stage(&mut ws, "orders/metadata.xml", "<edmx v=\"2\"/>").unwrap();
        stage(&mut ws, "orders/trace.odtrace", "{}").unwrap();
        assert_eq!(
            read(&ws, "orders/metadata.xml").as_deref(),
            Some("<edmx v=\"2\"/>")
        );
        assert_eq!(
            list(&ws, "orders"),
            ["orders/metadata.xml", "orders/trace.odtrace"]
        );
        assert_eq!(ws.dirty(), ["orders/metadata.xml", "orders/trace.odtrace"]);

        for bad in [
            "orders/folder.yml",
            "../x.xml",
            "/abs.xml",
            ".git/config",
            "a//b",
        ] {
            assert!(stage(&mut ws, bad, "x").is_err(), "{bad}");
        }
        let saved = ws.save().unwrap();
        assert_eq!(saved.len(), 2);
        assert!(ws.dirty().is_empty());
    }
}
