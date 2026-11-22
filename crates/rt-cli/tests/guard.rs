// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 10: a distribution's send guard (`rt_cli::run_distribution`) is asked by `rt send`,
//! `rt run` and both sending tools of `rt mcp`; a refused request never reaches the server,
//! `rt send`/`rt run` exit with `EXIT_BLOCKED`, and `rt mcp` records the rule in the audit log.
//!
//! No test harness (`harness = false`): the binary is both the test and, started again with
//! `RT_GUARD_TEST_CHILD=1`, an `rt` built like a distribution with the guard below.
//!
//! Run: `cargo test -p rt-cli --test guard`.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rt_core::guard::{Caller, Effect, GuardHandle, GuardVerdict, SendCheck, SendGuard};
use serde_json::{Value, json};

const SECRET: &str = "s3cr3t-guard-4711";

/// Refuses writing requests in production for agents, and every request to `blocked.invalid`.
struct TestGuard;

impl SendGuard for TestGuard {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        assert!(!check.url.contains(SECRET), "the guard saw a secret");
        if check.caller == Caller::Agent && check.production && check.effect == Effect::Write {
            return GuardVerdict::Refuse {
                rule: "agents.production-writes".into(),
                reason: "agents may not write to production".into(),
            };
        }
        if check.environment.as_deref() == Some("PRD") && check.caller == Caller::Cli {
            return GuardVerdict::Refuse {
                rule: "cli.production".into(),
                reason: "the command line may not send to production".into(),
            };
        }
        GuardVerdict::Allow
    }
}

fn main() {
    if std::env::var_os("RT_GUARD_TEST_CHILD").is_some() {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let code = rt_cli::run_distribution(
            args,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
            &rt_cli::Distribution {
                extensions: &[],
                guard: GuardHandle::new(Arc::new(TestGuard)),
                ..Default::default()
            },
        );
        std::process::exit(code);
    }
    let server = counting_server();
    let dir = tmp();
    let (root, data) = (dir.join("collection"), dir.join("data"));
    collection(&root, server.port);
    rt_send_and_rt_run_are_blocked(&root, &data, &server.hits);
    mcp_tools_are_refused_and_audited(&root, &data, &server.hits);
    let _ = std::fs::remove_dir_all(&dir);
    println!("guard: 2 passed");
}

fn rt_send_and_rt_run_are_blocked(root: &Path, data: &Path, hits: &AtomicUsize) {
    let file = root.join("orders/list.yml");
    let (code, out) = rt(&["send", file.to_str().unwrap(), "--env", "PRD"], data);
    assert_eq!(code, rt_cli::EXIT_BLOCKED, "{out}");
    let report: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["error"]["kind"], "blocked", "{report}");
    assert_eq!(report["error"]["message"]["rule"], "cli.production");
    assert!(!out.contains(SECRET));
    // Another environment is allowed.
    let (code, out) = rt(&["send", file.to_str().unwrap(), "--env", "Local"], data);
    assert_eq!(code, rt_cli::EXIT_OK, "{out}");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let (code, out) = rt(&["run", root.to_str().unwrap(), "--env", "PRD"], data);
    assert_eq!(code, rt_cli::EXIT_BLOCKED, "{out}");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "rt run sent nothing");
}

fn mcp_tools_are_refused_and_audited(root: &Path, data: &Path, hits: &AtomicUsize) {
    let before = hits.load(Ordering::SeqCst);
    let mut mcp = Mcp::start(root, data);
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/create.yml", "environment": "PRD"}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(
        text(&r).contains("agents may not write to production"),
        "{r}"
    );
    // Reading in production is allowed.
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/list.yml", "environment": "PRD"}),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(hits.load(Ordering::SeqCst), before + 1);
    // run_folder: the write is refused, the read goes out; no approval is asked.
    let r = mcp.call(
        "run_folder",
        json!({"folder": "orders", "environment": "PRD"}),
    );
    assert!(
        text(&r).contains("agents may not write to production"),
        "{r}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), before + 2);
    drop(mcp);

    let audit = std::fs::read_to_string(data.join("agents/audit.jsonl")).unwrap();
    assert!(!audit.contains(SECRET));
    let entries: Vec<Value> = audit
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let refused: Vec<&Value> = entries
        .iter()
        .filter(|e| e["tool"] == "send_request" && e["rule"] == "agents.production-writes")
        .collect();
    assert_eq!(refused.len(), 1, "{audit}");
    assert_eq!(refused[0]["outcome"], "refused");
    assert_eq!(refused[0]["environment"], "PRD");
    let run = entries
        .iter()
        .find(|e| e["tool"] == "run_folder")
        .expect("run_folder audited");
    assert_eq!(run["outcome"], "sent", "the read went out");
    assert_eq!(run["rule"], "agents.production-writes");
}

// ---- support ----------------------------------------------------------------------------

struct CountingServer {
    port: u16,
    hits: Arc<AtomicUsize>,
}

fn counting_server() -> CountingServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 16384];
            let _ = stream.read(&mut buf);
            h.fetch_add(1, Ordering::SeqCst);
            let body = r#"{"ok":true}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    CountingServer { port, hits }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(root: &Path, port: u16) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n\nextensions:\n  requesttailor:\n    agents:\n      access: read\n      environments:\n        Local: { access: write }\n        PRD: { access: write, production: true }\n",
    );
    for env in ["Local", "PRD"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
            ),
        );
    }
    write(
        &root.join("orders/list.yml"),
        "info:\n  name: List orders\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/orders?token={{token}}\"\n",
    );
    write(
        &root.join("orders/create.yml"),
        "info:\n  name: Create order\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{base}}/orders?token={{token}}\"\n",
    );
}

fn child(args: &[&str], data: &Path) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(args)
        .args(["--var", &format!("token={SECRET}")])
        .env("RT_GUARD_TEST_CHILD", "1")
        .env("RT_DATA_DIR", data);
    c
}

fn rt(args: &[&str], data: &Path) -> (i32, String) {
    let mut args = args.to_vec();
    args.push("--no-plugins");
    let out = child(&args, data).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start(root: &Path, data: &Path) -> Self {
        let mut child = child(&["mcp", root.to_str().unwrap()], data)
            .args(["--approval-timeout", "2"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut mcp = Mcp {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        mcp.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "guard-agent", "version": "1"}
            }),
        );
        let msg = json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}});
        writeln!(mcp.stdin, "{msg}").unwrap();
        mcp
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{msg}").unwrap();
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "rt mcp closed stdout"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))["result"].clone()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
