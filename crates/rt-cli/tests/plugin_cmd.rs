// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt plugin` (wave 18, strand I) on the built binary against a local fake index host: sources
//! are only stored until a command needs the index (the host sees nothing before), a search
//! reads the index, `install` verifies the SHA-256 (a wrong `--sha256`, a tampered module and a
//! republished release all refuse and leave nothing behind), `--yes` needs `--sha256`, the
//! rights shown are the module's own, an update with more rights asks again, `verify` finds a
//! module changed on disk, and a Git source can be cloned.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rt_app::catalogue::sha256_hex;
use serde_json::json;

struct Host {
    port: u16,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    asked: Arc<AtomicUsize>,
}

impl Host {
    fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
        let asked = Arc::new(AtomicUsize::new(0));
        let (f, a) = (files.clone(), asked.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let (f, a) = (f.clone(), a.clone());
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut chunk) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    a.fetch_add(1, Ordering::SeqCst);
                    let line = String::from_utf8_lossy(&buf);
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    let body = f.lock().unwrap().get(&path).cloned();
                    let (status, body) = match body {
                        Some(b) => ("200 OK", b),
                        None => ("404 Not Found", b"not found".to_vec()),
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(&body);
                });
            }
        });
        Self { port, files, asked }
    }
    fn serve(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), body.into());
    }
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
    /// Serves `plugins.json` with one entry and the module next to it.
    fn publish(&self, id: &str, version: &str, wasm: &[u8], claim: serde_json::Value) {
        let index = json!({
            "version": 1, "name": "Fake index", "plugins": [{
                "id": id, "name": "Demo plugin", "version": version, "api": 1,
                "description": "Synthetic", "license": "Apache-2.0",
                "wasm": { "url": format!("{id}-{version}.wasm"), "sha256": sha256_hex(wasm), "size": wasm.len() },
                "permissions": claim,
            }]
        });
        self.serve("/plugins.json", serde_json::to_vec(&index).unwrap());
        self.serve(&format!("/{id}-{version}.wasm"), wasm);
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rt-plugin/tests/fixtures")
            .join(name),
    )
    .unwrap()
}

struct Data(PathBuf);

impl Data {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rt-plugin-cmd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn rt(data: &Data, stdin: &str, args: &[&str]) -> Out {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("plugin")
        .args(args)
        .env("RT_DATA_DIR", &data.0)
        .env("NO_PROXY", "*")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run rt");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn republished(mut wasm: Vec<u8>) -> Vec<u8> {
    wasm.extend_from_slice(&[0x00, 0x02, 0x01, b'x']);
    wasm
}

#[test]
fn yes_needs_a_checksum_and_there_is_no_built_in_index() {
    let data = Data::new("usage");
    let o = rt(&data, "", &["install", "demo", "--yes"]);
    assert_eq!(o.code, 2, "{}", o.stderr);
    assert!(o.stderr.contains("--yes needs --sha256"), "{}", o.stderr);
    let o = rt(&data, "", &["search", "demo"]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("no built-in index"), "{}", o.stderr);
    let o = rt(&data, "", &["sources", "list"]);
    assert_eq!(o.code, 0);
    assert!(o.stdout.contains("No sources"), "{}", o.stdout);
    let o = rt(
        &data,
        "",
        &["sources", "add", "http://plugins.example.test/plugins.json"],
    );
    assert_eq!(o.code, 2, "plain http is only for localhost: {}", o.stderr);
}

#[test]
fn a_source_costs_nothing_until_a_command_needs_its_index() {
    let host = Host::start();
    host.publish(
        "demo",
        "1.0.0",
        &fixture("demo.wasm"),
        json!({"storage": true}),
    );
    let data = Data::new("quiet");
    let url = host.url("/plugins.json");
    assert_eq!(rt(&data, "", &["sources", "add", &url]).code, 0);
    assert!(rt(&data, "", &["sources", "list"]).stdout.contains(&url));
    assert_eq!(rt(&data, "", &["list"]).code, 0);
    assert_eq!(rt(&data, "", &["verify"]).code, 0);
    assert_eq!(host.asked(), 0, "the host saw nothing");
    let o = rt(&data, "", &["search", "demo"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("demo  1.0.0"), "{}", o.stdout);
    assert_eq!(host.asked(), 1, "one request: the index");
    assert!(
        rt(&data, "", &["sources", "list"])
            .stdout
            .contains("(Fake index)"),
        "the name is remembered"
    );
}

#[test]
fn install_with_a_verified_checksum_then_verify_finds_a_changed_module() {
    let host = Host::start();
    let wasm = fixture("demo.wasm");
    host.publish("demo", "1.0.0", &wasm, json!({"storage": true}));
    let data = Data::new("install");
    let url = host.url("/plugins.json");
    rt(&data, "", &["sources", "add", &url]);
    let sum = sha256_hex(&wasm);
    let o = rt(&data, "", &["install", "demo", "--yes", "--sha256", &sum]);
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
    assert!(o.stdout.contains("rights   storage"), "{}", o.stdout);
    assert!(o.stdout.contains(&sum));
    let installed = data.0.join("plugins/demo.wasm");
    assert_eq!(std::fs::read(&installed).unwrap(), wasm, "byte for byte");
    let o = rt(&data, "", &["list"]);
    assert!(o.stdout.contains("demo  1.0.0  on  from "), "{}", o.stdout);
    assert!(o.stdout.contains(&sum));
    let o = rt(&data, "", &["verify"]);
    assert_eq!(o.code, 0, "{}", o.stdout);
    assert!(o.stdout.contains("ok"), "{}", o.stdout);
    // Someone changes the module on disk.
    std::fs::write(&installed, republished(wasm)).unwrap();
    let o = rt(&data, "", &["verify"]);
    assert_eq!(o.code, 1);
    assert!(o.stdout.contains("MISMATCH"), "{}", o.stdout);
    let o = rt(&data, "", &["remove", "demo"]);
    assert_eq!(o.code, 0);
    assert!(!installed.exists());
}

#[test]
fn a_wrong_checksum_a_tampered_module_and_a_declined_question_install_nothing() {
    let host = Host::start();
    let wasm = fixture("demo.wasm");
    host.publish("demo", "1.0.0", &wasm, json!({"storage": true}));
    let data = Data::new("refuse");
    let url = host.url("/plugins.json");
    rt(&data, "", &["sources", "add", &url]);
    let none = || !data.0.join("plugins/demo.wasm").exists();

    // The user verified another checksum than the index lists.
    let o = rt(
        &data,
        "",
        &["install", "demo", "--yes", "--sha256", &"0".repeat(64)],
    );
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Nothing was installed"), "{}", o.stderr);
    assert!(none());

    // The module on the host is not the one the index describes.
    host.serve("/demo-1.0.0.wasm", republished(wasm.clone()));
    let o = rt(
        &data,
        "",
        &["install", "demo", "--yes", "--sha256", &sha256_hex(&wasm)],
    );
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("checksum does not match"), "{}", o.stderr);
    assert!(none());

    // Declined, or no answer at all.
    host.serve("/demo-1.0.0.wasm", wasm.clone());
    let o = rt(&data, "n\n", &["install", "demo"]);
    assert_eq!(o.code, 2, "{}", o.stdout);
    assert!(o.stdout.contains("Not installed"));
    assert!(none());
    let o = rt(&data, "", &["install", "demo"]);
    assert_eq!(o.code, 2);
    assert!(none());

    // An answer installs.
    let o = rt(&data, "y\n", &["install", "demo"]);
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
    assert!(!none());
}

#[test]
fn rights_come_from_the_module_and_a_difference_warns() {
    let host = Host::start();
    let wasm = fixture("demo.wasm"); // storage only
    host.publish("demo", "1.0.0", &wasm, json!({"network": ["*"]}));
    let data = Data::new("rights");
    rt(&data, "", &["sources", "add", &host.url("/plugins.json")]);
    let o = rt(&data, "n\n", &["install", "demo"]);
    assert!(o.stdout.contains("rights   storage"), "{}", o.stdout);
    assert!(
        o.stdout.contains("warning  The index names other rights"),
        "{}",
        o.stdout
    );
    assert!(
        !o.stdout.contains("network"),
        "the claim is not shown as a right: {}",
        o.stdout
    );
}

#[test]
fn a_republished_release_is_refused_and_an_update_with_more_rights_asks() {
    let host = Host::start();
    let v1 = fixture("demo.wasm");
    host.publish("demo", "1.0.0", &v1, json!({"storage": true}));
    let data = Data::new("update");
    rt(&data, "", &["sources", "add", &host.url("/plugins.json")]);
    assert_eq!(
        rt(
            &data,
            "",
            &["install", "demo", "--yes", "--sha256", &sha256_hex(&v1)]
        )
        .code,
        0
    );

    // The index now lists other bytes under 1.0.0.
    let changed = republished(v1.clone());
    host.publish("demo", "1.0.0", &changed, json!({"storage": true}));
    let o = rt(
        &data,
        "",
        &[
            "install",
            "demo",
            "--yes",
            "--sha256",
            &sha256_hex(&changed),
        ],
    );
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("the index changed a published release"),
        "{}",
        o.stderr
    );
    assert_eq!(
        std::fs::read(data.0.join("plugins/demo.wasm")).unwrap(),
        v1,
        "still the old module"
    );

    // Version 2 asks for network access: found when asked, asked about, never silent.
    let v2 = fixture("demo-v2.wasm");
    host.publish(
        "demo",
        "2.0.0",
        &v2,
        json!({"network": ["{request}"], "storage": true}),
    );
    let o = rt(&data, "", &["update", "--check"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("demo  1.0.0 -> 2.0.0"), "{}", o.stdout);
    assert_eq!(
        rt(&data, "", &["list"]).stdout.matches("1.0.0").count(),
        1,
        "--check installs nothing"
    );
    // `--yes` with the verified checksum still asks about more rights.
    let o = rt(
        &data,
        "",
        &["install", "demo", "--yes", "--sha256", &sha256_hex(&v2)],
    );
    assert_eq!(o.code, 2, "{}", o.stdout);
    assert!(
        o.stdout.contains("asks for more than you granted before"),
        "{}",
        o.stdout
    );
    let o = rt(&data, "y\n", &["update"]);
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
    assert!(rt(&data, "", &["list"]).stdout.contains("demo  2.0.0"));
}

#[test]
fn a_module_by_address_needs_a_question_or_a_checksum() {
    let host = Host::start();
    let wasm = fixture("demo.wasm");
    host.serve("/demo.wasm", wasm.clone());
    let data = Data::new("direct");
    let url = host.url("/demo.wasm");
    let o = rt(&data, "n\n", &["install", &url]);
    assert_eq!(o.code, 2);
    assert!(
        o.stdout.contains(&sha256_hex(&wasm)),
        "the computed checksum is shown: {}",
        o.stdout
    );
    let o = rt(
        &data,
        "",
        &["install", &url, "--yes", "--sha256", &"1".repeat(64)],
    );
    assert_eq!(o.code, 1, "{}", o.stdout);
    assert!(o.stderr.contains("checksum does not match"), "{}", o.stderr);
    let o = rt(
        &data,
        "",
        &["install", &url, "--yes", "--sha256", &sha256_hex(&wasm)],
    );
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
}

#[test]
fn a_git_source_can_be_cloned_for_its_index() {
    let git = |dir: &Path, args: &[&str]| {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.test")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.test")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    let data = Data::new("clone");
    let repo = data.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    let index = br#"{"version":1,"name":"In Git","plugins":[]}"#;
    std::fs::write(repo.join("plugins.json"), index).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "index"]);
    let source = rt_app::catalogue::GitSource {
        url: repo.to_str().unwrap().to_owned(),
        git_ref: None,
    };
    let (bytes, commit) = rt_cli::plugin_cmd::clone_index(&source).unwrap();
    assert_eq!(bytes, index);
    assert_eq!(commit.len(), 40);
    let missing = rt_app::catalogue::GitSource {
        url: repo.to_str().unwrap().to_owned(),
        git_ref: Some("nope".into()),
    };
    assert!(rt_cli::plugin_cmd::clone_index(&missing).is_err());
}
