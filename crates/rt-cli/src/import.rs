// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt import <file>... [--format F] --out <dir> [--json]`: converts an export of another
//! tool (`rt_format::import`) into a new collection folder and prints the loss report.
//! Owner: wave 4, strand B.
//!
//! Several files form one collection: a Postman collection with its environment and globals
//! files, `.http` files with `http-client.env.json`, several cURL files. `--out` must not exist
//! or be an empty folder. Exit code 0 when the collection was written, even with losses.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rt_format::import::{
    Import, ImportFormat, LossEffect, LossKind, MAX_INPUT_BYTES, SourceFile, import_files,
};

use crate::{EXIT_OK, EXIT_USAGE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportArgs {
    pub files: Vec<PathBuf>,
    pub format: Option<ImportFormat>,
    pub out: PathBuf,
    pub json: bool,
}

pub(crate) fn parse(parser: &mut lexopt::Parser) -> Result<Option<ImportArgs>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut files = Vec::new();
    let mut format = None;
    let mut out = None;
    let mut json = false;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('f') | Long("format") => {
                let name = parser.value().map_err(err)?.string().map_err(err)?;
                format = Some(parse_format(&name)?);
            }
            Short('o') | Long("out") => out = Some(PathBuf::from(parser.value().map_err(err)?)),
            Long("json") => json = true,
            Value(v) => files.push(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if files.is_empty() {
        return Err("import: missing <file>".into());
    }
    let out = out.ok_or("import: missing --out <dir>")?;
    Ok(Some(ImportArgs {
        files,
        format,
        out,
        json,
    }))
}

fn parse_format(name: &str) -> Result<ImportFormat, String> {
    serde_json::from_value(serde_json::Value::String(name.to_ascii_lowercase())).map_err(|_| {
        let names: Vec<String> = ImportFormat::ALL
            .iter()
            .map(|f| {
                serde_json::to_value(f)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            })
            .collect();
        format!("unknown format {name:?}; one of {}", names.join(", "))
    })
}

/// Refuses an `--out` that exists and is not an empty folder.
fn check_out(out: &Path) -> Result<(), String> {
    match std::fs::read_dir(out) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(format!("{} already exists and is not empty", out.display()));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) if out.exists() => Err(format!("{} exists and is not a folder", out.display())),
        Err(e) => Err(format!("cannot use {}: {e}", out.display())),
    }
}

/// Writes the collection's files below `out`.
fn write(out: &Path, import: &Import) -> Result<(), String> {
    for (path, bytes) in &import.files.files {
        let rel = Path::new(path);
        // The importer writes plain relative paths; anything else would leave `out`.
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(format!("refusing to write unexpected path {path:?}"));
        }
        let target = out.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, bytes)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    }
    Ok(())
}

/// The readable report: what was imported, then the losses grouped by kind.
pub fn render_text(import: &Import, out: &Path) -> String {
    let r = &import.report;
    let plural = |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut s = format!(
        "Imported {} \"{}\" into {}\n  {}, {}, {}, {}\n",
        r.format,
        import.collection_name,
        out.display(),
        plural(r.requests, "request", "requests"),
        plural(r.folders, "folder", "folders"),
        plural(r.environments, "environment", "environments"),
        plural(import.files.files.len() as u32, "file", "files"),
    );
    if r.losses.is_empty() {
        s.push_str("\nNothing was lost: everything was imported exactly.\n");
        return s;
    }
    let dropped = r
        .losses
        .iter()
        .filter(|l| l.effect == LossEffect::Dropped)
        .count();
    s.push_str(&format!(
        "\nLoss report: {} ({dropped} dropped, {} approximated)\n",
        r.losses.len(),
        r.losses.len() - dropped
    ));
    for kind in LossKind::ALL {
        let group: Vec<_> = r.losses.iter().filter(|l| l.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        s.push_str(&format!("\n{} ({})\n", kind.label(), group.len()));
        for l in group {
            let effect = match l.effect {
                LossEffect::Dropped => "dropped     ",
                LossEffect::Approximated => "approximated",
            };
            s.push_str(&format!("  {effect}  {}: {}\n", l.location, l.detail));
        }
    }
    s
}

pub fn render_json(import: &Import, out: &Path) -> String {
    let value = serde_json::json!({
        "collectionName": import.collection_name,
        "out": out.display().to_string(),
        "files": import.files.files.keys().collect::<Vec<_>>(),
        "report": import.report,
    });
    let mut s = serde_json::to_string_pretty(&value).expect("report serializes");
    s.push('\n');
    s
}

pub(crate) fn run(args: ImportArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, msg: &str| {
        let _ = writeln!(stderr, "rt import: {msg}");
        EXIT_USAGE
    };
    let mut data = Vec::with_capacity(args.files.len());
    let mut total = 0usize;
    for path in &args.files {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return fail(stderr, &format!("cannot read {}: {e}", path.display())),
        };
        total = total.saturating_add(bytes.len());
        if total > MAX_INPUT_BYTES {
            return fail(
                stderr,
                &format!(
                    "the files are larger than {} MB together",
                    MAX_INPUT_BYTES / (1024 * 1024)
                ),
            );
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        data.push((name, bytes));
    }
    if let Err(e) = check_out(&args.out) {
        return fail(stderr, &e);
    }
    let sources: Vec<SourceFile<'_>> = data
        .iter()
        .map(|(name, bytes)| SourceFile { name, data: bytes })
        .collect();
    let import = match import_files(args.format, &sources) {
        Ok(i) => i,
        Err(e) => return fail(stderr, &e.to_string()),
    };
    if let Err(e) = write(&args.out, &import) {
        return fail(stderr, &e);
    }
    let text = if args.json {
        render_json(&import, &args.out)
    } else {
        render_text(&import, &args.out)
    };
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
    EXIT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_import(args: &[&str]) -> Result<Option<ImportArgs>, String> {
        let mut parser = lexopt::Parser::from_args(args.iter().copied());
        parse(&mut parser)
    }

    #[test]
    fn arguments() {
        let a = parse_import(&[
            "a.json", "b.json", "--format", "Postman", "-o", "out", "--json",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(a.files, [PathBuf::from("a.json"), PathBuf::from("b.json")]);
        assert_eq!(a.format, Some(ImportFormat::Postman));
        assert_eq!(a.out, PathBuf::from("out"));
        assert!(a.json);
        assert_eq!(parse_import(&["--help"]).unwrap(), None);
        for bad in [
            &["--out", "o"][..],
            &["a.json"],
            &["a", "--out", "o", "--format", "nope"],
            &["a", "--bogus"],
        ] {
            assert!(parse_import(bad).is_err(), "{bad:?}");
        }
        let e = parse_import(&["a", "--out", "o", "-f", "nope"]).unwrap_err();
        assert!(e.contains("postman, insomnia"), "{e}");
    }
}
