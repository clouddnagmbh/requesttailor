// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt merge-driver %O %A %B %P`: Git merge driver; merges one collection file with
//! `rt_format::merge` and writes the result to `%A` (exit 0 clean, 1 with conflicts).
//!
//! Git hands over the three versions as temporary files (`%O` base, `%A` ours, which receives
//! the result, `%B` theirs) and the file's path in the repository (`%P`). The collection root
//! is the nearest folder above `%P` (in the working tree, where Git runs the driver) that
//! holds an `opencollection.yml`; the path relative to it decides how the file is read.
//! Files outside a collection merge line by line, like Git's text merge.
//!
//! With a conflict `%A` stays a valid file: ours in each conflicted field, a YAML comment
//! above it with theirs' value, everything else merged; the fields are listed on stderr.
//! Rebase and cherry-pick of the Git command line use the driver too.
//!
//! Owner: wave 12, strand B.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use rt_format::diff::ChangeTarget;

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt merge-driver <base> <ours> <theirs> [<path>]\n";

/// The section in `rt --help`.
pub const HELP: &str = "rt merge-driver merges collection files on the model, as Git merge driver. Set it up once\nper repository:\n  git config merge.requesttailor.driver \"rt merge-driver %O %A %B %P\"\n  echo \"*.yml merge=requesttailor\" >> .gitattributes\nGit then calls it for every *.yml file both sides changed (merge, rebase, cherry-pick).\nA conflict only where both sides changed the same field: exit 1, the file keeps ours\nwith a comment above each conflicted field (theirs' value) and stderr names the fields;\nresolve in RequestTailor or by hand. Exit 0 when the file merged cleanly. Files outside\na collection merge line by line.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The raw arguments after `rt merge-driver`.
    pub raw: Vec<OsString>,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    let raw: Vec<OsString> = parser.raw_args().map_err(|e| e.to_string())?.collect();
    if raw.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(None);
    }
    Ok(Some(Args { raw }))
}

/// `%P` relative to its collection's root, if it is in one.
fn collection_path(path: &str, workdir: &Path) -> Option<String> {
    let path = path.replace('\\', "/");
    let mut dir = Path::new(&path).parent();
    while let Some(d) = dir {
        if workdir.join(d).join(rt_format::COLLECTION_FILE).is_file() {
            let rel = Path::new(&path).strip_prefix(d).ok()?;
            return Some(rel.to_string_lossy().replace('\\', "/"));
        }
        dir = d.parent();
    }
    None
}

fn describe(target: &ChangeTarget) -> String {
    match target {
        ChangeTarget::Collection => "collection".into(),
        ChangeTarget::Folder { path } => format!("folder {path}"),
        ChangeTarget::Request { path } => format!("request {path}"),
        ChangeTarget::Environment { name } => format!("environment {name}"),
    }
}

pub fn run(args: Args, _stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let (base, ours, theirs, path) = match args.raw.as_slice() {
        [o, a, b] => (o, a, b, None),
        [o, a, b, p] => (o, a, b, Some(p.to_string_lossy().into_owned())),
        _ => {
            let _ = writeln!(
                stderr,
                "rt merge-driver: expected <base> <ours> <theirs> [<path>] (Git's %O %A %B %P)\nTry 'rt --help'."
            );
            return crate::EXIT_USAGE;
        }
    };
    let read = |p: &OsString| std::fs::read(PathBuf::from(p));
    let (b, o, t) = match (read(base), read(ours), read(theirs)) {
        (Ok(b), Ok(o), Ok(t)) => (b, o, t),
        (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => {
            let _ = writeln!(stderr, "rt merge-driver: cannot read a version: {e}");
            return crate::EXIT_USAGE;
        }
    };
    let shown = path
        .clone()
        .unwrap_or_else(|| ours.to_string_lossy().into_owned());
    let workdir = std::env::current_dir().unwrap_or_default();
    let rel = path.as_deref().and_then(|p| collection_path(p, &workdir));
    // Git passes an empty base when both sides added the file.
    let base = (!b.is_empty()).then_some(b.as_slice());
    let outcome = rt_format::merge::merge_file(rel.as_deref(), base, Some(&o), Some(&t));
    if let Err(e) = std::fs::write(PathBuf::from(ours), outcome.bytes.unwrap_or_default()) {
        let _ = writeln!(stderr, "rt merge-driver: cannot write {shown}: {e}");
        return crate::EXIT_USAGE;
    }
    for c in &outcome.conflicts {
        let _ = writeln!(
            stderr,
            "rt merge-driver: {shown}: conflict in {} ({})",
            c.field.join(" / "),
            describe(&c.target)
        );
    }
    if outcome.text_conflict {
        let _ = writeln!(
            stderr,
            "rt merge-driver: {shown}: conflicting lines, marked with <<<<<<< and >>>>>>>"
        );
    }
    if outcome.conflicts.is_empty() && !outcome.text_conflict {
        crate::EXIT_OK
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_collection_root_is_the_nearest_folder_with_a_collection_file() {
        let dir = std::env::temp_dir().join(format!("rt-merge-driver-root-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("api/users")).unwrap();
        std::fs::write(
            dir.join("api/opencollection.yml"),
            "opencollection: 1.0.0\n",
        )
        .unwrap();
        assert_eq!(
            collection_path("api/users/get.yml", &dir).as_deref(),
            Some("users/get.yml")
        );
        assert_eq!(
            collection_path("api/opencollection.yml", &dir).as_deref(),
            Some("opencollection.yml")
        );
        assert_eq!(collection_path("other/x.yml", &dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
