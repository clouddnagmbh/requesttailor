// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OpenAPI import wizard in `rt_format::import`: choices ([`OpenApiOptions`]), keeping the
//! spec in the collection, Swagger 2.0, multi-file specs and zips, and the overview
//! ([`inspect_openapi`]). The plain import (default options) is the import corpus
//! (`import_corpus.rs`); these tests use the corpus files under `testdata/import/openapi/`.

use std::path::{Path, PathBuf};

use rt_core::model::{Collection, Item};
use rt_format::import::{
    ImportError, ImportFormat, LossKind, OpenApiExamples, OpenApiGrouping, OpenApiNaming,
    OpenApiOptions, OpenApiServerChoice, SourceFile, import, import_files, import_openapi,
    inspect_openapi, openapi_references,
};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/import/openapi")
        .join(name)
}

/// One corpus file as `(name, bytes)`, or every file of a `.bundle` folder by relative path.
fn read(name: &str) -> Vec<(String, Vec<u8>)> {
    let path = corpus(name);
    if !path.is_dir() {
        return vec![(name.to_owned(), std::fs::read(&path).unwrap())];
    }
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy();
                out.push((rel.replace('\\', "/"), std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(&path, &path, &mut out);
    out
}

fn sources(files: &[(String, Vec<u8>)]) -> Vec<SourceFile<'_>> {
    files
        .iter()
        .map(|(name, data)| SourceFile { name, data })
        .collect()
}

fn run(name: &str, options: &OpenApiOptions) -> rt_format::import::Import {
    let files = read(name);
    import_openapi(&sources(&files), options).unwrap()
}

fn model(files: &FileSet) -> Collection {
    let doc = read_collection(files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    doc.model().clone()
}

/// `(folder, request names)` at the top level; `""` for requests at the root.
fn layout(c: &Collection) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut root = Vec::new();
    for item in &c.items {
        match item {
            Item::Folder(f) => out.push((
                f.name.clone(),
                f.items
                    .iter()
                    .filter_map(|i| match i {
                        Item::Request(r) => Some(r.spec.name.clone()),
                        Item::Folder(_) => None,
                    })
                    .collect(),
            )),
            Item::Request(r) => root.push(r.spec.name.clone()),
        }
    }
    if !root.is_empty() {
        out.push((String::new(), root));
    }
    out
}

fn requests(c: &Collection) -> Vec<rt_core::model::RequestSpec> {
    fn walk(items: &[Item], out: &mut Vec<rt_core::model::RequestSpec>) {
        for i in items {
            match i {
                Item::Folder(f) => walk(&f.items, out),
                Item::Request(r) => out.push(r.spec.clone()),
            }
        }
    }
    let mut out = Vec::new();
    walk(&c.items, &mut out);
    out
}

/// The files the format owns read back and written again give the same bytes.
fn round_trips(files: &FileSet) {
    let doc = read_collection(files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    let written = doc.write();
    for (path, bytes) in &written.files {
        assert_eq!(Some(bytes), files.files.get(path), "{path} changed");
    }
}

const PETSHOP: &str = "synthetic-petshop-swagger-2.0.json";
const KENNEL: &str = "synthetic-multi-file-3.0.bundle";

#[test]
fn default_options_are_the_plain_import() {
    for name in [
        "oai-petstore-3.0.yaml",
        "insomnia-oauth2-flows-3.0.yaml",
        PETSHOP,
        "synthetic-zipped-3.1.zip",
    ] {
        let files = read(name);
        let plain = import(None, name, &files[0].1).unwrap();
        let wizard = import_openapi(&sources(&files), &OpenApiOptions::default()).unwrap();
        assert_eq!(plain, wizard, "{name}");
    }
    // A multi-file spec through `import_files` (the shells' plain import) too.
    let files = read(KENNEL);
    assert_eq!(
        import_files(None, &sources(&files)).unwrap(),
        import_openapi(&sources(&files), &OpenApiOptions::default()).unwrap()
    );
}

#[test]
fn only_the_chosen_operations_are_imported() {
    let options = OpenApiOptions {
        operations: Some(vec!["GET /pets".into(), "POST /store/orders".into()]),
        ..OpenApiOptions::default()
    };
    let i = run(PETSHOP, &options);
    let c = model(&i.files);
    assert_eq!(
        layout(&c),
        [
            ("pets".to_owned(), vec!["Find pets by status".to_owned()]),
            ("store".to_owned(), vec!["Place an order".to_owned()]),
        ]
    );
    assert_eq!((i.report.requests, i.report.folders), (2, 2));
    // Auth variables only for what is imported: the global API key and the password flow,
    // not the client credentials secret of the inventory or the code flow of users.
    let env = &c.environments[0];
    let names: Vec<&str> = env.variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "baseUrl",
            "apiKey",
            "clientId",
            "clientSecret",
            "username",
            "password"
        ]
    );
    let only = OpenApiOptions {
        operations: Some(vec!["GET /pets".into()]),
        ..OpenApiOptions::default()
    };
    let c = model(&run(PETSHOP, &only).files);
    let names: Vec<String> = c.environments[0]
        .variables
        .iter()
        .map(|v| v.name.clone())
        .collect();
    assert_eq!(names, ["baseUrl", "apiKey"]);
    // Nothing chosen: an empty collection.
    let none = OpenApiOptions {
        operations: Some(Vec::new()),
        ..OpenApiOptions::default()
    };
    assert_eq!(run(PETSHOP, &none).report.requests, 0);
}

#[test]
fn grouping_by_path_or_flat() {
    let by_path = OpenApiOptions {
        grouping: OpenApiGrouping::Path,
        ..OpenApiOptions::default()
    };
    let c = model(&run(PETSHOP, &by_path).files);
    let folders: Vec<(String, usize)> = layout(&c).into_iter().map(|(f, r)| (f, r.len())).collect();
    assert_eq!(
        folders,
        [
            ("pets".to_owned(), 6),
            ("store".to_owned(), 2),
            ("users".to_owned(), 1)
        ]
    );
    let flat = OpenApiOptions {
        grouping: OpenApiGrouping::Flat,
        ..OpenApiOptions::default()
    };
    let i = run(KENNEL, &flat);
    assert_eq!(i.report.folders, 0);
    let c = model(&i.files);
    assert_eq!(
        layout(&c),
        [(
            String::new(),
            vec![
                "List dogs".to_owned(),
                "Add a dog".to_owned(),
                "Get a dog".to_owned(),
                "Set the owner".to_owned()
            ]
        )]
    );
    // Tag grouping keeps the tag's description as folder docs; path grouping has none.
    let tagged = run(KENNEL, &OpenApiOptions::default());
    let folder = String::from_utf8(tagged.files.files["dogs/folder.yml"].clone()).unwrap();
    assert!(folder.contains("Dogs in the kennel"), "{folder}");
}

#[test]
fn naming_by_operation_id() {
    let options = OpenApiOptions {
        naming: OpenApiNaming::OperationId,
        ..OpenApiOptions::default()
    };
    let i = run(PETSHOP, &options);
    let c = model(&i.files);
    let names: Vec<String> = requests(&c).into_iter().map(|r| r.name).collect();
    assert_eq!(
        names,
        [
            "findPets",
            "addPet",
            "getPet",
            "updatePetWithForm",
            "deletePet",
            "uploadPhoto",
            "placeOrder",
            "getInventory",
            "getUser"
        ]
    );
    assert!(i.files.files.contains_key("pets/findpets.yml"));
    // Without an operationId the summary is the name.
    let notes = run("synthetic-notes-swagger-2.0.yaml", &options);
    let names: Vec<String> = requests(&model(&notes.files))
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, ["List notes", "Create a note", "Get a note"]);
}

#[test]
fn examples_from_the_schema() {
    let spec = run(PETSHOP, &OpenApiOptions::default());
    let schema = run(
        PETSHOP,
        &OpenApiOptions {
            examples: OpenApiExamples::Schema,
            ..OpenApiOptions::default()
        },
    );
    let find = |i: &rt_format::import::Import, name: &str| {
        requests(&model(&i.files))
            .into_iter()
            .find(|r| r.name == name)
            .unwrap()
    };
    let body = |r: &rt_core::model::RequestSpec| match &r.body {
        rt_core::model::Body::Raw { data, .. } => data.clone(),
        other => panic!("{other:?}"),
    };
    // The spec's example (`x-examples`) …
    assert!(body(&find(&spec, "Place an order")).contains("\"petId\": 42"));
    // … or values generated from the schema.
    let generated = body(&find(&schema, "Place an order"));
    assert!(generated.contains("\"petId\": 0"), "{generated}");
    assert!(generated.contains("2024-01-01T00:00:00Z"), "{generated}");
    // Parameters: the spec's `x-example`, else the schema's default or first enum value.
    let pet = |i| find(i, "Get a pet").params[0].value.clone();
    assert_eq!((pet(&spec), pet(&schema)), ("42".to_owned(), String::new()));
    let limit = |i| find(i, "Find pets by status").params[1].value.clone();
    assert_eq!(limit(&schema), "20");
    // Schema example values (`example` keywords) are not used either.
    let added = body(&find(&schema, "Add a pet"));
    assert!(added.contains("\"name\": \"string\""), "{added}");
    assert!(body(&find(&spec, "Add a pet")).contains("\"name\": \"Rex\""));
}

#[test]
fn servers_become_the_chosen_environments() {
    let options = OpenApiOptions {
        servers: Some(vec![
            OpenApiServerChoice {
                index: 1,
                environment: Some("Plain HTTP".into()),
            },
            OpenApiServerChoice {
                index: 0,
                environment: None,
            },
        ]),
        default_server: Some(1),
        collection_name: Some("  My shop  ".into()),
        ..OpenApiOptions::default()
    };
    let i = run(PETSHOP, &options);
    assert_eq!(i.collection_name, "My shop");
    let c = model(&i.files);
    assert_eq!(c.name, "My shop");
    let envs: Vec<&str> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(envs, ["Plain HTTP"]);
    let base = c
        .defaults
        .vars
        .iter()
        .find(|v| v.name == "baseUrl")
        .unwrap();
    assert_eq!(
        base.value.as_deref(),
        Some("http://petshop.example.com/api/v2")
    );
    // The same name twice is made unique; an empty one falls back to the suggestion.
    let twice = OpenApiOptions {
        servers: Some(vec![
            OpenApiServerChoice {
                index: 0,
                environment: Some("Prod".into()),
            },
            OpenApiServerChoice {
                index: 1,
                environment: Some("Prod".into()),
            },
        ]),
        ..OpenApiOptions::default()
    };
    let c = model(&run(KENNEL, &twice).files);
    // (The model lists environments by file name.)
    let envs: Vec<&str> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(envs, ["Prod (2)", "Prod"]);
    let staging = c.environment("Prod (2)").unwrap();
    assert_eq!(
        staging.variables[0].value.as_deref(),
        Some("https://staging.kennel.example.com/v1")
    );
    let blank = OpenApiOptions {
        servers: Some(vec![OpenApiServerChoice {
            index: 1,
            environment: Some(" ".into()),
        }]),
        ..OpenApiOptions::default()
    };
    let c = model(&run(KENNEL, &blank).files);
    let envs: Vec<&str> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(envs, ["Staging"]);
    // No environment at all, but the auth needs variables: one "Default" environment.
    let no_env = OpenApiOptions {
        servers: Some(Vec::new()),
        ..OpenApiOptions::default()
    };
    let c = model(&run(KENNEL, &no_env).files);
    let envs: Vec<&str> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(envs, ["Default"]);
}

fn keep() -> OpenApiOptions {
    OpenApiOptions {
        keep_spec: true,
        ..OpenApiOptions::default()
    }
}

#[test]
fn a_kept_single_file_spec_keeps_its_bytes() {
    for (name, kept) in [
        ("synthetic-notes-swagger-2.0.yaml", "openapi.yaml"),
        (PETSHOP, "openapi.json"),
        ("oai-petstore-3.0.yaml", "openapi.yaml"),
        // `.yml` would be read as a request: kept as `openapi.yaml`.
        ("insomnia-multiple-api-keys-3.0.yml", "openapi.yaml"),
        ("oai-uspto-3.0.json", "openapi.json"),
    ] {
        let original = std::fs::read(corpus(name)).unwrap();
        let plain = run(name, &OpenApiOptions::default());
        let i = run(name, &keep());
        assert_eq!(i.files.files.get(kept), Some(&original), "{name}");
        assert!(
            !i.files
                .files
                .keys()
                .any(|k| k.starts_with("openapi.y") && k != kept)
        );
        assert_eq!(
            rt_format::contract::configured_spec(&i.files).as_deref(),
            Some(kept),
            "{name}"
        );
        let text = String::from_utf8(i.files.files[COLLECTION_FILE].clone()).unwrap();
        assert!(
            text.ends_with(&format!(
                "bundled: false\n\nextensions:\n  requesttailor:\n    contract:\n      spec: {kept}\n"
            )),
            "{text}"
        );
        round_trips(&i.files);
        // Everything else is the plain import.
        assert_eq!(model(&i.files), model(&plain.files), "{name}");
        assert_eq!(i.report, plain.report);
        assert_eq!(i.files.files.len(), plain.files.files.len() + 1);
        // The contract code reads the kept spec.
        let (title, _) = rt_format::contract::spec_info(&i.files.files[kept]).unwrap();
        assert_eq!(title, i.collection_name);
    }
}

#[test]
fn a_kept_multi_file_spec_is_bundled_into_one_json_file() {
    let i = run(KENNEL, &keep());
    assert_eq!(
        rt_format::contract::configured_spec(&i.files).as_deref(),
        Some("openapi.json")
    );
    round_trips(&i.files);
    let bundled: serde_json::Value =
        serde_json::from_slice(&i.files.files["openapi.json"]).unwrap();
    let hoisted = bundled["x-requesttailor-bundled"].as_object().unwrap();
    let keys: Vec<&str> = hoisted.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["parameters.yaml", "schemas_dog.yaml", "schemas_error.yaml"]
    );
    // References point into the one document, except the missing file and the URL.
    let mut refs = Vec::new();
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, v) in m {
                    match (k.as_str(), v) {
                        ("$ref", serde_json::Value::String(r)) => out.push(r.clone()),
                        _ => walk(v, out),
                    }
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    walk(&bundled, &mut refs);
    for r in &refs {
        if let Some(local) = r.strip_prefix('#') {
            assert!(bundled.pointer(local).is_some(), "{r} does not resolve");
        } else {
            assert!(
                r == "schemas/owner.yaml" || r.starts_with("https://"),
                "{r} not bundled"
            );
        }
    }
    assert!(refs.contains(&"#/x-requesttailor-bundled/parameters.yaml/breedName".to_owned()));
    assert!(refs.contains(
        &"#/x-requesttailor-bundled/schemas_dog.yaml/definitions/Vaccination".to_owned()
    ));
    // The contract reader takes it.
    let (title, version) = rt_format::contract::spec_info(&i.files.files["openapi.json"]).unwrap();
    assert_eq!((title.as_str(), version.as_str()), ("Kennel", "1.4.0"));
    // Importing the bundled file again gives the same requests.
    let again = import(None, "openapi.json", &i.files.files["openapi.json"]).unwrap();
    assert_eq!(requests(&model(&again.files)), requests(&model(&i.files)));
}

#[test]
fn swagger_2_specs_are_converted() {
    let i = run(PETSHOP, &OpenApiOptions::default());
    assert_eq!(i.report.format, ImportFormat::Openapi);
    let c = model(&i.files);
    let envs: Vec<&str> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        envs,
        [
            "petshop.example.com/api/v2 (HTTP)",
            "petshop.example.com/api/v2 (HTTPS)"
        ]
    );
    let all = requests(&c);
    let by = |n: &str| all.iter().find(|r| r.name == n).unwrap();
    assert!(matches!(
        &by("Upload a photo").body,
        rt_core::model::Body::Multipart { parts } if parts.len() == 2
            && parts[1].kind == rt_core::model::PartKind::File
    ));
    assert!(matches!(
        &by("Update a pet with form data").body,
        rt_core::model::Body::FormUrlEncoded { fields } if fields.len() == 2
    ));
    assert_eq!(by("Find pets by status").headers[0].name, "X-Trace-Id");
    assert!(matches!(
        by("Delete a pet").auth,
        rt_core::model::Auth::Basic { .. }
    ));
    // Swagger 1.x stays refused.
    let old = br#"{"swagger": "1.2", "apis": []}"#;
    assert!(matches!(
        import(None, "old.json", old),
        Err(ImportError::UnsupportedVersion { .. })
    ));
}

#[test]
fn multi_file_specs_find_their_main_file_and_report_what_is_missing() {
    // The order of the files does not matter; the main file is the one with `openapi`.
    let mut files = read(KENNEL);
    files.reverse();
    let i = import_openapi(&sources(&files), &OpenApiOptions::default()).unwrap();
    assert_eq!(i.collection_name, "Kennel");
    let missing: Vec<&str> = i
        .report
        .losses
        .iter()
        .filter(|l| l.kind == LossKind::Other)
        .map(|l| l.detail.as_str())
        .collect();
    assert_eq!(
        missing,
        [
            "`schemas/owner.yaml` was not given; references to it are not resolved",
            "references to URLs are not fetched (`https://schemas.example.com/problem-detail.json`); download the files and add them",
        ]
    );
    // Browsers hand over file names only: `schemas/dog.yaml` is found as `dog.yaml`.
    let flat: Vec<(String, Vec<u8>)> = read(KENNEL)
        .into_iter()
        .map(|(n, d)| (n.rsplit('/').next().unwrap().to_owned(), d))
        .collect();
    let j = import_openapi(&sources(&flat), &OpenApiOptions::default()).unwrap();
    assert_eq!(requests(&model(&j.files)), requests(&model(&i.files)));
    // Two specs: `openapi.*` wins.
    let a = br#"{"openapi": "3.0.3", "info": {"title": "A", "version": "1"}, "paths": {}}"#;
    let b = br#"{"openapi": "3.0.3", "info": {"title": "B", "version": "1"}, "paths": {}}"#;
    let two = [
        SourceFile {
            name: "a.json",
            data: a,
        },
        SourceFile {
            name: "openapi.json",
            data: b,
        },
    ];
    assert_eq!(
        import_openapi(&two, &OpenApiOptions::default())
            .unwrap()
            .collection_name,
        "B"
    );
    // A given file the spec does not use is reported.
    let mut extra = read(KENNEL);
    extra.push(("README.md".into(), b"# Kennel\n".to_vec()));
    let k = import_openapi(&sources(&extra), &OpenApiOptions::default()).unwrap();
    assert!(
        k.report
            .losses
            .iter()
            .any(|l| l.detail == "`README.md` is not referenced by the spec; not imported"),
        "{:?}",
        k.report.losses
    );
    // Several files without a spec.
    let none = [
        SourceFile {
            name: "a.yaml",
            data: b"type: object\n",
        },
        SourceFile {
            name: "b.yaml",
            data: b"type: string\n",
        },
    ];
    let e = import_openapi(&none, &OpenApiOptions::default()).unwrap_err();
    assert!(e.to_string().contains("none of the files"), "{e}");
}

#[test]
fn reference_cycles_between_files_end() {
    let main = br##"{"openapi": "3.0.3", "info": {"title": "Cycle", "version": "1"},
        "servers": [{"url": "https://x.example.com"}],
        "paths": {"/a": {"post": {"requestBody": {"content": {"application/json":
            {"schema": {"$ref": "a.json"}}}}, "responses": {}}}}}"##;
    let a =
        br##"{"type": "object", "properties": {"b": {"$ref": "b.json"}, "self": {"$ref": "#"}}}"##;
    let b = br##"{"type": "object", "properties": {"a": {"$ref": "a.json#/properties/b"}, "back": {"$ref": "openapi.json#/info"}}}"##;
    let files = [
        SourceFile {
            name: "openapi.json",
            data: main,
        },
        SourceFile {
            name: "a.json",
            data: a,
        },
        SourceFile {
            name: "b.json",
            data: b,
        },
    ];
    let i = import_openapi(&files, &keep()).unwrap();
    assert_eq!(i.report.requests, 1);
    let overview = inspect_openapi(&files).unwrap();
    assert_eq!(overview.files, 3);
    let bundled: serde_json::Value =
        serde_json::from_slice(&i.files.files["openapi.json"]).unwrap();
    assert_eq!(
        bundled.pointer("/x-requesttailor-bundled/b.json/properties/back/$ref"),
        Some(&serde_json::json!("#/info"))
    );
    assert_eq!(
        bundled.pointer("/x-requesttailor-bundled/a.json/properties/self/$ref"),
        Some(&serde_json::json!("#/x-requesttailor-bundled/a.json"))
    );
}

#[test]
fn the_overview_names_everything_as_the_import_does() {
    let files = read(PETSHOP);
    let o = inspect_openapi(&sources(&files)).unwrap();
    assert_eq!(
        (
            o.title.as_str(),
            o.version.as_str(),
            o.spec_version.as_str()
        ),
        ("Pet Shop", "2.1.0", "2.0")
    );
    assert_eq!((o.main_file.as_str(), o.files), (PETSHOP, 1));
    let c = model(&run(PETSHOP, &OpenApiOptions::default()).files);
    let mut suggested: Vec<&str> = o
        .servers
        .iter()
        .map(|s| s.suggested_name.as_str())
        .collect();
    assert_eq!(suggested[0], "petshop.example.com/api/v2 (HTTPS)");
    suggested.sort();
    let envs: Vec<&str> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(suggested, envs);
    assert_eq!(o.servers[0].url, "https://petshop.example.com/api/v2");
    let tags: Vec<(&str, u32)> = o
        .tags
        .iter()
        .map(|t| (t.name.as_str(), t.operations))
        .collect();
    assert_eq!(tags, [("pets", 6), ("store", 2), ("users", 1)]);
    let folders: Vec<String> = layout(&c).into_iter().map(|(f, _)| f).collect();
    assert_eq!(folders, ["pets", "store", "users"]);
    assert_eq!(o.operations.len(), 9);
    let op = &o.operations[4];
    assert_eq!(
        (op.key.as_str(), op.method.as_str(), op.path.as_str()),
        ("DELETE /pets/{petId}", "DELETE", "/pets/{petId}")
    );
    assert!(op.deprecated);
    assert_eq!(op.operation_id.as_deref(), Some("deletePet"));
    let security: Vec<(&str, &str, &str, Option<&str>)> = o
        .security
        .iter()
        .map(|s| {
            (
                s.name.as_str(),
                s.kind.as_str(),
                s.detail.as_str(),
                s.maps_to.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        security,
        [
            (
                "shop_key",
                "api-key",
                "header X-Shop-Key",
                Some("API key X-Shop-Key in header {{apiKey}}")
            ),
            (
                "basic",
                "basic",
                "HTTP Basic",
                Some("Basic auth {{username}} / {{password}}")
            ),
            (
                "shop_auth",
                "oauth2",
                "implicit flow",
                Some("OAuth 2.0 implicit (clientId)")
            ),
            (
                "shop_password",
                "oauth2",
                "password flow",
                Some("OAuth 2.0 password (clientId, clientSecret, username, password)")
            ),
            (
                "shop_app",
                "oauth2",
                "client credentials flow",
                Some("OAuth 2.0 client credentials (clientId, clientSecret)")
            ),
            (
                "shop_code",
                "oauth2",
                "authorization code flow",
                Some("OAuth 2.0 authorization code (clientId, clientSecret)")
            ),
        ]
    );
    assert_eq!(
        o.security[3].variables,
        ["clientId", "clientSecret", "username", "password"]
    );
    assert!(o.losses.is_empty(), "{:?}", o.losses);

    // A cookie API key is not imported.
    let notes = read("synthetic-notes-swagger-2.0.yaml");
    let o = inspect_openapi(&sources(&notes)).unwrap();
    let cookie = o
        .security
        .iter()
        .find(|s| s.name == "cookie_session")
        .unwrap();
    assert_eq!((cookie.maps_to.as_ref(), cookie.variables.len()), (None, 0));
    assert_eq!(o.servers[0].url, "/notes-api");
}

#[test]
fn the_overview_of_multi_file_specs_and_zips() {
    let files = read(KENNEL);
    let o = inspect_openapi(&sources(&files)).unwrap();
    assert_eq!((o.main_file.as_str(), o.files), ("openapi.yaml", 4));
    assert_eq!(o.spec_version, "3.0.3");
    let tags: Vec<(&str, u32)> = o
        .tags
        .iter()
        .map(|t| (t.name.as_str(), t.operations))
        .collect();
    assert_eq!(tags, [("dogs", 3), ("owners", 1)]);
    assert_eq!(o.losses.len(), 2);
    assert!(o.losses[0].detail.contains("schemas/owner.yaml"));
    assert_eq!(o.security[0].detail, "JWT");
    assert_eq!(
        o.security[0].maps_to.as_deref(),
        Some("Bearer token {{token}}")
    );

    let zip = read("synthetic-zipped-3.1.zip");
    let o = inspect_openapi(&sources(&zip)).unwrap();
    assert_eq!((o.main_file.as_str(), o.files), ("library/index.yaml", 2));
    assert_eq!(o.title, "Library");

    // One file with references to others: they are named in the losses.
    let main = files.iter().find(|(n, _)| n == "openapi.yaml").unwrap();
    let o = inspect_openapi(&[SourceFile {
        name: &main.0,
        data: &main.1,
    }])
    .unwrap();
    assert_eq!(o.files, 1);
    assert_eq!(o.losses.len(), 1);
    assert!(
        o.losses[0]
            .detail
            .starts_with("references to other files not resolved (`parameters.yaml`"),
        "{:?}",
        o.losses
    );
}

#[test]
fn references_are_listed_as_written() {
    let files = read(KENNEL);
    let main = files.iter().find(|(n, _)| n == "openapi.yaml").unwrap();
    let refs = openapi_references(&SourceFile {
        name: &main.0,
        data: &main.1,
    });
    assert_eq!(
        refs,
        [
            "parameters.yaml",
            "./schemas/dog.yaml",
            "schemas/dog.yaml",
            "schemas/owner.yaml",
            "schemas/error.yaml"
        ]
    );
    let dog = files.iter().find(|(n, _)| n == "schemas/dog.yaml").unwrap();
    assert_eq!(
        openapi_references(&SourceFile {
            name: &dog.0,
            data: &dog.1
        }),
        ["../parameters.yaml"]
    );
    assert!(
        openapi_references(&SourceFile {
            name: "x.bin",
            data: b"\xff\xfe\x00"
        })
        .is_empty()
    );
}
