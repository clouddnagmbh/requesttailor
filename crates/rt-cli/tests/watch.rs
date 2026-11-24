// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand D: `rt send --watch`. Sends again whenever the request, its body file, the
//! folder files, `opencollection.yml`, the environment or a companion file changes, and
//! prints one line per send. The budget `watch_resend` (file change to request on the wire)
//! is measured by `watch_resend_is_measured` (`node scripts/perf/run.mjs --only watch`).

#[path = "support/watch_server.rs"]
mod server;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

const SECRET: &str = "RT-CANARY-watch-1";
const WAIT: Duration = Duration::from_secs(10);

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-watch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(root: &Path, port: u16) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Items\n\nextensions:\n  requesttailor:\n    agents:\n      access: write\n      environments:\n        PRD: { access: write, production: true }\n",
    );
    for env in ["Local", "PRD"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - name: path\n    value: item\n  - secret: true\n    name: token\n"
            ),
        );
    }
    write(
        &root.join("items/folder.yml"),
        "info:\n  name: items\n  type: folder\n  seq: 1\n",
    );
    write(
        &root.join("items/get.yml"),
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/{{path}}?token={{token}}\"\n",
    );
    write(
        &root.join("items/post.yml"),
        "info:\n  name: Post\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{base}}/item\"\n  body:\n    type: json\n    data: '{\"a\":1}'\n",
    );
    write(&root.join("items/payload.json"), "{\"v\":1}");
    write(
        &root.join("items/file.yml"),
        "info:\n  name: File\n  type: http\n  seq: 3\n\nhttp:\n  method: GET\n  url: \"{{base}}/file\"\n  body:\n    type: file\n    data:\n      - filePath: items/payload.json\n        selected: true\n",
    );
}

struct Watch {
    child: Child,
    lines: Receiver<String>,
}

impl Watch {
    fn start(args: &[&str], data: &Path) -> Watch {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
            .arg("send")
            .args(args)
            .args([
                "--watch",
                "--no-plugins",
                "--var",
                &format!("token={SECRET}"),
            ])
            .env("RT_DATA_DIR", data)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Watch { child, lines }
    }

    fn line(&self) -> String {
        match self.lines.recv_timeout(WAIT) {
            Ok(l) => l,
            Err(RecvTimeoutError::Timeout) => panic!("no output within {WAIT:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("rt exited"),
        }
    }

    fn quiet(&self, how_long: Duration) {
        if let Ok(l) = self.lines.recv_timeout(how_long) {
            panic!("unexpected output: {l}");
        }
    }

    /// Ctrl-C and the exit code. Windows has no signal to send from here: the process is
    /// killed and 0 stands in for the code.
    fn interrupt(mut self) -> i32 {
        #[cfg(unix)]
        {
            Command::new("kill")
                .args(["-INT", &self.child.id().to_string()])
                .status()
                .unwrap();
            self.child.wait().unwrap().code().unwrap_or(-1)
        }
        #[cfg(not(unix))]
        {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
            0
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn args<'a>(file: &'a Path, extra: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec![file.to_str().unwrap(), "-e", "Local"];
    v.extend_from_slice(extra);
    v
}

#[test]
fn every_watched_file_triggers_a_send_with_a_line_about_the_difference() {
    let server = server::spawn();
    let dir = tmp("files");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, server.port);
    let get = root.join("items/get.yml");
    let watch = Watch::start(&args(&get, &[]), &data);

    let first = watch.line();
    assert!(first.starts_with("200 "), "{first}");
    assert!(first.contains("first response"), "{first}");
    assert!(!first.contains(SECRET));
    watch.quiet(Duration::from_millis(700));

    // Nothing changed in the answer: "same".
    write(
        &get,
        &std::fs::read_to_string(&get)
            .unwrap()
            .replace("seq: 1", "seq: 5"),
    );
    let same = watch.line();
    assert!(same.starts_with("200 ") && same.ends_with("same"), "{same}");

    // The answer changed: the paths are named.
    *server.name.lock().unwrap() = "coffee".into();
    write(
        &root.join("items/folder.yml"),
        "info:\n  name: items\n  type: folder\n  seq: 2\n",
    );
    let changed = watch.line();
    assert!(changed.contains("changed: $.name"), "{changed}");

    // opencollection.yml, the environment and a companion file each trigger a send.
    write(
        &root.join("opencollection.yml"),
        &std::fs::read_to_string(root.join("opencollection.yml"))
            .unwrap()
            .replace("Items", "Items 2"),
    );
    assert!(watch.line().starts_with("200 "));
    write(
        &root.join("environments/Local.yml"),
        &std::fs::read_to_string(root.join("environments/Local.yml"))
            .unwrap()
            .replace("value: item", "value: other"),
    );
    assert!(watch.line().starts_with("200 "));
    let before = server.seen.lock().unwrap().len();
    write(&root.join("items/get.snap.json"), "{}");
    assert!(watch.line().starts_with("200 "));
    assert_eq!(server.seen.lock().unwrap().len(), before + 1);
    assert!(
        server
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|(_, l)| l.contains("/other?")),
        "env value used"
    );

    // A request next door is not watched.
    write(
        &root.join("items/post.yml"),
        "info:\n  name: Post\n  type: http\n  seq: 9\n\nhttp:\n  method: POST\n  url: \"{{base}}/item\"\n",
    );
    watch.quiet(Duration::from_millis(800));

    assert_eq!(watch.interrupt(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_body_file_is_watched_and_a_json_line_is_the_report() {
    let server = server::spawn();
    let dir = tmp("body");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, server.port);
    let file = root.join("items/file.yml");
    let watch = Watch::start(&args(&file, &["--json"]), &data);
    let first: serde_json::Value = serde_json::from_str(&watch.line()).unwrap();
    assert_eq!(first["response"]["status"], 200);
    write(&root.join("items/payload.json"), "{\"v\":2}");
    let second: serde_json::Value = serde_json::from_str(&watch.line()).unwrap();
    assert_eq!(second["request_path"], first["request_path"]);
    assert!(!second.to_string().contains(SECRET));
    assert_eq!(watch.interrupt(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writing_requests_need_allow_writes_and_never_run_against_production() {
    let server = server::spawn();
    let dir = tmp("writes");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, server.port);
    let post = root.join("items/post.yml");
    let run = |extra: &[&str], env: &str| {
        Command::new(env!("CARGO_BIN_EXE_rt"))
            .arg("send")
            .arg(&post)
            .args(["-e", env, "--watch", "--no-plugins"])
            .args(extra)
            .env("RT_DATA_DIR", &data)
            .output()
            .unwrap()
    };
    let out = run(&[], "Local");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--allow-writes"));
    let out = run(&["--allow-writes"], "PRD");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("production"));
    assert!(server.seen.lock().unwrap().is_empty(), "nothing was sent");

    let watch = Watch::start(&args(&post, &["--allow-writes"]), &data);
    assert!(watch.line().starts_with("200 "));
    assert_eq!(watch.interrupt(), 0);
    // --allow-writes without --watch is a usage error.
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["send", post.to_str().unwrap(), "--allow-writes"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn ctrl_c_exits_with_the_code_of_the_last_run() {
    let dir = tmp("exit");
    let (root, data) = (dir.join("c"), dir.join("data"));
    // The server is gone: the first send fails with a transport error (exit code 1).
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    collection(&root, port);
    let get = root.join("items/get.yml");
    let watch = Watch::start(&args(&get, &[]), &data);
    let line = watch.line();
    assert!(line.starts_with("error"), "{line}");
    assert_eq!(watch.interrupt(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Budget `watch_resend`: from the change of a file to the request on the wire, median of 9.
#[test]
fn watch_resend_is_measured() {
    let server = server::spawn();
    let dir = tmp("perf");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, server.port);
    let get = root.join("items/get.yml");
    let watch = Watch::start(&args(&get, &[]), &data);
    watch.line();
    let mut samples = Vec::new();
    for n in 0..9 {
        // Do not start inside the same poll tick as the previous change.
        std::thread::sleep(Duration::from_millis(130 + 37 * n));
        let seen = server.seen.lock().unwrap().len();
        let text = std::fs::read_to_string(&get).unwrap();
        let t0 = Instant::now();
        write(&get, &format!("{text}\n# {n}\n"));
        watch.line();
        let arrival = server.seen.lock().unwrap()[seen].0;
        samples.push(arrival.duration_since(t0).as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[4];
    assert!(median < 500.0, "median {median} ms, {samples:?}");
    if let Some(out) = std::env::var_os("RT_PERF_OUT") {
        std::fs::write(
            Path::new(&out).join("watch.json"),
            serde_json::json!({ "watch_resend": median, "samples": samples }).to_string(),
        )
        .unwrap();
    }
    assert_eq!(watch.interrupt(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
