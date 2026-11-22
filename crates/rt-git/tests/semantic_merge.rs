// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The app's pull merges on the model (wave 12, strand B): different fields of one request
//! merge even on neighbouring lines; the same field stops the merge with a conflict per field
//! that `merge_state` recomputes from the index and `resolve_merge` decides; a request one
//! side renamed takes the other side's changes.

mod common;

use common::*;
use rt_app::git::{GitFileState, GitPullResult};
use rt_app::merges::{ConflictChoice, Side};
use rt_format::diff::{ChangeTarget, DiffValue};
use rt_git::{NoCredentials, Repo};

const COLLECTION: &str = "opencollection: 1.0.0\ninfo:\n  name: Shared\n";

fn request(method: &str, url: &str) -> String {
    format!(
        "info:\n  name: Get user\n  type: http\n  seq: 1\n\nhttp:\n  method: {method}\n  url: {url}\n  auth: inherit\n\ndocs: |-\n  Reads one user.\n"
    )
}

/// Ada pushed the collection; Bob cloned it.
fn setup(name: &str) -> (Tmp, Tmp, Tmp, std::path::PathBuf, std::path::PathBuf) {
    let remote = tmp(&format!("{name}-remote"));
    bare(remote.path());
    let work = tmp(&format!("{name}-a"));
    new_repo(work.path(), "Ada");
    add_remote(work.path(), remote.path().to_str().unwrap());
    let a = reopen(work.path());
    write(work.path(), "opencollection.yml", COLLECTION);
    write(
        work.path(),
        "users/get.yml",
        &request("GET", "https://api.test/users/1"),
    );
    a.commit(
        "init",
        &paths(&["opencollection.yml", "users/get.yml"]),
        None,
    )
    .unwrap();
    a.push(&NoCredentials).unwrap();
    let a_dir = work.path().to_path_buf();
    let parent = tmp(&format!("{name}-b"));
    let url = rt_git::local_url(remote.path());
    let b_dir = rt_git::clone(&url, parent.path(), &NoCredentials, &opts()).unwrap();
    set_user(&b_dir, "Bob");
    (remote, work, parent, a_dir, b_dir)
}

fn spec(dir: &std::path::Path, path: &str) -> rt_core::model::RequestSpec {
    let files = rt_format::FileSet::from_dir(dir).unwrap();
    rt_format::read_collection(&files)
        .unwrap()
        .model()
        .request(path)
        .unwrap_or_else(|| panic!("{path} is a request"))
        .request
        .spec
        .clone()
}

#[test]
fn different_fields_on_neighbouring_lines_merge() {
    let (_r, _w, _p, a_dir, b_dir) = setup("fields");
    let (a, b) = (reopen(&a_dir), reopen(&b_dir));
    write(
        &a_dir,
        "users/get.yml",
        &request("POST", "https://api.test/users/1"),
    );
    a.commit("Ada: method", &paths(&["users/get.yml"]), None)
        .unwrap();
    a.push(&NoCredentials).unwrap();
    write(
        &b_dir,
        "users/get.yml",
        &request("GET", "https://api.test/v2/users/1"),
    );
    b.commit("Bob: url", &paths(&["users/get.yml"]), None)
        .unwrap();

    let GitPullResult::Merged { .. } = b.pull(&NoCredentials, None).unwrap() else {
        panic!("a clean merge");
    };
    assert_eq!(
        read(&b_dir, "users/get.yml"),
        request("POST", "https://api.test/v2/users/1")
    );
    let status = b.status().unwrap();
    assert!(!status.merging && status.files.is_empty(), "{status:?}");
    assert_eq!(b.log(1).unwrap()[0].summary, "Merge origin/main into main");
}

#[test]
fn the_same_field_stops_with_a_conflict_per_field_resolved_in_the_app() {
    let (_r, _w, _p, a_dir, b_dir) = setup("same");
    let (a, b) = (reopen(&a_dir), reopen(&b_dir));
    write(
        &a_dir,
        "users/get.yml",
        &request("POST", "https://ada.test/users/1"),
    );
    a.commit("Ada", &paths(&["users/get.yml"]), None).unwrap();
    a.push(&NoCredentials).unwrap();
    write(
        &b_dir,
        "users/get.yml",
        &request("GET", "https://bob.test/users/1"),
    );
    b.commit("Bob", &paths(&["users/get.yml"]), None).unwrap();

    assert_eq!(
        b.pull(&NoCredentials, None).unwrap(),
        GitPullResult::Conflicts {
            paths: paths(&["users/get.yml"])
        }
    );
    // The working file is valid: ours in the conflicted field, theirs' method merged in.
    let text = read(&b_dir, "users/get.yml");
    assert!(
        text.contains("# rt merge-driver: conflict in url"),
        "{text}"
    );
    let s = spec(&b_dir, "users/get.yml");
    assert_eq!(
        (s.method.as_str(), s.url.as_str()),
        ("POST", "https://bob.test/users/1")
    );
    let status = b.status().unwrap();
    assert!(status.merging);
    assert_eq!(status.files[0].state, GitFileState::Conflicted);

    // Recomputed from the index, also after reopening.
    let state = reopen(&b_dir).merge_state().unwrap();
    assert!(state.merging);
    assert!(state.text_conflicts.is_empty());
    assert_eq!(state.conflicts.len(), 1, "{:?}", state.conflicts);
    let c = &state.conflicts[0];
    assert_eq!(c.field, vec!["url".to_string()]);
    assert_eq!(
        c.target,
        ChangeTarget::Request {
            path: "users/get.yml".into()
        }
    );
    assert_eq!(
        c.ours,
        Some(DiffValue::Text {
            text: "https://bob.test/users/1".into()
        })
    );
    assert_eq!(
        c.theirs,
        Some(DiffValue::Text {
            text: "https://ada.test/users/1".into()
        })
    );

    let choice = ConflictChoice::Field {
        target: c.target.clone(),
        field: c.field.clone(),
        side: Side::Theirs,
    };
    assert_eq!(
        b.resolve_merge(&[choice]).unwrap(),
        paths(&["users/get.yml"])
    );
    assert_eq!(
        read(&b_dir, "users/get.yml"),
        request("POST", "https://ada.test/users/1")
    );
    let state = b.merge_state().unwrap();
    assert!(state.merging && state.conflicts.is_empty());
    let status = b.status().unwrap();
    assert!(
        status
            .files
            .iter()
            .all(|f| f.state != GitFileState::Conflicted),
        "{status:?}"
    );
    b.commit("Merge", &[], None).unwrap();
    assert!(!b.status().unwrap().merging);
    assert!(!b.merge_state().unwrap().merging);
}

#[test]
fn a_request_renamed_on_one_side_takes_the_other_sides_change() {
    let (_r, _w, _p, a_dir, b_dir) = setup("rename");
    let (a, b) = (reopen(&a_dir), reopen(&b_dir));
    write(
        &a_dir,
        "users/get.yml",
        &request("GET", "https://api.test/v2/users/1"),
    );
    a.commit("Ada: url", &paths(&["users/get.yml"]), None)
        .unwrap();
    a.push(&NoCredentials).unwrap();
    std::fs::rename(
        b_dir.join("users/get.yml"),
        b_dir.join("users/get-user.yml"),
    )
    .unwrap();
    b.commit(
        "Bob: rename",
        &paths(&["users/get.yml", "users/get-user.yml"]),
        None,
    )
    .unwrap();

    let GitPullResult::Merged { .. } = b.pull(&NoCredentials, None).unwrap() else {
        panic!("a clean merge");
    };
    assert!(!b_dir.join("users/get.yml").exists());
    assert_eq!(
        spec(&b_dir, "users/get-user.yml").url,
        "https://api.test/v2/users/1"
    );
    assert!(b.status().unwrap().files.is_empty());
}

#[test]
fn no_merge_no_conflicts() {
    let (_r, _w, _p, _a_dir, b_dir) = setup("none");
    let b: Repo = reopen(&b_dir);
    let state = b.merge_state().unwrap();
    assert!(!state.merging && state.conflicts.is_empty() && state.text_conflicts.is_empty());
    assert!(b.resolve_merge(&[]).is_err());
}
