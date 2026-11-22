// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance for the command layer's workspace (wave 3, strand F):
//! "open folder, create request, send, save" and "no secret value is in any file after
//! saving", on real OpenCollection files.
//! Native only: it sends through the native transport to `rt-testserver`.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use rt_app::{AppError, FileChange, SendInput, TreeNode, Workspace};
use rt_core::model::{Auth, Body, Environment, KeyValue, RawKind, Variable};
use rt_core::send::{MASK, ReportBody, SendOptions};
use rt_core::transport::native::NativeTransport;
use rt_core::vars::VarSource;
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno")
}

fn corpus(name: &str) -> FileSet {
    FileSet::from_dir(&corpus_dir().join(name)).unwrap()
}

fn files(list: &[(&str, &str)]) -> FileSet {
    FileSet {
        files: list
            .iter()
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect(),
    }
}

/// What the shell does with `save`'s result. An empty text deletes the file.
fn apply(set: &mut FileSet, changes: &[FileChange]) {
    for c in changes {
        if c.deleted {
            set.files.remove(&c.path);
        } else {
            set.files
                .insert(c.path.clone(), c.text.clone().into_bytes());
        }
    }
}

fn paths(changes: &[FileChange]) -> Vec<&str> {
    changes.iter().map(|c| c.path.as_str()).collect()
}

/// A small collection: a folder with defaults, two requests, one environment.
fn sample() -> FileSet {
    files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: sample\nrequest:\n  variables:\n    - name: base\n      value: http://127.0.0.1:1\n    - name: who\n      value: collection\n",
        ),
        (
            "users/folder.yml",
            "info:\n  name: Users\n  type: folder\n  seq: 2\nrequest:\n  variables:\n    - name: section\n      value: users\n",
        ),
        (
            "users/get.yml",
            "info:\n  name: Get user\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/{{section}}/{{id}}?who={{who}}\"\n  headers:\n    - name: Authorization\n      value: Bearer {{token}}\n  auth: inherit\n\nruntime:\n  variables:\n    - name: id\n      value: \"7\"\n",
        ),
        (
            "ping.yml",
            "info:\n  name: Ping\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/ping\"\n",
        ),
        (
            "environments/Local.yml",
            "name: Local\nvariables:\n  - name: who\n    value: env\n  - secret: true\n    name: token\n",
        ),
    ])
}

const SECRET: &str = "s3cr3t-token-never-in-a-file";

// ---------------------------------------------------------------------------------------------
// Open, tree, request

#[test]
fn opening_the_corpus_and_saving_changes_nothing() {
    for entry in std::fs::read_dir(corpus_dir()).unwrap() {
        let dir = entry.unwrap().path();
        if !dir.join(COLLECTION_FILE).is_file() {
            continue;
        }
        let mut ws = Workspace::open(FileSet::from_dir(&dir).unwrap()).unwrap();
        assert!(ws.dirty().is_empty(), "{}", dir.display());
        assert_eq!(ws.save().unwrap(), Vec::new(), "{}", dir.display());
    }
}

#[test]
fn not_a_collection_is_an_error() {
    let err = Workspace::open(files(&[("a.yml", "x: 1\n")])).unwrap_err();
    assert!(matches!(err, AppError::Format(_)), "{err:?}");
}

#[test]
fn tree_lists_folders_and_requests_in_display_order() {
    let ws = Workspace::open(files(&[
        (COLLECTION_FILE, "info:\n  name: Ordered\n"),
        (
            "b.yml",
            "info:\n  name: B\n  type: http\n  seq: 2\nhttp:\n  method: POST\n  url: x\n",
        ),
        (
            "a.yml",
            "info:\n  name: A\n  type: http\n  seq: 3\nhttp:\n  method: GET\n  url: x\n",
        ),
        (
            "z.yml",
            "info:\n  name: Z\n  type: http\n  seq: 1\nhttp:\n  method: GET\n  url: x\n",
        ),
        (
            "no-seq.yml",
            "info:\n  name: Last\n  type: http\nhttp:\n  method: GET\n  url: x\n",
        ),
        ("dir/folder.yml", "info:\n  name: Dir\n  seq: 2\n"),
        (
            "dir/r.yml",
            "info:\n  name: R\n  type: http\n  seq: 1\nhttp:\n  method: DELETE\n  url: x\n",
        ),
        ("gql.yml", "info:\n  name: G\n  type: graphql\n  seq: 0\n"),
    ]))
    .unwrap();
    let req = |name: &str, path: &str, method: &str| TreeNode::Request {
        name: name.into(),
        path: path.into(),
        method: method.into(),
    };
    assert_eq!(
        ws.tree(),
        TreeNode::Folder {
            name: "Ordered".into(),
            path: String::new(),
            children: vec![
                // Wave 8: GraphQL items are requests (POST unless they say otherwise).
                req("G", "gql.yml", "POST"),
                req("Z", "z.yml", "GET"),
                // Same seq: folders before requests.
                TreeNode::Folder {
                    name: "Dir".into(),
                    path: "dir".into(),
                    children: vec![req("R", "dir/r.yml", "DELETE")],
                },
                req("B", "b.yml", "POST"),
                req("A", "a.yml", "GET"),
                req("Last", "no-seq.yml", "GET"),
            ],
        }
    );
}

/// Wave 6, strand F: plugin views add requests in folders that may not exist yet.
#[test]
fn folders_are_found_by_directory_or_name_and_created_when_missing() {
    let mut ws = Workspace::open(sample()).unwrap();
    assert_eq!(ws.create_folder("").unwrap(), "");
    assert_eq!(ws.create_folder("users").unwrap(), "users");
    assert_eq!(
        ws.create_folder("Users").unwrap(),
        "users",
        "by display name"
    );
    assert!(ws.dirty().is_empty());

    let path = ws.create_folder("Sales Orders/Items").unwrap();
    assert_eq!(path, "Sales Orders/Items");
    assert_eq!(ws.create_folder("Sales Orders/Items").unwrap(), path);
    let request = ws.create_request(&path, "List items").unwrap();
    assert_eq!(request, "Sales Orders/Items/list-items.yml");
    assert_eq!(
        ws.dirty(),
        [
            "Sales Orders/Items/folder.yml",
            "Sales Orders/Items/list-items.yml",
            "Sales Orders/folder.yml",
        ]
    );
    let mut on_disk = sample();
    apply(&mut on_disk, &ws.save().unwrap());
    let folder = String::from_utf8(on_disk.files["Sales Orders/folder.yml"].clone()).unwrap();
    assert_eq!(
        folder,
        "info:\n  name: Sales Orders\n  type: folder\n  seq: 3\n"
    );
    // Read back: the same tree, byte for byte the same files.
    let again = read_collection(&on_disk).unwrap();
    assert_eq!(again.write().files, on_disk.files);
    assert_eq!(Workspace::open(on_disk).unwrap().tree(), ws.tree());

    for bad in ["../x", "a//b", "a/./b", "a\\b"] {
        assert_eq!(
            ws.create_folder(bad),
            Err(AppError::FolderNotFound(bad.into())),
            "{bad}"
        );
    }
}

#[test]
fn unknown_paths_are_reported() {
    let mut ws = Workspace::open(sample()).unwrap();
    assert_eq!(
        ws.request("nope.yml"),
        Err(AppError::RequestNotFound("nope.yml".into()))
    );
    assert_eq!(
        ws.create_request("nope", "x"),
        Err(AppError::FolderNotFound("nope".into()))
    );
    let spec = ws.request("ping.yml").unwrap();
    assert_eq!(
        ws.update_request("nope.yml", spec),
        Err(AppError::RequestNotFound("nope.yml".into()))
    );
    assert_eq!(
        ws.delete_environment("Nope"),
        Err(AppError::EnvironmentNotFound("Nope".into()))
    );
    assert!(matches!(
        ws.preview("nope.yml", SendInput::default()),
        Err(AppError::RequestNotFound(_))
    ));
    assert!(matches!(
        ws.snapshot("nope.yml", SendInput::default()),
        Err(AppError::RequestNotFound(_))
    ));
}

// ---------------------------------------------------------------------------------------------
// Create, edit, save

#[test]
fn create_requests_in_root_and_folder_then_save() {
    let mut on_disk = sample();
    let mut ws = Workspace::open(on_disk.clone()).unwrap();
    let root = ws.create_request("", "Health Check").unwrap();
    let nested = ws.create_request("users", "List users").unwrap();
    assert_eq!(root, "health-check.yml");
    assert_eq!(nested, "users/list-users.yml");

    let created = ws.request(&nested).unwrap();
    assert_eq!(created.name, "List users");
    assert_eq!(created.method, "GET");
    assert_eq!(created.url, "");
    assert_eq!(created.seq, Some(2.0));
    assert_eq!(created.auth, Auth::Inherit);

    let mut edited = created.clone();
    edited.url = "{{base}}/users".into();
    edited
        .headers
        .push(KeyValue::new("Accept", "application/json"));
    ws.update_request(&nested, edited.clone()).unwrap();
    assert_eq!(ws.dirty(), vec![root.clone(), nested.clone()]);
    // The tree shows unsaved requests.
    let TreeNode::Folder { children, .. } = ws.tree() else {
        panic!()
    };
    assert!(
        children
            .iter()
            .any(|c| matches!(c, TreeNode::Request { path, .. } if *path == root))
    );

    let changes = ws.save().unwrap();
    assert_eq!(paths(&changes), vec![root.as_str(), nested.as_str()]);
    assert!(ws.dirty().is_empty());
    assert_eq!(ws.save().unwrap(), Vec::new());

    apply(&mut on_disk, &changes);
    let reread = read_collection(&on_disk).unwrap();
    assert_eq!(reread.model(), ws.collection());
    assert_eq!(
        reread.model().request(&nested).unwrap().request.spec,
        edited
    );
    assert_eq!(
        String::from_utf8(on_disk.files[&root].clone()).unwrap(),
        "info:\n  name: Health Check\n  type: http\n  seq: 3\n\nhttp:\n  method: GET\n  url: \"\"\n  auth: inherit\n"
    );
}

#[test]
fn editing_a_corpus_request_rewrites_only_its_file() {
    let input = corpus("request-description-yml");
    let path = "request-with-descriptions.yml";
    let mut ws = Workspace::open(input.clone()).unwrap();
    let mut spec = ws.request(path).unwrap();
    spec.url = "https://example.com/api/changed".into();
    spec.params.clear();
    spec.headers[0].value = "3.0".into();
    spec.headers.push(KeyValue::new("X-New", "1"));
    spec.body = Body::Raw {
        kind: RawKind::Text,
        data: "hello".into(),
    };
    spec.auth = Auth::Bearer {
        token: "{{apiToken}}".into(),
    };
    ws.update_request(path, spec.clone()).unwrap();
    assert_eq!(ws.dirty(), vec![path.to_string()]);
    let changes = ws.save().unwrap();
    assert_eq!(paths(&changes), vec![path]);

    let mut on_disk = input.clone();
    apply(&mut on_disk, &changes);
    for (p, bytes) in &input.files {
        if p != path {
            assert_eq!(&on_disk.files[p], bytes, "{p} must not change");
        }
    }
    let reread = read_collection(&on_disk).unwrap();
    assert_eq!(reread.model().request(path).unwrap().request.spec, spec);
    // What the model does not know (descriptions, assertions) is still there, unchanged.
    let before = String::from_utf8(input.files[path].clone()).unwrap();
    let after = &changes[0].text;
    assert!(after.contains("      value: \"3.0\"\n      description: Single-line header desc\n"));
    let tail = |t: &str| t[t.find("\nruntime:").unwrap()..].to_owned();
    assert_eq!(tail(after), tail(&before));
}

#[test]
fn editing_back_clears_dirty() {
    let mut ws = Workspace::open(corpus("request-description-yml")).unwrap();
    let path = "request-with-descriptions.yml";
    let original = ws.request(path).unwrap();
    let mut spec = original.clone();
    spec.url = "'quoted'".into();
    spec.headers.clear();
    ws.update_request(path, spec).unwrap();
    assert_eq!(ws.dirty(), vec![path.to_string()]);
    ws.update_request(path, original).unwrap();
    assert!(ws.dirty().is_empty());
    assert_eq!(ws.save().unwrap(), Vec::new());
}

#[test]
fn environments_are_created_updated_and_deleted() {
    let mut on_disk = sample();
    let mut ws = Workspace::open(on_disk.clone()).unwrap();
    let staging = Environment {
        client_certificates: Vec::new(),
        external_secrets: None,
        name: "Staging".into(),
        variables: vec![Variable::new("who", "staging")],
    };
    ws.update_environment(staging.clone()).unwrap();
    assert_eq!(ws.dirty(), vec!["environments/Staging.yml".to_string()]);
    let mut local = ws
        .environments()
        .into_iter()
        .find(|e| e.name == "Local")
        .unwrap();
    local.variables[0].value = Some("changed".into());
    ws.update_environment(local.clone()).unwrap();
    assert_eq!(
        ws.dirty(),
        vec![
            "environments/Local.yml".to_string(),
            "environments/Staging.yml".to_string()
        ]
    );
    let names: Vec<String> = ws.environments().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["Local", "Staging"]);
    let changes = ws.save().unwrap();
    apply(&mut on_disk, &changes);
    assert_eq!(
        read_collection(&on_disk).unwrap().model().environments,
        vec![local, staging]
    );

    ws.delete_environment("Staging").unwrap();
    assert_eq!(ws.dirty(), vec!["environments/Staging.yml".to_string()]);
    let changes = ws.save().unwrap();
    // Deletion is a change with `deleted: true` (see `Workspace::save`).
    assert_eq!(
        changes,
        vec![FileChange {
            path: "environments/Staging.yml".into(),
            text: String::new(),
            deleted: true,
        }]
    );
    apply(&mut on_disk, &changes);
    assert!(!on_disk.files.contains_key("environments/Staging.yml"));
    assert_eq!(ws.environments().len(), 1);
}

#[test]
fn deleting_an_unsaved_environment_leaves_nothing_to_save() {
    let mut ws = Workspace::open(sample()).unwrap();
    ws.update_environment(Environment {
        client_certificates: Vec::new(),
        external_secrets: None,
        name: "Tmp".into(),
        variables: Vec::new(),
    })
    .unwrap();
    ws.delete_environment("Tmp").unwrap();
    assert!(ws.dirty().is_empty());
    assert_eq!(ws.save().unwrap(), Vec::new());
}

// ---------------------------------------------------------------------------------------------
// Preview

#[test]
fn preview_shows_provenance_per_layer_and_masks_secrets() {
    let ws = Workspace::open(sample()).unwrap();
    let preview = ws
        .preview(
            "users/get.yml",
            SendInput {
                environment: Some("Local".into()),
                vars: vec![("id".into(), "42".into())],
                secrets: vec![("token".into(), SECRET.into())],
            },
        )
        .unwrap();
    assert_eq!(preview.error, None);
    assert_eq!(
        preview.url.as_deref(),
        Some("http://127.0.0.1:1/users/42?who=env")
    );
    assert_eq!(
        preview.headers,
        vec![("Authorization".to_string(), format!("Bearer {MASK}"))]
    );
    let var = |name: &str| {
        preview
            .variables
            .iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| panic!("{name} in {:?}", preview.variables))
            .clone()
    };
    assert_eq!(var("base").source, VarSource::Collection);
    assert_eq!(var("base").value.as_deref(), Some("http://127.0.0.1:1"));
    assert_eq!(
        var("section").source,
        VarSource::Folder {
            path: "users".into()
        }
    );
    assert_eq!(
        var("who").source,
        VarSource::Environment {
            name: "Local".into()
        }
    );
    assert_eq!(var("id").source, VarSource::Runtime);
    assert_eq!(var("id").value.as_deref(), Some("42"));
    let token = var("token");
    assert_eq!(
        token.source,
        VarSource::Environment {
            name: "Local".into()
        }
    );
    assert!(token.secret);
    assert_eq!(token.value, None);
    assert!(preview.unresolved.is_empty());
    assert!(!format!("{preview:?}").contains(SECRET));

    // Without runtime value the request layer wins; without the secret it is unresolved.
    let preview = ws
        .preview(
            "users/get.yml",
            SendInput {
                environment: Some("Local".into()),
                ..SendInput::default()
            },
        )
        .unwrap();
    assert_eq!(
        preview
            .variables
            .iter()
            .find(|v| v.name == "id")
            .unwrap()
            .source,
        VarSource::Request
    );
    assert_eq!(preview.unresolved, vec!["token".to_string()]);
}

#[test]
fn preview_of_an_unresolved_host_still_lists_variables() {
    let mut files = sample();
    files.files.insert(
        "host.yml".into(),
        b"info:\n  name: Host\n  type: http\n  seq: 3\n\nhttp:\n  method: GET\n  url: \"{{missing}}/{{who}}\"\n".to_vec(),
    );
    let ws = Workspace::open(files).unwrap();
    let preview = ws.preview("host.yml", SendInput::default()).unwrap();
    assert_eq!(preview.url, None);
    assert!(
        preview.error.as_deref().unwrap().contains("missing"),
        "{preview:?}"
    );
    assert_eq!(preview.unresolved, vec!["missing".to_string()]);
    let who = preview.variables.iter().find(|v| v.name == "who").unwrap();
    assert_eq!(who.source, VarSource::Collection);
    assert_eq!(who.value.as_deref(), Some("collection"));
}

#[test]
fn errors_never_carry_secret_values() {
    // The secret ends up in the URL; the URL is invalid, so preview and send fail with a
    // message that quotes it. The value must be masked there too.
    let mut files = sample();
    files.files.insert(
        "bad.yml".into(),
        b"info:\n  name: Bad\n  type: http\n  seq: 4\n\nhttp:\n  method: GET\n  url: \"ftp://example.test/{{token}}\"\n".to_vec(),
    );
    let ws = Workspace::open(files).unwrap();
    let input = || SendInput {
        environment: Some("Local".into()),
        vars: vec![],
        secrets: vec![("token".into(), SECRET.into())],
    };
    let preview = ws.preview("bad.yml", input()).unwrap();
    assert!(!format!("{preview:?}").contains(SECRET), "{preview:?}");

    let transport = rt_core::transport::native::NativeTransport::new().unwrap();
    let report = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(
            ws.snapshot("bad.yml", input())
                .unwrap()
                .send(&transport, &rt_core::send::SendOptions::default()),
        );
    let json = serde_json::to_string(&report).unwrap();
    assert!(report.error.is_some(), "{json}");
    assert!(!json.contains(SECRET), "{json}");
}

// ---------------------------------------------------------------------------------------------
// Send, and the secret gate

fn echo_json(report: &rt_core::send::SendReport) -> serde_json::Value {
    let response = report
        .response
        .as_ref()
        .unwrap_or_else(|| panic!("no response: {:?}", report.error));
    let ReportBody::Utf8(body) = &response.body else {
        panic!("binary echo")
    };
    serde_json::from_str(body).unwrap()
}

/// Gate: open a folder, create a request, send it, save; the secret value that was used for
/// sending is in no file afterwards.
#[test]
fn create_send_save_and_no_secret_value_in_any_file() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        let port = rt_testserver::spawn_local().await.port();
        let dir = std::env::temp_dir().join(format!("rt-app-workspace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        sample().write_to_dir(&dir).unwrap();

        // Open the folder.
        let mut ws = Workspace::open(FileSet::from_dir(&dir).unwrap()).unwrap();

        // The user marks `token` secret and types its value; the shell keeps the value in its
        // secret store and passes it on when sending.
        let mut env = ws
            .environments()
            .into_iter()
            .find(|e| e.name == "Local")
            .unwrap();
        env.variables
            .push(Variable::new("base", format!("http://127.0.0.1:{port}")));
        let token = env
            .variables
            .iter_mut()
            .find(|v| v.name == "token")
            .unwrap();
        token.value = Some(SECRET.into());
        ws.update_environment(env).unwrap();
        assert!(!format!("{:?}", ws.environments()).contains(SECRET));

        // Create a request that uses the secret.
        let path = ws.create_request("users", "Echo").unwrap();
        let mut spec = ws.request(&path).unwrap();
        spec.method = "POST".into();
        spec.url = "{{base}}/echo/{{section}}".into();
        spec.headers = vec![KeyValue::new("Authorization", "Bearer {{token}}")];
        spec.body = Body::Raw {
            kind: RawKind::Json,
            data: "{\"token\": \"{{token}}\"}".into(),
        };
        ws.update_request(&path, spec).unwrap();

        // Send.
        let input = SendInput {
            environment: Some("Local".into()),
            vars: Vec::new(),
            secrets: vec![("token".into(), SECRET.into())],
        };
        let snapshot = ws.snapshot(&path, input).unwrap();
        let transport = NativeTransport::new().unwrap();
        let report = snapshot.send(&transport, &SendOptions::default()).await;
        assert_eq!(report.error, None, "{report:?}");
        let echo = echo_json(&report);
        assert_eq!(echo["method"], "POST");
        assert_eq!(echo["path"], "/echo/users");
        // The secret was used on the wire...
        let auth = echo["headers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h[0] == "authorization")
            .unwrap_or_else(|| panic!("{echo}"));
        assert_eq!(auth[1], format!("Bearer {SECRET}"));
        assert_eq!(echo["body"], format!("{{\"token\": \"{SECRET}\"}}"));
        // ...but masked in what the report says was sent, with provenance "environment".
        let sent = report.request.as_ref().unwrap();
        assert!(
            sent.headers
                .contains(&("Authorization".into(), format!("Bearer {MASK}")))
        );
        let used = report.variables.iter().find(|v| v.name == "token").unwrap();
        assert!(used.secret);
        assert_eq!(
            used.source,
            VarSource::Environment {
                name: "Local".into()
            }
        );

        // Save.
        let changes = ws.save().unwrap();
        assert_eq!(
            paths(&changes),
            vec!["environments/Local.yml", path.as_str()]
        );
        for change in &changes {
            assert!(
                !change.text.contains(SECRET),
                "{}: {}",
                change.path,
                change.text
            );
            std::fs::write(dir.join(&change.path), &change.text).unwrap();
        }
        // No file in the folder holds the value, and the workspace does not either.
        for (file, bytes) in FileSet::from_dir(&dir).unwrap().files {
            assert!(
                !String::from_utf8_lossy(&bytes).contains(SECRET),
                "{file} contains the secret"
            );
        }
        assert!(!format!("{:?}", ws.collection()).contains(SECRET));
        assert!(!format!("{ws:?}").contains(SECRET));
        let saved = std::fs::read_to_string(dir.join("environments/Local.yml")).unwrap();
        assert!(
            saved.contains("  - secret: true\n    name: token\n"),
            "{saved}"
        );

        // The saved request reads back and sends the same way after reopening.
        let ws = Workspace::open(FileSet::from_dir(&dir).unwrap()).unwrap();
        let snapshot = ws
            .snapshot(
                &path,
                SendInput {
                    environment: Some("Local".into()),
                    vars: Vec::new(),
                    secrets: vec![("token".into(), SECRET.into())],
                },
            )
            .unwrap();
        let again = snapshot.send(&transport, &SendOptions::default()).await;
        assert_eq!(echo_json(&again)["body"], echo["body"]);
        std::fs::remove_dir_all(&dir).unwrap();
    });
}

#[test]
fn snapshot_carries_referenced_body_files() {
    let mut files = sample();
    files
        .files
        .insert("data/upload.bin".into(), b"payload".to_vec());
    let mut ws = Workspace::open(files).unwrap();
    let mut spec = ws.request("ping.yml").unwrap();
    spec.body = Body::File {
        path: "data/upload.bin".into(),
        content_type: None,
    };
    ws.update_request("ping.yml", spec).unwrap();
    let snapshot = ws.snapshot("ping.yml", SendInput::default()).unwrap();
    assert_eq!(
        snapshot.files.files.keys().collect::<Vec<_>>(),
        vec!["data/upload.bin"]
    );
}
