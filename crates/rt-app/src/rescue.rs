// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Secret rescue (wave 12): the secret scanner over the whole history, and rewriting the
//! history so a found value is replaced by a `{{variable}}` in every commit.
//!
//! - The scan covers every commit reachable from `HEAD` and the local branches, each blob
//!   once. A finding is known by [`HistoryFinding::id`] (where it was first seen); its value
//!   never leaves the shell: the rewrite reads it again from that blob.
//! - The rewrite runs only after the user confirmed it, with no unsaved changes and a clean
//!   collection; first it saves every rewritten branch under [`BACKUP_PREFIX`] (not a branch,
//!   so a later scan of the branches is clean; it still holds the secret until deleted). It
//!   rewrites the local branches that contain a finding, puts the value into the secret store
//!   (environment and variable of the plan) and updates the work tree to the new `HEAD`.
//!   Remote-tracking refs and tags stay; the result lists the branches that need a force push.
//!   The app warns before the force push and tells the user to rotate the secret.
//! - Natively with gitoxide (`rt-git`), in the web app with isomorphic-git; the text
//!   replacement is shared ([`replace_value`]).
//!
//! What lives here, so both shells do exactly the same (the web app through `rt-wasm`):
//!
//! - [`HistoryScanner`]: the shell walks the commits and hands over each commit (parents and
//!   files) and each blob once; the scanner runs `secret_scan` on the blobs and groups what
//!   it finds by value: one [`HistoryFinding`] per value, first seen in the oldest commit.
//! - [`plan_value`]: the value of a plan's findings, read again from their blobs (the UI only
//!   ever has the ids).
//! - [`contains_value`] and [`replace_value`]: which blobs the rewrite changes, and how.
//!
//! The scanner reports lines with masked snippets, not values; [`values_in`] gets the exact
//! values it masked from `secret_scan::scan_values`, which only Rust code sees.
//!
//! Owner: wave 12 interface; implementation strand G.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};

use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::git::GitCommit;
use crate::secret_scan::{self, SecretFinding};

/// Backups of rewritten branches: `refs/requesttailor/backup/<unix seconds>/<branch>`.
pub const BACKUP_PREFIX: &str = "refs/requesttailor/backup/";

/// Shorter values are not replaced: they would match unrelated text.
pub const MIN_VALUE_CHARS: usize = 4;

/// A credential found somewhere in the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryFinding {
    /// `<commit oid>:<path>:<line>` of the first (oldest) occurrence. A second value on the
    /// same line gets `.2`, a third `.3`, … after the line number.
    pub id: String,
    /// As the scanner reports it (snippet masked), at the first occurrence.
    pub finding: SecretFinding,
    /// The commit that introduced it.
    pub first: GitCommit,
    /// Commits that contain the value, oldest first.
    pub commits: Vec<String>,
    /// Local branches that contain it.
    pub branches: Vec<String>,
    /// `HEAD`'s files still contain it (a commit alone does not remove it from history).
    pub in_head: bool,
}

/// Progress of a history scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scanned: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HistoryScan {
    /// Commits scanned.
    pub commits: u32,
    pub findings: Vec<HistoryFinding>,
}

/// What to rewrite (`RescueApi.rewrite`), after the user confirmed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RescuePlan {
    /// [`HistoryFinding::id`]s; all must be about the same value.
    pub findings: Vec<String>,
    /// The variable the value becomes, e.g. `apiToken` → `{{apiToken}}`.
    pub variable: String,
    /// The environment whose secret store entry gets the value.
    pub environment: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewrittenBranch {
    pub name: String,
    pub old: String,
    pub new: String,
    /// Has an upstream: needs a force push to remove the value there.
    pub pushed: bool,
    /// The upstream's remote (`origin`), for the force-push command; `None` without one.
    #[serde(default)]
    pub remote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RescueResult {
    /// Commits written anew.
    pub rewritten: u32,
    pub branches: Vec<RewrittenBranch>,
    /// Ref prefix of this rescue's backups, e.g. `refs/requesttailor/backup/1795000000/`.
    pub backup: String,
    /// Tags that point at a rewritten commit: tags are not rewritten, so they still hold
    /// the value.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// `refs/requesttailor/backup/<unix>/`.
pub fn backup_prefix(unix: i64) -> String {
    format!("{BACKUP_PREFIX}{unix}/")
}

// ---------------------------------------------------------------------------------------------
// Finding ids

/// Where a finding was seen: [`HistoryFinding::id`] taken apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingRef {
    pub commit: String,
    /// Collection-relative path.
    pub path: String,
    /// 1-based.
    pub line: u32,
    /// 0 for the first value on the line.
    pub index: usize,
}

impl FindingRef {
    pub fn id(&self) -> String {
        if self.index == 0 {
            format!("{}:{}:{}", self.commit, self.path, self.line)
        } else {
            format!(
                "{}:{}:{}.{}",
                self.commit,
                self.path,
                self.line,
                self.index + 1
            )
        }
    }

    pub fn parse(id: &str) -> Result<Self, AppError> {
        let bad = || AppError::Git(format!("not a finding of the history scan: {id:?}"));
        let (commit, rest) = id.split_once(':').ok_or_else(bad)?;
        let (path, at) = rest.rsplit_once(':').ok_or_else(bad)?;
        if !(commit.len() == 40 || commit.len() == 64)
            || !commit.bytes().all(|b| b.is_ascii_hexdigit())
            || path.is_empty()
        {
            return Err(bad());
        }
        let (line, index) = match at.split_once('.') {
            Some((line, n)) => {
                let n: usize = n.parse().map_err(|_| bad())?;
                if n < 2 {
                    return Err(bad());
                }
                (line, n - 1)
            }
            None => (at, 0),
        };
        let line: u32 = line.parse().map_err(|_| bad())?;
        if line == 0 {
            return Err(bad());
        }
        Ok(Self {
            commit: commit.to_ascii_lowercase(),
            path: path.to_owned(),
            line,
            index,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Values

/// The scanner's findings in `text`, each with the values it masked on its line.
pub fn values_in(path: &str, text: &str) -> Vec<(SecretFinding, Vec<String>)> {
    secret_scan::scan_values(path, text)
}

/// The finding and value `at` names in `text` (the blob `at` names); `None` if it is not
/// there.
pub fn value_of(text: &str, at: &FindingRef) -> Option<(SecretFinding, String)> {
    let (finding, values) = values_in(&at.path, text)
        .into_iter()
        .find(|(f, _)| f.line == at.line)?;
    values.get(at.index).map(|v| (finding, v.clone()))
}

/// Refuses values the rewrite cannot replace safely.
pub fn check_value(value: &str, finding: &SecretFinding) -> Result<(), AppError> {
    if finding.kind.contains("private key") {
        return Err(AppError::Unsupported(
            "A private key cannot become a variable: remove the key file from the history \
             with a tool such as git filter-repo, and replace the key."
                .into(),
        ));
    }
    if value.chars().count() < MIN_VALUE_CHARS {
        return Err(AppError::Unsupported(format!(
            "The value is too short to replace safely (under {MIN_VALUE_CHARS} characters): \
             it would change unrelated text. Change it at the provider and edit the files by \
             hand."
        )));
    }
    Ok(())
}

/// A variable name `{{…}}` can hold: letters, digits, `_`, `-` and `.`, starting with a
/// letter or `_`.
pub fn check_variable(name: &str) -> Result<(), AppError> {
    let ok = !name.is_empty()
        && name.len() <= 100
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if ok {
        Ok(())
    } else {
        Err(AppError::Git(format!(
            "{name:?} is not a variable name: use letters, digits, `_`, `-` and `.`"
        )))
    }
}

/// Checks the plan and takes its finding ids apart.
pub fn check_plan(plan: &RescuePlan) -> Result<Vec<FindingRef>, AppError> {
    if plan.findings.is_empty() {
        return Err(AppError::Git("choose a finding to rescue".into()));
    }
    check_variable(&plan.variable)?;
    if plan.environment.trim().is_empty() {
        return Err(AppError::Git(
            "choose the environment that keeps the secret".into(),
        ));
    }
    plan.findings
        .iter()
        .map(|id| FindingRef::parse(id))
        .collect()
}

/// The one value of `plan`'s findings. `read` gives the text of the file a finding names
/// (`None`: no such file in that commit). Fails when a finding is not where its id says,
/// when they are about different values, or when the value cannot be replaced safely.
pub fn plan_value(
    plan: &RescuePlan,
    mut read: impl FnMut(&FindingRef) -> Result<Option<String>, AppError>,
) -> Result<String, AppError> {
    let refs = check_plan(plan)?;
    let gone = || {
        AppError::Git("a finding is no longer where the scan saw it: scan the history again".into())
    };
    let mut value: Option<String> = None;
    for at in &refs {
        let text = read(at)?.ok_or_else(gone)?;
        let (finding, v) = value_of(&text, at).ok_or_else(gone)?;
        check_value(&v, &finding)?;
        match &value {
            Some(prev) if *prev != v => {
                return Err(AppError::Git(
                    "the chosen findings are about different values: rescue one at a time".into(),
                ));
            }
            Some(_) => {}
            None => value = Some(v),
        }
    }
    value.ok_or_else(gone)
}

// ---------------------------------------------------------------------------------------------
// Replacing

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Byte offsets of the occurrences of `value` in `text` that are not part of a longer word
/// (`abcd` is not in `xabcd`, but is in `x-abcd`).
fn occurrences<'a>(text: &'a str, value: &'a str) -> impl Iterator<Item = usize> + 'a {
    let first = value.chars().next().is_some_and(word);
    let last = value.chars().next_back().is_some_and(word);
    text.match_indices(value).filter_map(move |(i, _)| {
        let before = text[..i].chars().next_back().is_some_and(word);
        let after = text[i + value.len()..].chars().next().is_some_and(word);
        (!((first && before) || (last && after))).then_some(i)
    })
}

/// Whether the rewrite changes `text` (it holds `value`, see [`replace_value`]).
pub fn contains_value(text: &str, value: &str) -> bool {
    !value.is_empty() && occurrences(text, value).next().is_some()
}

fn replace_plain(text: &str, value: &str, var: &str) -> String {
    if value.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let hits: Vec<usize> = occurrences(text, value).collect();
    for i in hits {
        if i < at {
            continue;
        }
        out.push_str(&text[at..i]);
        out.push_str(var);
        at = i + value.len();
    }
    out.push_str(&text[at..]);
    out
}

/// `text` with every occurrence of `value` replaced by `{{variable}}`.
///
/// Occurrences inside a longer word are left alone. The text is read as YAML lines (as the
/// scanner does): a plain scalar that would start with `{{` after the replacement is
/// double-quoted (`token: abc` → `token: "{{apiToken}}"`; unquoted, YAML would read a
/// mapping). Quoted scalars, block scalars (`|`, `>`) and anything else get the plain
/// replacement. Other lines stay byte for byte.
pub fn replace_value(text: &str, value: &str, variable: &str) -> String {
    if !contains_value(text, value) {
        return text.to_owned();
    }
    let var = format!("{{{{{variable}}}}}");
    let mut out = String::with_capacity(text.len() + 16);
    // Indentation of the line that opened the block scalar we are in.
    let mut block: Option<usize> = None;
    for (n, raw) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        if let Some(parent) = block {
            if raw.trim().is_empty() || indent > parent {
                out.push_str(&replace_plain(raw, value, &var));
                continue;
            }
            block = None;
        }
        if contains_value(raw, value) {
            out.push_str(&replace_line(raw, value, &var));
        } else {
            out.push_str(raw);
        }
        if opens_block_scalar(raw) {
            block = Some(indent);
        }
    }
    out
}

/// Where the scalar of a YAML line starts: after `- ` and after `key: `.
fn scalar_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut pos = line.len() - line.trim_start_matches(' ').len();
    while bytes.get(pos) == Some(&b'-') && bytes.get(pos + 1) == Some(&b' ') {
        pos += 2;
        while bytes.get(pos) == Some(&b' ') {
            pos += 1;
        }
    }
    if pos >= bytes.len() || bytes[pos] == b'#' {
        return None;
    }
    // A plain key: up to `: ` (quoted keys and keys with `: ` inside are rare here).
    if !matches!(bytes[pos], b'"' | b'\'' | b'{' | b'[')
        && let Some(colon) = line[pos..].find(": ")
        && !line[pos..pos + colon].contains(" #")
    {
        let mut v = pos + colon + 2;
        while bytes.get(v) == Some(&b' ') {
            v += 1;
        }
        return (v < bytes.len()).then_some(v);
    }
    Some(pos)
}

fn opens_block_scalar(line: &str) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let text = line.split(" #").next().unwrap_or("").trim_end();
    let Some(start) = scalar_start(text) else {
        return false;
    };
    let s = &text[start..];
    matches!(s.as_bytes().first(), Some(b'|' | b'>'))
        && s[1..]
            .chars()
            .all(|c| matches!(c, '-' | '+') || c.is_ascii_digit())
}

fn replace_line(raw: &str, value: &str, var: &str) -> String {
    let (line, cr) = match raw.strip_suffix('\r') {
        Some(l) => (l, "\r"),
        None => (raw, ""),
    };
    let Some(start) = scalar_start(line) else {
        return replace_plain(raw, value, var);
    };
    if matches!(
        line.as_bytes()[start],
        b'"' | b'\'' | b'|' | b'>' | b'[' | b'{' | b'&' | b'*' | b'!'
    ) {
        return replace_plain(raw, value, var);
    }
    // The plain scalar runs to a comment or the end of the line.
    let end = line[start..].find(" #").map_or(line.len(), |c| start + c);
    let scalar_end = start + line[start..end].trim_end().len();
    let new = replace_plain(&line[start..scalar_end], value, var);
    let new = if new.starts_with('{') {
        let escaped = new.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        new
    };
    format!(
        "{}{}{}{}",
        replace_plain(&line[..start], value, var),
        new,
        replace_plain(&line[scalar_end..], value, var),
        cr
    )
}

// ---------------------------------------------------------------------------------------------
// History scan

/// One commit as the shell hands it to [`HistoryScanner::add_commit`].
#[derive(Debug, Clone)]
struct CommitEntry {
    commit: GitCommit,
    parents: Vec<String>,
    /// Collection-relative path → blob id.
    files: Vec<(String, String)>,
}

#[derive(Debug, Default)]
struct BlobEntry {
    /// `None`: a file the scanner skips (binary, too large).
    text: Option<String>,
    /// The scanner's findings with their values.
    hits: Vec<(SecretFinding, Vec<String>)>,
}

/// The history scan without Git: the shell walks the commits (any order) and hands over
/// each commit with its parents and files, and each blob once ([`HistoryScanner::knows`]).
#[derive(Debug, Default)]
pub struct HistoryScanner {
    commits: HashMap<String, CommitEntry>,
    blobs: HashMap<String, BlobEntry>,
}

type Ready = std::cmp::Reverse<(i64, String)>;

impl HistoryScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether blob `id` was added already (the shell need not read it again).
    pub fn knows(&self, id: &str) -> bool {
        self.blobs.contains_key(id)
    }

    /// A blob's text (`None` for files the scanner skips, see `secret_scan::text_of`), seen
    /// first at `path`.
    pub fn add_blob(&mut self, id: &str, path: &str, text: Option<&str>) {
        if self.knows(id) {
            return;
        }
        let hits = text.map(|t| values_in(path, t)).unwrap_or_default();
        self.blobs.insert(
            id.to_owned(),
            BlobEntry {
                text: text.map(str::to_owned),
                hits,
            },
        );
    }

    /// A commit, its parents and its collection files (path, blob id). Add their blobs
    /// before [`HistoryScanner::finish`].
    pub fn add_commit(
        &mut self,
        commit: GitCommit,
        parents: Vec<String>,
        files: Vec<(String, String)>,
    ) {
        self.commits.insert(
            commit.oid.clone(),
            CommitEntry {
                commit,
                parents,
                files,
            },
        );
    }

    /// Commits, parents first; among those ready, the oldest (author time, then id) first.
    fn order(&self) -> Vec<&str> {
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut waiting: HashMap<&str, usize> = HashMap::new();
        for (id, c) in &self.commits {
            let parents: BTreeSet<&str> = c
                .parents
                .iter()
                .map(String::as_str)
                .filter(|p| self.commits.contains_key(*p))
                .collect();
            waiting.insert(id, parents.len());
            for p in parents {
                children.entry(p).or_default().push(id);
            }
        }
        let key = |id: &str| -> Ready {
            std::cmp::Reverse((self.commits[id].commit.time, id.to_owned()))
        };
        let mut ready: BinaryHeap<Ready> = waiting
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(id, _)| key(id))
            .collect();
        let mut out = Vec::with_capacity(self.commits.len());
        while let Some(std::cmp::Reverse((_, id))) = ready.pop() {
            let id = self
                .commits
                .get_key_value(id.as_str())
                .expect("known")
                .0
                .as_str();
            out.push(id);
            for child in children.get(id).into_iter().flatten() {
                let n = waiting.get_mut(child).expect("known");
                *n -= 1;
                if *n == 0 {
                    ready.push(key(child));
                }
            }
        }
        out
    }

    /// Commits reachable from `tip` (itself included).
    fn reachable(&self, tip: &str) -> BTreeSet<&str> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![tip];
        while let Some(id) = stack.pop() {
            let Some((key, c)) = self.commits.get_key_value(id) else {
                continue;
            };
            if seen.insert(key.as_str()) {
                stack.extend(c.parents.iter().map(String::as_str));
            }
        }
        seen
    }

    /// The findings: one per value, at its oldest occurrence. `branches`: local branches
    /// (name, tip); `head`: `HEAD`'s commit.
    pub fn finish(&self, branches: &[(String, String)], head: Option<&str>) -> HistoryScan {
        struct Found {
            value: String,
            first: (String, String, SecretFinding, usize),
            commits: Vec<String>,
            in_head: bool,
        }
        let order = self.order();
        let mut found: Vec<Found> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for id in &order {
            let mut files: Vec<&(String, String)> = self.commits[*id].files.iter().collect();
            files.sort();
            for (path, blob) in files {
                let Some(entry) = self.blobs.get(blob) else {
                    continue;
                };
                for (finding, values) in &entry.hits {
                    for (k, v) in values.iter().enumerate() {
                        if !seen.insert(v.clone()) {
                            continue;
                        }
                        let mut finding = finding.clone();
                        finding.path = path.clone();
                        found.push(Found {
                            value: v.clone(),
                            first: ((*id).to_owned(), path.clone(), finding, k),
                            commits: Vec::new(),
                            in_head: false,
                        });
                    }
                }
            }
        }
        // Which values each blob holds (also where the scanner did not flag them).
        let mut held: HashMap<&str, BTreeSet<usize>> = HashMap::new();
        for (id, entry) in &self.blobs {
            let set: BTreeSet<usize> = found
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    entry.hits.iter().any(|(_, vs)| vs.contains(&f.value))
                        || entry
                            .text
                            .as_deref()
                            .is_some_and(|t| contains_value(t, &f.value))
                })
                .map(|(n, _)| n)
                .collect();
            if !set.is_empty() {
                held.insert(id.as_str(), set);
            }
        }
        let values_of = |commit: &str| -> BTreeSet<usize> {
            self.commits[commit]
                .files
                .iter()
                .filter_map(|(_, b)| held.get(b.as_str()))
                .flatten()
                .copied()
                .collect()
        };
        for id in &order {
            for n in values_of(id) {
                found[n].commits.push((*id).to_owned());
            }
        }
        if let Some(head) = head.filter(|h| self.commits.contains_key(*h)) {
            for n in values_of(head) {
                found[n].in_head = true;
            }
        }
        let reach: BTreeMap<&str, BTreeSet<&str>> = branches
            .iter()
            .map(|(name, tip)| (name.as_str(), self.reachable(tip)))
            .collect();
        let findings = found
            .into_iter()
            .map(|f| {
                let (commit, path, finding, k) = f.first;
                let branches = reach
                    .iter()
                    .filter(|(_, set)| f.commits.iter().any(|c| set.contains(c.as_str())))
                    .map(|(name, _)| (*name).to_owned())
                    .collect();
                HistoryFinding {
                    id: FindingRef {
                        commit: commit.clone(),
                        path,
                        line: finding.line,
                        index: k,
                    }
                    .id(),
                    first: self.commits[&commit].commit.clone(),
                    finding,
                    commits: f.commits,
                    branches,
                    in_head: f.in_head,
                }
            })
            .collect();
        HistoryScan {
            commits: u32::try_from(self.commits.len()).unwrap_or(u32::MAX),
            findings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Assembled at run time, so secret scanners of the repository stay quiet.
    fn token() -> String {
        ["ghp_", "R3scue", "T0ken", "AbCdEfGhIjKlMnOpQrStUv", "01"].concat()
    }

    #[test]
    fn a_value_becomes_a_variable() {
        assert_eq!(
            replace_value("token: abc123\nx: \"abc123\"\n", "abc123", "apiToken"),
            "token: \"{{apiToken}}\"\nx: \"{{apiToken}}\"\n"
        );
        assert_eq!(replace_value("a", "", "v"), "a");
        // Part of a longer word: not the value.
        assert_eq!(replace_value("x: abc1234\n", "abc123", "v"), "x: abc1234\n");
        // Inside a plain scalar: no quotes needed.
        assert_eq!(
            replace_value("  value: Bearer abc123 # note\r\n", "abc123", "t"),
            "  value: Bearer {{t}} # note\r\n"
        );
        assert_eq!(replace_value("- abc123\n", "abc123", "t"), "- \"{{t}}\"\n");
        // Block scalars stay literal text.
        assert_eq!(
            replace_value(
                "data: |-\n  abc123\n  {\"k\": \"abc123\"}\nnext: abc123\n",
                "abc123",
                "t"
            ),
            "data: |-\n  {{t}}\n  {\"k\": \"{{t}}\"}\nnext: \"{{t}}\"\n"
        );
        // URLs and other characters around it.
        assert_eq!(
            replace_value("url: https://u:abc123@h/x?k=abc123\n", "abc123", "t"),
            "url: https://u:{{t}}@h/x?k={{t}}\n"
        );
        assert!(contains_value("a abc123", "abc123"));
        assert!(!contains_value("xabc123", "abc123"));
    }

    #[test]
    fn values_are_read_from_the_masked_lines() {
        let t = token();
        let text = format!(
            "headers:\n  - name: Authorization\n    value: Bearer {t}\nauth:\n  password: hunter22x\n"
        );
        let found = values_in("get.yml", &text);
        let values: Vec<(u32, Vec<String>)> =
            found.iter().map(|(f, v)| (f.line, v.clone())).collect();
        assert_eq!(
            values,
            vec![(3, vec![t.clone()]), (5, vec!["hunter22x".to_owned()])]
        );
        // Replaced, the scanner finds nothing.
        let mut clean = replace_value(&text, &t, "githubToken");
        clean = replace_value(&clean, "hunter22x", "password");
        assert!(
            secret_scan::scan(&[("get.yml", &clean)]).is_empty(),
            "{clean}"
        );
        assert!(clean.contains("value: Bearer {{githubToken}}"));
        assert!(clean.contains("password: \"{{password}}\""));
    }

    #[test]
    fn two_values_on_a_line_and_long_lines() {
        let t = token();
        let line = format!("url: https://ada:pa55word-x@api.example.com/v1?access_token={t}\n");
        let found = values_in("u.yml", &line);
        assert_eq!(found.len(), 1);
        let mut values = found[0].1.clone();
        values.sort();
        let mut want = vec!["pa55word-x".to_owned(), t.clone()];
        want.sort();
        assert_eq!(values, want);
        // A line longer than the snippet: the value before the cut is still found.
        let long = format!("value: Bearer {t} {}\n", "x".repeat(300));
        let found = values_in("l.yml", &long);
        assert_eq!(found[0].1, vec![t.clone()]);
    }

    #[test]
    fn ids_round_trip_and_plans_are_checked() {
        let at = FindingRef {
            commit: "a".repeat(40),
            path: "a/b:c.yml".into(),
            line: 12,
            index: 1,
        };
        assert_eq!(at.id(), format!("{}:a/b:c.yml:12.2", "a".repeat(40)));
        assert_eq!(FindingRef::parse(&at.id()).unwrap(), at);
        let first = FindingRef {
            index: 0,
            ..at.clone()
        };
        assert_eq!(FindingRef::parse(&first.id()).unwrap(), first);
        for bad in [
            String::new(),
            "x:y:1".into(),
            format!("{}:p:0", "a".repeat(40)),
            format!("{}::3", "a".repeat(40)),
        ] {
            assert!(FindingRef::parse(&bad).is_err(), "{bad}");
        }
        assert!(check_variable("apiToken").is_ok());
        assert!(check_variable("api.token-2").is_ok());
        for bad in ["", "1x", "a b", "{{x}}", "a/b"] {
            assert!(check_variable(bad).is_err(), "{bad}");
        }

        let t = token();
        let commit = "c".repeat(40);
        let text = format!("auth:\n  token: {t}\n");
        let plan = |ids: Vec<String>| RescuePlan {
            findings: ids,
            variable: "githubToken".into(),
            environment: "Local".into(),
        };
        let id = format!("{commit}:get.yml:2");
        let read = |_: &FindingRef| Ok(Some(text.clone()));
        assert_eq!(plan_value(&plan(vec![id.clone()]), read).unwrap(), t);
        assert!(plan_value(&plan(vec![]), read).is_err());
        assert!(plan_value(&plan(vec![format!("{commit}:get.yml:1")]), read).is_err());
        assert!(plan_value(&plan(vec![id.clone()]), |_: &FindingRef| Ok(None)).is_err());
        let blank_env = RescuePlan {
            environment: " ".into(),
            ..plan(vec![id.clone()])
        };
        assert!(plan_value(&blank_env, read).is_err());
        // Too short, private keys.
        let short = "auth:\n  password: abc\n".to_owned();
        let e = plan_value(
            &plan(vec![format!("{commit}:a.yml:2")]),
            |_: &FindingRef| Ok(Some(short.clone())),
        );
        assert!(matches!(e, Err(AppError::Unsupported(_))), "{e:?}");
        let pem = ["-----BEGIN RSA ", "PRIVATE KEY-----\nMIIB\n"].concat();
        let e = plan_value(
            &plan(vec![format!("{commit}:k.pem:1")]),
            |_: &FindingRef| Ok(Some(pem.clone())),
        );
        assert!(matches!(e, Err(AppError::Unsupported(_))), "{e:?}");
    }

    fn commit(n: u8, time: i64) -> GitCommit {
        GitCommit {
            oid: format!("{n:0>40}"),
            summary: format!("c{n}"),
            author: "Ada".into(),
            time,
        }
    }

    #[test]
    fn the_scanner_groups_by_value_oldest_first() {
        let t = token();
        let mut s = HistoryScanner::new();
        let with = format!("auth:\n  token: {t}\n");
        let moved = format!("x: 1\nauth:\n  token: {t}\n");
        s.add_blob("b1", "get.yml", Some(&with));
        s.add_blob("b2", "get.yml", Some("auth:\n  token: \"{{t}}\"\n"));
        s.add_blob("b3", "get.yml", Some(&moved));
        assert!(s.knows("b1") && !s.knows("b9"));
        // Added newest first: the order comes from the parents.
        let (c1, c2, c3, c4) = (
            commit(1, 100),
            commit(2, 200),
            commit(3, 300),
            commit(4, 150),
        );
        s.add_commit(
            c3.clone(),
            vec![c2.oid.clone()],
            vec![("get.yml".into(), "b2".into())],
        );
        s.add_commit(
            c4.clone(),
            vec![c1.oid.clone()],
            vec![("old/get.yml".into(), "b3".into())],
        );
        s.add_commit(
            c2.clone(),
            vec![c1.oid.clone()],
            vec![("get.yml".into(), "b1".into())],
        );
        s.add_commit(c1.clone(), vec![], vec![("get.yml".into(), "b1".into())]);
        let scan = s.finish(
            &[
                ("main".into(), c3.oid.clone()),
                ("side".into(), c4.oid.clone()),
            ],
            Some(&c3.oid),
        );
        assert_eq!(scan.commits, 4);
        assert_eq!(scan.findings.len(), 1, "{scan:?}");
        let f = &scan.findings[0];
        assert_eq!(f.id, format!("{}:get.yml:2", c1.oid));
        assert_eq!(f.first, c1);
        assert_eq!(
            f.commits,
            vec![c1.oid.clone(), c4.oid.clone(), c2.oid.clone()]
        );
        assert_eq!(f.branches, vec!["main".to_owned(), "side".to_owned()]);
        assert!(!f.in_head, "HEAD (c3) has the variable");
        assert!(!serde_json::to_string(&scan).unwrap().contains(&t));
    }
}
