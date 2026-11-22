// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance (wave 8, strand B, point 5): `rt send` on a gRPC request against
//! `rt-testserver`'s echo service prints the masked `StreamLog` with decoded JSON.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn testdata(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/grpc")
            .join(name),
    )
    .unwrap()
}

/// A collection with the echo protos under `protos/` and `include/`.
fn collection(tag: &str, url: &str, method: &str, messages: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("rt-cli-grpc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write(
        &root.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: g\nconfig:\n  protobuf:\n    protoFiles:\n      - path: protos/echo.proto\n    importPaths:\n      - path: include\n",
    );
    write(
        &root.join("protos/echo.proto"),
        &testdata("protos/echo.proto"),
    );
    write(
        &root.join("include/rt/common.proto"),
        &testdata("include/rt/common.proto"),
    );
    write(
        &root.join("call.yml"),
        &format!(
            "info:\n  name: call\n  type: grpc\n\ngrpc:\n  url: {url}\n  method: {method}\n  methodType: unary\n  metadata:\n    - name: x-trace\n      value: \"{{{{token}}}}\"\n{messages}"
        ),
    );
    root
}

fn rt(file: &Path, extra: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("send")
        .arg(file)
        .args(["--no-plugins"])
        .args(extra)
        .env("RT_DATA_DIR", file.parent().unwrap().join(".data"))
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn an_unknown_method_or_bad_message_is_a_prepare_error() {
    let root = collection("bad", "localhost:1", "/rt.echo.v1.EchoService/Nope", "");
    let (code, out, err) = rt(&root.join("call.yml"), &[]);
    assert_eq!(code, 3, "{err}");
    assert!(out.contains("/rt.echo.v1.EchoService/Nope"), "{out}{err}");

    // The message is encoded once the call is open.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let addr = runtime
        .block_on(rt_testserver::spawn_grpc(SocketAddr::from((
            [127, 0, 0, 1],
            0,
        ))))
        .unwrap();
    let root = collection(
        "msg",
        &format!("grpc://{addr}"),
        "/rt.echo.v1.EchoService/Echo",
        "  message: '{\"nope\": 1}'\n",
    );
    let (code, out, err) = rt(&root.join("call.yml"), &[]);
    assert_eq!(code, 3, "{err}");
    assert!(out.contains("nope"), "{out}{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rt_send_prints_the_decoded_stream_log() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let addr = runtime
        .block_on(rt_testserver::spawn_grpc(SocketAddr::from((
            [127, 0, 0, 1],
            0,
        ))))
        .unwrap();
    let root = collection(
        "echo",
        &format!("grpc://{addr}"),
        "/rt.echo.v1.EchoService/Echo",
        "  message: '{\"message\": \"hi\"}'\n",
    );
    let (code, out, err) = rt(&root.join("call.yml"), &["--var", "token=s3cret"]);
    assert_eq!(code, 0, "{err}\n{out}");
    let log: serde_json::Value = serde_json::from_str(&out).unwrap();
    let events = log["events"].as_array().unwrap();
    let received: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e["event"] == "message" && e["direction"] == "received")
        .collect();
    assert_eq!(received.len(), 1, "{out}");
    let reply: serde_json::Value =
        serde_json::from_str(received[0]["frame"]["data"].as_str().unwrap()).unwrap();
    assert_eq!(reply["message"], "hi");
    assert_eq!(received[0]["name"], "rt.echo.v1.EchoReply");
    assert_eq!(events.last().unwrap()["code"], 0);
    let _ = std::fs::remove_dir_all(root);
}
