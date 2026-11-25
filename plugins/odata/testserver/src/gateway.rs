// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! SAP Gateway extras of the S/4-style services (wave 18, strand J): the `sap-statistics`
//! response header and the full shape of Gateway error bodies (error details, resolution
//! hints, V2 as JSON or XML, V4 as JSON). Synthetic and unverified against a real system: the
//! header's format is the assumption the plugin's tolerant reader is tested against
//! (`plugins/odata/testdata/SOURCES.md`).

use serde_json::{Value, json};

use crate::odata::{Reply, header};
use crate::services::{Flavor, ServiceDef, Version};

/// The header value the server reports when asked (`sap-statistics: true`): milliseconds.
pub const STATISTICS: &str = "total=48,fw=7,app=21,db=9";

/// A transaction ID that is the same in every error, so tests can look for it.
pub const TRANSACTION_ID: &str = "0000000000000000000000000000RT01";
/// A fixed time stamp (`yyyymmddhhmmss`, UTC).
pub const TIMESTAMP: &str = "20261124103015";

const RESOLUTION: &str =
    "Run transaction /IWFND/ERROR_LOG on the gateway system and search for the transaction ID";

/// Whether the request asked for the statistics (header or query parameter).
pub fn wants_statistics(headers: &[(String, String)], query: &[(String, String)]) -> bool {
    header(headers, "sap-statistics").is_some_and(|v| v.eq_ignore_ascii_case("true"))
        || query
            .iter()
            .any(|(n, v)| n == "sap-statistics" && v.eq_ignore_ascii_case("true"))
}

/// Adds what the Gateway adds to a reply of an S/4-style service.
pub fn finish(
    def: &ServiceDef,
    headers: &[(String, String)],
    query: &[(String, String)],
    reply: &mut Reply,
) {
    if def.flavor != Flavor::S4 {
        return;
    }
    if wants_statistics(headers, query) && reply.status < 400 {
        reply
            .headers
            .push(("sap-statistics".into(), STATISTICS.into()));
    }
    if reply.status < 400 || !is_json(reply) {
        return;
    }
    let Ok(body) = serde_json::from_slice::<Value>(&reply.body) else {
        return;
    };
    let Some(error) = body.get("error") else {
        return;
    };
    let code = error["code"].as_str().unwrap_or_default().to_owned();
    let message = match def.version {
        Version::V2 => error["message"]["value"].as_str(),
        Version::V4 => error["message"].as_str(),
    }
    .unwrap_or_default()
    .to_owned();
    let wants_xml = header(headers, "Accept")
        .is_some_and(|a| a.contains("xml") && !a.contains("json") && def.version == Version::V2);
    if wants_xml {
        reply.body = xml(&code, &message).into_bytes();
        for (n, v) in &mut reply.headers {
            if n.eq_ignore_ascii_case("content-type") {
                *v = "application/xml".into();
            }
        }
    } else {
        let full = match def.version {
            Version::V2 => json!({ "error": {
                "code": code,
                "message": { "lang": error["message"]["lang"], "value": message },
                "innererror": {
                    "transactionid": TRANSACTION_ID,
                    "timestamp": TIMESTAMP,
                    "Error_Resolution": { "SAP_Transaction": RESOLUTION },
                    "errordetails": [
                        { "code": code, "message": message, "propertyref": "", "severity": "error", "target": "" }
                    ]
                }
            }}),
            Version::V4 => json!({ "error": {
                "code": code,
                "message": message,
                "details": [
                    { "code": code, "message": message, "target": "", "@Common.numericSeverity": 4 }
                ],
                "innererror": {
                    "transactionid": TRANSACTION_ID,
                    "timestamp": TIMESTAMP,
                    "Error_Resolution": { "SAP_Transaction": RESOLUTION }
                }
            }}),
        };
        reply.body = full.to_string().into_bytes();
    }
}

fn is_json(reply: &Reply) -> bool {
    header(&reply.headers, "Content-Type").is_some_and(|c| c.contains("json"))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn xml(code: &str, message: &str) -> String {
    let (code, message) = (escape(code), escape(message));
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<error xmlns=\"http://schemas.microsoft.com/ado/2007/08/dataservices/metadata\">\
<code>{code}</code><message xml:lang=\"en\">{message}</message>\
<innererror><transactionid>{TRANSACTION_ID}</transactionid><timestamp>{TIMESTAMP}</timestamp>\
<Error_Resolution><SAP_Transaction>{RESOLUTION}</SAP_Transaction></Error_Resolution>\
<errordetails><errordetail><code>{code}</code><message>{message}</message><propertyref/>\
<severity>error</severity><target/></errordetail></errordetails></innererror></error>"
    )
}
