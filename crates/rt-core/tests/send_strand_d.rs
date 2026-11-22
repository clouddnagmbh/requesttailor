// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `send_with` with the wave 5 strand D steps: AWS SigV4 secrets masked in the report,
//! cookies stored per environment, sent again and masked.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_core::cookies::SharedJars;
use rt_core::model::{
    Auth, AwsSigV4, Collection, Environment, Item, RequestEntry, RequestSpec, Variable,
};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::send::{MASK, SendError, SendOptions, SendReport, send, send_with};
use rt_core::services::Services;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

struct Empty;

impl futures_core::Stream for Empty {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

/// Response headers, or the transport error.
type Answer = Result<Vec<(String, String)>, TransportError>;

/// Answers with scripted results and records every request.
#[derive(Default)]
struct Script {
    answers: RefCell<VecDeque<Answer>>,
    seen: RefCell<Vec<HttpRequest>>,
}

impl Script {
    fn answer(&self, answer: Result<Vec<(&str, &str)>, TransportError>) {
        self.answers.borrow_mut().push_back(answer.map(|h| {
            h.into_iter()
                .map(|(n, v)| (n.to_owned(), v.to_owned()))
                .collect()
        }));
    }

    fn last_header(&self, name: &str) -> Option<String> {
        let seen = self.seen.borrow();
        let request = seen.last()?;
        request
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }
}

impl Transport for Script {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let url = request.url.clone();
        self.seen.borrow_mut().push(request);
        let headers = self
            .answers
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(Vec::new()))?;
        let body: BodyStream = Box::pin(Empty);
        Ok(Response {
            head: ResponseHead {
                status: 200,
                version: HttpVersion::Http11,
                headers,
                url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

struct NoFiles;

impl FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("{path}: not found"))
    }
}

fn collection(url: &str, auth: Auth) -> Collection {
    Collection {
        name: "d".into(),
        defaults: Default::default(),
        items: vec![Item::Request(RequestEntry {
            path: "r.yml".into(),
            spec: RequestSpec {
                name: "r".into(),
                method: "GET".into(),
                url: url.into(),
                auth,
                ..Default::default()
            },
        })],
        environments: ["Local", "Prod"]
            .into_iter()
            .map(|name| Environment {
                name: name.into(),
                variables: vec![Variable::new("env", name)],
                client_certificates: Vec::new(),
                external_secrets: None,
            })
            .collect(),
        config: Default::default(),
    }
}

fn input(environment: Option<&str>) -> PrepareInput<'_> {
    PrepareInput {
        environment,
        globals: Vec::new(),
        runtime: Vec::new(),
    }
}

const SECRET: &str = "wJalrXUtnFEMI-literal-secret";
const TOKEN: &str = "session-token-literal-42";

fn aws() -> Auth {
    // Literal values, not secret variables: only `sign`'s return values make them secrets.
    Auth::AwsSigV4(AwsSigV4 {
        access_key_id: "AKIDEXAMPLE".into(),
        secret_access_key: SECRET.into(),
        session_token: TOKEN.into(),
        service: "execute-api".into(),
        region: "eu-central-1".into(),
        profile_name: String::new(),
    })
}

#[test]
fn sigv4_secret_key_and_session_token_are_masked() {
    let c = collection("https://api.example.com/items", aws());
    let t = Script::default();
    t.answer(Err(TransportError::Other(format!(
        "upstream said: bad key {SECRET} and token {TOKEN}"
    ))));
    let r = block_on(send(
        &c,
        "r.yml",
        &input(None),
        &NoFiles,
        &t,
        &SendOptions::default(),
    ));
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains(SECRET), "{json}");
    assert!(!json.contains(TOKEN), "{json}");
    let sent = r.request.as_ref().unwrap();
    let header = |name: &str| {
        sent.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(header("x-amz-security-token"), Some(MASK));
    assert!(
        header("authorization")
            .unwrap()
            .starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/")
    );
    assert!(header("x-amz-date").is_some());
    match &r.error {
        Some(SendError::Transport(TransportError::Other(m))) => {
            assert_eq!(m, "upstream said: bad key *** and token ***")
        }
        other => panic!("{other:?}"),
    }
    // The wire got the real token.
    assert_eq!(
        t.last_header("x-amz-security-token").as_deref(),
        Some(TOKEN)
    );
}

#[test]
fn sigv4_profile_without_keys_fails_clearly() {
    let c = collection(
        "https://api.example.com/items",
        Auth::AwsSigV4(AwsSigV4 {
            profile_name: "work".into(),
            service: "s3".into(),
            region: "us-east-1".into(),
            ..Default::default()
        }),
    );
    let t = Script::default();
    let r = block_on(send(
        &c,
        "r.yml",
        &input(None),
        &NoFiles,
        &t,
        &SendOptions::default(),
    ));
    match &r.error {
        Some(SendError::Auth(m)) => assert!(m.contains("profile `work`"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(t.seen.borrow().is_empty(), "nothing is sent unsigned");
}

fn send_jar(c: &Collection, env: Option<&str>, t: &Script, jars: &SharedJars) -> SendReport {
    let services = Services {
        cookies: Some(jars),
        ..Services::none()
    };
    block_on(send_with(
        c,
        "r.yml",
        &input(env),
        &NoFiles,
        t,
        &SendOptions::default(),
        &services,
    ))
}

#[test]
fn cookies_are_stored_per_environment_sent_again_and_masked() {
    let c = collection("https://api.example.com/login", Auth::None);
    let t = Script::default();
    let jars = SharedJars::default();

    t.answer(Ok(vec![
        ("set-cookie", "sid=abc123; Path=/; HttpOnly"),
        ("set-cookie", "theme=dark; Path=/"),
    ]));
    let r = send_jar(&c, Some("Local"), &t, &jars);
    assert_eq!(r.error, None, "{r:?}");
    assert_eq!(t.last_header("cookie"), None, "empty jar");
    let set: Vec<&str> = r
        .response
        .as_ref()
        .unwrap()
        .headers
        .iter()
        .filter(|(n, _)| n == "set-cookie")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(set, ["sid=***; Path=/; HttpOnly", "theme=***; Path=/"]);
    let stored = jars.snapshot();
    assert_eq!(stored.scope("Local").unwrap().cookies().len(), 2);
    assert!(stored.scope("Prod").is_none());

    // The next send in the same environment carries them; the report shows names only.
    let r = send_jar(&c, Some("Local"), &t, &jars);
    assert_eq!(
        t.last_header("cookie").as_deref(),
        Some("sid=abc123; theme=dark")
    );
    let sent = r.request.unwrap();
    let cookie = sent
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("cookie"))
        .map(|(_, v)| v.as_str());
    assert_eq!(cookie, Some("sid=***; theme=***"));
    assert!(!serde_json::to_string(&sent).unwrap().contains("abc123"));

    // Another environment, or none, has its own (empty) jar.
    send_jar(&c, Some("Prod"), &t, &jars);
    assert_eq!(t.last_header("cookie"), None);
    send_jar(&c, None, &t, &jars);
    assert_eq!(t.last_header("cookie"), None);

    // An expiring Set-Cookie removes the cookie.
    t.answer(Ok(vec![("set-cookie", "sid=; Max-Age=0; Path=/")]));
    send_jar(&c, Some("Local"), &t, &jars);
    send_jar(&c, Some("Local"), &t, &jars);
    assert_eq!(t.last_header("cookie").as_deref(), Some("theme=dark"));
}

#[test]
fn without_a_jar_set_cookie_is_still_masked() {
    let c = collection("https://api.example.com/", Auth::None);
    let t = Script::default();
    t.answer(Ok(vec![("Set-Cookie", "sid=abc123")]));
    let r = block_on(send(
        &c,
        "r.yml",
        &input(None),
        &NoFiles,
        &t,
        &SendOptions::default(),
    ));
    let headers = &r.response.unwrap().headers;
    assert_eq!(headers, &[("Set-Cookie".to_owned(), "sid=***".to_owned())]);
}
