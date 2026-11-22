// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 acceptance against a real identity provider: Keycloak from `testdata/idp`
//! (`testdata/idp/idp.sh start`), realm `requesttailor`, access tokens living 10 s.
//!
//! Runs only when `RT_IDP_URL` is set (e.g. `http://localhost:8180`); otherwise every test
//! prints `SKIPPED` on the terminal and passes. Each test sends a real request
//! (`userinfo`) through `send_with`, so the token is proven valid and the report's masking
//! is checked on the way.
//!
//! - Client credentials, password: no prompt.
//! - Authorization code with PKCE (S256 enforced by the client, and plain): a test
//!   `AuthPrompt` fills Keycloak's login form over HTTP and returns the redirect URL.
//! - Device code: the test prompt confirms the user code in Keycloak's pages over HTTP.
//! - Refresh: the token expires, the next send refreshes it without the prompt (counted);
//!   a broken refresh token falls back to a new sign-in through the prompt.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write as _;

use rt_core::model::{
    Auth, Collection, Environment, Item, OAuth2, OAuth2Flow, PkceMethod, RequestDefaults,
    RequestEntry, RequestSpec, Variable,
};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::send::{SendOptions, SendReport, send_with};
use rt_core::services::{
    AuthPrompt, DeviceCodePrompt, LocalBoxFuture, Services, StoredToken, TokenStore,
};
use rt_core::transport::native::NativeTransport;
use rt_core::transport::{HttpBody, HttpRequest, Response, Transport, TransportError, collect};

const REALM: &str = "/realms/requesttailor";
const USER: &str = "alice";
const PASSWORD: &str = "alice-password";
const CALLBACK: &str = "http://127.0.0.1:47901/callback";

/// The IdP's base URL, or `None` after printing why the test is skipped. Written to the
/// terminal directly (not through the captured `eprintln!`), so the skip is visible.
fn idp(test: &str) -> Option<String> {
    match std::env::var("RT_IDP_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url.trim().trim_end_matches('/').to_owned()),
        _ => {
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED oauth2_idp::{test}: RT_IDP_URL is not set (testdata/idp/idp.sh start)"
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Services for the tests

#[derive(Default)]
struct MemTokens(RefCell<HashMap<String, StoredToken>>);

impl MemTokens {
    fn only(&self) -> (String, StoredToken) {
        let map = self.0.borrow();
        assert_eq!(map.len(), 1, "exactly one cached token: {:?}", map.keys());
        let (k, v) = map.iter().next().unwrap();
        (k.clone(), v.clone())
    }
    fn set(&self, key: &str, token: StoredToken) {
        self.0.borrow_mut().insert(key.to_owned(), token);
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

/// Counts requests to the token endpoint.
struct Counting {
    inner: NativeTransport,
    token_requests: Cell<usize>,
}

impl Counting {
    fn new() -> Self {
        Self {
            inner: NativeTransport::new().expect("native transport"),
            token_requests: Cell::new(0),
        }
    }
}

impl Transport for Counting {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        if request.url.contains("/protocol/openid-connect/token") {
            self.token_requests.set(self.token_requests.get() + 1);
        }
        self.inner.send(request).await
    }
}

/// A minimal browser over the native transport: cookies, manual redirects, form posts. Just
/// enough for Keycloak's login, consent and device pages.
struct Browser {
    transport: NativeTransport,
    cookies: RefCell<Vec<(String, String)>>,
}

struct Page {
    url: String,
    status: u16,
    location: Option<String>,
    body: String,
}

impl Browser {
    fn new() -> Self {
        Self {
            transport: NativeTransport::new().expect("native transport"),
            cookies: RefCell::new(Vec::new()),
        }
    }

    async fn request(&self, url: &str, form: Option<&[(String, String)]>) -> Page {
        let mut headers = vec![("Accept".to_owned(), "text/html".to_owned())];
        let cookie = self
            .cookies
            .borrow()
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        if !cookie.is_empty() {
            headers.push(("Cookie".to_owned(), cookie));
        }
        let body = form.map(|f| HttpBody {
            content_type: Some("application/x-www-form-urlencoded".to_owned()),
            bytes: url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(f.iter())
                .finish()
                .into(),
        });
        let request = HttpRequest {
            method: if form.is_some() { "POST" } else { "GET" }.to_owned(),
            url: url.to_owned(),
            headers,
            body,
            options: rt_core::transport::SendOptions {
                follow_redirects: false,
                timeout: Some(std::time::Duration::from_secs(30)),
                ..Default::default()
            },
        };
        let response = self.transport.send(request).await.expect("IdP reachable");
        let done = collect(response, 4 << 20, web_time::Instant::now())
            .await
            .expect("IdP answers");
        for (name, value) in &done.head.headers {
            if !name.eq_ignore_ascii_case("set-cookie") {
                continue;
            }
            let pair = value.split(';').next().unwrap_or_default();
            if let Some((n, v)) = pair.split_once('=') {
                let (n, v) = (n.trim().to_owned(), v.trim().to_owned());
                let mut jar = self.cookies.borrow_mut();
                jar.retain(|(k, _)| *k != n);
                if !v.is_empty() && !value.to_ascii_lowercase().contains("max-age=0") {
                    jar.push((n, v));
                }
            }
        }
        let location = done
            .head
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("location"))
            .map(|(_, v)| join(url, v));
        Page {
            url: url.to_owned(),
            status: done.head.status,
            location,
            body: String::from_utf8_lossy(&done.body).into_owned(),
        }
    }

    /// Opens `url` and acts like a user: follows redirects, signs in, grants consent. Stops
    /// at a redirect to `stop` (returned) or at a page with nothing left to do (`None`).
    async fn walk(&self, url: &str, stop: Option<&str>) -> Option<String> {
        let mut page = self.request(url, None).await;
        for _ in 0..20 {
            if let Some(location) = page.location.clone() {
                if stop.is_some_and(|s| location.starts_with(s)) {
                    return Some(location);
                }
                page = self.request(&location, None).await;
                continue;
            }
            assert_eq!(
                page.status, 200,
                "unexpected page {}: {}",
                page.url, page.body
            );
            let form = if page.body.contains("id=\"kc-form-login\"") {
                Some(vec![
                    ("username".to_owned(), USER.to_owned()),
                    ("password".to_owned(), PASSWORD.to_owned()),
                    ("credentialId".to_owned(), String::new()),
                ])
            } else if page.body.contains("name=\"accept\"") {
                Some(vec![("accept".to_owned(), "Yes".to_owned())])
            } else {
                None
            };
            let Some(mut fields) = form else {
                return None;
            };
            let action = form_action(&page.body).expect("form with action");
            fields.extend(hidden_inputs(&page.body));
            page = self.request(&join(&page.url, &action), Some(&fields)).await;
        }
        panic!("too many steps at {}", page.url);
    }
}

fn join(base: &str, reference: &str) -> String {
    url::Url::parse(base)
        .and_then(|b| b.join(reference))
        .map(|u| u.to_string())
        .unwrap_or_else(|_| reference.to_owned())
}

fn unescape_html(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#61;", "=")
        .replace("&#x3D;", "=")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[start..].find('"')? + start;
    Some(unescape_html(&tag[start..end]))
}

fn form_action(html: &str) -> Option<String> {
    let start = html.find("<form")?;
    let tag = &html[start..start + html[start..].find('>')?];
    attribute(tag, "action")
}

fn hidden_inputs(html: &str) -> Vec<(String, String)> {
    html.match_indices("<input")
        .filter_map(|(i, _)| {
            let tag = &html[i..i + html[i..].find('>')?];
            (attribute(tag, "type").as_deref() == Some("hidden")).then_some(())?;
            Some((
                attribute(tag, "name")?,
                attribute(tag, "value").unwrap_or_default(),
            ))
        })
        .collect()
}

/// Signs in through Keycloak's pages; counts how often it was asked.
struct FormPrompt {
    browser: Browser,
    authorize_calls: Cell<usize>,
    device_calls: Cell<usize>,
    device_done: Cell<usize>,
}

impl FormPrompt {
    fn new() -> Self {
        Self {
            browser: Browser::new(),
            authorize_calls: Cell::new(0),
            device_calls: Cell::new(0),
            device_done: Cell::new(0),
        }
    }
}

impl AuthPrompt for FormPrompt {
    fn authorize<'a>(
        &'a self,
        url: &'a str,
        redirect_uri: &'a str,
    ) -> LocalBoxFuture<'a, Result<String, String>> {
        self.authorize_calls.set(self.authorize_calls.get() + 1);
        Box::pin(async move {
            self.browser
                .walk(url, Some(redirect_uri))
                .await
                .ok_or_else(|| "the login did not redirect back".to_owned())
        })
    }

    /// Confirms the code right away (a user on another device would do it meanwhile).
    fn device_code<'a>(&'a self, prompt: &'a DeviceCodePrompt) -> LocalBoxFuture<'a, ()> {
        self.device_calls.set(self.device_calls.get() + 1);
        Box::pin(async move {
            let url = prompt
                .verification_uri_complete
                .clone()
                .expect("Keycloak sends verification_uri_complete");
            assert!(url.contains(&prompt.user_code));
            let end = self.browser.walk(&url, None).await;
            assert!(end.is_none());
        })
    }

    fn device_code_done<'a>(&'a self) -> LocalBoxFuture<'a, ()> {
        self.device_done.set(self.device_done.get() + 1);
        Box::pin(std::future::ready(()))
    }
}

// ---------------------------------------------------------------------------------------------
// The collection: one request to `userinfo`, the OAuth 2.0 settings as the collection default
// (device code is only allowed there), templates resolved from the environment.

struct NoFiles;

impl FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("no file {path}"))
    }
}

fn oauth(flow: OAuth2Flow, client: &str, secret: &str) -> OAuth2 {
    OAuth2 {
        flow,
        access_token_url: format!("{{{{idp}}}}{REALM}/protocol/openid-connect/token"),
        client_id: client.to_owned(),
        client_secret: secret.to_owned(),
        scope: "openid".to_owned(),
        ..OAuth2::default()
    }
}

fn collection(idp: &str, auth: OAuth2) -> Collection {
    let secret = |name: &str| Variable {
        name: name.to_owned(),
        value: None,
        enabled: true,
        secret: true,
    };
    Collection {
        name: "idp".into(),
        defaults: RequestDefaults {
            auth: Auth::OAuth2(Box::new(auth)),
            ..Default::default()
        },
        items: vec![Item::Request(RequestEntry {
            path: "userinfo.yml".into(),
            spec: RequestSpec {
                name: "userinfo".into(),
                method: "GET".into(),
                url: format!("{{{{idp}}}}{REALM}/protocol/openid-connect/userinfo"),
                auth: Auth::Inherit,
                ..Default::default()
            },
        })],
        environments: vec![Environment {
            name: "Local".into(),
            variables: vec![
                Variable::new("idp", idp),
                secret("clientSecret"),
                secret("userPassword"),
            ],
            client_certificates: Vec::new(),
            external_secrets: None,
        }],
        config: Default::default(),
    }
}

/// Secret values as the shells pass them (keychain → runtime layer of a secret variable).
const SECRETS: &[(&str, &str)] = &[
    ("clientSecret", "rt-client-credentials-secret"),
    ("userPassword", PASSWORD),
];

async fn send(
    collection: &Collection,
    transport: &Counting,
    services: &Services<'_>,
) -> SendReport {
    let input = PrepareInput {
        environment: Some("Local"),
        globals: Vec::new(),
        runtime: SECRETS
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
    };
    send_with(
        collection,
        "userinfo.yml",
        &input,
        &NoFiles,
        transport,
        &SendOptions::default(),
        services,
    )
    .await
}

/// The send worked with a valid token, and the report shows none of the secrets.
fn assert_ok(report: &SendReport, tokens: &MemTokens) {
    assert_eq!(report.error, None, "{report:#?}");
    let response = report.response.as_ref().expect("a response");
    assert_eq!(response.status, 200, "{report:#?}");
    let request = report.request.as_ref().expect("the sent request");
    let auth = request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("authorization"))
        .map(|(_, v)| v.as_str());
    assert_eq!(auth, Some("Bearer ***"), "token masked in the report");
    let json = serde_json::to_string(report).unwrap();
    let (_, token) = tokens.only();
    for value in [
        Some(token.access_token.as_str()),
        token.refresh_token.as_deref(),
        token.id_token.as_deref(),
        Some("rt-client-credentials-secret"),
        Some(PASSWORD),
    ]
    .into_iter()
    .flatten()
    {
        assert!(!json.contains(value), "secret value in the report: {value}");
    }
}

fn notes(report: &SendReport) -> Vec<String> {
    report
        .notes
        .iter()
        .filter(|n| n.source == "auth")
        .map(|n| n.text.clone())
        .collect()
}

/// Waits until `token` counts as expiring (a quarter of its lifetime before expiry).
async fn until_expiring(token: &StoredToken) {
    let expires_at = token.expires_at.expect("Keycloak sends expires_in");
    let margin = ((expires_at - token.obtained_at) / 4).min(30_000);
    let now = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let wait = (expires_at - margin - now + 300).max(0) as u64;
    tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
}

// ---------------------------------------------------------------------------------------------
// The flows

#[tokio::test]
async fn client_credentials_fetches_once_then_uses_the_cache() {
    let Some(idp) = idp("client_credentials_fetches_once_then_uses_the_cache") else {
        return;
    };
    let c = collection(
        &idp,
        oauth(
            OAuth2Flow::ClientCredentials,
            "rt-client-credentials",
            "{{clientSecret}}",
        ),
    );
    let tokens = MemTokens::default();
    let transport = Counting::new();
    let services = Services {
        tokens: Some(&tokens),
        ..Services::none()
    };
    let first = send(&c, &transport, &services).await;
    assert_ok(&first, &tokens);
    assert!(
        notes(&first).iter().any(|n| n.contains("new token")),
        "{:?}",
        first.notes
    );
    let second = send(&c, &transport, &services).await;
    assert_ok(&second, &tokens);
    assert_eq!(
        transport.token_requests.get(),
        1,
        "the cached token is reused"
    );
    assert!(notes(&second).is_empty(), "{:?}", second.notes);

    // No refresh token for client credentials: after expiry a new one is fetched.
    let (_, token) = tokens.only();
    assert_eq!(token.refresh_token, None);
    until_expiring(&token).await;
    let third = send(&c, &transport, &services).await;
    assert_ok(&third, &tokens);
    assert_eq!(transport.token_requests.get(), 2);
    assert_ne!(tokens.only().1.access_token, token.access_token);
}

#[tokio::test]
async fn password_grant() {
    let Some(idp) = idp("password_grant") else {
        return;
    };
    let c = collection(
        &idp,
        oauth(
            OAuth2Flow::Password {
                username: USER.into(),
                password: "{{userPassword}}".into(),
            },
            "rt-password",
            "rt-password-secret",
        ),
    );
    let tokens = MemTokens::default();
    let transport = Counting::new();
    let services = Services {
        tokens: Some(&tokens),
        ..Services::none()
    };
    let report = send(&c, &transport, &services).await;
    assert_ok(&report, &tokens);
    let body = match &report.response.as_ref().unwrap().body {
        rt_core::send::ReportBody::Utf8(s) => s.clone(),
        other => panic!("{other:?}"),
    };
    assert!(body.contains("\"preferred_username\":\"alice\""), "{body}");
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("rt-password-secret")
    );

    // A wrong password: the provider's error, masked.
    let mut wrong = c.clone();
    if let Auth::OAuth2(o) = &mut wrong.defaults.auth {
        o.flow = OAuth2Flow::Password {
            username: USER.into(),
            password: "not-the-password".into(),
        };
    }
    let report = send(&wrong, &transport, &Services::none()).await;
    let error = serde_json::to_string(&report.error).unwrap();
    assert!(error.contains("invalid_grant"), "{error}");
    assert!(!error.contains("not-the-password"), "{error}");
    assert!(!error.contains("rt-password-secret"), "{error}");
}

async fn authorization_code(client: &str, pkce: Option<PkceMethod>) {
    let Some(idp) = idp(&format!("authorization_code ({client})")) else {
        return;
    };
    let c = collection(
        &idp,
        oauth(
            OAuth2Flow::AuthorizationCode {
                authorization_url: format!("{{{{idp}}}}{REALM}/protocol/openid-connect/auth"),
                callback_url: CALLBACK.into(),
                state: String::new(),
                pkce,
            },
            client,
            "",
        ),
    );
    let tokens = MemTokens::default();
    let prompt = FormPrompt::new();
    let transport = Counting::new();
    let services = Services {
        tokens: Some(&tokens),
        prompt: Some(&prompt),
        ..Services::none()
    };
    let report = send(&c, &transport, &services).await;
    assert_ok(&report, &tokens);
    assert_eq!(prompt.authorize_calls.get(), 1);
    let (_, token) = tokens.only();
    assert!(
        token.refresh_token.is_some(),
        "Keycloak issues a refresh token"
    );
    assert!(token.id_token.is_some(), "scope openid gives an ID token");
}

#[tokio::test]
async fn authorization_code_with_pkce_s256() {
    // `rt-auth-code-s256` enforces S256: Keycloak refuses the code without the verifier.
    authorization_code("rt-auth-code-s256", Some(PkceMethod::S256)).await;
}

#[tokio::test]
async fn authorization_code_with_pkce_plain() {
    authorization_code("rt-auth-code", Some(PkceMethod::Plain)).await;
}

#[tokio::test]
async fn pkce_is_required_where_the_client_enforces_it() {
    let Some(idp) = idp("pkce_is_required_where_the_client_enforces_it") else {
        return;
    };
    let c = collection(
        &idp,
        oauth(
            OAuth2Flow::AuthorizationCode {
                authorization_url: format!("{{{{idp}}}}{REALM}/protocol/openid-connect/auth"),
                callback_url: CALLBACK.into(),
                state: String::new(),
                pkce: None,
            },
            "rt-auth-code-s256",
            "",
        ),
    );
    let prompt = FormPrompt::new();
    let transport = Counting::new();
    let services = Services {
        prompt: Some(&prompt),
        ..Services::none()
    };
    let report = send(&c, &transport, &services).await;
    let error = serde_json::to_string(&report.error).unwrap();
    assert!(error.contains("code_challenge"), "{error}");
}

#[tokio::test]
async fn device_code() {
    let Some(idp) = idp("device_code") else {
        return;
    };
    let mut auth = oauth(
        OAuth2Flow::DeviceCode {
            device_authorization_url: format!(
                "{{{{idp}}}}{REALM}/protocol/openid-connect/auth/device"
            ),
        },
        "rt-device",
        "",
    );
    auth.scope = "openid".into();
    let c = collection(&idp, auth);
    let tokens = MemTokens::default();
    let prompt = FormPrompt::new();
    let transport = Counting::new();
    let services = Services {
        tokens: Some(&tokens),
        prompt: Some(&prompt),
        ..Services::none()
    };
    let report = send(&c, &transport, &services).await;
    assert_ok(&report, &tokens);
    assert_eq!(prompt.device_calls.get(), 1);
    assert_eq!(prompt.device_done.get(), 1);
    assert!(
        notes(&report).iter().any(|n| n.contains("device code")),
        "{:?}",
        report.notes
    );
}

#[tokio::test]
async fn tokens_refresh_without_the_prompt() {
    let Some(idp) = idp("tokens_refresh_without_the_prompt") else {
        return;
    };
    let c = collection(
        &idp,
        oauth(
            OAuth2Flow::AuthorizationCode {
                authorization_url: format!("{{{{idp}}}}{REALM}/protocol/openid-connect/auth"),
                callback_url: CALLBACK.into(),
                state: String::new(),
                pkce: Some(PkceMethod::S256),
            },
            "rt-auth-code",
            "",
        ),
    );
    let tokens = MemTokens::default();
    let prompt = FormPrompt::new();
    let transport = Counting::new();
    let services = Services {
        tokens: Some(&tokens),
        prompt: Some(&prompt),
        ..Services::none()
    };
    assert_ok(&send(&c, &transport, &services).await, &tokens);
    assert_eq!(prompt.authorize_calls.get(), 1);
    let (key, first) = tokens.only();

    // Valid: reused, no network for the token.
    assert_ok(&send(&c, &transport, &services).await, &tokens);
    assert_eq!(transport.token_requests.get(), 1);

    // About to expire: refreshed on the next send, without asking the user.
    until_expiring(&first).await;
    let report = send(&c, &transport, &services).await;
    assert_ok(&report, &tokens);
    assert_eq!(prompt.authorize_calls.get(), 1, "no prompt for a refresh");
    assert_eq!(transport.token_requests.get(), 2);
    assert!(
        notes(&report).iter().any(|n| n.contains("refreshed")),
        "{:?}",
        report.notes
    );
    let (_, second) = tokens.only();
    assert_ne!(second.access_token, first.access_token);

    // A refresh token the provider no longer accepts: the next send signs in again.
    tokens.set(
        &key,
        StoredToken {
            refresh_token: Some("revoked-refresh-token".into()),
            expires_at: Some(second.obtained_at),
            ..second.clone()
        },
    );
    let report = send(&c, &transport, &services).await;
    assert_ok(&report, &tokens);
    assert_eq!(
        prompt.authorize_calls.get(),
        2,
        "signed in again after the failed refresh"
    );
    let warning = notes(&report).join("\n");
    assert!(warning.contains("refreshing the token failed"), "{warning}");
}
