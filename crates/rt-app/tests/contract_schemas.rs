// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! GraphQL and Protobuf contracts in the app layer (wave 18, strand H): finding the schema files
//! of a side, comparing two sides (revisions), naming the requests each change affects, and the
//! changelog sections. In memory: a side is a set of files and a reader, as the shells give it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use rt_app::AppError;
use rt_app::contract::{
    ChangelogSection, SchemaKind, SchemaSection, Side, SpecChangeKind, changelog_with,
    schema_changes,
};
use rt_format::FileSet;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/contract")
}

fn collection() -> FileSet {
    FileSet::from_dir(&fixtures().join("schemas-collection")).unwrap()
}

/// The files outside the collection's `*.yml`, as the shell reads them at one version.
type Tree = BTreeMap<String, Vec<u8>>;

fn version(n: u8) -> Tree {
    let mut t = Tree::new();
    let graphql = std::fs::read(fixtures().join(format!("graphql/shop-v{n}.graphql"))).unwrap();
    t.insert("api/shop.graphql".into(), graphql);
    let dir = fixtures().join(format!("protobuf/v{n}"));
    for name in ["orders.proto", "common/money.proto"] {
        t.insert(
            format!("protos/{name}"),
            std::fs::read(dir.join(name)).unwrap(),
        );
    }
    t
}

/// A reader over `tree` that remembers what it was asked for. A leading `/` is the repository
/// root, which is the collection folder here.
fn reader<'a>(
    tree: &'a Tree,
    asked: &'a RefCell<Vec<String>>,
) -> impl Fn(&str) -> Result<Option<Vec<u8>>, AppError> + 'a {
    move |path| {
        asked.borrow_mut().push(path.to_owned());
        Ok(tree.get(path.trim_start_matches('/')).cloned())
    }
}

fn run<T>(old: &Tree, new: &Tree, files: &FileSet, f: impl FnOnce(&Side<'_>, &Side<'_>) -> T) -> T {
    let (a, b) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
    let (ro, rn) = (reader(old, &a), reader(new, &b));
    f(&Side { files, read: &ro }, &Side { files, read: &rn })
}

#[test]
fn graphql_changes_between_two_sides_name_the_requests() {
    let files = collection();
    let c = run(&version(1), &version(3), &files, |o, n| {
        schema_changes(SchemaKind::Graphql, o, n).unwrap()
    });
    let field = c
        .changes
        .iter()
        .find(|x| x.operation.as_deref() == Some("Product.legacyCode"))
        .unwrap();
    assert!(field.breaking);
    assert_eq!(field.kind, SpecChangeKind::FieldRemoved);
    assert_eq!(field.requests, ["graphql/products.yml"]);
    assert_eq!(c.breaking().count(), 16, "{:#?}", c.changes);
}

#[test]
fn protobuf_changes_between_two_sides_name_the_requests() {
    let files = collection();
    let c = run(&version(1), &version(3), &files, |o, n| {
        schema_changes(SchemaKind::Protobuf, o, n).unwrap()
    });
    let watch = c
        .changes
        .iter()
        .find(|x| x.operation.as_deref() == Some("/shop.orders.Orders/Watch"))
        .unwrap();
    assert_eq!(watch.requests, ["grpc/watch.yml"]);
    assert_eq!(c.breaking().count(), 13, "{:#?}", c.changes);
}

#[test]
fn schemas_that_only_grew_break_nothing() {
    let files = collection();
    for kind in [SchemaKind::Graphql, SchemaKind::Protobuf] {
        let c = run(&version(1), &version(2), &files, |o, n| {
            schema_changes(kind, o, n).unwrap()
        });
        assert!(!c.changes.is_empty());
        assert!(c.breaking().next().is_none(), "{kind:?}: {:#?}", c.changes);
    }
}

#[test]
fn the_files_are_found_the_way_the_collection_names_them() {
    let files = collection();
    let (old, new) = (version(1), version(1));
    let asked = RefCell::new(Vec::new());
    let r = reader(&new, &asked);
    let o = Side {
        files: &files,
        read: &|p: &str| Ok(old.get(p.trim_start_matches('/')).cloned()),
    };
    let n = Side {
        files: &files,
        read: &r,
    };
    schema_changes(SchemaKind::Graphql, &o, &n).unwrap();
    // `extensions.requesttailor.contract.graphql` names it: nothing else is looked for
    assert_eq!(*asked.borrow(), ["api/shop.graphql"]);
    asked.borrow_mut().clear();
    schema_changes(SchemaKind::Protobuf, &o, &n).unwrap();
    let asked = asked.borrow();
    assert!(
        asked.iter().any(|p| p == "protos/orders.proto"),
        "{asked:?}"
    );
    // the import is looked up through the import path (and so is found)
    assert!(
        asked.iter().any(|p| p == "protos/common/money.proto"),
        "{asked:?}"
    );
}

fn bare() -> FileSet {
    FileSet {
        files: [(
            "opencollection.yml".to_owned(),
            b"opencollection: 1.0.0\ninfo:\n  name: Bare\n".to_vec(),
        )]
        .into(),
    }
}

#[test]
fn without_a_configuration_the_usual_names_are_tried_next_to_the_collection_then_at_the_root() {
    let files = bare();
    let sdl = std::fs::read(fixtures().join("graphql/shop-v1.graphql")).unwrap();
    for (path, found) in [
        ("schema.graphql", true),
        ("schema.gql", true),
        ("schema.graphqls", true),
        ("/schema.graphql", true),
        ("/schema.gql", true),
        ("/schema.graphqls", true),
        ("other.graphql", false),
    ] {
        let mut tree = Tree::new();
        tree.insert(path.trim_start_matches('/').to_owned(), sdl.clone());
        let c = run(&Tree::new(), &tree, &files, |o, n| {
            schema_changes(SchemaKind::Graphql, o, n).unwrap()
        });
        assert_eq!(!c.changes.is_empty(), found, "{path}");
    }
}

#[test]
fn a_schema_next_to_the_collection_wins_over_the_one_at_the_root() {
    let files = bare();
    let mut tree = Tree::new();
    tree.insert("schema.graphql".into(), b"type Query { a: Int }".to_vec());
    let asked = RefCell::new(Vec::new());
    let r = reader(&tree, &asked);
    let n = Side {
        files: &files,
        read: &r,
    };
    let o = Side {
        files: &files,
        read: &|_: &str| Ok(None),
    };
    let c = schema_changes(SchemaKind::Graphql, &o, &n).unwrap();
    assert_eq!(c.changes.len(), 1);
    assert!(
        !asked.borrow().iter().any(|p| p.starts_with('/')),
        "{:?}",
        asked.borrow()
    );
}

#[test]
fn a_named_schema_that_does_not_exist_is_an_error() {
    let files = collection();
    let none = Tree::new();
    let err = run(&none, &none, &files, |o, n| {
        schema_changes(SchemaKind::Graphql, o, n).unwrap_err()
    });
    assert!(matches!(err, AppError::Format(m) if m.contains("api/shop.graphql")));
}

#[test]
fn a_schema_path_must_stay_inside_the_collection_or_the_repository() {
    let mut files = collection();
    files.files.insert(
        "opencollection.yml".into(),
        b"opencollection: 1.0.0\ninfo:\n  name: X\nextensions:\n  requesttailor:\n    contract:\n      graphql: ../outside.graphql\n".to_vec(),
    );
    let t = Tree::new();
    let err = run(&t, &t, &files, |o, n| {
        schema_changes(SchemaKind::Graphql, o, n).unwrap_err()
    });
    assert!(matches!(err, AppError::Format(_)), "{err:?}");
}

#[test]
fn a_collection_without_protobuf_configuration_has_no_protobuf_contract() {
    let files = bare();
    let t = version(3);
    let c = run(&t, &t, &files, |o, n| {
        schema_changes(SchemaKind::Protobuf, o, n).unwrap()
    });
    assert!(c.changes.is_empty());
}

#[test]
fn a_collection_that_cannot_be_read_is_an_error_not_a_silent_pass() {
    let files = FileSet::default();
    let t = version(1);
    let r = run(&t, &t, &files, |o, n| {
        schema_changes(SchemaKind::Protobuf, o, n)
    });
    assert!(r.is_err());
}

fn sections_text(old: &Tree, new: &Tree, files: &FileSet) -> String {
    run(old, new, files, |o, n| {
        let graphql = SchemaSection::new(SchemaKind::Graphql);
        let protobuf = SchemaSection::new(SchemaKind::Protobuf);
        let sections: [&dyn ChangelogSection; 2] = [&graphql, &protobuf];
        changelog_with(o, n, "v1", "v3", &sections).unwrap()
    })
}

#[test]
fn the_changelog_sections_come_after_the_rest_and_say_which_schema() {
    let text = sections_text(&version(1), &version(3), &collection());
    let (g, p) = (
        text.find("## GraphQL").unwrap(),
        text.find("## Protobuf").unwrap(),
    );
    assert!(text.find("## Breaking changes").unwrap() < g, "{text}");
    assert!(g < p, "{text}");
    let graphql = &text[g..p];
    let protobuf = &text[p..];
    assert!(graphql.contains("### Breaking changes"), "{graphql}");
    assert!(graphql.contains("`Product.legacyCode`"), "{graphql}");
    assert!(
        graphql.contains("(affects `graphql/products.yml`)"),
        "{graphql}"
    );
    assert!(
        protobuf.contains("`/shop.orders.Orders/Watch`"),
        "{protobuf}"
    );
    assert!(
        protobuf.contains("(affects `grpc/watch.yml`)"),
        "{protobuf}"
    );
    // breaking first, then the rest
    assert!(graphql.contains("### Other changes") || !graphql.contains("Other"));
}

#[test]
fn a_section_with_nothing_to_say_is_left_out() {
    let text = sections_text(&version(1), &version(1), &collection());
    assert!(
        !text.contains("## GraphQL") && !text.contains("## Protobuf"),
        "{text}"
    );
    let none = Tree::new();
    let text = sections_text(&none, &none, &bare());
    assert!(
        !text.contains("## GraphQL") && !text.contains("## Protobuf"),
        "{text}"
    );
}

#[test]
fn growing_schemas_have_sections_without_breaking_changes() {
    let text = sections_text(&version(1), &version(2), &collection());
    let g = &text[text.find("## GraphQL").unwrap()..];
    assert!(g.contains("### Breaking changes\n\nNone."), "{g}");
    assert!(g.contains("### Other changes"), "{g}");
}
