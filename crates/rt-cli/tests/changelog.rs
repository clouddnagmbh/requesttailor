// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt changelog <rev>..<rev>` (wave 12, strand F) on the built binary, against a temporary Git
//! repository with two versions of the spec and the collection of `testdata/contract`.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/contract")
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.test"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn copy_collection(to: &Path) {
    let from = fixtures().join("collection");
    for entry in walk(&from) {
        let rel = entry.strip_prefix(&from).unwrap();
        let target = to.join(rel);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&entry, &target).unwrap();
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// A repository whose collection (at `sub`, or at the root) has `openapi.yaml` = `first`, then
/// `second`. The collection names no spec, so the candidates apply.
fn repo(name: &str, sub: Option<&str>, first: &str, second: &str) -> Dir {
    let root = std::env::temp_dir().join(format!("rt-changelog-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let col = sub.map_or(root.clone(), |s| root.join(s));
    std::fs::create_dir_all(&col).unwrap();
    copy_collection(&col);
    std::fs::write(
        col.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: Orders\n",
    )
    .unwrap();
    git(&root, &["init", "-q"]);
    let spec = |name: &str| std::fs::read(fixtures().join("specs").join(name)).unwrap();
    std::fs::write(root.join("openapi.yaml"), spec(first)).unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "v1"]);
    std::fs::write(root.join("openapi.yaml"), spec(second)).unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "v2"]);
    Dir(root)
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn rt(cwd: &Path, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("changelog")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run rt");
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[test]
fn breaking_changes_are_listed_with_their_requests() {
    let r = repo("breaking", None, "v1.yaml", "v3.yaml");
    let out = rt(&r.0, &["HEAD~1..HEAD"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.starts_with("# Orders API 1.0.0 → 2.0.0\n"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("## Breaking changes"), "{}", out.stdout);
    assert!(
        out.stdout.contains("`DELETE /orders/{id}`"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("`orders/delete-order.yml`"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("region"), "{}", out.stdout);
    // The collection did not change, only the spec.
    assert!(!out.stdout.contains("## Collection"), "{}", out.stdout);
}

#[test]
fn a_compatible_change_says_none() {
    let r = repo("compatible", None, "v1.yaml", "v2.yaml");
    let out = rt(&r.0, &["HEAD~1..HEAD"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.contains("## Breaking changes\n\nNone."),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("## Other changes"), "{}", out.stdout);
    assert!(
        out.stdout.contains("GET /orders/{id}/items"),
        "{}",
        out.stdout
    );
}

#[test]
fn a_collection_in_a_subfolder_and_the_out_file() {
    let r = repo("sub", Some("api"), "v1.yaml", "v3.yaml");
    let file = r.0.join("CHANGELOG-API.md");
    let out = rt(
        &r.0,
        &["HEAD~1..HEAD", "api", "--out", file.to_str().unwrap()],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("`orders/delete-order.yml`"), "{text}");
}

#[test]
fn a_range_is_required_and_revisions_must_exist() {
    let r = repo("usage", None, "v1.yaml", "v2.yaml");
    let out = rt(&r.0, &[]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("<rev>..<rev>"), "{}", out.stderr);
    let out = rt(&r.0, &["HEAD"]);
    assert_eq!(out.code, 2);
    let out = rt(&r.0, &["nope..HEAD"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("nope"), "{}", out.stderr);
}

#[test]
fn a_removed_spec_breaks_and_collection_changes_are_listed() {
    let r = repo("nospec", None, "v1.yaml", "v2.yaml");
    std::fs::remove_file(r.0.join("openapi.yaml")).unwrap();
    std::fs::write(
        r.0.join("health.yml"),
        "info:\n  name: Health\n  type: http\n  seq: 5\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/ping\"\n",
    )
    .unwrap();
    git(&r.0, &["add", "-A"]);
    git(&r.0, &["commit", "-q", "-m", "drop spec"]);
    let out = rt(&r.0, &["HEAD~1..HEAD"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    // The spec disappeared: every operation is removed, which breaks.
    assert!(out.stdout.contains("## Breaking changes"), "{}", out.stdout);
    assert!(out.stdout.contains("health.yml"), "{}", out.stdout);
}

/// What a distribution appends (wave 13): here, whether `service/metadata.xml` changed.
struct MetadataSection;

impl rt_app::contract::ChangelogSection for MetadataSection {
    fn section(
        &self,
        old: &rt_app::contract::Side<'_>,
        new: &rt_app::contract::Side<'_>,
        from: &str,
        to: &str,
    ) -> Result<Option<String>, rt_app::AppError> {
        let path = "service/metadata.xml";
        // `files` holds the collection files; others are read like the spec.
        let (a, b) = ((old.read)(path)?, (new.read)(path)?);
        Ok((a != b).then(|| format!("## Service metadata\n\n`{path}` changed ({from} → {to}).\n")))
    }
}

/// Says nothing, so nothing is added.
struct Silent;

impl rt_app::contract::ChangelogSection for Silent {
    fn section(
        &self,
        _: &rt_app::contract::Side<'_>,
        _: &rt_app::contract::Side<'_>,
        _: &str,
        _: &str,
    ) -> Result<Option<String>, rt_app::AppError> {
        Ok(None)
    }
}

#[test]
fn a_distribution_appends_its_sections_also_without_a_spec() {
    let r = repo("sections", None, "v1.yaml", "v2.yaml");
    std::fs::remove_file(r.0.join("openapi.yaml")).unwrap();
    std::fs::create_dir_all(r.0.join("service")).unwrap();
    std::fs::write(r.0.join("service/metadata.xml"), "<edmx v=\"1\"/>").unwrap();
    git(&r.0, &["add", "-A"]);
    git(&r.0, &["commit", "-q", "-m", "metadata 1"]);
    std::fs::write(r.0.join("service/metadata.xml"), "<edmx v=\"2\"/>").unwrap();
    git(&r.0, &["add", "-A"]);
    git(&r.0, &["commit", "-q", "-m", "metadata 2"]);

    let (mut out, mut err) = (Vec::new(), Vec::new());
    let sections: &[&dyn rt_app::contract::ChangelogSection] = &[&Silent, &MetadataSection];
    let code = rt_cli::run_distribution(
        ["changelog", "HEAD~1..HEAD", r.0.to_str().unwrap()],
        &mut out,
        &mut err,
        &rt_cli::Distribution {
            changelog: sections,
            ..Default::default()
        },
    );
    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        out.ends_with(
            "\n\n## Service metadata\n\n`service/metadata.xml` changed (HEAD~1 → HEAD).\n"
        ),
        "{out}"
    );
    // The builds of this repository add nothing.
    let mut plain = Vec::new();
    assert_eq!(
        rt_cli::run(
            ["changelog", "HEAD~1..HEAD", r.0.to_str().unwrap()],
            &mut plain,
            &mut Vec::new()
        ),
        0
    );
    assert!(
        !String::from_utf8(plain)
            .unwrap()
            .contains("Service metadata")
    );
}
