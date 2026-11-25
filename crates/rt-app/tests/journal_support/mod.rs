// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Shared by the journal tests: a scripted transport, a small collection and snapshots.
#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::runner::RunSnapshot;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

pub const CANARY: &str = "RT-CANARY-journal-secret-5d21";

pub fn block_on<F: Future>(fut: F) -> F::Output {
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

/// What the scripted server answers.
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

pub fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> Answer {
    Answer {
        status,
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
        body: body.to_owned(),
    }
}

type Script = Box<dyn Fn(&HttpRequest) -> Answer + Send + Sync>;

/// Answers with `script` and records every request as `METHOD url`.
/// Method, URL and headers of one request that went out.
pub type Seen = (String, String, Vec<(String, String)>);

pub struct Wire {
    pub script: Script,
    pub seen: Mutex<Vec<Seen>>,
}

impl Wire {
    pub fn new(script: impl Fn(&HttpRequest) -> Answer + Send + Sync + 'static) -> Self {
        Self {
            script: Box::new(script),
            seen: Mutex::new(Vec::new()),
        }
    }

    pub fn requests(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(m, u, _)| format!("{m} {u}"))
            .collect()
    }

    pub fn headers_of(&self, index: usize) -> Vec<(String, String)> {
        self.seen.lock().unwrap()[index].2.clone()
    }
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push((
            request.method.clone(),
            request.url.clone(),
            request.headers.clone(),
        ));
        let a = (self.script)(&request);
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(a.body))));
        Ok(Response {
            head: ResponseHead {
                status: a.status,
                version: HttpVersion::Http11,
                headers: a.headers,
                url: request.url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

fn request_file(name: &str, seq: u32, method: &str, url: &str, extra: &str) -> (String, Vec<u8>) {
    (
        format!("{name}.yml"),
        format!(
            "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {method}\n  url: \"{url}\"\n{extra}"
        )
        .into_bytes(),
    )
}

/// A collection called `name` with environments `dev` and `prod` (both with a secret
/// `token`) and the given requests `(file name, method, url)`.
pub fn collection(name: &str, requests: &[(&str, &str, &str)]) -> FileSet {
    let mut files = vec![
        (
            COLLECTION_FILE.to_owned(),
            format!("opencollection: \"1.0.0\"\ninfo:\n  name: {name}\n").into_bytes(),
        ),
        (
            "environments/dev.yml".to_owned(),
            b"name: dev\nvariables:\n  - name: base\n    value: http://dev.test\n  - name: token\n    secret: true\n".to_vec(),
        ),
        (
            "environments/prod.yml".to_owned(),
            b"name: prod\nvariables:\n  - name: base\n    value: http://prod.test\n  - name: token\n    secret: true\n".to_vec(),
        ),
    ];
    for (i, (file, method, url)) in requests.iter().enumerate() {
        files.push(request_file(file, i as u32 + 1, method, url, ""));
    }
    FileSet {
        files: files.into_iter().collect(),
    }
}

pub fn snapshot(files: &FileSet, environment: &str) -> RunSnapshot {
    let mut collection = read_collection(files).unwrap().model().clone();
    for env in &mut collection.environments {
        for v in &mut env.variables {
            if v.secret {
                v.value = Some(CANARY.to_owned());
            }
        }
    }
    RunSnapshot {
        collection,
        environment: Some(environment.to_owned()),
        vars: Vec::new(),
        files: FileSet::default(),
    }
}

/// A unique directory under the system temp folder, removed on drop.
pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rt-journal-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// No body files.
pub struct NoFiles;

impl rt_core::prepare::FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("no file {path}"))
    }
}
