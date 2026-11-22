// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 flows against a scripted fake token endpoint: every grant, the token cache,
//! refresh and its fallbacks, PKCE, state, device code polling, error cases and masking.
//! The integration tests against Keycloak are in `crates/rt-core/tests/oauth2_idp.rs`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use bytes::Bytes;
use sha2::Digest as _;

use super::*;
use crate::model::{
    CredentialsPlacement, OAuth2, OAuth2Flow, OAuth2Param, OAuth2Step, ParamPlacement, PkceMethod,
    TokenPlacement, TokenSource, TokenUse,
};
use crate::services::{
    AuthPrompt, DeviceCodePrompt, LocalBoxFuture, NoteLevel, Services, StoredToken, TokenStore,
};
use crate::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use crate::vars::{Scope, VarLayer, VarSource};

// ---------------------------------------------------------------------------------------------
// Fakes

/// The fakes are always ready (the clock's sleep included), so a busy poll is enough.
fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

struct Chunks(VecDeque<Bytes>);

impl futures_core::Stream for Chunks {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.pop_front().map(Ok))
    }
}

/// What the fake endpoint answers: status and JSON body.
type Answer = (u16, String);
type Script = Box<dyn Fn(&Seen, usize) -> Answer>;
type Redirect = Box<dyn Fn(&url::Url) -> Result<String, String>>;

/// A token endpoint (and device authorization endpoint) answering from a script. Records
/// every request.
struct FakeIdp {
    answer: Script,
    seen: RefCell<Vec<Seen>>,
}

/// A request as the fake saw it, with the form body decoded.
#[derive(Debug, Clone)]
struct Seen {
    url: String,
    headers: Vec<(String, String)>,
    form: Vec<(String, String)>,
}

impl Seen {
    fn field(&self, name: &str) -> Option<&str> {
        self.form
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl FakeIdp {
    fn new(answer: impl Fn(&Seen, usize) -> Answer + 'static) -> Self {
        Self {
            answer: Box::new(answer),
            seen: RefCell::new(Vec::new()),
        }
    }

    /// Always answers with a fresh token `tok-<n>` (n counts the requests), refresh token
    /// `ref-<n>`, lifetime 300 s.
    fn tokens() -> Self {
        Self::new(|_, n| ok_token(&format!("tok-{n}"), Some(&format!("ref-{n}")), 300))
    }

    fn requests(&self) -> Vec<Seen> {
        self.seen.borrow().clone()
    }
}

fn ok_token(access: &str, refresh: Option<&str>, expires_in: u64) -> Answer {
    let mut v = serde_json::json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": expires_in,
    });
    if let Some(r) = refresh {
        v["refresh_token"] = r.into();
    }
    (200, v.to_string())
}

fn oauth_error(status: u16, code: &str, description: &str) -> Answer {
    (
        status,
        serde_json::json!({"error": code, "error_description": description}).to_string(),
    )
}

impl Transport for FakeIdp {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let form = request
            .body
            .as_ref()
            .map(|b| {
                url::form_urlencoded::parse(&b.bytes)
                    .into_owned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let seen = Seen {
            url: request.url.clone(),
            headers: request.headers.clone(),
            form,
        };
        let n = self.seen.borrow().len();
        let (status, body) = (self.answer)(&seen, n);
        self.seen.borrow_mut().push(seen);
        let body: BodyStream = Box::pin(Chunks(VecDeque::from([Bytes::from(body)])));
        Ok(Response {
            head: ResponseHead {
                status,
                version: HttpVersion::Http11,
                headers: vec![("content-type".into(), "application/json".into())],
                url: request.url,
                redirects: vec![],
                timing: Timing::default(),
            },
            body,
        })
    }
}

/// Virtual time: `sleep` advances the clock at once and is recorded.
struct FakeClock {
    now: Cell<i64>,
    sleeps: RefCell<Vec<u64>>,
    counter: Cell<u8>,
}

const T0: i64 = 1_800_000_000_000;

impl FakeClock {
    fn new() -> Self {
        Self {
            now: Cell::new(T0),
            sleeps: RefCell::new(Vec::new()),
            counter: Cell::new(0),
        }
    }
    fn advance(&self, ms: i64) {
        self.now.set(self.now.get() + ms);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.now.get()
    }
    fn sleep(&self, ms: u64) -> LocalBoxFuture<'_, ()> {
        self.sleeps.borrow_mut().push(ms);
        self.now.set(self.now.get() + ms as i64);
        Box::pin(std::future::ready(()))
    }
    fn fill_random(&self, buf: &mut [u8]) -> Result<(), String> {
        for b in buf {
            let c = self.counter.get().wrapping_add(1);
            self.counter.set(c);
            *b = c;
        }
        Ok(())
    }
}

#[derive(Default)]
struct MemTokens(RefCell<HashMap<String, StoredToken>>);

impl MemTokens {
    fn with(key: &str, token: StoredToken) -> Self {
        let s = Self::default();
        s.0.borrow_mut().insert(key.to_owned(), token);
        s
    }
    fn get_now(&self, key: &str) -> Option<StoredToken> {
        self.0.borrow().get(key).cloned()
    }
}

impl TokenStore for MemTokens {
    fn get<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, Option<StoredToken>> {
        Box::pin(std::future::ready(self.0.borrow().get(key).cloned()))
    }
    fn put<'a>(&'a self, key: &'a str, token: &'a StoredToken) -> LocalBoxFuture<'a, ()> {
        self.0.borrow_mut().insert(key.to_owned(), token.clone());
        Box::pin(std::future::ready(()))
    }
    fn remove<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, ()> {
        self.0.borrow_mut().remove(key);
        Box::pin(std::future::ready(()))
    }
}

/// Counts every interaction. `redirect` builds the redirect URL from the authorization URL.
struct FakePrompt {
    redirect: Redirect,
    authorize_urls: RefCell<Vec<(String, String)>>,
    device: RefCell<Vec<DeviceCodePrompt>>,
    done: Cell<usize>,
    cancel_after_polls: Cell<Option<usize>>,
    cancel_checks: Cell<usize>,
}

impl FakePrompt {
    fn new(redirect: impl Fn(&url::Url) -> Result<String, String> + 'static) -> Self {
        Self {
            redirect: Box::new(redirect),
            authorize_urls: RefCell::new(Vec::new()),
            device: RefCell::new(Vec::new()),
            done: Cell::new(0),
            cancel_after_polls: Cell::new(None),
            cancel_checks: Cell::new(0),
        }
    }

    /// Signs in at once: redirects with `code=the-code` and the state it was given.
    fn approving() -> Self {
        Self::new(|url| {
            let state = query(url.as_str(), "state").unwrap_or_default();
            let redirect = query(url.as_str(), "redirect_uri").unwrap();
            Ok(format!("{redirect}?code=the-code&state={state}"))
        })
    }

    fn interactions(&self) -> usize {
        self.authorize_urls.borrow().len() + self.device.borrow().len()
    }
}

impl AuthPrompt for FakePrompt {
    fn authorize<'a>(
        &'a self,
        url: &'a str,
        redirect_uri: &'a str,
    ) -> LocalBoxFuture<'a, Result<String, String>> {
        self.authorize_urls
            .borrow_mut()
            .push((url.to_owned(), redirect_uri.to_owned()));
        let parsed = url::Url::parse(url).expect("authorization URL parses");
        Box::pin(std::future::ready((self.redirect)(&parsed)))
    }
    fn device_code<'a>(&'a self, prompt: &'a DeviceCodePrompt) -> LocalBoxFuture<'a, ()> {
        self.device.borrow_mut().push(prompt.clone());
        Box::pin(std::future::ready(()))
    }
    fn device_code_done<'a>(&'a self) -> LocalBoxFuture<'a, ()> {
        self.done.set(self.done.get() + 1);
        Box::pin(std::future::ready(()))
    }
    fn cancelled(&self) -> bool {
        let n = self.cancel_checks.get() + 1;
        self.cancel_checks.set(n);
        self.cancel_after_polls.get().is_some_and(|after| n > after)
    }
}

fn query(url: &str, name: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.into_owned())
}

// ---------------------------------------------------------------------------------------------
// Fixtures

fn scope() -> Scope {
    Scope {
        layers: vec![VarLayer {
            source: VarSource::Environment {
                name: "Local".into(),
            },
            vars: vec![
                ("idp".into(), "https://idp.example".into(), false),
                ("clientSecret".into(), "s3cr3t-value".into(), true),
                ("userPassword".into(), "pa ss&word".into(), true),
            ],
        }],
    }
}

fn client_credentials() -> OAuth2 {
    OAuth2 {
        flow: OAuth2Flow::ClientCredentials,
        access_token_url: "{{idp}}/token".into(),
        client_id: "my-client".into(),
        client_secret: "{{clientSecret}}".into(),
        scope: "read write".into(),
        ..OAuth2::default()
    }
}

fn auth_code(pkce: Option<PkceMethod>) -> OAuth2 {
    OAuth2 {
        flow: OAuth2Flow::AuthorizationCode {
            authorization_url: "{{idp}}/authorize".into(),
            callback_url: "http://127.0.0.1:47900/callback".into(),
            state: String::new(),
            pkce,
        },
        ..client_credentials()
    }
}

fn device_code() -> OAuth2 {
    OAuth2 {
        flow: OAuth2Flow::DeviceCode {
            device_authorization_url: "{{idp}}/device".into(),
        },
        client_secret: String::new(),
        ..client_credentials()
    }
}

fn request() -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: "https://api.example/items".into(),
        headers: vec![("Accept".into(), "application/json".into())],
        body: None,
        options: crate::transport::SendOptions::default(),
    }
}

fn services<'a>(tokens: Option<&'a MemTokens>, prompt: Option<&'a FakePrompt>) -> Services<'a> {
    Services {
        tokens: tokens.map(|t| t as &dyn TokenStore),
        prompt: prompt.map(|p| p as &dyn AuthPrompt),
        cookies: None,
        hooks: None,
        scripts: None,
    }
}

fn run_with(
    auth: &OAuth2,
    idp: &FakeIdp,
    services: &Services<'_>,
    clock: &FakeClock,
) -> Result<Authorized, String> {
    block_on(run(auth, &scope(), request(), idp, services, clock))
}

fn header<'r>(request: &'r HttpRequest, name: &str) -> Option<&'r str> {
    request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn token(
    access: &str,
    refresh: Option<&str>,
    obtained_at: i64,
    expires_at: Option<i64>,
) -> StoredToken {
    StoredToken {
        access_token: access.into(),
        refresh_token: refresh.map(str::to_owned),
        id_token: None,
        token_type: Some("Bearer".into()),
        scope: None,
        expires_at,
        obtained_at,
    }
}

// ---------------------------------------------------------------------------------------------
// Client credentials, password, SAML bearer

#[test]
fn client_credentials_with_basic_auth_header() {
    let idp = FakeIdp::tokens();
    let clock = FakeClock::new();
    let got = run_with(&client_credentials(), &idp, &services(None, None), &clock).unwrap();

    let seen = idp.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].url, "https://idp.example/token");
    assert_eq!(seen[0].field("grant_type"), Some("client_credentials"));
    assert_eq!(seen[0].field("scope"), Some("read write"));
    assert_eq!(seen[0].field("client_secret"), None);
    let basic = format!("Basic {}", STANDARD.encode("my-client:s3cr3t-value"));
    assert_eq!(seen[0].header("Authorization"), Some(basic.as_str()));
    assert_eq!(seen[0].header("Accept"), Some("application/json"));

    assert_eq!(header(&got.request, "Authorization"), Some("Bearer tok-0"));
    assert_eq!(header(&got.request, "Accept"), Some("application/json"));
    assert!(got.secrets.contains(&"tok-0".to_owned()));
    assert!(got.secrets.contains(&"ref-0".to_owned()));
    assert!(got.secrets.contains(&"s3cr3t-value".to_owned()));
    assert_eq!(got.notes.len(), 1);
    assert_eq!(got.notes[0].source, "auth");
    assert_eq!(got.notes[0].level, NoteLevel::Info);
    assert!(got.notes[0].text.contains("new token"), "{:?}", got.notes);
}

#[test]
fn basic_auth_header_form_encodes_the_credentials() {
    // RFC 6749 §2.3.1: client id and secret are form-encoded before base64.
    let idp = FakeIdp::tokens();
    let auth = OAuth2 {
        client_id: "a:b c".into(),
        client_secret: "p&q".into(),
        ..client_credentials()
    };
    run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap();
    let basic = format!("Basic {}", STANDARD.encode("a%3Ab+c:p%26q"));
    assert_eq!(
        idp.requests()[0].header("Authorization"),
        Some(basic.as_str())
    );
}

#[test]
fn client_credentials_in_the_body() {
    let idp = FakeIdp::tokens();
    let auth = OAuth2 {
        credentials_placement: CredentialsPlacement::Body,
        ..client_credentials()
    };
    run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap();
    let seen = &idp.requests()[0];
    assert_eq!(seen.field("client_id"), Some("my-client"));
    assert_eq!(seen.field("client_secret"), Some("s3cr3t-value"));
    assert_eq!(seen.header("Authorization"), None);
}

#[test]
fn a_public_client_sends_only_its_id() {
    let idp = FakeIdp::tokens();
    let auth = OAuth2 {
        client_secret: String::new(),
        ..client_credentials()
    };
    run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap();
    let seen = &idp.requests()[0];
    assert_eq!(seen.field("client_id"), Some("my-client"));
    assert_eq!(seen.header("Authorization"), None);
}

#[test]
fn password_grant_and_invalid_grant_error() {
    let auth = OAuth2 {
        flow: OAuth2Flow::Password {
            username: "alice".into(),
            password: "{{userPassword}}".into(),
        },
        ..client_credentials()
    };
    let idp = FakeIdp::tokens();
    run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap();
    let seen = &idp.requests()[0];
    assert_eq!(seen.field("grant_type"), Some("password"));
    assert_eq!(seen.field("username"), Some("alice"));
    assert_eq!(seen.field("password"), Some("pa ss&word"));

    // The provider echoes the password in its message: it must not reach the report.
    let idp = FakeIdp::new(|_, _| oauth_error(400, "invalid_grant", "bad password pa ss&word"));
    let err = run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap_err();
    assert!(err.contains("invalid_grant"), "{err}");
    assert!(err.contains("bad password ***"), "{err}");
    assert!(!err.contains("pa ss&word"), "{err}");
}

#[test]
fn a_token_endpoint_without_json_is_a_clear_error() {
    let idp = FakeIdp::new(|_, _| (502, "<html>Bad gateway</html>".into()));
    let err = run_with(
        &client_credentials(),
        &idp,
        &services(None, None),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("HTTP 502"), "{err}");
    let idp = FakeIdp::new(|_, _| (200, r#"{"token_type":"Bearer"}"#.into()));
    let err = run_with(
        &client_credentials(),
        &idp,
        &services(None, None),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("access_token"), "{err}");
}

#[test]
fn saml2_bearer_exchanges_the_assertion() {
    let auth = OAuth2 {
        flow: OAuth2Flow::Saml2Bearer {
            assertion: "PHNhbWw+YXNzZXJ0aW9uPC9zYW1sPg".into(),
        },
        ..client_credentials()
    };
    let idp = FakeIdp::tokens();
    let got = run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap();
    let seen = &idp.requests()[0];
    assert_eq!(
        seen.field("grant_type"),
        Some("urn:ietf:params:oauth:grant-type:saml2-bearer")
    );
    assert_eq!(
        seen.field("assertion"),
        Some("PHNhbWw+YXNzZXJ0aW9uPC9zYW1sPg")
    );
    assert!(
        got.secrets
            .contains(&"PHNhbWw+YXNzZXJ0aW9uPC9zYW1sPg".to_owned())
    );

    let idp = FakeIdp::new(|s, _| {
        oauth_error(
            400,
            "invalid_grant",
            &format!("assertion {} expired", s.field("assertion").unwrap()),
        )
    });
    let err = run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap_err();
    assert!(!err.contains("PHNhbWw"), "{err}");
}

#[test]
fn implicit_is_refused_clearly() {
    let auth = OAuth2 {
        flow: OAuth2Flow::Implicit {
            authorization_url: "https://idp.example/authorize".into(),
            callback_url: "http://127.0.0.1:1/cb".into(),
            state: String::new(),
        },
        ..client_credentials()
    };
    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::approving();
    let err = run_with(
        &auth,
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("implicit"), "{err}");
    assert!(err.contains("authorization code"), "{err}");
    assert!(idp.requests().is_empty());
    assert_eq!(prompt.interactions(), 0);
}

#[test]
fn unresolved_variables_stop_before_the_network() {
    let auth = OAuth2 {
        access_token_url: "{{missing}}/token".into(),
        ..client_credentials()
    };
    let idp = FakeIdp::tokens();
    let err = run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap_err();
    assert!(err.contains("missing"), "{err}");
    assert!(idp.requests().is_empty());
}

#[test]
fn extra_parameters_go_where_they_are_placed() {
    let p = |step, name: &str, placement| OAuth2Param {
        step,
        name: name.into(),
        value: format!("{name}-value"),
        placement,
    };
    let auth = OAuth2 {
        parameters: vec![
            p(OAuth2Step::AccessToken, "audience", ParamPlacement::Body),
            p(OAuth2Step::AccessToken, "X-Tenant", ParamPlacement::Header),
            p(OAuth2Step::AccessToken, "realm", ParamPlacement::Query),
            p(OAuth2Step::Refresh, "only-refresh", ParamPlacement::Body),
        ],
        ..client_credentials()
    };
    let idp = FakeIdp::tokens();
    run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap();
    let seen = &idp.requests()[0];
    assert_eq!(seen.field("audience"), Some("audience-value"));
    assert_eq!(seen.header("X-Tenant"), Some("X-Tenant-value"));
    assert_eq!(seen.url, "https://idp.example/token?realm=realm-value");
    assert_eq!(seen.field("only-refresh"), None);
}

// ---------------------------------------------------------------------------------------------
// Token placement

#[test]
fn token_in_the_query_and_custom_prefix() {
    let auth = OAuth2 {
        token: TokenUse {
            placement: TokenPlacement::Query {
                name: "access_token".into(),
            },
            ..TokenUse::default()
        },
        ..client_credentials()
    };
    let idp = FakeIdp::new(|_, _| ok_token("a b+c", None, 60));
    let mut req = request();
    req.url = "https://api.example/items?x=1#frag".into();
    let got = block_on(run(
        &auth,
        &scope(),
        req,
        &idp,
        &services(None, None),
        &FakeClock::new(),
    ))
    .unwrap();
    assert_eq!(
        got.request.url,
        "https://api.example/items?x=1&access_token=a%20b%2Bc#frag"
    );
    assert_eq!(header(&got.request, "Authorization"), None);

    let auth = OAuth2 {
        token: TokenUse {
            placement: TokenPlacement::Header {
                prefix: String::new(),
            },
            ..TokenUse::default()
        },
        ..client_credentials()
    };
    let mut req = request();
    req.headers
        .push(("authorization".into(), "Basic old".into()));
    let got = block_on(run(
        &auth,
        &scope(),
        req,
        &FakeIdp::tokens(),
        &services(None, None),
        &FakeClock::new(),
    ))
    .unwrap();
    assert_eq!(header(&got.request, "Authorization"), Some("tok-0"));
    assert_eq!(
        got.request
            .headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("authorization"))
            .count(),
        1
    );
}

#[test]
fn id_token_as_the_source() {
    let auth = OAuth2 {
        token: TokenUse {
            source: TokenSource::IdToken,
            ..TokenUse::default()
        },
        ..client_credentials()
    };
    let idp = FakeIdp::new(|_, _| {
        (
            200,
            r#"{"access_token":"acc","id_token":"idt","expires_in":"60"}"#.into(),
        )
    });
    let got = run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap();
    assert_eq!(header(&got.request, "Authorization"), Some("Bearer idt"));
    assert!(got.secrets.contains(&"idt".to_owned()));
    assert!(got.secrets.contains(&"acc".to_owned()));

    let idp = FakeIdp::new(|_, _| ok_token("acc", None, 60));
    let err = run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap_err();
    assert!(err.contains("id_token"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// Cache and refresh

#[test]
fn a_valid_cached_token_is_reused() {
    let tokens = MemTokens::default();
    let idp = FakeIdp::tokens();
    let clock = FakeClock::new();
    let s = services(Some(&tokens), None);
    let first = run_with(&client_credentials(), &idp, &s, &clock).unwrap();
    clock.advance(60_000);
    let second = run_with(&client_credentials(), &idp, &s, &clock).unwrap();
    assert_eq!(idp.requests().len(), 1);
    assert_eq!(
        header(&first.request, "Authorization"),
        header(&second.request, "Authorization")
    );
    assert!(second.notes.is_empty(), "{:?}", second.notes);
    assert!(second.secrets.contains(&"tok-0".to_owned()));

    let key = cache_key(&client_credentials(), &scope());
    let stored = tokens.get_now(&key).unwrap();
    assert_eq!(stored.access_token, "tok-0");
    assert_eq!(stored.obtained_at, T0);
    assert_eq!(stored.expires_at, Some(T0 + 300_000));
}

#[test]
fn an_expiring_token_is_refreshed_without_a_prompt() {
    let key = cache_key(&auth_code(Some(PkceMethod::S256)), &scope());
    // Obtained 280 s ago with 300 s lifetime: within the margin, so it counts as expired.
    let tokens = MemTokens::with(
        &key,
        token("old", Some("refresh-1"), T0 - 280_000, Some(T0 + 20_000)),
    );
    let idp = FakeIdp::new(|_, _| ok_token("fresh-access-token", None, 300));
    let prompt = FakePrompt::approving();
    let got = run_with(
        &auth_code(Some(PkceMethod::S256)),
        &idp,
        &services(Some(&tokens), Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap();

    assert_eq!(prompt.interactions(), 0);
    let seen = idp.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].field("grant_type"), Some("refresh_token"));
    assert_eq!(seen[0].field("refresh_token"), Some("refresh-1"));
    assert_eq!(
        header(&got.request, "Authorization"),
        Some("Bearer fresh-access-token")
    );
    assert!(
        got.notes.iter().any(|n| n.text.contains("refreshed")),
        "{:?}",
        got.notes
    );
    assert!(got.secrets.contains(&"refresh-1".to_owned()));
    // The provider sent no new refresh token: the old one stays usable.
    let stored = tokens.get_now(&key).unwrap();
    assert_eq!(stored.access_token, "fresh-access-token");
    assert_eq!(stored.refresh_token.as_deref(), Some("refresh-1"));
    assert_eq!(stored.expires_at, Some(T0 + 300_000));
}

#[test]
fn refresh_goes_to_the_refresh_url_with_its_parameters() {
    let auth = OAuth2 {
        refresh_token_url: "{{idp}}/refresh".into(),
        parameters: vec![OAuth2Param {
            step: OAuth2Step::Refresh,
            name: "resource".into(),
            value: "api".into(),
            placement: ParamPlacement::Body,
        }],
        ..client_credentials()
    };
    let key = cache_key(&auth, &scope());
    let tokens = MemTokens::with(
        &key,
        token("old", Some("r"), T0 - 400_000, Some(T0 - 100_000)),
    );
    let idp = FakeIdp::tokens();
    run_with(
        &auth,
        &idp,
        &services(Some(&tokens), None),
        &FakeClock::new(),
    )
    .unwrap();
    let seen = &idp.requests()[0];
    assert_eq!(seen.url, "https://idp.example/refresh");
    assert_eq!(seen.field("resource"), Some("api"));
    let basic = format!("Basic {}", STANDARD.encode("my-client:s3cr3t-value"));
    assert_eq!(seen.header("Authorization"), Some(basic.as_str()));
}

#[test]
fn a_failed_refresh_fetches_a_new_token() {
    let key = cache_key(&client_credentials(), &scope());
    let tokens = MemTokens::with(&key, token("old", Some("dead"), T0 - 400_000, Some(T0 - 1)));
    let idp = FakeIdp::new(|s, n| match s.field("grant_type") {
        Some("refresh_token") => oauth_error(400, "invalid_grant", "Token is not active"),
        _ => ok_token(&format!("new-{n}"), None, 300),
    });
    let got = run_with(
        &client_credentials(),
        &idp,
        &services(Some(&tokens), None),
        &FakeClock::new(),
    )
    .unwrap();
    assert_eq!(idp.requests().len(), 2);
    assert_eq!(header(&got.request, "Authorization"), Some("Bearer new-1"));
    let warning = got
        .notes
        .iter()
        .find(|n| n.level == NoteLevel::Warning)
        .expect("a warning about the refresh");
    assert!(warning.text.contains("invalid_grant"), "{warning:?}");
    assert_eq!(tokens.get_now(&key).unwrap().access_token, "new-1");
}

#[test]
fn a_failed_refresh_of_an_interactive_flow_prompts_again() {
    let auth = auth_code(Some(PkceMethod::S256));
    let key = cache_key(&auth, &scope());
    let tokens = MemTokens::with(&key, token("old", Some("dead"), T0 - 400_000, Some(T0 - 1)));
    let idp = FakeIdp::new(|s, _| match s.field("grant_type") {
        Some("refresh_token") => oauth_error(400, "invalid_grant", "Session not active"),
        _ => ok_token("after-login", Some("r2"), 300),
    });
    let prompt = FakePrompt::approving();
    let got = run_with(
        &auth,
        &idp,
        &services(Some(&tokens), Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap();
    assert_eq!(prompt.authorize_urls.borrow().len(), 1);
    assert_eq!(
        header(&got.request, "Authorization"),
        Some("Bearer after-login")
    );
}

#[test]
fn without_auto_refresh_an_expired_token_is_fetched_anew() {
    let auth = OAuth2 {
        auto_refresh: false,
        ..client_credentials()
    };
    let key = cache_key(&auth, &scope());
    let tokens = MemTokens::with(&key, token("old", Some("r"), T0 - 400_000, Some(T0 - 1)));
    let idp = FakeIdp::tokens();
    run_with(
        &auth,
        &idp,
        &services(Some(&tokens), None),
        &FakeClock::new(),
    )
    .unwrap();
    assert_eq!(
        idp.requests()[0].field("grant_type"),
        Some("client_credentials")
    );
}

#[test]
fn without_auto_fetch_a_missing_token_fails() {
    let auth = OAuth2 {
        auto_fetch: false,
        ..client_credentials()
    };
    let tokens = MemTokens::default();
    let idp = FakeIdp::tokens();
    let err = run_with(
        &auth,
        &idp,
        &services(Some(&tokens), None),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("auto fetch"), "{err}");
    assert!(idp.requests().is_empty());
    // Without a token store nothing can be cached: the same error.
    let err = run_with(&auth, &idp, &services(None, None), &FakeClock::new()).unwrap_err();
    assert!(err.contains("auto fetch"), "{err}");
    assert!(idp.requests().is_empty());

    // An explicit fetch ("Get token") still works and fills the cache.
    let (key, got) = block_on(fetch_with(
        &auth,
        &scope(),
        &idp,
        &services(Some(&tokens), None),
        &FakeClock::new(),
    ))
    .unwrap();
    assert_eq!(key, cache_key(&auth, &scope()));
    assert_eq!(got.access_token, "tok-0");
    run_with(
        &auth,
        &idp,
        &services(Some(&tokens), None),
        &FakeClock::new(),
    )
    .unwrap();
    assert_eq!(idp.requests().len(), 1);
}

#[test]
fn a_token_without_lifetime_never_expires() {
    let key = cache_key(&client_credentials(), &scope());
    let tokens = MemTokens::with(&key, token("forever", None, T0 - 10_000_000_000, None));
    let idp = FakeIdp::tokens();
    let got = run_with(
        &client_credentials(),
        &idp,
        &services(Some(&tokens), None),
        &FakeClock::new(),
    )
    .unwrap();
    assert!(idp.requests().is_empty());
    assert_eq!(
        header(&got.request, "Authorization"),
        Some("Bearer forever")
    );
}

// ---------------------------------------------------------------------------------------------
// Authorization code

fn verifier_and_challenge(prompt: &FakePrompt, idp: &FakeIdp) -> (String, String) {
    let (url, _) = prompt.authorize_urls.borrow()[0].clone();
    let challenge = query(&url, "code_challenge").unwrap_or_default();
    let verifier = idp.requests()[0]
        .field("code_verifier")
        .unwrap_or_default()
        .to_owned();
    (verifier, challenge)
}

#[test]
fn authorization_code_with_pkce_s256() {
    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::approving();
    let got = run_with(
        &auth_code(Some(PkceMethod::S256)),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap();

    let (url, redirect) = prompt.authorize_urls.borrow()[0].clone();
    assert!(url.starts_with("https://idp.example/authorize?"), "{url}");
    assert_eq!(redirect, "http://127.0.0.1:47900/callback");
    assert_eq!(query(&url, "response_type").as_deref(), Some("code"));
    assert_eq!(query(&url, "client_id").as_deref(), Some("my-client"));
    assert_eq!(
        query(&url, "redirect_uri").as_deref(),
        Some(redirect.as_str())
    );
    assert_eq!(query(&url, "scope").as_deref(), Some("read write"));
    assert_eq!(
        query(&url, "code_challenge_method").as_deref(),
        Some("S256")
    );
    let state = query(&url, "state").unwrap();
    assert!(state.len() >= 16, "{state}");
    assert!(
        !url.contains("s3cr3t"),
        "the secret never goes to the browser"
    );

    let (verifier, challenge) = verifier_and_challenge(&prompt, &idp);
    // RFC 7636: 43..128 characters of [A-Z a-z 0-9 - . _ ~].
    assert!((43..=128).contains(&verifier.len()), "{verifier}");
    assert!(
        verifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
    );
    let expected = URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    assert_eq!(challenge, expected);

    let seen = &idp.requests()[0];
    assert_eq!(seen.field("grant_type"), Some("authorization_code"));
    assert_eq!(seen.field("code"), Some("the-code"));
    assert_eq!(seen.field("redirect_uri"), Some(redirect.as_str()));
    assert_eq!(header(&got.request, "Authorization"), Some("Bearer tok-0"));
}

#[test]
fn pkce_plain_and_disabled() {
    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::approving();
    run_with(
        &auth_code(Some(PkceMethod::Plain)),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap();
    let (url, _) = prompt.authorize_urls.borrow()[0].clone();
    assert_eq!(
        query(&url, "code_challenge_method").as_deref(),
        Some("plain")
    );
    let (verifier, challenge) = verifier_and_challenge(&prompt, &idp);
    assert_eq!(verifier, challenge);
    assert!(verifier.len() >= 43);

    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::approving();
    run_with(
        &auth_code(None),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap();
    let (url, _) = prompt.authorize_urls.borrow()[0].clone();
    assert_eq!(query(&url, "code_challenge"), None);
    assert_eq!(query(&url, "code_challenge_method"), None);
    assert_eq!(idp.requests()[0].field("code_verifier"), None);
}

#[test]
fn every_run_gets_a_new_verifier_and_state() {
    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::approving();
    let clock = FakeClock::new();
    let auth = auth_code(Some(PkceMethod::S256));
    run_with(&auth, &idp, &services(None, Some(&prompt)), &clock).unwrap();
    run_with(&auth, &idp, &services(None, Some(&prompt)), &clock).unwrap();
    let urls = prompt.authorize_urls.borrow();
    assert_ne!(query(&urls[0].0, "state"), query(&urls[1].0, "state"));
    assert_ne!(
        query(&urls[0].0, "code_challenge"),
        query(&urls[1].0, "code_challenge")
    );
}

#[test]
fn a_configured_state_is_sent_and_checked() {
    let mut auth = auth_code(None);
    if let OAuth2Flow::AuthorizationCode { state, .. } = &mut auth.flow {
        *state = "fixed-state".into();
    }
    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::approving();
    run_with(
        &auth,
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap();
    let (url, _) = prompt.authorize_urls.borrow()[0].clone();
    assert_eq!(query(&url, "state").as_deref(), Some("fixed-state"));
}

#[test]
fn a_wrong_state_is_refused() {
    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::new(|url| {
        let redirect = query(url.as_str(), "redirect_uri").unwrap();
        Ok(format!("{redirect}?code=stolen&state=someone-elses"))
    });
    let err = run_with(
        &auth_code(Some(PkceMethod::S256)),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("state"), "{err}");
    assert!(idp.requests().is_empty(), "the code must not be exchanged");

    let prompt = FakePrompt::new(|url| {
        let redirect = query(url.as_str(), "redirect_uri").unwrap();
        Ok(format!("{redirect}?code=no-state"))
    });
    let err = run_with(
        &auth_code(None),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("state"), "{err}");
}

#[test]
fn provider_errors_and_cancelled_sign_in() {
    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::new(|url| {
        let redirect = query(url.as_str(), "redirect_uri").unwrap();
        let state = query(url.as_str(), "state").unwrap();
        Ok(format!(
            "{redirect}?error=access_denied&error_description=User+said+no&state={state}"
        ))
    });
    let err = run_with(
        &auth_code(None),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(
        err.contains("access_denied") && err.contains("User said no"),
        "{err}"
    );

    let prompt = FakePrompt::new(|_| Err("cancelled".into()));
    let err = run_with(
        &auth_code(None),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("cancelled"), "{err}");
    assert!(idp.requests().is_empty());
}

#[test]
fn the_code_exchange_error_is_reported() {
    let idp = FakeIdp::new(|_, _| oauth_error(400, "invalid_grant", "PKCE verification failed"));
    let prompt = FakePrompt::approving();
    let err = run_with(
        &auth_code(Some(PkceMethod::S256)),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(
        err.contains("invalid_grant: PKCE verification failed"),
        "{err}"
    );
}

#[test]
fn interactive_flows_need_a_prompt() {
    let idp = FakeIdp::tokens();
    let err = run_with(
        &auth_code(None),
        &idp,
        &services(None, None),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("browser"), "{err}");
    let err = run_with(
        &device_code(),
        &idp,
        &services(None, None),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("device code"), "{err}");
    assert!(idp.requests().is_empty());

    let auth = OAuth2 {
        flow: OAuth2Flow::AuthorizationCode {
            authorization_url: "https://idp.example/authorize".into(),
            callback_url: String::new(),
            state: String::new(),
            pkce: None,
        },
        ..client_credentials()
    };
    let prompt = FakePrompt::approving();
    let err = run_with(
        &auth,
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("callback URL"), "{err}");
}

#[test]
fn authorization_parameters_go_into_the_authorization_url() {
    let mut auth = auth_code(None);
    auth.parameters = vec![
        OAuth2Param {
            step: OAuth2Step::Authorization,
            name: "prompt".into(),
            value: "login".into(),
            placement: ParamPlacement::Query,
        },
        OAuth2Param {
            step: OAuth2Step::Authorization,
            name: "X-Ignored".into(),
            value: "v".into(),
            placement: ParamPlacement::Header,
        },
    ];
    let idp = FakeIdp::tokens();
    let prompt = FakePrompt::approving();
    let got = run_with(
        &auth,
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap();
    let (url, _) = prompt.authorize_urls.borrow()[0].clone();
    assert_eq!(query(&url, "prompt").as_deref(), Some("login"));
    assert!(
        got.notes
            .iter()
            .any(|n| n.level == NoteLevel::Warning && n.text.contains("X-Ignored")),
        "{:?}",
        got.notes
    );
}

// ---------------------------------------------------------------------------------------------
// Device code

fn device_idp(token_answers: Vec<Answer>) -> FakeIdp {
    let answers = RefCell::new(VecDeque::from(token_answers));
    FakeIdp::new(move |s, _| {
        if s.url.ends_with("/device") {
            return (
                200,
                serde_json::json!({
                    "device_code": "dev-code-1",
                    "user_code": "WDJB-MJHT",
                    "verification_uri": "https://idp.example/activate",
                    "verification_uri_complete": "https://idp.example/activate?user_code=WDJB-MJHT",
                    "expires_in": 600,
                    "interval": 5,
                })
                .to_string(),
            );
        }
        answers
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| oauth_error(400, "authorization_pending", ""))
    })
}

#[test]
fn device_code_polls_with_interval_and_slow_down() {
    let idp = device_idp(vec![
        oauth_error(400, "authorization_pending", "not yet"),
        oauth_error(400, "slow_down", ""),
        oauth_error(400, "authorization_pending", ""),
        ok_token("device-token", Some("device-refresh"), 300),
    ]);
    let prompt = FakePrompt::approving();
    let tokens = MemTokens::default();
    let clock = FakeClock::new();
    let got = run_with(
        &device_code(),
        &idp,
        &services(Some(&tokens), Some(&prompt)),
        &clock,
    )
    .unwrap();

    let seen = idp.requests();
    assert_eq!(seen[0].url, "https://idp.example/device");
    assert_eq!(seen[0].field("client_id"), Some("my-client"));
    assert_eq!(seen[0].field("scope"), Some("read write"));
    assert_eq!(seen.len(), 5);
    for poll in &seen[1..] {
        assert_eq!(poll.url, "https://idp.example/token");
        assert_eq!(
            poll.field("grant_type"),
            Some("urn:ietf:params:oauth:grant-type:device_code")
        );
        assert_eq!(poll.field("device_code"), Some("dev-code-1"));
        assert_eq!(poll.field("client_id"), Some("my-client"));
    }
    // RFC 8628 §3.5: slow_down adds 5 seconds to the interval for all later polls.
    assert_eq!(*clock.sleeps.borrow(), vec![5_000, 5_000, 10_000, 10_000]);

    let shown = prompt.device.borrow();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].user_code, "WDJB-MJHT");
    assert_eq!(shown[0].verification_uri, "https://idp.example/activate");
    assert_eq!(
        shown[0].verification_uri_complete.as_deref(),
        Some("https://idp.example/activate?user_code=WDJB-MJHT")
    );
    assert_eq!(shown[0].expires_in, 600);
    assert_eq!(prompt.done.get(), 1);
    assert_eq!(
        header(&got.request, "Authorization"),
        Some("Bearer device-token")
    );
    assert!(got.secrets.contains(&"dev-code-1".to_owned()));
    assert!(
        tokens
            .get_now(&cache_key(&device_code(), &scope()))
            .is_some()
    );
}

#[test]
fn device_code_errors_end_the_prompt() {
    for (answer, expected) in [
        (oauth_error(400, "access_denied", ""), "denied"),
        (oauth_error(400, "expired_token", ""), "expired"),
        (
            oauth_error(400, "invalid_client", "Unknown client"),
            "invalid_client",
        ),
    ] {
        let idp = device_idp(vec![answer]);
        let prompt = FakePrompt::approving();
        let err = run_with(
            &device_code(),
            &idp,
            &services(None, Some(&prompt)),
            &FakeClock::new(),
        )
        .unwrap_err();
        assert!(err.contains(expected), "{err}");
        assert_eq!(prompt.done.get(), 1, "{expected}");
    }
}

#[test]
fn device_code_stops_when_the_code_expires() {
    let idp = device_idp(vec![]);
    let prompt = FakePrompt::approving();
    let clock = FakeClock::new();
    let err = run_with(&device_code(), &idp, &services(None, Some(&prompt)), &clock).unwrap_err();
    assert!(err.contains("expired"), "{err}");
    assert_eq!(prompt.done.get(), 1);
    // 600 s at 5 s intervals: no poll after the deadline.
    assert!(clock.now_ms() <= T0 + 605_000);
    assert!(idp.requests().len() <= 121);
}

#[test]
fn device_code_can_be_cancelled() {
    let idp = device_idp(vec![]);
    let prompt = FakePrompt::approving();
    prompt.cancel_after_polls.set(Some(2));
    let err = run_with(
        &device_code(),
        &idp,
        &services(None, Some(&prompt)),
        &FakeClock::new(),
    )
    .unwrap_err();
    assert!(err.contains("cancelled"), "{err}");
    assert_eq!(idp.requests().len(), 3, "device request and two polls");
    assert_eq!(prompt.done.get(), 1);
}

#[test]
fn device_code_defaults_and_verification_url_spelling() {
    // Google spells it `verification_url`; `interval` defaults to 5 s.
    let idp = FakeIdp::new(|s, _| {
        if s.url.ends_with("/device") {
            (
                200,
                r#"{"device_code":"d","user_code":"U","verification_url":"https://g.example/device","expires_in":"1800"}"#.into(),
            )
        } else {
            ok_token("t", None, 60)
        }
    });
    let prompt = FakePrompt::approving();
    let clock = FakeClock::new();
    run_with(&device_code(), &idp, &services(None, Some(&prompt)), &clock).unwrap();
    assert_eq!(
        prompt.device.borrow()[0].verification_uri,
        "https://g.example/device"
    );
    assert_eq!(prompt.device.borrow()[0].expires_in, 1800);
    assert_eq!(*clock.sleeps.borrow(), vec![5_000]);
}

// ---------------------------------------------------------------------------------------------
// Cache keys

#[test]
fn cache_keys_identify_provider_client_scope_and_id() {
    let s = scope();
    let base = cache_key(&client_credentials(), &s);
    assert!(base.starts_with("oauth2|"), "{base}");
    assert_eq!(base, cache_key(&client_credentials(), &s), "stable");
    assert!(!base.contains("s3cr3t"), "no secret in the key: {base}");
    assert!(
        base.contains("https://idp.example/token"),
        "resolved: {base}"
    );

    let differs = |auth: OAuth2| assert_ne!(cache_key(&auth, &s), base, "{auth:?}");
    differs(OAuth2 {
        client_id: "other".into(),
        ..client_credentials()
    });
    differs(OAuth2 {
        scope: "read".into(),
        ..client_credentials()
    });
    differs(OAuth2 {
        access_token_url: "https://other.example/token".into(),
        ..client_credentials()
    });
    differs(OAuth2 {
        token: TokenUse {
            id: "shared".into(),
            ..TokenUse::default()
        },
        ..client_credentials()
    });
    differs(auth_code(None));
    // Not part of the key: the secret, placement, auto settings.
    assert_eq!(
        cache_key(
            &OAuth2 {
                client_secret: "rotated".into(),
                auto_refresh: false,
                credentials_placement: CredentialsPlacement::Body,
                ..client_credentials()
            },
            &s
        ),
        base
    );

    // Users of the password flow get their own tokens.
    let user = |name: &str| OAuth2 {
        flow: OAuth2Flow::Password {
            username: name.into(),
            password: "{{userPassword}}".into(),
        },
        ..client_credentials()
    };
    assert_ne!(cache_key(&user("alice"), &s), cache_key(&user("bob"), &s));
    assert!(!cache_key(&user("alice"), &s).contains("pa ss"));

    // Another environment resolving to another provider gets another token.
    let mut other = scope();
    other.layers[0].vars[0].1 = "https://staging.example".into();
    assert_ne!(cache_key(&client_credentials(), &other), base);

    // A separator inside a part cannot make two keys equal.
    let a = OAuth2 {
        client_id: "a|b".into(),
        scope: "c".into(),
        ..client_credentials()
    };
    let b = OAuth2 {
        client_id: "a".into(),
        scope: "b|c".into(),
        ..client_credentials()
    };
    assert_ne!(cache_key(&a, &s), cache_key(&b, &s));
}

#[test]
fn key_labels_for_display() {
    let key = cache_key(&client_credentials(), &scope());
    assert_eq!(key_label(&key), "idp.example · my-client · read write");
    let shared = cache_key(
        &OAuth2 {
            scope: String::new(),
            token: TokenUse {
                id: "team".into(),
                ..TokenUse::default()
            },
            ..client_credentials()
        },
        &scope(),
    );
    assert_eq!(key_label(&shared), "idp.example · my-client · team");
    assert_eq!(key_label("something else"), "something else");
}

#[test]
fn masking_covers_encoded_forms() {
    let secrets = vec!["pa ss&word".to_owned()];
    assert_eq!(mask("x pa ss&word y", &secrets), "x *** y");
    assert_eq!(mask("x=pa+ss%26word", &secrets), "x=***");
    assert_eq!(mask("x=pa%20ss%26word", &secrets), "x=***");
}
