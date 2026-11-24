// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sharing in the desktop shell (wave 17, strand C), through `Core` without a window: a link
//! made from the active collection carries no secret (the keychain value of the environment is
//! a canary), starts with the owner's base `https://app.requesttailor.com/#rt=`, reads back, and the
//! snippets keep the secret as a variable of the target tool.

use std::collections::HashMap;
use std::sync::Mutex;

use rt_app::share::{CopyKind, CopyOptions, ShareOptions};
use rt_app::{AppError, SecretKey, SecretStore};
use rt_desktop::{Core, share};

const SECRET: &str = "RT-CANARY-desktop-share-1";

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

#[test]
fn a_link_and_a_snippet_carry_no_secret() {
    let dir = std::env::temp_dir().join(format!("rt-desktop-share-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("environments")).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Shares\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("environments/Local.yml"),
        "name: Local\nvariables:\n  - name: base\n    value: http://127.0.0.1:1\n  - secret: true\n    name: token\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("get.yml"),
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/echo/x\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{token}}\"\n",
    )
    .unwrap();
    let core = Core::with_data_dir(MemorySecrets::default(), None).unwrap();
    core.open(&dir).unwrap();
    core.secret_set("Local", "token", SECRET).unwrap();

    // The UI may send a base; the desktop uses only its own constant.
    let options = ShareOptions {
        include_values: true,
        environment: Some("Local".into()),
        base: Some("https://elsewhere.test/".into()),
    };
    let link = share::link(&core, "get.yml", options).unwrap();
    assert!(
        link.text
            .starts_with("https://app.requesttailor.com/#rt=1."),
        "{}",
        link.text
    );
    assert!(!link.text.contains(SECRET) && !link.preview.contains(SECRET));
    assert!(
        link.preview.contains("http://127.0.0.1:1"),
        "the plain value was asked for"
    );

    let opened = share::open(&link.text).unwrap();
    assert_eq!(opened.request.url, "{{base}}/echo/x");
    assert!(
        opened
            .variables
            .iter()
            .any(|v| v.name == "token" && v.secret && v.value.is_none())
    );
    assert!(share::open("hello").is_err());
    // A bare payload is understood too.
    let bare = format!("rt:{}", link.text.split("#rt=").nth(1).unwrap());
    assert_eq!(share::open(&bare).unwrap().request.url, "{{base}}/echo/x");

    let o = CopyOptions {
        resolve_variables: true,
        environment: Some("Local".into()),
    };
    for kind in [CopyKind::Curl, CopyKind::Fetch, CopyKind::Python] {
        let c = share::copy_as(&core, "get.yml", kind, &o).unwrap();
        assert!(!c.text.contains(SECRET), "{}", c.text);
        assert!(
            c.text.contains("http://127.0.0.1:1/echo/x") && c.text.contains("TOKEN"),
            "{}",
            c.text
        );
    }
    assert!(share::qr(&link.text).unwrap().contains("<svg"));
    assert!(matches!(
        share::link(&core, "nope.yml", ShareOptions::default()),
        Err(AppError::RequestNotFound(_))
    ));
    let _ = std::fs::remove_dir_all(&dir);
}
