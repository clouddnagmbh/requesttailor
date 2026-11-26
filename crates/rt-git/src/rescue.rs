// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Secret rescue (`rt_app::rescue`, wave 12, strand G) on the core's history walk and rewrite:
//!
//! - [`scan_history`] hands each commit and each blob of the collection once to
//!   [`HistoryScanner`] (the web app does the same on isomorphic-git), through
//!   `Repo::walk_history`. Backups (`refs/requesttailor/backup/…`), remote-tracking refs and
//!   tags are not scanned.
//! - [`rewrite_history`] refuses during a merge, on a detached `HEAD` and with local changes in
//!   the collection. It reads the value again from the blob of the plan's findings and lets
//!   `Repo::rewrite_blobs` replace it in every text file of the repository that holds it
//!   (inside the collection or not, [`rescue::replace_value`]) with `{{variable}}`; `store`
//!   (the secret store) gets the value before any ref moves, the old tips are saved under
//!   `refs/requesttailor/backup/<unix>/<branch>`.

use rt_app::rescue::{
    self, HistoryScan, HistoryScanner, RescuePlan, RescueResult, RewrittenBranch, ScanProgress,
};
use rt_app::secret_scan;
use tk_git::{GitResult, HistoryEdit, HistoryVisitor, Repo};
use tk_git_types::GitCommit;

/// Hands the walk to the secret scanner (text as `secret_scan::text_of` reads it).
struct Scan(HistoryScanner);

impl HistoryVisitor for Scan {
    fn knows(&self, id: &str) -> bool {
        self.0.knows(id)
    }

    fn blob(&mut self, id: &str, path: &str, content: &[u8]) {
        self.0.add_blob(id, path, secret_scan::text_of(content));
    }

    fn commit(&mut self, commit: GitCommit, parents: Vec<String>, files: Vec<(String, String)>) {
        self.0.add_commit(commit, parents, files);
    }
}

pub(crate) fn scan_history(
    repo: &Repo,
    progress: &mut dyn FnMut(ScanProgress),
) -> GitResult<HistoryScan> {
    let mut scan = Scan(HistoryScanner::new());
    let tips = repo.walk_history(&mut scan, &mut |scanned, total| {
        progress(ScanProgress { scanned, total })
    })?;
    Ok(scan.0.finish(&tips.branches, tips.head.as_deref()))
}

/// Text of the collection file `path` in `commit`, `None` if it is not there.
fn text_at(repo: &Repo, commit: &str, path: &str) -> GitResult<Option<String>> {
    Ok(repo
        .file_at(commit, path)?
        .and_then(|bytes| secret_scan::text_of(&bytes).map(str::to_owned)))
}

pub(crate) fn rewrite_history(
    repo: &Repo,
    plan: &RescuePlan,
    now: i64,
    store: &mut dyn FnMut(&str) -> GitResult<()>,
) -> GitResult<RescueResult> {
    rescue::check_plan(plan)?;
    repo.check_rewrite()?;
    let value = rescue::plan_value(plan, |at| Ok(text_at(repo, &at.commit, &at.path)?))?;
    let mut edit = |bytes: &[u8]| match secret_scan::text_of(bytes) {
        Some(text) if rescue::contains_value(text, &value) => {
            Some(rescue::replace_value(text, &value, &plan.variable).into_bytes())
        }
        _ => None,
    };
    let detail = format!("value replaced by {{{{{}}}}}", plan.variable);
    let what = HistoryEdit {
        what: "secret rescue",
        detail: &detail,
        unchanged: "the value is not in the history of any local branch: scan the history again",
    };
    let done = repo.rewrite_blobs(&mut edit, &what, now, &mut || store(&value))?;
    Ok(RescueResult {
        rewritten: done.rewritten,
        branches: done
            .branches
            .into_iter()
            .map(|b| RewrittenBranch {
                pushed: b.remote.is_some(),
                remote: b.remote,
                old: b.old,
                new: b.new,
                name: b.name,
            })
            .collect(),
        backup: done.backup,
        tags: done.tags,
    })
}
