// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 for `send_with` (wave 5, strand C): client credentials, password, authorization
//! code with PKCE, device code (RFC 8628), SAML 2.0 bearer (RFC 7522) and refresh. Tokens
//! are cached through `Services::tokens` and refreshed before they expire; the token
//! endpoint is called through the same `Transport` as the request itself, so the flow works
//! identically natively, in the browser and over the companion.
//!
//! # Token lifecycle
//!
//! 1. A cached token (key: [`cache_key`]) that is not about to expire is used as is.
//! 2. A token that expires within the margin (a quarter of its lifetime, at most 30 s) is
//!    refreshed with its refresh token when `auto_refresh` is on: no prompt, no browser.
//! 3. Without a refresh token, or when the refresh fails (e.g. `invalid_grant` after the
//!    provider's session ended), a new token is fetched when `auto_fetch` is on; interactive
//!    flows then go through `Services::prompt`. Without a token store every send fetches.
//!
//! # Secrets
//!
//! Token values, refresh tokens, device codes, client secrets, passwords and assertions are
//! returned in [`Authorized::secrets`] for masking, and masked in every error message this
//! module returns (providers like to echo what they got). They are only ever written to the
//! shell's `TokenStore`.
//!
//! The implicit flow is read and written in files but not executed (OAuth 2.1 drops it).

use crate::model::{
    CredentialsPlacement, OAuth2, OAuth2Flow, OAuth2Step, ParamPlacement, PkceMethod,
    TokenPlacement, TokenSource,
};
use crate::services::{AuthPrompt, LocalBoxFuture, Note, NoteLevel, Services, StoredToken};
use crate::transport::{HttpRequest, Transport};
use crate::vars::Scope;

mod endpoint;
mod pkce;
#[cfg(test)]
mod tests;

use endpoint::{EndpointError, Form};

/// The request with the token applied.
#[derive(Debug)]
pub struct Authorized {
    pub request: HttpRequest,
    /// Token values (and resolved client secrets) to mask in the report.
    pub secrets: Vec<String>,
    /// E.g. "token refreshed", "new token fetched".
    pub notes: Vec<Note>,
}

/// Gets a token for `auth` (from the cache, by refreshing, or by running the flow) and puts
/// it on `request` as `auth.token` says. Templates in `auth` are resolved against `scope`.
/// `Err` is a message for the report; it is masked before it gets there.
pub async fn authorize<T: Transport>(
    auth: &OAuth2,
    scope: &Scope,
    request: HttpRequest,
    transport: &T,
    services: &Services<'_>,
) -> Result<Authorized, String> {
    run(auth, scope, request, transport, services, &SystemClock).await
}

/// Runs the flow of `auth` now, whatever is cached ("Get token" in the UI), and stores the
/// new token under its [`cache_key`], which is returned with it. Interactive flows prompt.
pub async fn fetch_token<T: Transport>(
    auth: &OAuth2,
    scope: &Scope,
    transport: &T,
    services: &Services<'_>,
) -> Result<(String, StoredToken), String> {
    fetch_with(auth, scope, transport, services, &SystemClock).await
}

/// The key under which the token for `auth` is cached (resolved against `scope`): stable
/// for the same provider, client, scope and `token.id`, different otherwise. Contains no
/// secret value.
///
/// Layout: `oauth2|1|<flow>|<endpoint>|<client id>|<scope>|<token id>|<extra>`, every part
/// with `%` and `|` percent-encoded. `<endpoint>` is the token URL; `<extra>` is the user
/// name for the password flow and a hash of the assertion for SAML 2.0 bearer (different
/// users and assertions must not share a token). [`key_label`] makes it readable.
pub fn cache_key(auth: &OAuth2, scope: &Scope) -> String {
    key_of(&resolve(auth, scope))
}

/// A short description of a cache key for lists: token endpoint host, client id, scope (or
/// token id). Anything that is not a cache key is returned as is.
pub fn key_label(key: &str) -> String {
    let parts: Vec<String> = key.split('|').map(unescape).collect();
    let [kind, version, _flow, endpoint, client, scope, id, extra] = &parts[..] else {
        return key.to_owned();
    };
    if kind != "oauth2" || version != "1" {
        return key.to_owned();
    }
    let host = url::Url::parse(endpoint)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| endpoint.clone());
    let mut label = vec![host, client.clone()];
    for part in [scope, id] {
        if !part.is_empty() {
            label.push(part.clone());
        }
    }
    if let Some(user) = extra.strip_prefix("user=") {
        label.push(user.to_owned());
    }
    label.retain(|p| !p.is_empty());
    label.join(" · ")
}

// ---------------------------------------------------------------------------------------------
// Time and randomness, replaceable in tests

/// What the flows need from the platform. Tests use a virtual clock, so device code polling
/// and token expiry run without waiting.
pub(crate) trait Clock {
    /// Unix milliseconds.
    fn now_ms(&self) -> i64;
    fn sleep(&self, ms: u64) -> LocalBoxFuture<'_, ()>;
    fn fill_random(&self, buf: &mut [u8]) -> Result<(), String>;
}

struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sleep(&self, ms: u64) -> LocalBoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(std::time::Duration::from_millis(ms)))
    }

    #[cfg(target_arch = "wasm32")]
    fn sleep(&self, ms: u64) -> LocalBoxFuture<'_, ()> {
        use wasm_bindgen::{JsCast, JsValue};
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            let global = js_sys::global();
            let set_timeout = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
                .ok()
                .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
            match set_timeout {
                Some(f) => {
                    let _ = f.call2(&global, &resolve, &JsValue::from_f64(ms as f64));
                }
                None => {
                    let _ = resolve.call0(&JsValue::NULL);
                }
            }
        });
        Box::pin(async move {
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        })
    }

    fn fill_random(&self, buf: &mut [u8]) -> Result<(), String> {
        getrandom::getrandom(buf).map_err(|e| format!("no random source: {e}"))
    }
}

// ---------------------------------------------------------------------------------------------
// Settings with templates resolved

/// `auth` with every template resolved.
#[derive(Debug, Clone)]
struct Resolved {
    flow: Flow,
    token_url: String,
    refresh_url: String,
    client_id: String,
    client_secret: String,
    placement: CredentialsPlacement,
    scope: String,
    params: Vec<Param>,
    token_id: String,
    token_placement: TokenPlacement,
    token_source: TokenSource,
    auto_fetch: bool,
    auto_refresh: bool,
    /// Placeholders without a value, in any field.
    unresolved: Vec<String>,
}

#[derive(Debug, Clone)]
enum Flow {
    ClientCredentials,
    Password {
        username: String,
        password: String,
    },
    AuthorizationCode {
        authorization_url: String,
        callback_url: String,
        state: String,
        pkce: Option<PkceMethod>,
    },
    Implicit,
    DeviceCode {
        device_authorization_url: String,
    },
    Saml2Bearer {
        assertion: String,
    },
}

#[derive(Debug, Clone)]
struct Param {
    step: OAuth2Step,
    name: String,
    value: String,
    placement: ParamPlacement,
}

impl Flow {
    fn name(&self) -> &'static str {
        match self {
            Flow::ClientCredentials => "client-credentials",
            Flow::Password { .. } => "password",
            Flow::AuthorizationCode { .. } => "authorization-code",
            Flow::Implicit => "implicit",
            Flow::DeviceCode { .. } => "device-code",
            Flow::Saml2Bearer { .. } => "saml2-bearer",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Flow::ClientCredentials => "client credentials",
            Flow::Password { .. } => "password",
            Flow::AuthorizationCode { .. } => "authorization code",
            Flow::Implicit => "implicit",
            Flow::DeviceCode { .. } => "device code",
            Flow::Saml2Bearer { .. } => "SAML 2.0 bearer",
        }
    }
}

fn resolve(auth: &OAuth2, scope: &Scope) -> Resolved {
    let mut unresolved: Vec<String> = Vec::new();
    let mut r = |template: &str| {
        let i = scope.interpolate(template);
        unresolved.extend(i.unresolved);
        i.value
    };
    let flow = match &auth.flow {
        OAuth2Flow::ClientCredentials => Flow::ClientCredentials,
        OAuth2Flow::Password { username, password } => Flow::Password {
            username: r(username),
            password: r(password),
        },
        OAuth2Flow::AuthorizationCode {
            authorization_url,
            callback_url,
            state,
            pkce,
        } => Flow::AuthorizationCode {
            authorization_url: r(authorization_url),
            callback_url: r(callback_url),
            state: r(state),
            pkce: *pkce,
        },
        OAuth2Flow::Implicit { .. } => Flow::Implicit,
        OAuth2Flow::DeviceCode {
            device_authorization_url,
        } => Flow::DeviceCode {
            device_authorization_url: r(device_authorization_url),
        },
        OAuth2Flow::Saml2Bearer { assertion } => Flow::Saml2Bearer {
            assertion: r(assertion),
        },
    };
    let token_url = r(&auth.access_token_url);
    let refresh_url = r(&auth.refresh_token_url);
    let client_id = r(&auth.client_id);
    let client_secret = r(&auth.client_secret);
    let scope_value = r(&auth.scope);
    let params = auth
        .parameters
        .iter()
        .map(|p| Param {
            step: p.step,
            name: r(&p.name),
            value: r(&p.value),
            placement: p.placement,
        })
        .collect();
    let token_id = r(&auth.token.id);
    let token_placement = match &auth.token.placement {
        TokenPlacement::Header { prefix } => TokenPlacement::Header { prefix: r(prefix) },
        TokenPlacement::Query { name } => TokenPlacement::Query { name: r(name) },
    };
    unresolved.sort();
    unresolved.dedup();
    Resolved {
        flow,
        token_url,
        refresh_url,
        client_id,
        client_secret,
        placement: auth.credentials_placement,
        scope: scope_value,
        params,
        token_id,
        token_placement,
        token_source: auth.token.source,
        auto_fetch: auth.auto_fetch,
        auto_refresh: auth.auto_refresh,
        unresolved,
    }
}

impl Resolved {
    /// Values of the settings that must never show (masked in errors and the report).
    fn secrets(&self) -> Vec<String> {
        let mut v = vec![self.client_secret.clone()];
        match &self.flow {
            Flow::Password { password, .. } => v.push(password.clone()),
            Flow::Saml2Bearer { assertion } => v.push(assertion.clone()),
            _ => {}
        }
        v
    }

    fn params(&self, step: OAuth2Step) -> impl Iterator<Item = &Param> {
        self.params.iter().filter(move |p| p.step == step)
    }
}

fn escape(part: &str) -> String {
    part.replace('%', "%25").replace('|', "%7C")
}

fn unescape(part: &str) -> String {
    part.replace("%7C", "|").replace("%25", "%")
}

fn key_of(r: &Resolved) -> String {
    let endpoint = match &r.flow {
        Flow::Implicit => String::new(),
        _ => r.token_url.clone(),
    };
    let extra = match &r.flow {
        Flow::Password { username, .. } => format!("user={username}"),
        Flow::Saml2Bearer { assertion } => {
            use sha2::Digest as _;
            let hash = sha2::Sha256::digest(assertion.as_bytes());
            let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
            format!("assertion={hex}")
        }
        _ => String::new(),
    };
    let parts = [
        "oauth2",
        "1",
        r.flow.name(),
        &endpoint,
        &r.client_id,
        &r.scope,
        &r.token_id,
        &extra,
    ];
    parts
        .iter()
        .map(|p| escape(p))
        .collect::<Vec<_>>()
        .join("|")
}

// ---------------------------------------------------------------------------------------------
// The lifecycle

/// Collects what a run adds: values to mask and notes.
struct Run {
    secrets: Vec<String>,
    notes: Vec<Note>,
}

impl Run {
    fn note(&mut self, level: NoteLevel, text: impl Into<String>) {
        self.notes.push(Note {
            source: "auth".to_owned(),
            level,
            text: text.into(),
        });
    }

    fn token_secrets(&mut self, t: &StoredToken) {
        self.secrets.push(t.access_token.clone());
        self.secrets.extend(t.refresh_token.clone());
        self.secrets.extend(t.id_token.clone());
    }

    fn sorted_secrets(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .secrets
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();
        v.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        v.dedup();
        v
    }
}

pub(crate) async fn run<T: Transport>(
    auth: &OAuth2,
    scope: &Scope,
    mut request: HttpRequest,
    transport: &T,
    services: &Services<'_>,
    clock: &dyn Clock,
) -> Result<Authorized, String> {
    let r = resolve(auth, scope);
    let mut run = Run {
        secrets: r.secrets(),
        notes: Vec::new(),
    };
    let result = obtain(&r, transport, services, clock, &mut run).await;
    let secrets = run.sorted_secrets();
    let token = result.map_err(|e| mask(&format!("OAuth 2.0: {e}"), &secrets))?;
    let value = match r.token_source {
        TokenSource::AccessToken => token.access_token.clone(),
        TokenSource::IdToken => token.id_token.clone().ok_or_else(|| {
            "OAuth 2.0: the token response has no id_token, but the token source is the ID \
             token (request the `openid` scope or use the access token)"
                .to_owned()
        })?,
    };
    apply(&mut request, &r.token_placement, &value);
    let mut notes = run.notes;
    for n in &mut notes {
        n.text = mask(&n.text, &secrets);
    }
    Ok(Authorized {
        request,
        secrets,
        notes,
    })
}

pub(crate) async fn fetch_with<T: Transport>(
    auth: &OAuth2,
    scope: &Scope,
    transport: &T,
    services: &Services<'_>,
    clock: &dyn Clock,
) -> Result<(String, StoredToken), String> {
    let r = resolve(auth, scope);
    let mut run = Run {
        secrets: r.secrets(),
        notes: Vec::new(),
    };
    let key = key_of(&r);
    let result = async {
        check(&r)?;
        let token = fetch(&r, transport, services.prompt, clock, &mut run).await?;
        if let Some(store) = services.tokens {
            store.put(&key, &token).await;
        }
        Ok::<_, String>(token)
    }
    .await;
    let secrets = run.sorted_secrets();
    result
        .map(|t| (key, t))
        .map_err(|e| mask(&format!("OAuth 2.0: {e}"), &secrets))
}

/// Refuses what cannot run at all, before any network or prompt.
fn check(r: &Resolved) -> Result<(), String> {
    if let Flow::Implicit = r.flow {
        return Err(
            "the implicit flow is not supported (OAuth 2.1 removes it, the token \
                    would travel in the URL); switch this request to the authorization code \
                    flow with PKCE"
                .to_owned(),
        );
    }
    if !r.unresolved.is_empty() {
        return Err(format!(
            "the settings use variables without a value: {}",
            r.unresolved.join(", ")
        ));
    }
    if r.token_url.trim().is_empty() {
        return Err("no access token URL".to_owned());
    }
    Ok(())
}

const NO_TOKEN: &str = "no token yet and auto fetch is off; get one with “Get token”";

/// The token to use for this send: cached, refreshed or new.
async fn obtain<T: Transport>(
    r: &Resolved,
    transport: &T,
    services: &Services<'_>,
    clock: &dyn Clock,
    run: &mut Run,
) -> Result<StoredToken, String> {
    check(r)?;
    let Some(store) = services.tokens else {
        // Nothing can be cached: with auto fetch every send fetches, without it none can
        // (the same answer natively and in the browser).
        if !r.auto_fetch {
            return Err(NO_TOKEN.to_owned());
        }
        return fetch(r, transport, services.prompt, clock, run).await;
    };
    let key = key_of(r);
    let cached = store.get(&key).await;
    if let Some(t) = &cached {
        run.token_secrets(t);
    }
    let now = clock.now_ms();
    let expired = match cached {
        Some(t) if !expiring(&t, now) => return Ok(t),
        Some(t) => Some(t),
        None => None,
    };
    if let Some(old) = expired {
        match old.refresh_token.as_deref() {
            Some(refresh_token) if r.auto_refresh => {
                match refresh(r, &old, refresh_token, transport, clock, run).await {
                    Ok(t) => {
                        run.note(NoteLevel::Info, "OAuth 2.0: token refreshed");
                        store.put(&key, &t).await;
                        return Ok(t);
                    }
                    Err(e) => run.note(
                        NoteLevel::Warning,
                        format!("OAuth 2.0: refreshing the token failed ({e}); getting a new one"),
                    ),
                }
            }
            _ => {}
        }
        store.remove(&key).await;
        if !r.auto_fetch {
            return Err(
                "the token expired and auto fetch is off; get a new token with “Get \
                        token”"
                    .to_owned(),
            );
        }
    } else if !r.auto_fetch {
        return Err(NO_TOKEN.to_owned());
    }
    let t = fetch(r, transport, services.prompt, clock, run).await?;
    store.put(&key, &t).await;
    Ok(t)
}

/// `true` if `t` expires within the margin: a quarter of its lifetime, at most 30 s.
fn expiring(t: &StoredToken, now: i64) -> bool {
    let Some(expires_at) = t.expires_at else {
        return false;
    };
    let lifetime = (expires_at - t.obtained_at).max(0);
    let margin = (lifetime / 4).min(30_000);
    now >= expires_at - margin
}

/// Runs the flow and returns a new token.
async fn fetch<T: Transport>(
    r: &Resolved,
    transport: &T,
    prompt: Option<&dyn AuthPrompt>,
    clock: &dyn Clock,
    run: &mut Run,
) -> Result<StoredToken, String> {
    let mut form = Form::default();
    match &r.flow {
        Flow::ClientCredentials => {
            form.add("grant_type", "client_credentials");
            form.add_nonempty("scope", &r.scope);
        }
        Flow::Password { username, password } => {
            form.add("grant_type", "password");
            form.add("username", username);
            form.add("password", password);
            form.add_nonempty("scope", &r.scope);
        }
        Flow::Saml2Bearer { assertion } => {
            form.add(
                "grant_type",
                "urn:ietf:params:oauth:grant-type:saml2-bearer",
            );
            form.add("assertion", assertion);
            form.add_nonempty("scope", &r.scope);
        }
        Flow::AuthorizationCode { .. } => {
            let Some(prompt) = prompt else {
                return Err(
                    "the authorization code flow needs the browser to sign in, which \
                            is not available here (e.g. `rt send --no-prompt`)"
                        .to_owned(),
                );
            };
            return authorization_code(r, transport, prompt, clock, run).await;
        }
        Flow::DeviceCode { .. } => {
            let Some(prompt) = prompt else {
                return Err(
                    "the device code flow needs to show a code to the user, which is \
                            not possible here (e.g. `rt send --no-prompt`)"
                        .to_owned(),
                );
            };
            return device_code(r, transport, prompt, clock, run).await;
        }
        Flow::Implicit => return Err("the implicit flow is not supported".to_owned()),
    }
    let t = endpoint::token(
        r,
        &r.token_url,
        OAuth2Step::AccessToken,
        form,
        transport,
        clock,
    )
    .await
    .map_err(|e| format!("token request failed: {e}"))?;
    run.token_secrets(&t);
    run.note(
        NoteLevel::Info,
        format!("OAuth 2.0: new token fetched ({})", r.flow.description()),
    );
    Ok(t)
}

async fn refresh<T: Transport>(
    r: &Resolved,
    old: &StoredToken,
    refresh_token: &str,
    transport: &T,
    clock: &dyn Clock,
    run: &mut Run,
) -> Result<StoredToken, String> {
    let mut form = Form::default();
    form.add("grant_type", "refresh_token");
    form.add("refresh_token", refresh_token);
    let url = if r.refresh_url.trim().is_empty() {
        &r.token_url
    } else {
        &r.refresh_url
    };
    let mut t = endpoint::token(r, url, OAuth2Step::Refresh, form, transport, clock)
        .await
        .map_err(|e| e.to_string())?;
    // RFC 6749 §6: the provider may keep the refresh token (and the ID token) as they were.
    if t.refresh_token.is_none() {
        t.refresh_token = old.refresh_token.clone();
    }
    if t.id_token.is_none() {
        t.id_token = old.id_token.clone();
    }
    run.token_secrets(&t);
    Ok(t)
}

async fn authorization_code<T: Transport>(
    r: &Resolved,
    transport: &T,
    prompt: &dyn AuthPrompt,
    clock: &dyn Clock,
    run: &mut Run,
) -> Result<StoredToken, String> {
    let Flow::AuthorizationCode {
        authorization_url,
        callback_url,
        state,
        pkce,
    } = &r.flow
    else {
        unreachable!("called for the authorization code flow");
    };
    if callback_url.trim().is_empty() {
        return Err("the authorization code flow needs a callback URL (e.g. \
                    http://127.0.0.1:8765/callback, registered at the provider)"
            .to_owned());
    }
    let state = if state.is_empty() {
        pkce::random_string(clock, 24)?
    } else {
        state.clone()
    };
    let verifier = match pkce {
        Some(_) => Some(pkce::random_string(clock, 48)?),
        None => None,
    };
    let mut url = url::Url::parse(authorization_url)
        .map_err(|e| format!("invalid authorization URL {authorization_url:?}: {e}"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", &r.client_id);
        q.append_pair("redirect_uri", callback_url);
        if !r.scope.is_empty() {
            q.append_pair("scope", &r.scope);
        }
        q.append_pair("state", &state);
        if let (Some(method), Some(verifier)) = (pkce, &verifier) {
            q.append_pair("code_challenge", &pkce::challenge(*method, verifier));
            q.append_pair(
                "code_challenge_method",
                match method {
                    PkceMethod::S256 => "S256",
                    PkceMethod::Plain => "plain",
                },
            );
        }
        for p in r.params(OAuth2Step::Authorization) {
            if p.placement == ParamPlacement::Query {
                q.append_pair(&p.name, &p.value);
            }
        }
    }
    for p in r.params(OAuth2Step::Authorization) {
        if p.placement != ParamPlacement::Query {
            run.note(
                NoteLevel::Warning,
                format!(
                    "OAuth 2.0: parameter `{}` of the authorization request was not sent: the \
                     browser step can only carry query parameters",
                    p.name
                ),
            );
        }
    }
    let redirected = prompt
        .authorize(url.as_str(), callback_url)
        .await
        .map_err(|e| format!("sign-in did not complete: {e}"))?;
    let code = code_from_redirect(&redirected, &state)?;
    run.secrets.push(code.clone());
    let mut form = Form::default();
    form.add("grant_type", "authorization_code");
    form.add("code", &code);
    form.add("redirect_uri", callback_url);
    if let Some(verifier) = &verifier {
        form.add("code_verifier", verifier);
    }
    let t = endpoint::token(
        r,
        &r.token_url,
        OAuth2Step::AccessToken,
        form,
        transport,
        clock,
    )
    .await
    .map_err(|e| format!("exchanging the authorization code failed: {e}"))?;
    run.token_secrets(&t);
    run.note(
        NoteLevel::Info,
        "OAuth 2.0: new token fetched (authorization code)",
    );
    Ok(t)
}

/// The `code` of the redirect, after checking `state` and `error` (RFC 6749 §4.1.2).
fn code_from_redirect(redirected: &str, state: &str) -> Result<String, String> {
    let url = url::Url::parse(redirected)
        .map_err(|_| "the sign-in returned no valid redirect URL".to_owned())?;
    let mut pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    if pairs.is_empty()
        && let Some(fragment) = url.fragment()
    {
        pairs = url::form_urlencoded::parse(fragment.as_bytes())
            .into_owned()
            .collect();
    }
    let get = |name: &str| {
        pairs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    if get("state") != Some(state) {
        return Err(
            "the redirect's state does not match this sign-in (it may belong to \
                    another attempt or be forged); sign in again"
                .to_owned(),
        );
    }
    if let Some(error) = get("error") {
        return Err(match get("error_description").filter(|d| !d.is_empty()) {
            Some(d) => format!("the provider refused the sign-in: {error}: {d}"),
            None => format!("the provider refused the sign-in: {error}"),
        });
    }
    get("code")
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "the redirect carries no authorization code".to_owned())
}

async fn device_code<T: Transport>(
    r: &Resolved,
    transport: &T,
    prompt: &dyn AuthPrompt,
    clock: &dyn Clock,
    run: &mut Run,
) -> Result<StoredToken, String> {
    let Flow::DeviceCode {
        device_authorization_url,
    } = &r.flow
    else {
        unreachable!("called for the device code flow");
    };
    let mut form = Form::default();
    form.add_nonempty("scope", &r.scope);
    let device = endpoint::device_authorization(r, device_authorization_url, form, transport)
        .await
        .map_err(|e| format!("device authorization request failed: {e}"))?;
    run.secrets.push(device.device_code.clone());
    prompt.device_code(&device.prompt).await;
    let result = poll(r, &device, transport, prompt, clock).await;
    prompt.device_code_done().await;
    let t = result?;
    run.token_secrets(&t);
    run.note(
        NoteLevel::Info,
        "OAuth 2.0: new token fetched (device code)",
    );
    Ok(t)
}

/// Polls the token endpoint until the user approved, denied, cancelled or the code expired
/// (RFC 8628 §3.4, §3.5).
async fn poll<T: Transport>(
    r: &Resolved,
    device: &endpoint::DeviceAuthorization,
    transport: &T,
    prompt: &dyn AuthPrompt,
    clock: &dyn Clock,
) -> Result<StoredToken, String> {
    let deadline = clock.now_ms() + (device.prompt.expires_in as i64) * 1000;
    let mut interval = device.interval.max(1) * 1000;
    loop {
        clock.sleep(interval).await;
        if prompt.cancelled() {
            return Err("sign-in cancelled".to_owned());
        }
        if clock.now_ms() > deadline {
            return Err("the device code expired before the sign-in was completed".to_owned());
        }
        let mut form = Form::default();
        form.add("grant_type", "urn:ietf:params:oauth:grant-type:device_code");
        form.add("device_code", &device.device_code);
        match endpoint::token(
            r,
            &r.token_url,
            OAuth2Step::AccessToken,
            form,
            transport,
            clock,
        )
        .await
        {
            Ok(t) => return Ok(t),
            Err(EndpointError::OAuth { code, .. }) if code == "authorization_pending" => {}
            Err(EndpointError::OAuth { code, .. }) if code == "slow_down" => interval += 5_000,
            Err(EndpointError::OAuth { code, .. }) if code == "access_denied" => {
                return Err("the sign-in was denied".to_owned());
            }
            Err(EndpointError::OAuth { code, .. }) if code == "expired_token" => {
                return Err("the device code expired before the sign-in was completed".to_owned());
            }
            Err(e) => return Err(format!("token request failed: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Applying the token, masking

fn apply(request: &mut HttpRequest, placement: &TokenPlacement, token: &str) {
    match placement {
        TokenPlacement::Header { prefix } => {
            let value = if prefix.is_empty() {
                token.to_owned()
            } else {
                format!("{prefix} {token}")
            };
            set_header(&mut request.headers, "Authorization", value);
        }
        TokenPlacement::Query { name } => {
            let name = if name.is_empty() {
                "access_token"
            } else {
                name
            };
            request.url = endpoint::with_query(&request.url, name, token);
        }
    }
}

/// Replaces the first header `name` (any case) and drops later ones; else appends.
fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    match headers
        .iter()
        .position(|(n, _)| n.eq_ignore_ascii_case(name))
    {
        Some(first) => {
            headers[first] = (name.to_owned(), value);
            let mut index = 0;
            headers.retain(|(n, _)| {
                let keep = index <= first || !n.eq_ignore_ascii_case(name);
                index += 1;
                keep
            });
        }
        None => headers.push((name.to_owned(), value)),
    }
}

/// `text` with every secret replaced by `***`, also in its form- and percent-encoded forms.
/// `secrets` longest first.
fn mask(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        let form: String = url::form_urlencoded::byte_serialize(secret.as_bytes()).collect();
        let percent = endpoint::percent(secret);
        for variant in [secret.as_str(), form.as_str(), percent.as_str()] {
            if !variant.is_empty() {
                out = out.replace(variant, crate::send::MASK);
            }
        }
    }
    out
}
