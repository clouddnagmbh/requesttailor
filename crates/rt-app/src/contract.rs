// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OpenAPI contract for the app (wave 12): finding the spec, and the shared steps of the
//! contract check, breaking changes and `rt changelog` that desktop, web and CLI run the same
//! way on top of [`rt_format::contract`].
//!
//! The shell tells this module how to read a spec file (`read`: the work tree, or
//! `GitApi.show` at a revision); everything else is pure. The web shell does the same in
//! TypeScript over the functions of `rt-wasm` and uses [`changelog_title`] from here.
//!
//! Owner: wave 12, strand F.

use rt_format::FileSet;
pub use rt_format::contract::{
    ContractError, ContractFinding, ContractReport, ContractSpec, FindingKind, SPEC_CANDIDATES,
    SpecChange, SpecChangeKind, SpecChanges,
};

use crate::AppError;

impl From<ContractError> for AppError {
    fn from(e: ContractError) -> Self {
        match e {
            ContractError::Spec(m) => AppError::Format(m),
            ContractError::Unsupported(m) => AppError::Unsupported(m),
        }
    }
}

type Result<T> = std::result::Result<T, AppError>;

/// Reads one spec path (collection-relative, or repository-relative with a leading `/`);
/// `Ok(None)` if the file does not exist.
pub type ReadFile<'a> = &'a dyn Fn(&str) -> Result<Option<Vec<u8>>>;

/// The spec path of the collection in `files`: configured in `opencollection.yml`, else the
/// first of [`SPEC_CANDIDATES`] for which `exists` says yes (the shell asks its work tree or
/// `GitApi.show`).
pub fn locate(files: &rt_format::FileSet, exists: impl Fn(&str) -> bool) -> Option<String> {
    rt_format::contract::configured_spec(files).or_else(|| {
        SPEC_CANDIDATES
            .iter()
            .find(|p| exists(p))
            .map(|p| (*p).to_owned())
    })
}

/// A spec path must stay inside the collection or the repository.
pub fn safe_path(path: &str) -> Result<()> {
    let bad = path.contains('\\')
        || path.contains(':')
        || path.split('/').any(|s| s == "..")
        || path.is_empty();
    if bad {
        return Err(AppError::Format(format!(
            "the spec path {path:?} must stay inside the collection or the repository"
        )));
    }
    Ok(())
}

/// The spec of one side: its description and bytes.
pub struct Loaded {
    pub spec: ContractSpec,
    pub bytes: Vec<u8>,
}

/// Finds and reads the spec of `files`. `None` if there is none. A configured spec that does
/// not exist is an error (the configuration is wrong), a found one that does not parse too.
pub fn load(files: &FileSet, read: ReadFile<'_>) -> Result<Option<Loaded>> {
    let configured = rt_format::contract::configured_spec(files);
    let (path, bytes) = match &configured {
        Some(path) => {
            safe_path(path)?;
            let Some(bytes) = read(path)? else {
                return Err(AppError::Format(format!(
                    "the spec {path} named in opencollection.yml does not exist"
                )));
            };
            (path.clone(), bytes)
        }
        None => {
            let mut found = None;
            for candidate in SPEC_CANDIDATES {
                if let Some(bytes) = read(candidate)? {
                    found = Some(((*candidate).to_owned(), bytes));
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => return Ok(None),
            }
        }
    };
    let (title, version) = rt_format::contract::spec_info(&bytes)?;
    Ok(Some(Loaded {
        spec: ContractSpec {
            path,
            title,
            version,
            configured: configured.is_some(),
        },
        bytes,
    }))
}

/// The collection checked against its spec.
pub fn check(files: &FileSet, read: ReadFile<'_>) -> Result<ContractReport> {
    let Some(loaded) = load(files, read)? else {
        return Err(AppError::Format(no_spec()));
    };
    Ok(rt_format::contract::check(files, &loaded.bytes)?)
}

fn no_spec() -> String {
    "no OpenAPI spec found: add openapi.yaml next to the collection or at the repository root, or name it in opencollection.yml under extensions.requesttailor.contract.spec".into()
}

/// One side of a range: the collection's files and how to read its spec.
pub struct Side<'a> {
    pub files: &'a FileSet,
    pub read: ReadFile<'a>,
}

/// Changes of the spec between two sides. A side without a spec counts as "did not exist".
pub fn changes(old: &Side<'_>, new: &Side<'_>) -> Result<SpecChanges> {
    let old_spec = load(old.files, old.read)?;
    let new_spec = load(new.files, new.read)?;
    Ok(rt_format::contract::changes(
        old_spec.as_ref().map(|l| l.bytes.as_slice()),
        new_spec.as_ref().map(|l| l.bytes.as_slice()),
        new.files,
    )?)
}

/// The heading of a changelog: `Orders API 1.1.0 → 2.0.0` when the spec's version is known,
/// else `Changes <from> → <to>`. `name`: the spec's title, if known.
pub fn changelog_title(name: Option<&str>, spec: &SpecChanges, from: &str, to: &str) -> String {
    match (name, &spec.from_version, &spec.to_version) {
        (Some(n), Some(a), Some(b)) => format!("{n} {a} → {b}"),
        (Some(n), None, Some(b)) => format!("{n} {b}"),
        (Some(n), _, _) => format!("{n}: {from} → {to}"),
        (None, _, _) => format!("Changes {from} → {to}"),
    }
}

/// A section a distribution adds to the changelog (since wave 13), e.g. the changes of
/// another description file next to the collection. `section` returns Markdown starting with
/// a `## ` heading, or `None` when it has nothing to say for this range. `Side::files` are the
/// collection files (`*.yml`); any other file is read with `Side::read` (collection-relative,
/// or repository-relative with a leading `/`), as the spec is.
pub trait ChangelogSection {
    fn section(
        &self,
        old: &Side<'_>,
        new: &Side<'_>,
        from: &str,
        to: &str,
    ) -> Result<Option<String>>;
}

/// [`changelog`] followed by the `sections` that have something to say, in the order given,
/// each separated by a blank line.
pub fn changelog_with(
    old: &Side<'_>,
    new: &Side<'_>,
    from: &str,
    to: &str,
    sections: &[&dyn ChangelogSection],
) -> Result<String> {
    let mut text = changelog(old, new, from, to)?;
    for section in sections {
        if let Some(more) = section.section(old, new, from, to)? {
            let more = more.trim();
            if more.is_empty() {
                continue;
            }
            while !text.ends_with("\n\n") {
                text.push('\n');
            }
            text.push_str(more);
            text.push('\n');
        }
    }
    Ok(text)
}

/// The API changelog (Markdown) of a range, as `rt changelog` writes it. `from` and `to` are
/// the labels of the sides (revisions).
pub fn changelog(old: &Side<'_>, new: &Side<'_>, from: &str, to: &str) -> Result<String> {
    let old_spec = load(old.files, old.read)?;
    let new_spec = load(new.files, new.read)?;
    let spec = rt_format::contract::changes(
        old_spec.as_ref().map(|l| l.bytes.as_slice()),
        new_spec.as_ref().map(|l| l.bytes.as_slice()),
        new.files,
    )?;
    let collection = rt_format::diff::diff(old.files, new.files)?;
    let name = new_spec
        .as_ref()
        .or(old_spec.as_ref())
        .map(|l| l.spec.title.as_str());
    let title = changelog_title(name, &spec, from, to);
    Ok(rt_format::contract::changelog(&title, &spec, &collection))
}

// ---- Wave 16 interface: changelog writers (e.g. release notes written by a model) ----

/// A commit of a changelog's range (first-parent chain from `from` to `to`), for writers that
/// turn the deterministic changelog into release notes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangelogCommit {
    pub oid: String,
    pub summary: String,
    /// The message after the summary line.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    pub author: String,
    /// A pull or merge request number found in the message (`Merge pull request #12`,
    /// `See merge request group/repo!12`, `(#12)`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull: Option<u64>,
}

/// Everything deterministic about a range that a writer may build on: the changelog as `rt
/// changelog` writes it, its parts as data, the distribution's sections and the commits. A
/// writer must not state anything that is not in here.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChangelogFacts {
    pub from: String,
    pub to: String,
    pub title: String,
    pub spec: SpecChanges,
    pub collection: rt_format::diff::CollectionDiff,
    /// Markdown of each distribution section that had something to say (`ChangelogSection`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<ChangelogCommit>,
    /// [`changelog_with`] of the range.
    pub markdown: String,
}

/// The facts of a range: [`changelog_with`] and its parts. `commits` come from the shell
/// (`rt-git` natively, isomorphic-git in the browser).
pub fn changelog_facts(
    old: &Side<'_>,
    new: &Side<'_>,
    from: &str,
    to: &str,
    sections: &[&dyn ChangelogSection],
    commits: Vec<ChangelogCommit>,
) -> Result<ChangelogFacts> {
    let old_spec = load(old.files, old.read)?;
    let new_spec = load(new.files, new.read)?;
    let spec = rt_format::contract::changes(
        old_spec.as_ref().map(|l| l.bytes.as_slice()),
        new_spec.as_ref().map(|l| l.bytes.as_slice()),
        new.files,
    )?;
    let collection = rt_format::diff::diff(old.files, new.files)?;
    let name = new_spec
        .as_ref()
        .or(old_spec.as_ref())
        .map(|l| l.spec.title.as_str());
    let title = changelog_title(name, &spec, from, to);
    let mut extra = Vec::new();
    for section in sections {
        if let Some(text) = section.section(old, new, from, to)? {
            let text = text.trim();
            if !text.is_empty() {
                extra.push(text.to_owned());
            }
        }
    }
    let markdown = changelog_with(old, new, from, to, sections)?;
    Ok(ChangelogFacts {
        from: from.to_owned(),
        to: to.to_owned(),
        title,
        spec,
        collection,
        sections: extra,
        commits,
        markdown,
    })
}

/// Another way to write the changelog of a range, chosen with a flag of `rt changelog` (since
/// wave 16), e.g. `rt changelog v1..v2 --notes --audience developers`. A distribution passes
/// writers in `rt_cli::Distribution::changelog_writers`; the builds of this repository pass
/// none, and `rt changelog` without the flag is unchanged. Called outside any async runtime.
pub trait ChangelogWriter {
    /// The flag without dashes, e.g. `notes` for `--notes`.
    fn flag(&self) -> &'static str;
    /// Options it takes as `--<name> <value>` (names without dashes).
    fn options(&self) -> &'static [&'static str];
    /// Lines for `rt changelog --help`.
    fn help(&self) -> &'static str;
    /// The text to write instead of [`ChangelogFacts::markdown`]; an error ends `rt changelog`
    /// with its message (exit code 2).
    fn write(&self, facts: &ChangelogFacts, options: &[(String, String)]) -> Result<String>;
}
