// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The request files the importer writes are the files `rt-format` writes: reading and writing
//! gives the same bytes, rewriting a request with its own model changes nothing, and a new
//! request added through `rt-format` has the same text as the importer's.

#![allow(clippy::collapsible_if)]

mod common;

use common::{import, read, testdata};
use rt_format::{FileSet, read_collection};

fn imports() -> Vec<(&'static str, rt_plugin_sdk::api::ImportReply)> {
    vec![
        (
            "calculator",
            import(
                "wsdl",
                vec![testdata("calculator.wsdl"), testdata("types.xsd")],
            ),
        ),
        ("secure", import("wsdl", vec![testdata("secure.wsdl")])),
        ("weather", import("wsdl", vec![testdata("weather20.wsdl")])),
    ]
}

fn files_of(reply: &rt_plugin_sdk::api::ImportReply) -> FileSet {
    FileSet {
        files: reply
            .files
            .iter()
            .map(|(p, t)| (p.clone(), t.clone().into_bytes()))
            .collect(),
    }
}

#[test]
fn read_then_write_is_byte_identical() {
    for (name, reply) in imports() {
        let files = files_of(&reply);
        let doc = read_collection(&files).unwrap();
        assert!(doc.issues().is_empty(), "{name}: {:?}", doc.issues());
        assert_eq!(doc.write(), files, "{name}");
    }
}

#[test]
fn rewriting_a_request_with_its_own_model_changes_nothing() {
    for (name, reply) in imports() {
        let files = files_of(&reply);
        let mut doc = read_collection(&files).unwrap();
        let paths: Vec<String> = reply
            .files
            .iter()
            .map(|(p, _)| p.clone())
            .filter(|p| {
                p.ends_with(".yml")
                    && !p.ends_with("folder.yml")
                    && p != "opencollection.yml"
                    && !p.starts_with("environments/")
            })
            .collect();
        assert!(!paths.is_empty());
        for path in paths {
            let spec = find(doc.model(), &path);
            doc.set_request(&path, &spec).unwrap();
        }
        assert_eq!(doc.write(), files, "{name}");
    }
}

/// The request at `path` of the model (folder directory = folder name in these imports).
fn find(model: &rt_plugin_sdk::model::Collection, path: &str) -> rt_plugin_sdk::model::RequestSpec {
    use rt_plugin_sdk::model::Item;
    let (dir, file) = path.split_once('/').unwrap();
    let stem = file.trim_end_matches(".yml");
    for item in &model.items {
        if let Item::Folder(f) = item {
            if f.path == dir {
                for i in &f.items {
                    if let Item::Request(r) = i {
                        if r.path == path {
                            return r.spec.clone();
                        }
                    }
                }
            }
        }
    }
    panic!("no request {stem} in {dir}");
}

#[test]
fn the_importers_text_is_what_rt_format_writes_for_a_new_request() {
    for (name, reply) in imports() {
        let model = read(&reply);
        let mut doc = read_collection(&FileSet {
            files: [(
                "opencollection.yml".to_owned(),
                b"opencollection: 1.0.0\n\ninfo:\n  name: \"x\"\n".to_vec(),
            )]
            .into_iter()
            .collect(),
        })
        .unwrap();
        let mut folders: Vec<String> = Vec::new();
        for (path, text) in reply.files.iter().filter(|(p, _)| {
            p.ends_with(".yml")
                && p.contains('/')
                && !p.ends_with("folder.yml")
                && !p.starts_with("environments/")
        }) {
            let spec = {
                let mut s = find(&model, path);
                s.seq = None;
                s
            };
            let dir = path.split('/').next().unwrap().to_owned();
            if !folders.contains(&dir) {
                doc.add_folder("", &dir).unwrap();
                folders.push(dir.clone());
            }
            let added = doc.add_request(&dir, &spec).unwrap();
            let theirs = String::from_utf8(doc.write().files[&added].clone()).unwrap();
            assert_eq!(&theirs, text, "{name}: {path}");
        }
    }
}
