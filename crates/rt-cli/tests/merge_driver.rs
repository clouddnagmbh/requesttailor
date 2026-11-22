// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt merge-driver` as Git's merge driver (wave 12, gate item 2), with the real `git`:
//! different fields of a request merge cleanly; the same field stops the merge (exit 1) with
//! a valid file holding ours, a comment at the field and the field named on stderr.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RT: &str = env!("CARGO_BIN_EXE_rt");

struct Repo {
    dir: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Repo {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rt-merge-driver-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Self { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.test"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&[
            "config",
            "merge.requesttailor.driver",
            &format!("\"{RT}\" merge-driver %O %A %B %P"),
        ]);
        repo.write(".gitattributes", "*.yml merge=requesttailor\n");
        repo.write(
            "api/opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: API\n",
        );
        repo.write("api/users/get.yml", &request("https://api.test/users", ""));
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "base"]);
        repo
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .env("GIT_CONFIG_GLOBAL", self.dir.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("RT_DATA_DIR", self.dir.join(".rt-data"))
            .output()
            .expect("git runs")
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join(path)).unwrap()
    }

    fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }
}

fn request(url: &str, headers: &str) -> String {
    format!(
        "info:\n  name: Get users\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: {url}\n{headers}  auth: inherit\n\ndocs: |-\n  Lists the users.\n"
    )
}

const HEADER: &str = "  headers:\n    - name: Accept\n      value: application/json\n";

fn collection_reads(dir: &Path) -> rt_core::model::RequestSpec {
    let files = rt_format::FileSet::from_dir(&dir.join("api")).unwrap();
    let doc = rt_format::read_collection(&files).unwrap();
    doc.model()
        .request("users/get.yml")
        .expect("the request is still there")
        .request
        .spec
        .clone()
}

#[test]
fn different_fields_merge_cleanly_through_git() {
    let repo = Repo::new("clean");
    repo.git(&["checkout", "-q", "-b", "theirs"]);
    repo.write(
        "api/users/get.yml",
        &request("https://api.test/users", HEADER),
    );
    repo.commit_all("theirs: header");
    repo.git(&["checkout", "-q", "main"]);
    repo.write(
        "api/users/get.yml",
        &request("https://api.test/v2/users", ""),
    );
    repo.commit_all("ours: url");

    let out = repo.run(&["merge", "--no-edit", "theirs"]);
    assert!(
        out.status.success(),
        "clean merge: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        repo.read("api/users/get.yml"),
        request("https://api.test/v2/users", HEADER),
        "both changes, nothing else"
    );
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

#[test]
fn the_same_field_stops_the_merge_with_a_conflict_per_field() {
    let repo = Repo::new("conflict");
    repo.git(&["checkout", "-q", "-b", "theirs"]);
    repo.write(
        "api/users/get.yml",
        &request("https://theirs.test/users", HEADER),
    );
    repo.commit_all("theirs: url and header");
    repo.git(&["checkout", "-q", "main"]);
    repo.write("api/users/get.yml", &request("https://ours.test/users", ""));
    repo.commit_all("ours: url");

    let out = repo.run(&["merge", "--no-edit", "theirs"]);
    assert!(!out.status.success(), "the merge stops");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("api/users/get.yml: conflict in url"),
        "the driver names the field: {stderr}"
    );
    // Git keeps the conflict stages; the file is valid and holds ours, plus theirs' header.
    let unmerged = repo.git(&["ls-files", "-u"]);
    assert_eq!(unmerged.lines().count(), 3, "{unmerged}");
    let text = repo.read("api/users/get.yml");
    assert!(
        text.contains("# rt merge-driver: conflict in url (ours kept below). Theirs: https://theirs.test/users"),
        "{text}"
    );
    let spec = collection_reads(&repo.dir);
    assert_eq!(spec.url, "https://ours.test/users");
    assert_eq!(spec.headers.len(), 1, "theirs' header merged in");
}

#[test]
fn the_driver_runs_on_its_own_and_explains_the_setup() {
    let out = Command::new(RT)
        .args(["merge-driver", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(
        help.contains("git config merge.requesttailor.driver \"rt merge-driver %O %A %B %P\""),
        "{help}"
    );
    assert!(help.contains("*.yml merge=requesttailor"), "{help}");

    let out = Command::new(RT)
        .args(["merge-driver", "only-one"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(rt_cli::EXIT_USAGE));

    // Outside a collection, files merge line by line.
    let dir = std::env::temp_dir().join(format!("rt-merge-driver-plain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = |name: &str, text: &str| {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    };
    let (o, a, b) = (
        f("o", "a\nb\nc\n"),
        f("a", "A\nb\nc\n"),
        f("b", "a\nb\nC\n"),
    );
    let out = Command::new(RT)
        .current_dir(&dir)
        .args(["merge-driver"])
        .args([&o, &a, &b])
        .arg("notes.txt")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "A\nb\nC\n");
    let _ = std::fs::remove_dir_all(&dir);
}
