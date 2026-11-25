// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt changelog` with GraphQL and Protobuf sections (wave 18, strand H) on the built binary,
//! against a temporary Git repository: the collection of `testdata/contract/schemas-collection`
//! with three versions of its schema files.

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

fn copy_tree(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let target = to.join(p.file_name().unwrap());
        if p.is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_tree(&p, &target);
        } else {
            std::fs::copy(&p, &target).unwrap();
        }
    }
}

fn put_version(root: &Path, n: u8) {
    std::fs::create_dir_all(root.join("api")).unwrap();
    std::fs::copy(
        fixtures().join(format!("graphql/shop-v{n}.graphql")),
        root.join("api/shop.graphql"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("protos/common")).unwrap();
    for name in ["orders.proto", "common/money.proto"] {
        std::fs::copy(
            fixtures().join(format!("protobuf/v{n}/{name}")),
            root.join("protos").join(name),
        )
        .unwrap();
    }
}

/// A repository with the collection at its root and three commits: schema version 1, 2, 3.
fn repo(name: &str, versions: &[u8]) -> Dir {
    let root = std::env::temp_dir().join(format!(
        "rt-changelog-schemas-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_tree(&fixtures().join("schemas-collection"), &root);
    git(&root, &["init", "-q"]);
    for n in versions {
        put_version(&root, *n);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", &format!("v{n}")]);
    }
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
fn breaking_schema_changes_get_their_own_sections_with_the_requests_they_affect() {
    let d = repo("breaking", &[1, 2, 3]);
    let out = rt(&d.0, &["HEAD~2..HEAD"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let text = &out.stdout;
    let g = text
        .find("\n## GraphQL\n")
        .unwrap_or_else(|| panic!("{text}"));
    let p = text
        .find("\n## Protobuf\n")
        .unwrap_or_else(|| panic!("{text}"));
    assert!(g < p);
    let graphql = &text[g..p];
    assert!(graphql.contains("### Breaking changes"), "{graphql}");
    assert!(graphql.contains("`Product.legacyCode`"), "{graphql}");
    assert!(
        graphql.contains("(affects `graphql/products.yml`)"),
        "{graphql}"
    );
    assert!(graphql.contains("`Query.legacyStats`"), "{graphql}");
    let protobuf = &text[p..];
    assert!(
        protobuf.contains("`/shop.orders.Orders/Watch`"),
        "{protobuf}"
    );
    assert!(
        protobuf.contains("(affects `grpc/watch.yml`)"),
        "{protobuf}"
    );
    assert!(
        protobuf.contains("`shop.orders.Order.legacy`"),
        "{protobuf}"
    );
    // the collection's own part is still there (the schema files are not requests)
    assert!(text.starts_with("# Changes HEAD~2 → HEAD"), "{text}");
}

#[test]
fn schemas_that_only_grew_say_so() {
    let d = repo("growing", &[1, 2]);
    let out = rt(&d.0, &["HEAD~1..HEAD"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let g = &out.stdout[out.stdout.find("## GraphQL").unwrap()..];
    assert!(g.contains("### Breaking changes\n\nNone."), "{g}");
    assert!(
        out.stdout.contains("`/shop.orders.Orders/Refund`"),
        "{}",
        out.stdout
    );
}

#[test]
fn unchanged_schemas_add_nothing() {
    let d = repo("same", &[1]);
    std::fs::write(d.0.join("note.txt"), "x").unwrap();
    git(&d.0, &["add", "-A"]);
    git(&d.0, &["commit", "-q", "-m", "note"]);
    let out = rt(&d.0, &["HEAD~1..HEAD"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        !out.stdout.contains("## GraphQL") && !out.stdout.contains("## Protobuf"),
        "{}",
        out.stdout
    );
}

#[test]
fn a_collection_without_schemas_gets_the_changelog_it_always_got() {
    let root =
        std::env::temp_dir().join(format!("rt-changelog-schemas-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let d = Dir(root.clone());
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: Plain\n",
    )
    .unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "one"]);
    std::fs::write(root.join("a.yml"), "info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/a\"\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "two"]);
    let out = rt(&d.0, &["HEAD~1..HEAD"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        !out.stdout.contains("GraphQL") && !out.stdout.contains("Protobuf"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("## Collection"), "{}", out.stdout);
}

#[test]
fn a_schema_that_does_not_parse_stops_the_changelog_with_the_file_named() {
    let d = repo("broken", &[1]);
    std::fs::write(d.0.join("api/shop.graphql"), "type Query { a: }").unwrap();
    git(&d.0, &["add", "-A"]);
    git(&d.0, &["commit", "-q", "-m", "broken"]);
    let out = rt(&d.0, &["HEAD~1..HEAD"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("api/shop.graphql"), "{}", out.stderr);
}

#[test]
fn the_help_mentions_the_schema_sections() {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("--help")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("GraphQL") && text.contains("Protobuf"),
        "{text}"
    );
}
