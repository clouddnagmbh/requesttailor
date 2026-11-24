// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand E: the review providers for the web shell (`rt_app::review`, over the web transport).
//!
//! The web shell's `src/review.ts` declares the TypeScript types of these `Workspace`
//! methods (as `src/compare.ts` does); `src/wasm.ts` stays untouched.
//!
//! ```text
//! Workspace.reviewDetect(remoteUrl, hosts) -> ReviewRepo | null
//! Workspace.reviewNormalizeHosts(hosts) -> ReviewHost[]
//! Workspace.reviewCall(repo, token, op, number, payload, companion) -> Promise<any>
//! Workspace.reviewDeviceClientId(repo) -> string | null
//! Workspace.reviewDeviceStart(repo, clientId, companion) -> Promise<DeviceLogin>
//! Workspace.reviewDevicePoll(repo, clientId, login, companion)
//!     -> Promise<{ state: "pending" } | { state: "slow-down", interval } | { state: "token", token }>
//! ```
//!
//! `op` is `list`, `get`, `comments`, `comment` (`payload`: a `NewReviewComment`), `submit`
//! (`payload`: a `ReviewSubmission`), `ci` or `create` (`payload`: a `NewPullRequest`, answers
//! the new `PullRequest`); `number` is the PR number (ignored by `list`).
//! Calls go out with `fetch`, or through the companion when it is passed. Rejects with an
//! `Error` named `AppError` whose `kind` is a `ReviewError` kind (`no-token`, `auth`,
//! `not-found`, `transport`, `host`); the token is masked in every message.

use js_sys::Promise;
use rt_app::review::device::{self, DeviceLogin, DevicePoll};
use rt_app::review::{
    self, NewPullRequest, NewReviewComment, ReviewError, ReviewHost, ReviewRepo, ReviewSubmission,
};
use rt_core::transport::Transport;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::Companion;
use crate::workspace_js::{WorkspaceJs, from_js, named_error, to_js};

fn review_error(e: ReviewError) -> JsValue {
    let kind = serde_json::to_value(&e)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "shell".to_owned());
    named_error(&kind, &e.to_string())
}

fn companion_of(value: &JsValue) -> Result<Option<Companion>, JsValue> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        Ok(Some(from_js(value, "companion")?))
    }
}

async fn call<T: Transport>(
    transport: &T,
    repo: &ReviewRepo,
    token: &str,
    op: &str,
    number: u64,
    payload: Value,
) -> Result<Value, ReviewError> {
    let p = review::provider(transport, repo, token);
    let to_json = |v: &dyn erased::Json| v.json();
    let bad = |what: &str| ReviewError::Host(format!("the {what} is malformed"));
    Ok(match op {
        "list" => to_json(&p.list().await?),
        "get" => to_json(&p.get(number).await?),
        "comments" => to_json(&p.comments(number).await?),
        "comment" => {
            let pull = p.get(number).await?;
            let c: NewReviewComment =
                serde_json::from_value(payload).map_err(|_| bad("comment"))?;
            to_json(&p.comment(&pull, &c).await?)
        }
        "submit" => {
            let pull = p.get(number).await?;
            let r: ReviewSubmission = serde_json::from_value(payload).map_err(|_| bad("review"))?;
            p.submit(&pull, &r).await?;
            Value::Null
        }
        "ci" => {
            let pull = p.get(number).await?;
            to_json(&p.ci(&pull).await?)
        }
        "create" => {
            let n: NewPullRequest =
                serde_json::from_value(payload).map_err(|_| bad("pull request"))?;
            to_json(&p.create(&n).await?)
        }
        other => {
            return Err(ReviewError::Host(format!(
                "unknown review operation {other:?}"
            )));
        }
    })
}

/// A tiny shim so `call` can turn any serializable answer into JSON in one expression.
mod erased {
    pub trait Json {
        fn json(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
        }
    }
}

fn poll_json(p: DevicePoll) -> Value {
    match p {
        DevicePoll::Pending => json!({ "state": "pending" }),
        DevicePoll::SlowDown(interval) => json!({ "state": "slow-down", "interval": interval }),
        DevicePoll::Token(token) => json!({ "state": "token", "token": token }),
    }
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// See the module documentation.
    #[wasm_bindgen(js_name = reviewDetect)]
    pub fn review_detect(&self, remote_url: &str, hosts: JsValue) -> Result<JsValue, JsValue> {
        let hosts: Vec<ReviewHost> = from_js(&hosts, "hosts")?;
        to_js(&review::detect(remote_url, &hosts))
    }

    #[wasm_bindgen(js_name = reviewNormalizeHosts)]
    pub fn review_normalize_hosts(&self, hosts: JsValue) -> Result<JsValue, JsValue> {
        let hosts: Vec<ReviewHost> = from_js(&hosts, "hosts")?;
        to_js(&review::normalize_hosts(&hosts).map_err(review_error)?)
    }

    #[wasm_bindgen(js_name = reviewCall)]
    pub fn review_call(
        &self,
        repo: JsValue,
        token: String,
        op: String,
        number: f64,
        payload: JsValue,
        companion: JsValue,
    ) -> Result<Promise, JsValue> {
        let repo: ReviewRepo = from_js(&repo, "repo")?;
        let payload: Value = if payload.is_null() || payload.is_undefined() {
            Value::Null
        } else {
            from_js(&payload, "payload")?
        };
        let companion = companion_of(&companion)?;
        let number = number as u64;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let answer = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    call(&t, &repo, &token, &op, number, payload).await
                }
                None => {
                    let t = FetchTransport::new();
                    call(&t, &repo, &token, &op, number, payload).await
                }
            };
            to_js(&answer.map_err(review_error)?)
        }))
    }

    #[wasm_bindgen(js_name = reviewDeviceClientId)]
    pub fn review_device_client_id(&self, repo: JsValue) -> Result<JsValue, JsValue> {
        let repo: ReviewRepo = from_js(&repo, "repo")?;
        to_js(&device::client_id(&repo))
    }

    #[wasm_bindgen(js_name = reviewDeviceStart)]
    pub fn review_device_start(
        &self,
        repo: JsValue,
        client_id: String,
        companion: JsValue,
    ) -> Result<Promise, JsValue> {
        let repo: ReviewRepo = from_js(&repo, "repo")?;
        let c = companion_of(&companion)?
            .ok_or_else(|| review_error(ReviewError::Transport("signing in with a code needs the companion (`rt agent`): the host's login pages cannot be called from a browser".into())))?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let t = CompanionTransport::new(c.url, c.token);
            let login = device::start(&t, &repo, &client_id)
                .await
                .map_err(review_error)?;
            to_js(&login)
        }))
    }

    #[wasm_bindgen(js_name = reviewDevicePoll)]
    pub fn review_device_poll(
        &self,
        repo: JsValue,
        client_id: String,
        login: JsValue,
        companion: JsValue,
    ) -> Result<Promise, JsValue> {
        let repo: ReviewRepo = from_js(&repo, "repo")?;
        let login: DeviceLogin = from_js(&login, "login")?;
        let c = companion_of(&companion)?.ok_or_else(|| {
            review_error(ReviewError::Transport(
                "signing in with a code needs the companion (`rt agent`)".into(),
            ))
        })?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let t = CompanionTransport::new(c.url, c.token);
            let polled = device::poll(&t, &repo, &client_id, &login)
                .await
                .map_err(review_error)?;
            to_js(&poll_json(polled))
        }))
    }
}
