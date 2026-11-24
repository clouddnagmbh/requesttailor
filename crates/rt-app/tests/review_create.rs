// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! "Create pull request" (wave 15, strand E): `ReviewProvider::create` for GitHub, GitHub
//! Enterprise and GitLab against a fake `Transport` that records the call and answers as the
//! host does (success, "already exists", rejected token). No network.

use std::cell::RefCell;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use rt_app::review::{NewPullRequest, PullState, ReviewError, ReviewKind, ReviewRepo, provider};
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use serde_json::{Value, json};

struct Once(Option<Bytes>);

impl Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

/// Answers every call with `status` and `body`, remembers the requests.
struct Fake {
    status: u16,
    body: Value,
    seen: RefCell<Vec<HttpRequest>>,
}

impl Fake {
    fn new(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            seen: RefCell::new(Vec::new()),
        }
    }
    fn last(&self) -> HttpRequest {
        self.seen.borrow().last().cloned().expect("a request")
    }
    fn sent_json(&self) -> Value {
        serde_json::from_slice(&self.last().body.expect("a body").bytes).unwrap()
    }
}

impl Transport for Fake {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let url = request.url.clone();
        self.seen.borrow_mut().push(request);
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(self.body.to_string()))));
        Ok(Response {
            head: ResponseHead {
                status: self.status,
                version: HttpVersion::Http11,
                headers: vec![("content-type".into(), "application/json".into())],
                url,
                redirects: vec![],
                timing: Timing::default(),
            },
            body,
        })
    }
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

fn repo(kind: ReviewKind, api: &str, project: &str) -> ReviewRepo {
    ReviewRepo {
        kind,
        host: "git.example.test".into(),
        api: api.into(),
        project: project.into(),
    }
}

fn new_pull(draft: bool) -> NewPullRequest {
    NewPullRequest {
        title: "Add currency".into(),
        body: "Adds the field.".into(),
        head: "feature/currency".into(),
        base: "main".into(),
        draft,
    }
}

const TOKEN: &str = "tok-secret-123";

fn github_pull() -> Value {
    json!({
        "number": 7, "title": "Add currency", "user": { "login": "ada" }, "state": "open",
        "draft": true, "merged_at": null,
        "head": { "ref": "feature/currency", "sha": "aaa" },
        "base": { "ref": "main", "sha": "bbb" },
        "html_url": "https://github.com/acme/orders/pull/7",
        "updated_at": "2026-11-24T10:00:00Z"
    })
}

#[test]
fn github_creates_a_draft_pull_request() {
    let t = Fake::new(201, github_pull());
    let p = provider(
        &t,
        &repo(ReviewKind::Github, "https://api.github.com", "acme/orders"),
        TOKEN,
    );
    let made = run(p.create(&new_pull(true))).unwrap();
    assert_eq!(made.number, 7);
    assert_eq!(made.state, PullState::Open);
    assert!(made.draft);
    assert_eq!(made.url, "https://github.com/acme/orders/pull/7");
    let sent = t.last();
    assert_eq!(sent.method, "POST");
    assert_eq!(sent.url, "https://api.github.com/repos/acme/orders/pulls");
    assert_eq!(
        t.sent_json(),
        json!({ "title": "Add currency", "body": "Adds the field.", "head": "feature/currency", "base": "main", "draft": true })
    );
}

#[test]
fn github_enterprise_uses_its_own_api_base() {
    let t = Fake::new(201, github_pull());
    let p = provider(
        &t,
        &repo(
            ReviewKind::Github,
            "https://git.example.test/api/v3",
            "acme/orders",
        ),
        TOKEN,
    );
    run(p.create(&new_pull(false))).unwrap();
    assert_eq!(
        t.last().url,
        "https://git.example.test/api/v3/repos/acme/orders/pulls"
    );
    assert_eq!(t.sent_json()["draft"], json!(false));
}

#[test]
fn github_explains_an_existing_pull_request_and_hides_the_token() {
    let t = Fake::new(
        422,
        json!({ "message": "Validation Failed", "errors": [{ "message": format!("A pull request already exists for acme:feature/currency. {TOKEN}") }] }),
    );
    let p = provider(
        &t,
        &repo(ReviewKind::Github, "https://api.github.com", "acme/orders"),
        TOKEN,
    );
    let err = run(p.create(&new_pull(false))).unwrap_err();
    let ReviewError::Host(text) = &err else {
        panic!("{err:?}")
    };
    assert!(text.contains("already exists"), "{text}");
    assert!(!text.contains(TOKEN), "{text}");
}

#[test]
fn a_rejected_token_is_an_auth_error() {
    let t = Fake::new(401, json!({ "message": "Bad credentials" }));
    let p = provider(
        &t,
        &repo(ReviewKind::Github, "https://api.github.com", "acme/orders"),
        TOKEN,
    );
    assert!(matches!(
        run(p.create(&new_pull(false))),
        Err(ReviewError::Auth(_))
    ));
}

fn gitlab_mr() -> Value {
    json!({
        "iid": 3, "title": "Draft: Add currency", "author": { "username": "ada" },
        "state": "opened", "draft": true, "source_branch": "feature/currency",
        "target_branch": "main", "sha": "aaa",
        "web_url": "https://gitlab.com/group/sub/api/-/merge_requests/3",
        "updated_at": "2026-11-24T10:00:00Z",
        "diff_refs": { "base_sha": "bbb" }
    })
}

#[test]
fn gitlab_creates_a_merge_request_in_a_subgroup_project() {
    let t = Fake::new(201, gitlab_mr());
    let p = provider(
        &t,
        &repo(
            ReviewKind::Gitlab,
            "https://gitlab.com/api/v4",
            "group/sub/api",
        ),
        TOKEN,
    );
    let made = run(p.create(&new_pull(true))).unwrap();
    assert_eq!(made.number, 3);
    assert!(made.draft);
    let sent = t.last();
    assert_eq!(sent.method, "POST");
    assert_eq!(
        sent.url,
        "https://gitlab.com/api/v4/projects/group%2Fsub%2Fapi/merge_requests"
    );
    // GitLab marks a draft by the title; it has no flag on create.
    assert_eq!(
        t.sent_json(),
        json!({ "title": "Draft: Add currency", "description": "Adds the field.", "source_branch": "feature/currency", "target_branch": "main" })
    );
}

#[test]
fn gitlab_does_not_double_the_draft_prefix_and_keeps_ready_titles() {
    let t = Fake::new(201, gitlab_mr());
    let p = provider(
        &t,
        &repo(ReviewKind::Gitlab, "https://gitlab.com/api/v4", "group/api"),
        TOKEN,
    );
    let mut already = new_pull(true);
    already.title = "Draft: Add currency".into();
    run(p.create(&already)).unwrap();
    assert_eq!(t.sent_json()["title"], json!("Draft: Add currency"));
    run(p.create(&new_pull(false))).unwrap();
    assert_eq!(t.sent_json()["title"], json!("Add currency"));
}

#[test]
fn gitlab_reports_a_refused_merge_request() {
    let t = Fake::new(
        409,
        json!({ "message": ["Another open merge request already exists for this source branch: !2"] }),
    );
    let p = provider(
        &t,
        &repo(ReviewKind::Gitlab, "https://gitlab.com/api/v4", "group/api"),
        TOKEN,
    );
    let err = run(p.create(&new_pull(false))).unwrap_err();
    let ReviewError::Host(text) = &err else {
        panic!("{err:?}")
    };
    assert!(text.starts_with("409"), "{text}");
    assert!(text.contains("already exists"), "{text}");
}
