// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance (wave 4): the import corpus imports completely and every loss is in the report.
//!
//! Corpus: `testdata/import/<source>/`. A case is one input file (also a `.zip` of a
//! multi-file OpenAPI spec), or a folder `<name>.bundle/` whose files are imported together (a
//! Postman collection with its environments, several `.http` files with
//! `http-client.env.json`, a multi-file OpenAPI spec with subfolders, ...). Next to each case,
//! `<case>.expected/` holds the snapshot: `report.json` (the loss report) and `collection/`
//! (every file of the imported collection), compared byte for byte. `RT_UPDATE_SNAPSHOTS=1` rewrites them.
//!
//! `testdata/import/invalid/` holds files that must be refused with an error (malformed or
//! unsupported input); their snapshot is `<case>.expected/error.txt`.
//!
//! Every imported collection must read back without issues, and writing it again must give
//! the same bytes (round trip).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rt_format::import::{ImportFormat, SourceFile, import_files};
use rt_format::{FileSet, read_collection};

const SOURCES: &[(&str, ImportFormat)] = &[
    ("postman", ImportFormat::Postman),
    ("insomnia", ImportFormat::Insomnia),
    ("openapi", ImportFormat::Openapi),
    ("curl", ImportFormat::Curl),
    ("har", ImportFormat::Har),
    ("http", ImportFormat::Http),
];

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/import")
}

fn update() -> bool {
    std::env::var_os("RT_UPDATE_SNAPSHOTS").is_some_and(|v| v == "1")
}

/// Input cases of a source folder: files and `.bundle` folders, sorted.
fn cases(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if name.starts_with('.') || name.ends_with(".expected") || name.ends_with(".md") {
                return false;
            }
            if p.is_dir() {
                name.ends_with(".bundle")
            } else {
                true
            }
        })
        .collect();
    out.sort();
    out
}

/// The files of a case, as `(name, bytes)`; files in subfolders of a bundle (a multi-file
/// OpenAPI spec) are named by their `/`-separated path within it.
fn inputs(case: &Path) -> Vec<(String, Vec<u8>)> {
    if case.is_dir() {
        read_tree(case).into_iter().collect()
    } else {
        vec![(
            case.file_name().unwrap().to_string_lossy().into_owned(),
            std::fs::read(case).unwrap(),
        )]
    }
}

fn run(case: &Path) -> Result<rt_format::import::Import, rt_format::import::ImportError> {
    let inputs = inputs(case);
    let files: Vec<SourceFile<'_>> = inputs
        .iter()
        .map(|(name, data)| SourceFile { name, data })
        .collect();
    import_files(None, &files)
}

fn expected_dir(case: &Path) -> PathBuf {
    let mut name = case.file_name().unwrap().to_os_string();
    name.push(".expected");
    case.with_file_name(name)
}

/// All files below `dir`, keyed by `/`-separated relative path.
fn read_tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap();
                let key = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(key, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn write_tree(dir: &Path, files: &BTreeMap<String, Vec<u8>>) {
    let _ = std::fs::remove_dir_all(dir);
    for (path, bytes) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, bytes).unwrap();
    }
}

/// Compares `got` with the snapshot below `dir` (or rewrites it); returns the differences.
fn compare(dir: &Path, got: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    if update() {
        write_tree(dir, got);
        return Vec::new();
    }
    let want = read_tree(dir);
    let mut diffs = Vec::new();
    for (path, bytes) in got {
        match want.get(path) {
            None => diffs.push(format!("{path}: not in the snapshot")),
            Some(w) if w != bytes => diffs.push(format!(
                "{path}: differs\n--- snapshot\n{}\n--- imported\n{}",
                String::from_utf8_lossy(w),
                String::from_utf8_lossy(bytes)
            )),
            Some(_) => {}
        }
    }
    for path in want.keys() {
        if !got.contains_key(path) {
            diffs.push(format!("{path}: in the snapshot but not imported"));
        }
    }
    diffs
}

fn round_trip(files: &FileSet) -> Result<(), String> {
    let doc = read_collection(files).map_err(|e| e.to_string())?;
    if !doc.issues().is_empty() {
        return Err(format!("issues: {:?}", doc.issues()));
    }
    if doc.write() != *files {
        return Err("writing the read collection changes the files".into());
    }
    Ok(())
}

/// Snapshot of one successful case.
fn snapshot(case: &Path, format: ImportFormat) -> Vec<String> {
    let label = case.strip_prefix(corpus()).unwrap().display().to_string();
    let import = match run(case) {
        Ok(i) => i,
        Err(e) => return vec![format!("{label}: import failed: {e}")],
    };
    let mut problems = Vec::new();
    if import.report.format != format {
        problems.push(format!(
            "{label}: detected as {:?}, expected {format:?}",
            import.report.format
        ));
    }
    if let Err(e) = round_trip(&import.files) {
        problems.push(format!("{label}: round trip: {e}"));
    }
    let mut got = BTreeMap::new();
    let mut report = serde_json::to_string_pretty(&serde_json::json!({
        "collectionName": import.collection_name,
        "report": import.report,
    }))
    .unwrap();
    report.push('\n');
    got.insert("report.json".to_owned(), report.into_bytes());
    for (path, bytes) in &import.files.files {
        got.insert(format!("collection/{path}"), bytes.clone());
    }
    problems.extend(
        compare(&expected_dir(case), &got)
            .into_iter()
            .map(|d| format!("{label}: {d}")),
    );
    problems
}

#[test]
fn every_corpus_file_imports_and_matches_its_snapshot() {
    let mut problems = Vec::new();
    let mut failed = 0;
    let mut count = 0;
    for (source, format) in SOURCES {
        let dir = corpus().join(source);
        let cases = cases(&dir);
        assert!(!cases.is_empty(), "{source}: no corpus files");
        for case in cases {
            let p = snapshot(&case, *format);
            failed += usize::from(!p.is_empty());
            problems.extend(p);
            count += 1;
        }
    }
    assert!(
        problems.is_empty(),
        "{failed} of {count} cases failed:\n{}",
        problems.join("\n\n")
    );
}

#[test]
fn invalid_files_are_refused_with_a_clear_error() {
    let dir = corpus().join("invalid");
    let mut problems = Vec::new();
    for case in cases(&dir) {
        let label = case.file_name().unwrap().to_string_lossy().into_owned();
        let result = std::panic::catch_unwind(|| run(&case));
        let got = match result {
            Err(_) => {
                problems.push(format!("{label}: panicked"));
                continue;
            }
            Ok(Ok(_)) => {
                problems.push(format!("{label}: imported, but should be refused"));
                continue;
            }
            Ok(Err(e)) => format!("{e}\n"),
        };
        let mut files = BTreeMap::new();
        files.insert("error.txt".to_owned(), got.into_bytes());
        problems.extend(
            compare(&expected_dir(&case), &files)
                .into_iter()
                .map(|d| format!("{label}: {d}")),
        );
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

/// Every source format has at least one case in which nothing is lost, so a report without
/// losses is actually reachable.
#[test]
fn reports_without_losses_exist() {
    for (source, _) in SOURCES {
        let complete = cases(&corpus().join(source))
            .iter()
            .filter_map(|c| run(c).ok())
            .any(|i| i.report.is_complete());
        assert!(complete, "{source}: no case imports without losses");
    }
}

/// Prints the import of one case: `RT_IMPORT_CASE=postman/x.json cargo test -p rt-format
/// --test import_corpus print_case -- --ignored --nocapture`.
#[test]
#[ignore = "debugging aid"]
fn print_case() {
    let Some(case) = std::env::var_os("RT_IMPORT_CASE") else {
        return;
    };
    match run(&corpus().join(case)) {
        Ok(i) => {
            for (path, bytes) in &i.files.files {
                println!("===== {path}\n{}", String::from_utf8_lossy(bytes));
            }
            println!("{}", serde_json::to_string_pretty(&i.report).unwrap());
        }
        Err(e) => println!("error: {e}"),
    }
}
