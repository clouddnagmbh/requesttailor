// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The app's run snapshot (wave 7, strand D): plan in tree order, secret values in the
//! environment, body files of every request, and a run through a fake transport.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::{SendInput, TreeNode, Workspace};
use rt_core::runner::{RunEvent, RunPlan};
use rt_core::send::{MASK, SendOptions};
use rt_core::services::Services;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::{COLLECTION_FILE, FileSet};

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

#[derive(Default)]
struct Wire {
    seen: RefCell<Vec<HttpRequest>>,
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let url = request.url.clone();
        self.seen.borrow_mut().push(request);
        let body: BodyStream = Box::pin(Once(Some(Bytes::from_static(b"{}"))));
        Ok(Response {
            head: ResponseHead {
                status: 200,
                version: HttpVersion::Http11,
                headers: Vec::new(),
                url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

fn files(list: &[(&str, &str)]) -> FileSet {
    FileSet {
        files: list
            .iter()
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect(),
    }
}

/// `b/` (seq 1) with `b/z.yml` (seq 1) and `b/a.yml` (seq 2); `a.yml` (seq 2) with a file
/// body; `c.yml` without seq.
fn sample() -> FileSet {
    files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: run\n",
        ),
        (
            "b/folder.yml",
            "info:\n  name: B\n  type: folder\n  seq: 1\n",
        ),
        (
            "b/z.yml",
            "info:\n  name: Z\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://h/z?t={{token}}\n",
        ),
        (
            "b/a.yml",
            "info:\n  name: A in B\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: http://h/b-a\n",
        ),
        (
            "a.yml",
            "info:\n  name: A\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: http://h/a\n  body:\n    type: file\n    data:\n      - filePath: files/payload.txt\n        contentType: text/plain\n        selected: true\n",
        ),
        (
            "c.yml",
            "info:\n  name: C\n  type: http\n\nhttp:\n  method: GET\n  url: http://h/c\n",
        ),
        ("files/payload.txt", "payload"),
        ("files/unused.txt", "not read"),
        (
            "environments/Local.yml",
            "name: Local\nvariables:\n  - secret: true\n    name: token\n",
        ),
    ])
}

fn tree_paths(node: &TreeNode, out: &mut Vec<String>) {
    match node {
        TreeNode::Folder { children, .. } => children.iter().for_each(|c| tree_paths(c, out)),
        TreeNode::Request { path, .. } => out.push(path.clone()),
    }
}

#[test]
fn plan_follows_the_tree_and_the_run_has_secrets_and_body_files() {
    let ws = Workspace::open(sample()).unwrap();
    let mut tree = Vec::new();
    tree_paths(&ws.tree(), &mut tree);
    let snapshot = ws.run_snapshot(SendInput {
        environment: Some("Local".into()),
        vars: vec![("extra".into(), "1".into())],
        secrets: vec![("token".into(), "s3cr3t".into())],
    });
    let plan = snapshot.plan(None);
    assert_eq!(plan, tree);
    assert_eq!(plan, ["b/z.yml", "b/a.yml", "a.yml", "c.yml"]);
    assert_eq!(snapshot.plan(Some("b")), ["b/z.yml", "b/a.yml"]);
    // Only the files bodies read.
    assert_eq!(
        snapshot.files.files.keys().collect::<Vec<_>>(),
        ["files/payload.txt"]
    );
    assert_eq!(snapshot.vars, [("extra".to_owned(), "1".to_owned())]);

    let wire = Wire::default();
    let mut steps = Vec::new();
    let summary = block_on(snapshot.run(
        &RunPlan {
            requests: plan,
            iterations: 1,
            ..Default::default()
        },
        &wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |e| {
            if let RunEvent::Step { step } = e {
                steps.push(step);
            }
        },
        &|| false,
    ));
    assert_eq!(summary.requests, 4);
    assert_eq!(summary.passed, 4);
    let seen = wire.seen.borrow();
    // The secret value goes on the wire, masked in the report.
    assert_eq!(seen[0].url, "http://h/z?t=s3cr3t");
    assert_eq!(
        steps[0].report.request.as_ref().unwrap().url,
        format!("http://h/z?t={MASK}")
    );
    // The file body was read from the snapshot.
    assert_eq!(
        seen[2].body.as_ref().map(|b| b.bytes.to_vec()),
        Some(b"payload".to_vec())
    );
}
