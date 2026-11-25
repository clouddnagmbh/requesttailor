// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The requests the plugin writes: trigger an iFlow and read its message processing log. The
//! checks are ordinary assertions and variable actions of the core, so they run the same in the
//! app, the runner and `rt run`; the plugin is not needed to run them.

use rt_plugin_sdk::model::{
    Assertion, Body, KeyValue, RawKind, RequestSpec, ScriptPhase, VarAction, VarScope,
};

use crate::mpl::LOG_ID_HEADER;

/// The variable the trigger request fills with the id of its message processing log.
pub const LOG_VAR: &str = "mplId";
/// Folder of the iFlow requests and folder of the log requests (relative to the collection).
pub const FLOW_FOLDER: &str = "iflow";
pub const LOG_FOLDER: &str = "message-processing-log";

pub fn assertion(expression: &str, operator: &str, value: &str, why: &str) -> Assertion {
    Assertion {
        expression: expression.into(),
        operator: operator.into(),
        value: value.into(),
        description: Some(why.into()),
        enabled: true,
    }
}

/// The action that copies the log id of an answer into [`LOG_VAR`] (runtime layer: it lives
/// for the session or the run and is never written to a file).
pub fn capture_action() -> VarAction {
    VarAction {
        phase: ScriptPhase::AfterResponse,
        expression: LOG_ID_HEADER.into(),
        method: "header".into(),
        variable: LOG_VAR.into(),
        scope: VarScope::Runtime,
        enabled: true,
    }
}

/// Whether `spec` already has the capture action.
pub fn captures_log_id(spec: &RequestSpec) -> bool {
    spec.actions
        .iter()
        .any(|a| a.variable == LOG_VAR && a.method == "header" && a.expression == LOG_ID_HEADER)
}

/// `POST {{runtimeUrl}}/http/{{iflowPath}}`: starts the iFlow and keeps the log id.
pub fn trigger() -> RequestSpec {
    RequestSpec {
        name: "Trigger iFlow".into(),
        method: "POST".into(),
        url: "{{runtimeUrl}}/http/{{iflowPath}}".into(),
        headers: vec![KeyValue::new("Content-Type", "application/json")],
        body: Body::Raw {
            kind: RawKind::Json,
            data: "{\n  \"hint\": \"replace this with the payload your iFlow expects\"\n}".into(),
        },
        assertions: vec![
            assertion("res.status", "lt", "300", "the iFlow accepted the message"),
            assertion(
                &format!("res.headers.{LOG_ID_HEADER}"),
                "isDefined",
                "",
                "the runtime named the message processing log",
            ),
        ],
        actions: vec![capture_action()],
        ..RequestSpec::default()
    }
}

fn log_url(id: &str) -> String {
    format!("{{{{apiUrl}}}}/api/v1/MessageProcessingLogs('{id}')?$format=json")
}

/// The log of one message (`id` is a log id or a `{{variable}}`), with the expected status as an
/// assertion: `COMPLETED` for a run that has to succeed, `FAILED` for a negative test.
pub fn log_check(name: &str, id: &str, expected: &str) -> RequestSpec {
    RequestSpec {
        name: name.into(),
        method: "GET".into(),
        url: log_url(id),
        headers: vec![KeyValue::new("Accept", "application/json")],
        assertions: vec![
            assertion(
                "res.status",
                "eq",
                "200",
                "the log exists (it can take a moment)",
            ),
            assertion(
                "res.body.d.Status",
                "eq",
                expected,
                &format!("the message ended with status {expected}"),
            ),
        ],
        ..RequestSpec::default()
    }
}

/// The log of the last trigger of this session.
pub fn log_of_last_trigger(expected: &str) -> RequestSpec {
    log_check(
        "Log of the last trigger",
        &format!("{{{{{LOG_VAR}}}}}"),
        expected,
    )
}

/// The latest failed messages of the iFlow named by `{{iflowName}}`.
pub fn failed_logs() -> RequestSpec {
    RequestSpec {
        name: "Failed messages of the iFlow".into(),
        method: "GET".into(),
        url: "{{apiUrl}}/api/v1/MessageProcessingLogs?$filter=IntegrationFlowName eq '{{iflowName}}' and Status eq 'FAILED'&$orderby=LogStart desc&$top=5&$format=json".into(),
        headers: vec![KeyValue::new("Accept", "application/json")],
        ..RequestSpec::default()
    }
}

/// The error text of a log (`id` as for [`log_check`]). Answers 404 for a message without an
/// error.
pub fn error_text(name: &str, id: &str) -> RequestSpec {
    RequestSpec {
        name: name.into(),
        method: "GET".into(),
        url: format!("{{{{apiUrl}}}}/api/v1/MessageProcessingLogs('{id}')/ErrorInformation/$value"),
        headers: vec![KeyValue::new("Accept", "text/plain")],
        ..RequestSpec::default()
    }
}

pub fn error_text_of_last_trigger() -> RequestSpec {
    error_text(
        "Error text of the last trigger",
        &format!("{{{{{LOG_VAR}}}}}"),
    )
}
