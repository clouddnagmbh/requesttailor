// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Pull-request review providers (wave 12, strand E) against the local fake Git host
//! (`testdata/review/fake-host.mjs`, Node): GitHub and GitLab list, get, comments, a comment
//! on a line, the three verdicts and CI, the error kinds, and that the token appears in no
//! error. Never a real host, never a real token. Needs `node` and `git` on the PATH.

#![cfg(not(target_arch = "wasm32"))]

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, Command, Stdio};

use rt_app::review::{
    CiState, NewReviewComment, PullState, ReviewComment, ReviewError, ReviewHost, ReviewKind,
    ReviewRepo, ReviewSubmission, ReviewVerdict, detect, device, normalize_hosts, provider,
    review_base_ref, review_ref, token_slot,
};
use rt_core::transport::native::NativeTransport;
use serde_json::Value;

struct FakeHost {
    child: Child,
    stdin: Option<ChildStdin>,
    info: Value,
}

impl FakeHost {
    fn start(extra: &[&str]) -> FakeHost {
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/review/fake-host.mjs"
        );
        let mut child = Command::new("node")
            .arg(script)
            .args(["--github-port", "0", "--gitlab-port", "0"])
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("node is needed for the fake Git host");
        let stdin = child.stdin.take().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let json = line
            .trim()
            .strip_prefix("READY ")
            .expect("fake host did not start");
        FakeHost {
            child,
            stdin: Some(stdin),
            info: serde_json::from_str(json).unwrap(),
        }
    }

    fn token(&self) -> String {
        self.info["token"].as_str().unwrap().to_owned()
    }

    fn repo(&self, kind: ReviewKind) -> ReviewRepo {
        let (port, api) = match kind {
            ReviewKind::Github => (self.info["github"].as_u64().unwrap(), "api/v3"),
            ReviewKind::Gitlab => (self.info["gitlab"].as_u64().unwrap(), "api/v4"),
        };
        ReviewRepo {
            kind,
            host: format!("127.0.0.1:{port}"),
            api: format!("http://127.0.0.1:{port}/{api}"),
            project: "acme/orders".into(),
        }
    }

    fn state(&self, kind: ReviewKind) -> Value {
        let repo = self.repo(kind);
        let host = repo.host;
        let out = Command::new("curl")
            .args(["-s", &format!("http://{host}/__state")])
            .output()
            .unwrap();
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

impl Drop for FakeHost {
    fn drop(&mut self) {
        // Closing its stdin makes it remove its work dir and exit.
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

fn comment(path: &str, line: u32, body: &str) -> NewReviewComment {
    NewReviewComment {
        path: path.into(),
        line,
        body: body.into(),
    }
}

/// Everything a provider must do, for either host.
async fn exercise(host: &FakeHost, kind: ReviewKind) {
    let transport = NativeTransport::new().unwrap();
    let repo = host.repo(kind);
    let p = provider(&transport, &repo, &host.token());
    let head = host.info["head"].as_str().unwrap();

    // list: both open ones, most recently updated first; the draft is marked.
    let pulls = p.list().await.unwrap();
    assert_eq!(pulls.iter().map(|x| x.number).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(pulls[0].title, "Add currency and cancel-order");
    assert_eq!(pulls[0].author, "ada");
    assert_eq!(pulls[0].state, PullState::Open);
    assert!(!pulls[0].draft && pulls[1].draft);
    assert_eq!(
        (pulls[0].source.as_str(), pulls[0].target.as_str()),
        ("feature/currency", "main")
    );
    assert_eq!(pulls[0].head, head);
    assert_eq!(pulls[0].updated, 1_795_165_200);
    assert!(pulls[0].url.contains("/acme/orders/"));

    // get: the single one has the base too.
    let one = p.get(1).await.unwrap();
    assert_eq!(one.head, head);
    assert_eq!(one.base, host.info["base"].as_str().unwrap());

    assert_eq!(
        p.head_ref(1),
        match kind {
            ReviewKind::Github => "refs/pull/1/head",
            ReviewKind::Gitlab => "refs/merge-requests/1/head",
        }
    );

    // comments: general and on a line; the line comment whose line moved is outdated.
    let comments = p.comments(1).await.unwrap();
    let by = |pred: &dyn Fn(&ReviewComment) -> bool| comments.iter().find(|c| pred(c)).cloned();
    let on_line = by(&|c| c.path.is_some() && !c.outdated).expect("a line comment");
    assert_eq!(on_line.path.as_deref(), Some("orders/create-order.yml"));
    assert_eq!(on_line.line, Some(17));
    assert_eq!(on_line.author, "grace");
    let outdated = by(&|c| c.outdated).expect("an outdated comment");
    assert_eq!(outdated.path.as_deref(), Some("orders/create-order.yml"));
    let general = by(&|c| c.path.is_none()).expect("a general comment");
    assert!(general.body.contains("contract notes"));
    assert!(comments.windows(2).all(|w| w[0].created <= w[1].created));

    // comment on a line of the head.
    let made = p
        .comment(&one, &comment("orders/list-orders.yml", 12, "Why 25?"))
        .await
        .unwrap();
    assert_eq!(made.body, "Why 25?");
    assert_eq!(made.path.as_deref(), Some("orders/list-orders.yml"));
    assert_eq!(made.line, Some(12));
    let after = p.comments(1).await.unwrap();
    assert!(
        after
            .iter()
            .any(|c| c.body == "Why 25?" && c.line == Some(12))
    );
    // A line that does not exist at the head is refused, with the host's reason.
    let bad = p
        .comment(&one, &comment("orders/list-orders.yml", 999, "x"))
        .await
        .unwrap_err();
    assert!(matches!(bad, ReviewError::Host(_)), "{bad:?}");

    // the three verdicts.
    for (verdict, body) in [
        (ReviewVerdict::Approve, "Looks good"),
        (ReviewVerdict::RequestChanges, "Please keep the old limit"),
        (ReviewVerdict::Comment, "A remark"),
    ] {
        p.submit(
            &one,
            &ReviewSubmission {
                verdict,
                body: body.into(),
            },
        )
        .await
        .unwrap();
    }
    let state = host.state(kind);
    match kind {
        ReviewKind::Github => {
            let events: Vec<&str> = state["reviews"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["event"].as_str().unwrap())
                .collect();
            assert_eq!(events, ["APPROVE", "REQUEST_CHANGES", "COMMENT"]);
            assert_eq!(state["reviews"][0]["commit_id"], head);
            // GitHub refuses "request changes" without a reason.
            let e = p
                .submit(
                    &one,
                    &ReviewSubmission {
                        verdict: ReviewVerdict::RequestChanges,
                        body: String::new(),
                    },
                )
                .await
                .unwrap_err();
            assert!(
                matches!(&e, ReviewError::Host(m) if m.contains("422")),
                "{e:?}"
            );
        }
        ReviewKind::Gitlab => {
            let approvals: Vec<bool> = state["approvals"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a["approved"].as_bool().unwrap())
                .collect();
            // approve, then request changes withdraws it.
            assert_eq!(approvals, [true, false]);
            let notes: Vec<&str> = state["notes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n["body"].as_str().unwrap())
                .collect();
            assert_eq!(
                notes,
                ["Looks good", "Please keep the old limit", "A remark"]
            );
        }
    }

    // CI: one failed, one pending, one succeeded: failure overall.
    let ci = p.ci(&one).await.unwrap();
    assert_eq!(ci.state, CiState::Failure);
    let state_of = |name: &str| ci.checks.iter().find(|c| c.name == name).map(|c| c.state);
    assert_eq!(state_of("build"), Some(CiState::Success));
    assert_eq!(state_of("rt run"), Some(CiState::Failure));
    assert_eq!(state_of("lint"), Some(CiState::Pending));
    assert!(ci.checks.iter().all(|c| {
        c.url
            .as_deref()
            .is_none_or(|u| u.starts_with("https://ci.example.test/"))
    }));
}

#[test]
fn github_provider_does_everything() {
    let host = FakeHost::start(&[]);
    run(exercise(&host, ReviewKind::Github));
}

#[test]
fn gitlab_provider_does_everything() {
    let host = FakeHost::start(&[]);
    run(exercise(&host, ReviewKind::Gitlab));
}

#[test]
fn lists_follow_the_hosts_paging() {
    // One item per page: both providers must follow the next link / header.
    let host = FakeHost::start(&["--max-per-page", "1"]);
    run(async {
        let transport = NativeTransport::new().unwrap();
        for kind in [ReviewKind::Github, ReviewKind::Gitlab] {
            let p = provider(&transport, &host.repo(kind), &host.token());
            let numbers: Vec<u64> = p.list().await.unwrap().iter().map(|x| x.number).collect();
            assert_eq!(numbers, [1, 2], "{kind:?}");
        }
    });
}

#[test]
fn error_kinds_and_the_token_never_shows() {
    let host = FakeHost::start(&[]);
    let secret = "ghp_SECRET-token-VALUE-0123456789";
    run(async {
        let transport = NativeTransport::new().unwrap();
        for kind in [ReviewKind::Github, ReviewKind::Gitlab] {
            // A wrong token: auth.
            let wrong = provider(&transport, &host.repo(kind), secret);
            let e = wrong.list().await.unwrap_err();
            assert!(matches!(e, ReviewError::Auth(_)), "{e:?}");
            let e2 = wrong.get(1).await.unwrap_err();
            assert!(!format!("{e:?}{e}{e2:?}").contains(secret));

            // A right token: an unknown PR is not-found.
            let right = provider(&transport, &host.repo(kind), &host.token());
            let e = right.get(99).await.unwrap_err();
            assert!(matches!(e, ReviewError::NotFound(_)), "{e:?}");

            // Nothing listens: transport, token masked even if the URL carried it.
            let mut gone = host.repo(kind);
            gone.api = format!("http://127.0.0.1:1/{secret}/api");
            let p = provider(&transport, &gone, secret);
            let e = p.list().await.unwrap_err();
            assert!(matches!(e, ReviewError::Transport(_)), "{e:?}");
            assert!(!format!("{e:?}{e}").contains(secret), "{e}");
        }
    });
}

#[test]
fn the_host_sees_only_the_bearer_header() {
    // The token is sent as a header and nowhere else; the fake host records what it was sent.
    let host = FakeHost::start(&[]);
    run(async {
        let transport = NativeTransport::new().unwrap();
        let p = provider(&transport, &host.repo(ReviewKind::Github), &host.token());
        p.list().await.unwrap();
    });
    let state = host.state(ReviewKind::Github);
    let seen: Vec<&str> = state["authorizations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert!(!seen.is_empty());
    assert!(
        seen.iter()
            .all(|a| *a == format!("Bearer {}", host.token()))
    );
}

#[test]
fn device_code_login_against_the_fake_host() {
    let host = FakeHost::start(&[]);
    run(async {
        let transport = NativeTransport::new().unwrap();
        for kind in [ReviewKind::Github, ReviewKind::Gitlab] {
            let repo = host.repo(kind);
            let id = host.info["clientId"].as_str().unwrap();
            // Not registered in the public build: no device code for the real hosts.
            assert_eq!(
                device::client_id(&detect("https://github.com/a/b", &[]).unwrap()),
                None
            );
            assert_eq!(
                device::client_id(&detect("https://gitlab.com/a/b", &[]).unwrap()),
                None
            );
            assert!(device::client_id(&repo).is_none());

            let login = device::start(&transport, &repo, id).await.unwrap();
            assert_eq!(login.prompt.user_code, "WDJB-MJHT");
            assert!(
                login
                    .prompt
                    .verification_uri
                    .starts_with("http://127.0.0.1:")
            );
            let first = device::poll(&transport, &repo, id, &login).await.unwrap();
            assert_eq!(first, device::DevicePoll::Pending);
            let second = device::poll(&transport, &repo, id, &login).await.unwrap();
            assert_eq!(second, device::DevicePoll::Token(host.token()));
            // A wrong client id is refused.
            let e = device::start(&transport, &repo, "other").await.unwrap_err();
            assert!(matches!(e, ReviewError::Host(_)), "{e:?}");
        }
    });
}

#[test]
fn slots_refs_and_hosts() {
    let slot = token_slot("gitlab.example.com:8443");
    assert_eq!(slot.collection, "review:tokens");
    assert_eq!(slot.environment, "gitlab.example.com:8443");
    assert_eq!(slot.variable, "token");
    assert_eq!(
        review_ref(ReviewKind::Github, 7),
        "refs/requesttailor/review/github/7"
    );
    assert_eq!(
        review_base_ref(ReviewKind::Gitlab, 7),
        "refs/requesttailor/review/gitlab/7-base"
    );
    let ok = normalize_hosts(&[ReviewHost {
        host: " Git.Example.com ".into(),
        kind: ReviewKind::Github,
        api: "https://git.example.com/api/v3/".into(),
    }])
    .unwrap();
    assert_eq!(ok[0].host, "git.example.com");
    assert_eq!(ok[0].api, "https://git.example.com/api/v3");
    let bad = |host: &str, api: &str| {
        normalize_hosts(&[ReviewHost {
            host: host.into(),
            kind: ReviewKind::Gitlab,
            api: api.into(),
        }])
        .is_err()
    };
    assert!(bad("", "https://x.example/api/v4"));
    assert!(bad("a/b", "https://x.example/api/v4"));
    assert!(bad("x.example", "ftp://x.example/api"));
    assert!(bad("x.example", ""));
}
