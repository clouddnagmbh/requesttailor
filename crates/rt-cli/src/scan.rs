// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt scan [<path>...] [--json]` (wave 5, strand E): the secret scanner of
//! `rt_app::secret_scan` for pipelines and as a Git pre-commit hook. Exit code 0 when clean,
//! 1 with findings, 2 when a path cannot be read.
//!
//! Folders are scanned recursively (without `.git` and `node_modules`); binary files, files
//! that are not UTF-8 and files over 4 MiB are skipped, as in the apps' commit check.

use std::io::Write;
use std::path::{Path, PathBuf};

use rt_app::secret_scan::{SecretFinding, scan, text_of};

/// Exit code when credentials were found.
pub const EXIT_FOUND: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanArgs {
    /// Files or folders; empty: the current folder.
    pub paths: Vec<PathBuf>,
    /// `--json`: print `{"scanned": n, "findings": [...]}` instead of text.
    pub json: bool,
}

/// `Ok(None)` for `--help`.
pub(crate) fn parse(parser: &mut lexopt::Parser) -> Result<Option<ScanArgs>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut paths = Vec::new();
    let mut json = false;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("json") => json = true,
            Value(v) => paths.push(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(ScanArgs { paths, json }))
}

/// Folders that are never scanned.
fn skipped_dir(name: &str) -> bool {
    matches!(name, ".git" | "node_modules")
}

/// The files below `path` (or `path` itself), sorted.
fn files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !path.is_dir() {
        // A missing file is an error, reported when it is read.
        out.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for entry in entries {
        let name = entry
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if entry.is_dir() {
            if !skipped_dir(&name) {
                files(&entry, out)?;
            }
        } else if entry.is_file() {
            out.push(entry);
        }
    }
    Ok(())
}

/// How a path is shown: as given, `/`-separated, without a leading `./`.
fn shown(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    s.strip_prefix("./").map(str::to_owned).unwrap_or(s)
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

pub fn run(args: ScanArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let roots = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths
    };
    let mut paths = Vec::new();
    for root in &roots {
        if let Err(e) = files(root, &mut paths) {
            let _ = writeln!(stderr, "rt: scan: cannot read {}: {e}", root.display());
            return crate::EXIT_USAGE;
        }
    }
    let mut findings: Vec<SecretFinding> = Vec::new();
    let mut scanned = 0;
    for path in &paths {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                let _ = writeln!(stderr, "rt: scan: cannot read {}: {e}", path.display());
                return crate::EXIT_USAGE;
            }
        };
        let Some(text) = text_of(&bytes) else {
            continue;
        };
        scanned += 1;
        findings.extend(scan(&[(&shown(path), text)]));
    }
    if args.json {
        let out = serde_json::json!({ "scanned": scanned, "findings": findings });
        let _ = writeln!(stdout, "{out}");
    } else {
        for f in &findings {
            let _ = writeln!(
                stdout,
                "{}:{}: {}\n    {}\n    {}\n",
                f.path, f.line, f.kind, f.snippet, f.hint
            );
        }
        if findings.is_empty() {
            let _ = writeln!(
                stdout,
                "No credentials found ({} scanned).",
                plural(scanned, "file")
            );
        } else {
            let mut files: Vec<&str> = findings.iter().map(|f| f.path.as_str()).collect();
            files.dedup();
            let _ = writeln!(
                stdout,
                "{} in {} ({} scanned). Move them into secret variables, or mark a line \
                 that is not a credential with `# rt-scan:allow`.",
                plural(findings.len(), "credential"),
                plural(files.len(), "file"),
                plural(scanned, "file")
            );
        }
    }
    let _ = stdout.flush();
    if findings.is_empty() {
        crate::EXIT_OK
    } else {
        EXIT_FOUND
    }
}
