// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gate flow of Git in the desktop shell, without a window, through `Core` and the
//! functions behind the Tauri commands (`rt_desktop::git`): init, commit, push to a bare
//! remote, create and switch branches (the reopened collection shows the other branch's
//! requests), refusal while there are unsaved changes, clone, pull, credentials and identity.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::git::{GitFileState, GitIdentity, GitPullResult};
use rt_app::{AppError, SecretKey, SecretStore, TreeNode};
use rt_desktop::Core;
use rt_desktop::git;

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
        self.0.lock().unwrap().insert(key.clone(), value.to_owned());
        Ok(())
    }
    fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("rt-desktop-git-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(rt_git::canonical(&dir).unwrap())
}

fn append_config(work: &Path, text: &str) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(work.join(".git/config"))
        .unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

fn set_user(work: &Path, name: &str) {
    append_config(
        work,
        &format!("[user]\n\tname = {name}\n\temail = {name}@example.com\n"),
    );
}

fn requests(node: &TreeNode) -> Vec<String> {
    match node {
        TreeNode::Request { path, .. } => vec![path.clone()],
        TreeNode::Folder { children, .. } => children.iter().flat_map(requests).collect(),
    }
}

fn changed(core: &Core<MemorySecrets>) -> Vec<String> {
    git::status(core)
        .unwrap()
        .files
        .into_iter()
        .map(|f| f.path)
        .collect()
}

#[test]
fn commit_push_switch_branches_clone_and_pull() {
    let remote = temp("remote");
    gix::init_bare(&remote.0).unwrap();
    let work = temp("work");
    std::fs::write(
        work.0.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Git flow\n",
    )
    .unwrap();

    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&work.0).unwrap();
    assert!(!git::status(&core).unwrap().repo);
    git::init(&core).unwrap();
    set_user(&work.0, "Ada");
    append_config(
        &work.0,
        &format!(
            "[remote \"origin\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
            remote.0.display().to_string().replace('\\', "/")
        ),
    );
    assert_eq!(
        git::identity(&core).unwrap().map(|i| i.name).as_deref(),
        Some("Ada")
    );

    // Commit the collection with one request, push it.
    let ping = core.create_request("", "Ping").unwrap();
    core.save().unwrap();
    let status = git::status(&core).unwrap();
    assert!(status.repo);
    assert!(
        status
            .files
            .iter()
            .all(|f| f.state == GitFileState::Untracked)
    );
    let all = changed(&core);
    assert!(
        all.contains(&ping) && all.contains(&"opencollection.yml".to_owned()),
        "{all:?}"
    );
    git::commit(&core, "Add the collection", &all).unwrap();
    assert!(changed(&core).is_empty());
    git::push(&core).unwrap();
    let main = git::status(&core).unwrap().branch.unwrap();
    assert_eq!(
        git::status(&core).unwrap().upstream,
        Some(format!("origin/{main}"))
    );

    // A feature branch with a second request, pushed too.
    git::create_branch(&core, "feature", true).unwrap();
    let extra = core.create_request("", "Feature only").unwrap();
    core.save().unwrap();
    git::commit(&core, "Feature request", std::slice::from_ref(&extra)).unwrap();
    git::push(&core).unwrap();
    assert_eq!(git::log(&core, 10).unwrap().len(), 2);

    // Switching branches reopens the collection with the other branch's requests.
    git::checkout(&core, &main).unwrap();
    let tree = requests(&core.tree().unwrap());
    assert_eq!(tree, std::slice::from_ref(&ping));
    git::checkout(&core, "feature").unwrap();
    let tree = requests(&core.tree().unwrap());
    assert!(tree.contains(&ping) && tree.contains(&extra), "{tree:?}");
    let names: Vec<_> = git::branches(&core)
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert!(names.contains(&"feature".to_owned()) && names.contains(&format!("origin/{main}")));

    // Unsaved changes block everything that rewrites files.
    let mut spec = core.get_request(&ping).unwrap();
    spec.url = "https://example.com/unsaved".into();
    core.update_request(&ping, spec).unwrap();
    assert!(matches!(git::checkout(&core, &main), Err(AppError::Git(m)) if m.contains("save")));
    assert!(matches!(git::pull(&core), Err(AppError::Git(m)) if m.contains("save")));
    assert_eq!(
        requests(&core.tree().unwrap()).len(),
        2,
        "nothing was switched"
    );
    core.save().unwrap();
    let diff = git::diff(&core, &ping).unwrap();
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.text.contains("unsaved"))
    );
    git::commit(&core, "Point Ping elsewhere", std::slice::from_ref(&ping)).unwrap();
    git::push(&core).unwrap();

    // A second machine: clone opens the collection; pull brings the next change.
    let parent = temp("clone");
    let other = Core::new(MemorySecrets::default()).unwrap();
    let info = git::clone_into(&other, &rt_git::local_url(&remote.0), &parent.0).unwrap();
    assert_eq!(info.name, "Git flow");
    let cloned = other.root().unwrap();
    set_user(&cloned, "Bob");
    assert_eq!(
        requests(&other.tree().unwrap()),
        std::slice::from_ref(&ping)
    );
    git::checkout(&other, "origin/feature").unwrap();
    assert_eq!(requests(&other.tree().unwrap()).len(), 2);
    assert_eq!(
        other.get_request(&ping).unwrap().url,
        "https://example.com/unsaved"
    );

    let mut spec = core.get_request(&extra).unwrap();
    spec.url = "https://example.com/from-ada".into();
    core.update_request(&extra, spec).unwrap();
    core.save().unwrap();
    git::commit(&core, "Ada's change", std::slice::from_ref(&extra)).unwrap();
    git::push(&core).unwrap();
    assert_eq!(
        git::pull(&other).unwrap(),
        GitPullResult::FastForward { commits: 1 }
    );
    assert_eq!(
        other.get_request(&extra).unwrap().url,
        "https://example.com/from-ada",
        "pull reopened the collection"
    );
}

#[test]
fn credentials_and_identity_through_the_shell() {
    let data = temp("data");
    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.0.clone())).unwrap();
    assert!(!git::has_credentials(&core, "github.com").unwrap());
    git::set_credentials(&core, "github.com", "ada", "token").unwrap();
    assert!(git::has_credentials(&core, "github.com").unwrap());
    // Kept apart from collection secrets.
    let keys: Vec<SecretKey> = core.secrets().0.lock().unwrap().keys().cloned().collect();
    assert!(keys.iter().all(|k| k.collection == "git:credentials"));
    git::delete_credentials(&core, "github.com").unwrap();
    assert!(!git::has_credentials(&core, "github.com").unwrap());

    let me = GitIdentity {
        name: "App Identity".into(),
        email: "app@example.com".into(),
    };
    git::set_identity(&core, &me).unwrap();
    assert!(data.0.join(rt_git::identity::FILE).is_file());

    // In a repository without a configured user (and when the global configuration has
    // none either) commits fall back to the app's identity.
    let work = temp("identity");
    std::fs::write(
        work.0.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Identity\n",
    )
    .unwrap();
    core.open(&work.0).unwrap();
    git::init(&core).unwrap();
    let configured = rt_git::global_identity(&rt_git::Options::default());
    let expected = configured.clone().unwrap_or(me);
    assert_eq!(git::identity(&core).unwrap(), Some(expected.clone()));
    let commit = git::commit(&core, "first", &["opencollection.yml".to_owned()]).unwrap();
    assert_eq!(commit.author, expected.name);
}
