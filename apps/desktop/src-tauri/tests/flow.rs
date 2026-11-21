// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gate flow of the desktop shell without a window: open a folder, create a request, send it
//! to rt-testserver with a secret from the (here: in-memory) keychain, save, read the file back.
//!
//! Ignored until the wave 3 integration: needs `rt_app::Workspace` (strand F) and
//! `rt_app::ResponseStore` (strand C). Run with
//! `cargo test -p rt-desktop --test flow -- --include-ignored`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rt_app::{AppError, SecretKey, SecretStore, SendInput, TreeNode};
use rt_desktop::Core;

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
        self.0.lock().unwrap().insert(key.clone(), value.to_owned());
        Ok(())
    }
    fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn collection(base_url: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("rt-desktop-flow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("environments")).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Shell flow\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("environments/Local.yml"),
        format!(
            "name: Local\nvariables:\n  - name: baseUrl\n    value: {base_url}\n  - secret: true\n    name: token\n"
        ),
    )
    .unwrap();
    TempDir(dir)
}

fn requests(node: &TreeNode) -> Vec<String> {
    match node {
        TreeNode::Request { path, .. } => vec![path.clone()],
        TreeNode::Folder { children, .. } => children.iter().flat_map(requests).collect(),
    }
}

const SECRET: &str = "s3cr3t-desktop-token";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs rt_app::Workspace (strand F) and ResponseStore (strand C)"]
async fn open_create_send_save() {
    let addr = rt_testserver::spawn_local().await;
    let dir = collection(&format!("http://{addr}"));
    let core = Core::new(MemorySecrets::default()).unwrap();

    let info = core.open(&dir.0).unwrap();
    assert_eq!(info.name, "Shell flow");
    assert_eq!(
        info.id,
        std::fs::canonicalize(&dir.0).unwrap().to_string_lossy()
    );
    core.secret_set("Local", "token", SECRET).unwrap();
    assert!(core.secret_has("Local", "token").unwrap());

    let path = core.create_request("", "Ping").unwrap();
    let mut spec = core.get_request(&path).unwrap();
    spec.url = "{{baseUrl}}/echo/ping".into();
    spec.headers.push(rt_core::model::KeyValue {
        name: "Authorization".into(),
        value: "Bearer {{token}}".into(),
        enabled: true,
    });
    core.update_request(&path, spec).unwrap();
    assert_eq!(core.dirty().unwrap(), std::slice::from_ref(&path));

    let input = SendInput {
        environment: Some("Local".into()),
        vars: Vec::new(),
        // Whatever the webview sends here is replaced by the keychain values.
        secrets: vec![("token".into(), "from-the-webview".into())],
    };
    let stored = core.send(&path, input).await.unwrap();
    let response = stored.report.response.as_ref().expect("response");
    assert_eq!(response.status, 200, "{:?}", stored.report.error);
    let body = match &response.body {
        rt_core::send::ReportBody::Utf8(text) => text.clone(),
        other => panic!("unexpected body {other:?}"),
    };
    // The secret reached the server ...
    assert!(body.contains(SECRET), "{body}");
    assert!(!body.contains("from-the-webview"));
    // ... but is masked in what the UI sees about the request.
    let sent = serde_json::to_string(&stored.report.request).unwrap();
    assert!(!sent.contains(SECRET), "{sent}");

    let id = stored.response_id.unwrap();
    assert!(
        core.response_lines(id, 0, 10)
            .unwrap()
            .lines
            .join("\n")
            .contains("/echo/ping")
    );
    core.release_response(id);

    let changes = core.save().unwrap();
    assert!(changes.iter().any(|c| c.path == path));
    assert!(core.dirty().unwrap().is_empty());
    let written = std::fs::read_to_string(dir.0.join(&path)).unwrap();
    assert!(written.contains("/echo/ping"), "{written}");
    for change in &changes {
        assert!(!change.text.contains(SECRET), "{}", change.path);
    }

    // Reopening reads the saved request back from disk.
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&dir.0).unwrap();
    assert!(requests(&core.tree().unwrap()).contains(&path));
    assert_eq!(
        core.get_request(&path).unwrap().url,
        "{{baseUrl}}/echo/ping"
    );
}
