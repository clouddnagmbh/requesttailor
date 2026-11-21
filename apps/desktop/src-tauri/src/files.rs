// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The desktop app's only file I/O: reading a collection folder into a [`FileSet`] and
//! writing the [`FileChange`]s that `Workspace::save` returns.
//!
//! # What is read
//!
//! Every regular file below the root, as bytes (collection YAML and body files referenced by
//! requests), except:
//!
//! - folders whose name starts with `.` (`.git`, `.idea`, ...) and `node_modules`;
//! - folders that contain their own `opencollection.yml` (a separate, nested collection);
//! - files whose name starts with `.`;
//! - files larger than [`MAX_FILE_BYTES`] (big binaries, build output);
//! - symbolic links (never followed, they could point outside the collection).
//!
//! A folder with more than [`MAX_FILES`] files or [`MAX_TOTAL_BYTES`] bytes (after skipping)
//! is rejected with a clear message instead of filling memory. The web shell applies the same
//! rules (`apps/web/src/fs/directory.ts`).
//!
//! # How changes are written
//!
//! Each file is written to a temporary file next to it, flushed to disk, then renamed over
//! the target, so a crash never leaves a half-written request file. Paths are relative and
//! `/`-separated; anything that could leave the root (`..`, absolute, `\`) is rejected.
//!
//! A change with empty `text` means the file was deleted (`Workspace::save` reports a deleted
//! environment that way): the file is removed; a file that is already gone is fine.

use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

use rt_app::FileChange;
use rt_format::{COLLECTION_FILE, FileSet};

/// Files above this size are skipped when reading.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// At most this many files are read.
pub const MAX_FILES: usize = 20_000;
/// At most this many bytes are read in total.
pub const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

/// Folders that never belong to a collection.
pub fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules"
}

/// Files that are never read.
pub fn skip_file(name: &str, size: u64) -> bool {
    name.starts_with('.') || size > MAX_FILE_BYTES
}

/// Reads the collection folder `root` (see the module documentation for the rules).
pub fn read_folder(root: &Path) -> io::Result<FileSet> {
    let mut set = FileSet::default();
    let mut total = 0u64;
    walk(root, "", &mut set, &mut total)?;
    Ok(set)
}

fn walk(dir: &Path, prefix: &str, set: &mut FileSet, total: &mut u64) -> io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        // `file_type` does not follow symbolic links, so links are neither files nor folders.
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
            if !skip_dir(&name) && !entry.path().join(COLLECTION_FILE).is_file() {
                walk(&entry.path(), &rel, set, total)?;
            }
        } else if kind.is_file() {
            let size = entry.metadata()?.len();
            if skip_file(&name, size) {
                continue;
            }
            if set.files.len() >= MAX_FILES {
                return Err(too_large(format!("more than {MAX_FILES} files")));
            }
            *total += size;
            if *total > MAX_TOTAL_BYTES {
                return Err(too_large(format!(
                    "more than {} MiB of files",
                    MAX_TOTAL_BYTES / 1024 / 1024
                )));
            }
            set.files.insert(rel, std::fs::read(entry.path())?);
        }
    }
    Ok(())
}

fn too_large(why: String) -> io::Error {
    io::Error::other(format!(
        "this folder is too large for a collection ({why}); open the collection's own folder"
    ))
}

/// Writes `changes` below `root`, each file atomically (temporary file, then rename); a change
/// with empty `text` deletes its file. Stops at the first error; files written before it stay
/// written.
pub fn write_changes(root: &Path, changes: &[FileChange]) -> io::Result<()> {
    // Validate everything first, so a bad path does not leave a partial save behind.
    let targets = changes
        .iter()
        .map(|c| resolve(root, &c.path))
        .collect::<io::Result<Vec<_>>>()?;
    for (change, target) in changes.iter().zip(targets) {
        if change.text.is_empty() {
            match std::fs::remove_file(&target) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        } else {
            write_atomic(&target, change.text.as_bytes())?;
        }
    }
    Ok(())
}

fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = target.parent().expect("resolved paths have a parent");
    std::fs::create_dir_all(parent)?;
    let name = target
        .file_name()
        .expect("resolved paths have a file name")
        .to_string_lossy();
    // Hidden, so a leftover after a crash is skipped by `read_folder`.
    let temp = parent.join(format!(".{name}.rt-tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Replaces an existing file on every platform (MoveFileExW with REPLACE_EXISTING on
        // Windows).
        std::fs::rename(&temp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Joins a `/`-separated relative path onto `root`, rejecting anything that could leave it.
pub fn resolve(root: &Path, rel: &str) -> io::Result<PathBuf> {
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
            (Some(Component::Normal(c)), None) if c.to_str() == Some(part) => path.push(c),
            _ => return Err(invalid()),
        }
    }
    Ok(path)
}

/// The collection id on this device: the canonical root path (`SecretKey::collection`).
/// On Windows the verbatim prefix `\\?\` is removed so the id reads like a normal path.
pub fn collection_id(root: &Path) -> io::Result<(PathBuf, String)> {
    let canonical = std::fs::canonicalize(root)?;
    let text = canonical.to_string_lossy();
    let id = text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned();
    Ok((canonical, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "rt-desktop-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn put(&self, rel: &str, bytes: &[u8]) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_collection_and_body_files_and_skips_the_rest() {
        let dir = TempDir::new("read");
        dir.put("opencollection.yml", b"opencollection: 1.0.0\n");
        dir.put("users/get.yml", b"info:\n  name: get\n");
        dir.put("environments/Local.yml", b"name: Local\n");
        dir.put("files/payload.bin", &[0, 1, 2, 255]);
        dir.put(".git/config", b"[core]\n");
        dir.put(".hidden/x.yml", b"x\n");
        dir.put("node_modules/pkg/index.yml", b"x\n");
        dir.put(".DS_Store", b"x");
        dir.put("nested/opencollection.yml", b"opencollection: 1.0.0\n");
        dir.put("nested/a.yml", b"x\n");
        let big = vec![b'x'; (MAX_FILE_BYTES + 1) as usize];
        dir.put("files/big.bin", &big);

        let set = read_folder(&dir.0).unwrap();
        let paths: Vec<&str> = set.files.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            [
                "environments/Local.yml",
                "files/payload.bin",
                "opencollection.yml",
                "users/get.yml"
            ]
        );
        assert_eq!(set.files["files/payload.bin"], [0, 1, 2, 255]);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symbolic_links() {
        let dir = TempDir::new("links");
        let outside = TempDir::new("links-outside");
        outside.put("secret.yml", b"x\n");
        dir.put("opencollection.yml", b"opencollection: 1.0.0\n");
        std::os::unix::fs::symlink(&outside.0, dir.0.join("linked-dir")).unwrap();
        std::os::unix::fs::symlink(outside.0.join("secret.yml"), dir.0.join("linked.yml")).unwrap();
        let set = read_folder(&dir.0).unwrap();
        assert_eq!(set.files.keys().collect::<Vec<_>>(), ["opencollection.yml"]);
    }

    #[test]
    fn writes_changes_atomically_and_creates_folders() {
        let dir = TempDir::new("write");
        dir.put("a.yml", b"old\n");
        let changes = vec![
            FileChange {
                path: "a.yml".into(),
                text: "new\n".into(),
            },
            FileChange {
                path: "users/admin/b.yml".into(),
                text: "b\n".into(),
            },
        ];
        write_changes(&dir.0, &changes).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.0.join("a.yml")).unwrap(),
            "new\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.0.join("users/admin/b.yml")).unwrap(),
            "b\n"
        );
        // No temporary files left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn empty_text_deletes_the_file() {
        let dir = TempDir::new("delete");
        dir.put("environments/Old.yml", b"name: Old\n");
        let changes = vec![
            FileChange {
                path: "environments/Old.yml".into(),
                text: String::new(),
            },
            // Already gone: not an error.
            FileChange {
                path: "environments/Gone.yml".into(),
                text: String::new(),
            },
        ];
        write_changes(&dir.0, &changes).unwrap();
        assert!(!dir.0.join("environments/Old.yml").exists());
        assert!(!dir.0.join("environments/Gone.yml").exists());
    }

    #[test]
    fn rejects_paths_outside_the_root_before_writing_anything() {
        let dir = TempDir::new("escape");
        for bad in [
            "",
            "../x.yml",
            "a/../../x.yml",
            "/abs.yml",
            "a//b.yml",
            "a\\b.yml",
            "./a.yml",
        ] {
            let changes = vec![
                FileChange {
                    path: "ok.yml".into(),
                    text: "ok\n".into(),
                },
                FileChange {
                    path: bad.into(),
                    text: "x".into(),
                },
            ];
            let err = write_changes(&dir.0, &changes).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad}");
            assert!(!dir.0.join("ok.yml").exists(), "{bad}");
        }
    }

    #[test]
    fn collection_id_is_the_canonical_path() {
        let dir = TempDir::new("id");
        dir.put("sub/x", b"");
        let (canonical, id) = collection_id(&dir.0.join("sub").join("..")).unwrap();
        assert_eq!(canonical, std::fs::canonicalize(&dir.0).unwrap());
        assert!(!id.starts_with(r"\\?\"));
        assert!(!id.contains(".."));
    }
}
