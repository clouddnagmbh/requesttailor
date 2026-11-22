// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send` with plugins (wave 5, strand A): `--plugin` files, the installed plugins of the
//! shared plugin folder (`RT_DATA_DIR`), `--no-plugins`. The demo plugin is the fixture of
//! `crates/rt-plugin/tests/fixtures`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn demo() -> PathBuf {
    root().join("crates/rt-plugin/tests/fixtures/demo.wasm")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-cli-plugins-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `rt send basics/get.yml` against the test server with `extra` arguments.
fn send(port: u16, data_dir: &Path, extra: &[&str]) -> (i32, serde_json::Value, String) {
    let request = root().join("testdata/conformance/collection/basics/get.yml");
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("send")
        .arg(request)
        .args(["--var", &format!("baseUrl=http://127.0.0.1:{port}")])
        .args(extra)
        .env("RT_DATA_DIR", data_dir)
        .output()
        .expect("run rt");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    (
        out.status.code().unwrap_or(-1),
        report,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn echoed_header(report: &serde_json::Value, name: &str) -> Option<String> {
    let body = report["response"]["body"]["data"].as_str()?;
    let echo: serde_json::Value = serde_json::from_str(body).ok()?;
    echo["headers"]
        .as_array()?
        .iter()
        .find(|h| h[0].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .and_then(|h| h[1].as_str().map(str::to_owned))
}

#[test]
fn plugins_run_in_rt_send_like_in_the_apps() {
    let server = tokio::runtime::Runtime::new().unwrap();
    let port = server.block_on(rt_testserver::spawn_local()).port();
    let empty = temp("empty");

    // A --plugin file: prepare adds a header, both hooks add notes.
    let demo = demo();
    let (code, report, stderr) = send(port, &empty, &["--plugin", demo.to_str().unwrap()]);
    assert_eq!(code, 0, "{report:#}\n{stderr}");
    assert_eq!(
        echoed_header(&report, "x-demo-plugin").as_deref(),
        Some("1.0.0"),
        "{report:#}"
    );
    let notes: Vec<&str> = report["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        notes,
        [
            "Demo plugin added X-Demo-Plugin",
            "Demo plugin saw the response"
        ]
    );
    assert_eq!(report["notes"][0]["source"], "plugin:demo");

    // No plugins: no notes, no header.
    let (code, report, _) = send(port, &empty, &[]);
    assert_eq!(code, 0);
    assert!(report.get("notes").is_none(), "{report:#}");
    assert_eq!(echoed_header(&report, "x-demo-plugin"), None);

    // Installed into the shared folder (as the desktop app does): runs without --plugin,
    // and not with --no-plugins.
    let data = temp("installed");
    rt_plugin::host::dir::PluginDir::in_data_dir(&data)
        .install(&std::fs::read(&demo).unwrap())
        .unwrap();
    let (_, report, _) = send(port, &data, &[]);
    assert_eq!(
        echoed_header(&report, "x-demo-plugin").as_deref(),
        Some("1.0.0")
    );
    let (_, report, _) = send(port, &data, &["--no-plugins"]);
    assert_eq!(echoed_header(&report, "x-demo-plugin"), None);

    // A file that is not a plugin fails the send with a plugin error (exit 3).
    let junk = empty.join("junk.wasm");
    std::fs::write(&junk, b"junk").unwrap();
    let (code, report, _) = send(port, &empty, &["--plugin", junk.to_str().unwrap()]);
    assert_eq!(code, 3);
    assert_eq!(report["error"]["kind"], "plugin");

    let _ = std::fs::remove_dir_all(&empty);
    let _ = std::fs::remove_dir_all(&data);
}
