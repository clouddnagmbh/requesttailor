// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand E: the web shell's call for `Backend.inbox.makeRequest`. A browser cannot
//! listen, so the inbox itself runs in the companion (`rt agent`, route `/v1/inbox`); what the
//! page needs from the core is the one conversion that has rules: a captured request into a
//! request against `{{baseUrl}}` with its secrets as variables without a value.
//!
//! ```text
//! inboxMakeRequest(request: CapturedRequest) -> InboxDraft
//! ```

use rt_app::inbox::{self, CapturedRequest};
use wasm_bindgen::prelude::*;

use crate::workspace_js::{from_js, to_js};

#[wasm_bindgen(js_name = inboxMakeRequest)]
pub fn inbox_make_request(request: JsValue) -> Result<JsValue, JsValue> {
    let request: CapturedRequest = from_js(&request, "request")?;
    to_js(&inbox::draft(&request))
}
