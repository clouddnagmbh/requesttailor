// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Bisect with the runner (wave 12): the first commit between a good and a bad one at which a
//! test, an assertion or a request fails. `rt bisect` and the app (`CompareApi.bisect`) share
//! the search; each tested commit is a run at a revision (`compare`), recorded like any run
//! (`timeline`).
//!
//! The candidates are the first-parent chain from `bad` back to `good`
//! (`GitApi.commits(good, bad)`), so merges are tested as a whole.
//!
//! Owner: wave 12 interface; implementation strand C.

use rt_core::runner::RunStep;
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_core::transport::Transport;
use serde::{Deserialize, Serialize};

use crate::compare::{RevisionOutcome, run_snapshot, snapshot_at};
use crate::git::{GitCommit, GitTree};
use crate::runner::RunSnapshot;

/// What to bisect. The send input is passed next to it, as for `RunnerApi.run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectRequest {
    /// A revision where everything passes.
    pub good: String,
    /// A later revision where something fails.
    pub bad: String,
    /// `None` or `""`: the whole collection.
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BisectVerdict {
    Good,
    Bad,
    /// Not testable (the collection cannot be read at this commit, or the folder is missing).
    Skip,
}

/// One tested commit (progress and history of a bisect).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectStep {
    pub commit: GitCommit,
    pub verdict: BisectVerdict,
    /// What failed (request names with their failed test or assertion), for `Bad`.
    pub failed: Vec<String>,
    /// Candidates left after this step.
    pub remaining: u32,
}

/// The result of a bisect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectOutcome {
    /// The first bad commit; `None` if `bad` passes now, or only skipped commits are left.
    pub first_bad: Option<GitCommit>,
    /// Commits between good (excluded) and bad (included).
    pub candidates: u32,
    pub steps: Vec<BisectStep>,
}

/// Searches the first bad commit among `candidates` (newest first, as `GitApi.commits(good,
/// bad)` returns them: `bad` first, `good` excluded) by halving.
///
/// The newest candidate is tested first: if it is not bad there is nothing to find
/// (`first_bad: None`). Then the range between the last good and the first known bad commit
/// is halved; a `Skip` makes the nearest untested neighbour stand in; when only skipped
/// commits are left the outcome has no `first_bad`. `cancelled` is polled before each test;
/// a cancelled search ends with the steps so far and no `first_bad`; the test that was running
/// when it was cancelled is dropped (a run cut short says nothing about its commit).
pub async fn search(
    candidates: Vec<GitCommit>,
    mut test: impl AsyncFnMut(&GitCommit) -> (BisectVerdict, Vec<String>),
    on_step: &mut dyn FnMut(&BisectStep),
    cancelled: &dyn Fn() -> bool,
) -> BisectOutcome {
    // Oldest first: index 0 is the child of `good`.
    let commits: Vec<GitCommit> = candidates.into_iter().rev().collect();
    let total = u32::try_from(commits.len()).unwrap_or(u32::MAX);
    let mut steps: Vec<BisectStep> = Vec::new();
    let done = |first_bad: Option<usize>, steps: Vec<BisectStep>| BisectOutcome {
        first_bad: first_bad.map(|i| commits[i].clone()),
        candidates: total,
        steps,
    };
    if commits.is_empty() || cancelled() {
        return done(None, steps);
    }
    let mut record = |steps: &mut Vec<BisectStep>,
                      i: usize,
                      verdict: BisectVerdict,
                      failed: Vec<String>,
                      remaining: usize| {
        let step = BisectStep {
            commit: commits[i].clone(),
            verdict,
            failed,
            remaining: u32::try_from(remaining).unwrap_or(u32::MAX),
        };
        on_step(&step);
        steps.push(step);
    };

    // The newest commit first.
    let tip = commits.len() - 1;
    let (verdict, failed) = test(&commits[tip]).await;
    if cancelled() {
        // A run cut short says nothing about the commit.
        return done(None, steps);
    }
    record(&mut steps, tip, verdict, failed, tip);
    if verdict != BisectVerdict::Bad {
        return done(None, steps);
    }

    // `lo`: the last commit known good (`None`: `good` itself); `hi`: the first known bad.
    let mut lo: Option<usize> = None;
    let mut hi = tip;
    let mut skipped: Vec<usize> = Vec::new();
    loop {
        let start = lo.map_or(0, |l| l + 1);
        if start >= hi {
            break;
        }
        if cancelled() {
            return done(None, steps);
        }
        // The middle of the untested commits in (lo, hi), or its nearest neighbour.
        let mid = start + (hi - start) / 2;
        let Some(pick) = (0..hi - start)
            .flat_map(|d| [mid.checked_sub(d), Some(mid + d)])
            .flatten()
            .find(|i| (start..hi).contains(i) && !skipped.contains(i))
        else {
            // Only skipped commits are left between good and bad.
            return done(None, steps);
        };
        let (verdict, failed) = test(&commits[pick]).await;
        if cancelled() {
            return done(None, steps);
        }
        match verdict {
            BisectVerdict::Good => lo = Some(pick),
            BisectVerdict::Bad => hi = pick,
            BisectVerdict::Skip => skipped.push(pick),
        }
        let remaining = (lo.map_or(0, |l| l + 1)..hi)
            .filter(|i| !skipped.contains(i))
            .count();
        record(&mut steps, pick, verdict, failed, remaining);
    }
    // Skipped commits just below `hi` could be the first bad one: it is not certain.
    let first = lo.map_or(0, |l| l + 1);
    if (first..hi).any(|i| skipped.contains(&i)) {
        return done(None, steps);
    }
    done(Some(hi), steps)
}

/// What a run says about a commit: `Bad` if a request, test or assertion failed, with what
/// failed (`name: reason`).
pub fn verdict_of(outcome: &RevisionOutcome) -> (BisectVerdict, Vec<String>) {
    let failed: Vec<String> = outcome.steps.iter().filter_map(failure).collect();
    let s = &outcome.summary;
    let bad = !failed.is_empty() || s.failed > 0 || s.tests_failed > 0 || s.assertions_failed > 0;
    (
        if bad {
            BisectVerdict::Bad
        } else {
            BisectVerdict::Good
        },
        failed,
    )
}

fn failure(step: &RunStep) -> Option<String> {
    if step.passed {
        return None;
    }
    let r = &step.report;
    let why = if let Some(a) = r.assertions.iter().find(|a| !a.passed) {
        match &a.actual {
            Some(actual) => format!(
                "{} {} {} (got {actual})",
                a.expression, a.operator, a.expected
            ),
            None => format!("{} {} {}", a.expression, a.operator, a.expected),
        }
    } else if let Some(t) = r.tests.iter().find(|t| !t.passed) {
        match &t.error {
            Some(e) => format!("{}: {e}", t.name),
            None => t.name.clone(),
        }
    } else if r.error.is_some() {
        "the request failed".to_owned()
    } else {
        "failed".to_owned()
    };
    Some(format!("{}: {why}", step.request_name))
}

/// Tests one commit: runs the folder (or the whole collection) at `tree`. A commit whose
/// collection cannot be read, or that has nothing to run, is `Skip`. `on_run` gets the
/// finished run (the time travel records it under the commit, `rt_app::timeline`).
#[allow(clippy::too_many_arguments)]
pub async fn test_revision<T: Transport>(
    today: &RunSnapshot,
    tree: Result<GitTree, String>,
    folder: Option<&str>,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    on_run: &mut dyn FnMut(&RevisionOutcome),
    cancelled: &dyn Fn() -> bool,
) -> (BisectVerdict, Vec<String>) {
    let Ok(tree) = tree else {
        return (BisectVerdict::Skip, Vec::new());
    };
    let Ok(snapshot) = snapshot_at(today, &tree) else {
        return (BisectVerdict::Skip, Vec::new());
    };
    if snapshot.plan(folder).is_empty() {
        return (BisectVerdict::Skip, Vec::new());
    }
    let outcome = run_snapshot(
        &snapshot,
        folder,
        None,
        transport,
        options,
        services,
        &mut |_| {},
        cancelled,
    )
    .await;
    on_run(&outcome);
    verdict_of(&outcome)
}
