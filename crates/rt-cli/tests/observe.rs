// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17: a distribution's exchange observers (`rt_cli::Distribution.observers`) get one
//! masked exchange from `rt send`, `rt run`, `rt mcp` and `rt agent`. A canary secret in the
//! URL, a header, a cookie and the response body never reaches them.
//!
//! No test harness (`harness = false`): the binary is both the test and, started again with
//! `RT_OBSERVE_TEST_CHILD=1`, an `rt` built like a distribution whose observer appends every
//! exchange as one JSON line to `RT_OBSERVE_FILE`.
//!
//! Ports: the agent uses 48503. Run: `cargo test -p rt-cli --test observe`.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use rt_core::observe::{Exchange, ExchangeObserver, ObserverSet};
use serde_json::{Value, json};

const SECRET: &str = "RT-CANARY-4f81c3d7";
const AGENT_PORT: u16 = 48503;
const ORIGIN: &str = "http://127.0.0.1:5187";

/// Appends each exchange to the file named by `RT_OBSERVE_FILE`.
struct ToFile(PathBuf);

impl ExchangeObserver for ToFile {
    fn observe(&self, exchange: &Exchange) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.0)
            .unwrap();
        writeln!(f, "{}", serde_json::to_string(exchange).unwrap()).unwrap();
    }
}

fn main() {
    if let Some(file) = std::env::var_os("RT_OBSERVE_FILE")
        && std::env::var_os("RT_OBSERVE_TEST_CHILD").is_some()
    {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let code = rt_cli::run_distribution(
            args,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
            &rt_cli::Distribution {
                extensions: &[],
                observers: ObserverSet::new().with(Arc::new(ToFile(PathBuf::from(file)))),
                ..Default::default()
            },
        );
        std::process::exit(code);
    }
    let server = server();
    let dir = tmp();
    let (root, data) = (dir.join("collection"), dir.join("data"));
    collection(&root, server);
    send_and_run_deliver_one_masked_exchange(&root, &data, &dir);
    mcp_delivers_with_the_agent_caller(&root, &data, &dir);
    agent_delivers_without_collection(&data, &dir, server);
    disabled_in_the_collection_nothing_is_delivered(&root, &data, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    println!("observe: 4 passed");
}

fn exchanges(file: &Path) -> Vec<Value> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn assert_masked(file: &Path) {
    let raw = std::fs::read_to_string(file).unwrap_or_default();
    assert!(!raw.contains(SECRET), "canary leaked to an observer: {raw}");
    assert!(!raw.contains("sessioncookie"), "cookie leaked: {raw}");
}

fn send_and_run_deliver_one_masked_exchange(root: &Path, data: &Path, dir: &Path) {
    let file = dir.join("send.jsonl");
    let _ = std::fs::remove_file(&file);
    let request = root.join("orders/list.yml");
    let (code, out) = rt(&["send", request.to_str().unwrap()], data, &file);
    assert_eq!(code, rt_cli::EXIT_OK, "{out}");
    let got = exchanges(&file);
    assert_eq!(got.len(), 1, "rt send: one exchange");
    assert_eq!(got[0]["caller"], "cli");
    assert_eq!(got[0]["collection"], "Orders");
    assert_eq!(got[0]["report"]["response"]["status"], 200);
    assert_eq!(
        got[0]["report"]["response"]["body"]["data"],
        r#"{"token":"***"}"#
    );
    assert_masked(&file);

    let file = dir.join("run.jsonl");
    let _ = std::fs::remove_file(&file);
    let (code, out) = rt(&["run", root.to_str().unwrap()], data, &file);
    assert_eq!(code, rt_cli::EXIT_OK, "{out}");
    let got = exchanges(&file);
    assert_eq!(got.len(), 2, "rt run: one exchange per request");
    assert!(got.iter().all(|x| x["caller"] == "cli"));
    assert_masked(&file);
}

fn disabled_in_the_collection_nothing_is_delivered(root: &Path, data: &Path, dir: &Path) {
    let off = dir.join("off");
    collection_with(&off, server(), "    observe:\n      enabled: false\n");
    let file = dir.join("off.jsonl");
    let _ = std::fs::remove_file(&file);
    let request = off.join("orders/list.yml");
    let (code, _) = rt(&["send", request.to_str().unwrap()], data, &file);
    assert_eq!(code, rt_cli::EXIT_OK);
    assert!(exchanges(&file).is_empty());
    // Control: the other collection still delivers.
    let request = root.join("orders/list.yml");
    rt(&["send", request.to_str().unwrap()], data, &file);
    assert_eq!(exchanges(&file).len(), 1);
}

fn mcp_delivers_with_the_agent_caller(root: &Path, data: &Path, dir: &Path) {
    let file = dir.join("mcp.jsonl");
    let _ = std::fs::remove_file(&file);
    let mut mcp = Mcp::start(root, data, &file);
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/list.yml", "environment": "Local"}),
    );
    assert_eq!(r["isError"], false, "{r}");
    drop(mcp);
    let got = exchanges(&file);
    assert_eq!(got.len(), 1, "rt mcp: one exchange");
    assert_eq!(got[0]["caller"], "agent");
    assert_masked(&file);
}

fn agent_delivers_without_collection(data: &Path, dir: &Path, server: u16) {
    let file = dir.join("agent.jsonl");
    let _ = std::fs::remove_file(&file);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["agent", "--port", &AGENT_PORT.to_string()])
        .args(["--allow-origin", ORIGIN])
        .env("RT_OBSERVE_TEST_CHILD", "1")
        .env("RT_OBSERVE_FILE", &file)
        .env("RT_DATA_DIR", data)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let code = loop {
        let line = lines.next().expect("pairing code").unwrap();
        if let Some(rest) = line.strip_prefix("Pairing code: ") {
            break rest.split_whitespace().next().unwrap().to_owned();
        }
    };
    let token = {
        let r = post("/v1/pair", None, &json!({ "code": code }));
        serde_json::from_slice::<Value>(&r).unwrap()["token"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let wire = json!({
        "method": "GET",
        "url": format!("http://127.0.0.1:{server}/items?api_key={SECRET}"),
        "headers": [["Authorization", format!("Bearer {SECRET}")], ["Cookie", "sid=sessioncookie"]],
        "body": null,
        "content_type": null,
        "timeout_ms": 10000,
        "follow_redirects": true,
        "max_redirects": 5
    });
    let reply = post("/v1/send", Some(&token), &wire);
    // An egress call of an extension (wave 19) is answered and handed to no observer.
    let mut marked = wire.clone();
    marked["headers"] = json!([[rt_core::transport::companion::EGRESS_HEADER, "1"]]);
    let egress_reply = post("/v1/send", Some(&token), &marked);
    assert!(String::from_utf8_lossy(&egress_reply).contains(SECRET));
    let _ = child.kill();
    let _ = child.wait();
    // The browser still gets the real body: the companion streams it unchanged.
    assert!(
        String::from_utf8_lossy(&reply).contains(SECRET),
        "the client's copy is unchanged"
    );
    let got = exchanges(&file);
    assert_eq!(got.len(), 1, "rt agent: one exchange");
    assert_eq!(got[0]["caller"], "app");
    assert!(got[0]["collection"].is_null());
    assert!(got[0]["request_path"].is_null());
    assert_eq!(got[0]["report"]["response"]["status"], 200);
    assert_masked(&file);
}

// ---- support ----------------------------------------------------------------------------

/// A server that answers every request with a body echoing the canary and a session cookie.
fn server() -> u16 {
    static PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 16384];
                let _ = stream.read(&mut buf);
                let body = format!(r#"{{"token":"{SECRET}"}}"#);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nset-cookie: sid=sessioncookie; Path=/\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        port
    })
}

fn post(path: &str, token: Option<&str>, body: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(body).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", AGENT_PORT)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{AGENT_PORT}\r\nOrigin: {ORIGIN}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(t) = token {
        head.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(&body).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
    let body = raw[end + 4..].to_vec();
    if head.contains("transfer-encoding: chunked") {
        dechunk(&body)
    } else {
        body
    }
}

fn dechunk(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").unwrap();
        let size = usize::from_str_radix(
            std::str::from_utf8(&data[..line_end])
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .trim(),
            16,
        )
        .unwrap();
        data = &data[line_end + 2..];
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(root: &Path, port: u16) {
    collection_with(root, port, "");
}

fn collection_with(root: &Path, port: u16, extra: &str) {
    write(
        &root.join("opencollection.yml"),
        &format!(
            "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n\nextensions:\n  requesttailor:\n    agents:\n      access: read\n{extra}"
        ),
    );
    write(
        &root.join("environments/Local.yml"),
        &format!(
            "name: Local\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
        ),
    );
    for (name, seq) in [("list", 1), ("detail", 2)] {
        write(
            &root.join(format!("orders/{name}.yml")),
            &format!(
                "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}/orders?token={{{{token}}}}\"\n"
            ),
        );
    }
}

fn child(args: &[&str], data: &Path, file: &Path) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(args)
        .args(["--var", &format!("token={SECRET}")])
        .env("RT_OBSERVE_TEST_CHILD", "1")
        .env("RT_OBSERVE_FILE", file)
        .env("RT_DATA_DIR", data);
    c
}

fn rt(args: &[&str], data: &Path, file: &Path) -> (i32, String) {
    let mut args = args.to_vec();
    args.extend(["--no-plugins", "--env", "Local"]);
    let out = child(&args, data, file).output().unwrap();
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
    fn start(root: &Path, data: &Path, file: &Path) -> Self {
        let mut child = child(&["mcp", root.to_str().unwrap()], data, file)
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
                "clientInfo": {"name": "observe-agent", "version": "1"}
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

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-observe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
