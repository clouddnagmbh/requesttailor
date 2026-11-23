// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 14, strand A: the collections in the sidebar. Discovery (the one rule every shell
//! walks with) and `scaffold` (a new collection, read back through rt-format).

use rt_app::collections::{
    MAX_COLLECTIONS, MAX_DEPTH, NewCollection, check_new_dir, discover, discover_files, enter,
    scaffold,
};
use rt_app::{AppError, Workspace};
use rt_format::FileSet;

fn spec(name: &str) -> NewCollection {
    NewCollection {
        name: name.into(),
        root: Some("root".into()),
        dir: "orders".into(),
        base_url: None,
        environment: None,
    }
}

fn files_of(changes: &[rt_app::FileChange]) -> FileSet {
    FileSet {
        files: changes
            .iter()
            .map(|c| (c.path.clone(), c.text.clone().into_bytes()))
            .collect(),
    }
}

/// Manifests of a root with three collections in subfolders, one nested in another, and
/// decoys a walk must never enter.
fn sample_root() -> FileSet {
    let manifest = |name: &str| format!("opencollection: \"1.0.0\"\ninfo:\n  name: {name}\n");
    let mut files = FileSet::default();
    for (path, name) in [
        ("billing/opencollection.yml", "Billing"),
        ("services/orders/opencollection.yml", "Orders"),
        // Nested inside Orders: its own node, left out of Orders' files.
        ("services/orders/legacy/opencollection.yml", "Legacy orders"),
        ("node_modules/some-lib/opencollection.yml", "Decoy"),
        (".git/opencollection.yml", "Decoy"),
        ("target/debug/opencollection.yml", "Decoy"),
        ("a/b/c/d/e/opencollection.yml", "Too deep"),
    ] {
        files.files.insert(path.into(), manifest(name).into_bytes());
    }
    files
        .files
        .insert("billing/invoices.yml".into(), b"not a manifest".to_vec());
    files.files.insert(
        "README.md".into(),
        b"# root of several collections".to_vec(),
    );
    files
}

#[test]
fn a_root_with_three_collections_gives_three_nodes() {
    let found = discover_files(&sample_root());
    assert_eq!(
        found.dirs,
        vec!["billing", "services/orders", "services/orders/legacy"]
    );
    assert!(!found.truncated);
}

#[test]
fn a_root_that_is_a_collection_comes_first() {
    let found = discover(["sub", "", "a"]);
    assert_eq!(found.dirs, vec!["", "a", "sub"]);
}

#[test]
fn hidden_dependency_build_and_deep_folders_are_never_entered() {
    assert!(!enter(".github", 1));
    assert!(!enter("node_modules", 3));
    assert!(!enter("target", 2));
    assert!(enter("api", MAX_DEPTH));
    assert!(!enter("api", MAX_DEPTH + 1));
    // A manifest at depth MAX_DEPTH is found, one below is not.
    let found = discover(["a/b/c/d", "a/b/c/d/e"]);
    assert_eq!(found.dirs, vec!["a/b/c/d"]);
}

#[test]
fn discovery_stops_at_the_limit_and_says_so() {
    let dirs: Vec<String> = (0..MAX_COLLECTIONS + 1)
        .map(|i| format!("c{i:03}"))
        .collect();
    let found = discover(&dirs);
    assert_eq!(found.dirs.len(), MAX_COLLECTIONS);
    assert!(found.truncated);
}

#[test]
fn a_folder_without_collections_opens_as_one_new_collection() {
    let mut files = FileSet::default();
    files
        .files
        .insert("notes.txt".into(), b"no collection here".to_vec());
    assert_eq!(discover_files(&files).dirs, vec![""]);
}

#[test]
fn scaffold_writes_a_collection_that_reads_back() {
    let changes = scaffold(&spec("Orders API")).unwrap();
    assert!(changes.iter().all(|c| !c.deleted));
    assert_eq!(
        changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        vec!["opencollection.yml"]
    );
    let ws = Workspace::open(files_of(&changes)).unwrap();
    assert_eq!(ws.collection().name, "Orders API");
    assert!(ws.collection().items.is_empty());
    assert!(ws.environments().is_empty());
    assert!(ws.dirty().is_empty());
    // Written as rt-format writes: reading and writing again gives the same bytes.
    let doc = rt_format::read_collection(&files_of(&changes)).unwrap();
    assert_eq!(doc.write(), files_of(&changes));
}

#[test]
fn scaffold_puts_base_url_into_the_first_environment() {
    let changes = scaffold(&NewCollection {
        base_url: Some("http://localhost:8080".into()),
        environment: Some("Local".into()),
        ..spec("Orders API")
    })
    .unwrap();
    let ws = Workspace::open(files_of(&changes)).unwrap();
    let envs = ws.environments();
    assert_eq!(envs.len(), 1);
    assert_eq!(envs[0].name, "Local");
    let var = envs[0]
        .variables
        .iter()
        .find(|v| v.name == "baseUrl")
        .unwrap();
    assert_eq!(var.value.as_deref(), Some("http://localhost:8080"));
    assert!(!var.secret);
    assert!(
        ws.collection()
            .defaults
            .vars
            .iter()
            .all(|v| v.name != "baseUrl")
    );
}

#[test]
fn scaffold_without_an_environment_keeps_base_url_on_the_collection() {
    let changes = scaffold(&NewCollection {
        base_url: Some("https://api.example.com".into()),
        ..spec("Orders API")
    })
    .unwrap();
    let ws = Workspace::open(files_of(&changes)).unwrap();
    assert!(ws.environments().is_empty());
    let var = ws
        .collection()
        .defaults
        .vars
        .iter()
        .find(|v| v.name == "baseUrl")
        .unwrap();
    assert_eq!(var.value.as_deref(), Some("https://api.example.com"));
}

#[test]
fn scaffold_with_only_an_environment_creates_it_empty() {
    let changes = scaffold(&NewCollection {
        environment: Some("Local".into()),
        ..spec("Orders")
    })
    .unwrap();
    let ws = Workspace::open(files_of(&changes)).unwrap();
    assert_eq!(ws.environments().len(), 1);
    assert!(ws.environments()[0].variables.is_empty());
}

#[test]
fn scaffold_keeps_names_that_need_quoting() {
    let name = "Orders: \"v2\" # internal";
    let changes = scaffold(&spec(name)).unwrap();
    let ws = Workspace::open(files_of(&changes)).unwrap();
    assert_eq!(ws.collection().name, name);
}

#[test]
fn scaffold_refuses_an_empty_name() {
    assert!(matches!(scaffold(&spec("  ")), Err(AppError::Shell(_))));
    assert!(matches!(
        scaffold(&NewCollection {
            environment: Some(" ".into()),
            ..spec("Orders")
        }),
        Err(AppError::Shell(_))
    ));
}

#[test]
fn new_collection_folders_are_where_discovery_finds_them() {
    assert_eq!(check_new_dir("orders").unwrap(), "orders");
    assert_eq!(check_new_dir("/apis/orders/").unwrap(), "apis/orders");
    assert_eq!(check_new_dir("").unwrap(), "");
    for bad in [
        "../outside",
        "a/../b",
        "./a",
        "a\\b",
        ".hidden",
        "node_modules/x",
        "target",
        "a/b/c/d/e",
        "a//b",
        "con:",
    ] {
        assert!(
            matches!(check_new_dir(bad), Err(AppError::Shell(_))),
            "{bad} must be refused"
        );
    }
}
