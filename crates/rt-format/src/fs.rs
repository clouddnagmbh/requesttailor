// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reading and writing a [`FileSet`] on disk. Native only; everything else works in memory.

use std::io;
use std::path::{Component, Path, PathBuf};

use crate::{COLLECTION_FILE, FileSet};

/// File extension of OpenCollection files.
const EXTENSION: &str = "yml";

/// Directories that never belong to a collection.
fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules"
}

pub(crate) fn from_dir(root: &Path) -> io::Result<FileSet> {
    let mut set = FileSet::default();
    walk(root, "", &mut set)?;
    Ok(set)
}

fn walk(dir: &Path, prefix: &str, set: &mut FileSet) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        // Symbolic links are not followed: they could point outside the collection.
        let kind = entry.file_type()?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if kind.is_dir() {
            // A folder with its own collection file is a separate (nested) collection.
            if !skip_dir(&name) && !entry.path().join(COLLECTION_FILE).is_file() {
                walk(&entry.path(), &rel, set)?;
            }
        } else if kind.is_file() && Path::new(&name).extension().is_some_and(|e| e == EXTENSION) {
            set.files.insert(rel, std::fs::read(entry.path())?);
        }
    }
    Ok(())
}

pub(crate) fn write_to_dir(set: &FileSet, root: &Path) -> io::Result<()> {
    for (rel, bytes) in &set.files {
        let path = resolve(root, rel)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    Ok(())
}

/// Joins a `/`-separated relative path onto `root`, rejecting anything that could leave it.
fn resolve(root: &Path, rel: &str) -> io::Result<PathBuf> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid collection path {rel:?}"),
        )
    };
    if rel.is_empty() || rel.contains('\\') || rel.contains('\0') {
        return Err(invalid());
    }
    let mut path = root.to_path_buf();
    for part in rel.split('/') {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(c)), None) => path.push(c),
            _ => return Err(invalid()),
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escaping_paths() {
        let root = Path::new("/collection");
        for bad in [
            "",
            "../x.yml",
            "a/../../x.yml",
            "/abs.yml",
            "a//b.yml",
            "a\\b.yml",
            "./a.yml",
            "C:x.yml",
        ] {
            if cfg!(windows) || bad != "C:x.yml" {
                assert!(resolve(root, bad).is_err(), "{bad}");
            }
        }
        assert_eq!(
            resolve(root, "users/get.yml").unwrap(),
            root.join("users").join("get.yml")
        );
    }

    #[test]
    fn reads_and_writes_directories() {
        let dir = std::env::temp_dir().join(format!("rt-format-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut set = FileSet::default();
        for (p, c) in [
            ("opencollection.yml", "opencollection: 1.0.0\n"),
            ("users/folder.yml", "info:\n  name: users\n"),
            ("users/admin/get.yml", "info:\n  name: get\n"),
            ("environments/Local.yml", "name: Local\n"),
        ] {
            set.files.insert(p.into(), c.as_bytes().to_vec());
        }
        set.write_to_dir(&dir).unwrap();
        // Not part of the collection: other files, hidden and dependency folders, and a
        // nested collection.
        std::fs::write(dir.join("README.md"), "x").unwrap();
        for skipped in [".git", "node_modules", "nested"] {
            std::fs::create_dir_all(dir.join(skipped)).unwrap();
            std::fs::write(dir.join(skipped).join("x.yml"), "x").unwrap();
        }
        std::fs::write(dir.join("nested").join(COLLECTION_FILE), "x").unwrap();

        assert_eq!(FileSet::from_dir(&dir).unwrap(), set);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
