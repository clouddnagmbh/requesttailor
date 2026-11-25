// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand D: the two-user access check (`rt_app::access`) over the open `Workspace`,
//! with the same transports and services as `Workspace.send` (so the send guard answers for
//! both requests). The web shell's `src/access.ts` declares the TypeScript type.
//!
//! ```text
//! Workspace.accessCheck(path, profile, options, inputs, companion, services, cancelled)
//!     -> Promise<AccessCheck>
//! ```
//!
//! `profile` is an `AccessProfile`, `options` an `AccessOptions`; `inputs` has one `SendInput`
//! per profile (A first: environment name, runtime variables, the secret values of that
//! environment, which never leave the core), `services` one send-services object (or `null`)
//! per profile, `cancelled()` is polled before each request. The web keeps no audit log.

use js_sys::{Array, Function, Promise};
use rt_app::SendInput;
use rt_app::access::{AccessEnv, AccessOptions, AccessProfile, check};
use rt_app::envcompare::EnvRun;
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;

use crate::services_js::SendServices;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Checks one request as user A and user B; see the module documentation.
    #[wasm_bindgen(js_name = accessCheck)]
    pub fn access_check_js(
        &self,
        path: String,
        profile: JsValue,
        options: JsValue,
        inputs: JsValue,
        companion: JsValue,
        services: JsValue,
        cancelled: Function,
    ) -> Result<Promise, JsValue> {
        let profile: AccessProfile = from_js(&profile, "profile")?;
        let options: AccessOptions = from_js(&options, "options")?;
        let inputs: Vec<SendInput> = from_js(&inputs, "inputs")?;
        let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
            None
        } else {
            Some(from_js(&companion, "companion")?)
        };
        let parts: Vec<SendServices> = Array::from(&services)
            .iter()
            .map(|s| SendServices::from_js(&s))
            .collect::<Result<_, _>>()?;
        if inputs.len() != 2 || parts.len() != 2 {
            return Err(JsValue::from_str(
                "accessCheck needs one input and one services object per profile",
            ));
        }
        let mut snapshots: Vec<_> = inputs
            .into_iter()
            .map(|input| self.workspace().run_snapshot(input))
            .collect();
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let b = snapshots.pop();
            let a = snapshots.pop();
            let (Some(a), Some(b)) = (a, b) else {
                return Err(JsValue::from_str("accessCheck needs two profiles"));
            };
            let a = EnvRun {
                snapshot: a,
                services: parts[0].services(),
            };
            let b = EnvRun {
                snapshot: b,
                services: parts[1].services(),
            };
            let send_options = SendOptions {
                body_limit: BODY_LIMIT,
                iteration: None,
            };
            let stop = || {
                cancelled
                    .call0(&JsValue::NULL)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
            };
            let result = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    let env = AccessEnv {
                        a: &a,
                        b: &b,
                        transport: &t,
                        options: &send_options,
                        caller: "app",
                        audit: &|_| {},
                        cancelled: &stop,
                    };
                    check(&env, &path, &profile, &options).await
                }
                None => {
                    let t = FetchTransport::new();
                    let env = AccessEnv {
                        a: &a,
                        b: &b,
                        transport: &t,
                        options: &send_options,
                        caller: "app",
                        audit: &|_| {},
                        cancelled: &stop,
                    };
                    check(&env, &path, &profile, &options).await
                }
            };
            to_js(&result.map_err(app_error)?)
        }))
    }
}
