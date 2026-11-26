// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Resolving a merge per field (wave 12, strand B): nothing about the conflicts is stored;
//! [`merge_state`] recomputes them from the index stages (`Repo::conflict_texts`) with
//! `rt_app::merges`, so a merge started on the command line shows the same, and
//! [`resolve_merge`] writes the files with the user's choices (`Repo::write_resolved`).

use rt_app::git::GitTreeFiles as _;
use rt_app::merges::{ConflictChoice, ConflictStages, FileSet, MergeState};
use tk_git::{GitError, GitResult, Repo};

/// The conflicted files of the collection with their index stages.
fn conflict_stages(repo: &Repo) -> GitResult<Vec<ConflictStages>> {
    Ok(repo
        .conflict_texts()?
        .into_iter()
        .map(|c| ConflictStages {
            path: c.path,
            base: c.base,
            ours: c.ours,
            theirs: c.theirs,
        })
        .collect())
}

/// The collection at `HEAD`, the context for recomputing conflicts.
fn head_files(repo: &Repo) -> GitResult<FileSet> {
    Ok(match repo.head_oid()? {
        Some(id) => repo.tree_at(&id)?.to_file_set(),
        None => Default::default(),
    })
}

pub(crate) fn merge_state(repo: &Repo) -> GitResult<MergeState> {
    let merging = repo.in_progress()?;
    let stages = conflict_stages(repo)?;
    if stages.is_empty() {
        return Ok(MergeState {
            merging,
            ..MergeState::default()
        });
    }
    Ok(rt_app::merges::state(&head_files(repo)?, &stages, merging)?)
}

pub(crate) fn resolve_merge(repo: &Repo, choices: &[ConflictChoice]) -> GitResult<Vec<String>> {
    if !repo.in_progress()? {
        return Err(GitError::failed("no merge is in progress"));
    }
    let stages = conflict_stages(repo)?;
    let files = rt_app::merges::resolution(&head_files(repo)?, &stages, choices)?;
    repo.write_resolved(&files)?;
    Ok(files.into_iter().map(|(p, _)| p).collect())
}
