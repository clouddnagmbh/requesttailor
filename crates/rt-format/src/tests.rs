// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Proof that writing is produced from the parsed tree and not a copy of the input: a value
//! changed in the document shows up in the output, and nothing else changes.

use std::path::Path;

use super::*;

fn corpus(name: &str) -> FileSet {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/collections/bruno")
        .join(name);
    FileSet::from_dir(&dir).unwrap()
}

fn set_str(doc: &mut CollectionDoc, file: &str, path: &[&str], value: &str) {
    let yaml = doc.yaml_mut(file).unwrap();
    let newline = yaml.newline;
    let mut node = &mut yaml.root;
    for key in path {
        node = node.get_mut(key).unwrap();
    }
    node.as_scalar_mut().unwrap().set_str(value, newline);
}

/// The lines of `a` and `b` that differ, as (line number, a, b).
fn changed_lines<'s>(a: &'s str, b: &'s str) -> Vec<(usize, &'s str, &'s str)> {
    assert_eq!(a.lines().count(), b.lines().count());
    a.lines()
        .zip(b.lines())
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(n, (x, y))| (n + 1, x, y))
        .collect()
}

#[test]
fn changed_value_appears_in_output_and_nothing_else() {
    let files = corpus("response-example-try-example");
    let mut doc = read_collection(&files).unwrap();
    set_str(
        &mut doc,
        "try-example.yml",
        &["http", "url"],
        "{{baseUrl}}/api/echo/changed",
    );
    let out = doc.write();
    for (path, bytes) in &files.files {
        if path != "try-example.yml" {
            assert_eq!(&out.files[path], bytes, "{path} must not change");
        }
    }
    let before = std::str::from_utf8(&files.files["try-example.yml"]).unwrap();
    let after = std::str::from_utf8(&out.files["try-example.yml"]).unwrap();
    assert_eq!(
        changed_lines(before, after),
        [(
            8,
            "  url: http://localhost:8081/api/echo/anything/try?version={{apiVersion}}",
            // Rendered the way Bruno writes it: a leading `{` needs quotes.
            "  url: \"{{baseUrl}}/api/echo/changed\"",
        )]
    );
    // Reading the output back yields the new value.
    let reread = read_collection(&out).unwrap();
    let r = reread.model().request("try-example.yml").unwrap().request;
    assert_eq!(r.spec.url, "{{baseUrl}}/api/echo/changed");
}

#[test]
fn changed_script_keeps_block_style_and_surroundings() {
    let files = corpus("scripting-crlf-line-comment-crlf-line-comment");
    let path = COLLECTION_FILE;
    let mut doc = read_collection(&files).unwrap();
    let yaml = doc.yaml_mut(path).unwrap();
    let scripts = yaml
        .root
        .get_mut("request")
        .and_then(|r| r.get_mut("scripts"))
        .unwrap();
    let yaml::Node::Seq(seq) = scripts else {
        panic!("scripts is a sequence")
    };
    let code = seq.items[0].value.get_mut("code").unwrap();
    let code = code.as_scalar_mut().unwrap();
    // The CRLF line endings inside the script force a double-quoted scalar, as in Bruno.
    assert!(code.value.contains("\r\n"));
    code.set_str("console.log(1);\nconsole.log(2);", "\n");
    let after = String::from_utf8(doc.write().files[path].clone()).unwrap();
    assert_eq!(
        after,
        "opencollection: \"1.0.0\"\ninfo:\n  name: crlf-line-comment\nrequest:\n  scripts:\n    - type: before-request\n      # Intentional: the \\r escapes below produce CRLF line endings, which this test depends on.\n      # Keep this as a double-quoted scalar; a block scalar (| or >) would silently drop them.\n      code: |-\n        console.log(1);\n        console.log(2);\n"
    );
}
