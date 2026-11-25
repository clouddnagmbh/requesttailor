// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The notes of the `response` hook and the two tabs, on fixed answers.

use rt_plugin_sap_integration::Integration;
use rt_plugin_sdk::NoteLevel;
use rt_plugin_sdk::api::{
    Block, Call, NotesReply, PluginBody, PluginResponse, TabTarget, ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::testing;

fn response(status: u16, headers: &[(&str, &str)], body: &str) -> PluginResponse {
    PluginResponse {
        status,
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
        url: "https://t.example/".into(),
        body: PluginBody {
            content_type: Some("application/json".into()),
            text: Some(body.into()),
            base64: None,
        },
        truncated: false,
    }
}

fn notes(url: &str, response: PluginResponse) -> NotesReply {
    testing::call(
        &Integration,
        &Call::Response {
            request: testing::request("GET", url),
            response,
            context: testing::context("r.yml"),
        },
    )
    .unwrap()
}

const FAILED: &str = r#"{"d":{"MessageGuid":"G1","CorrelationId":"C1","LogStart":"/Date(1764000000000)/","LogEnd":"/Date(1764000003000)/","Sender":"Shop","Receiver":"ERP","IntegrationFlowName":"Orders","Status":"FAILED"}}"#;
const LIST: &str = r#"{"d":{"results":[
  {"MessageGuid":"G1","IntegrationFlowName":"Orders","Status":"FAILED","LogStart":"/Date(1764000000000)/","LogEnd":"/Date(1764000001000)/"},
  {"MessageGuid":"G2","IntegrationFlowName":"Orders","Status":"COMPLETED","LogStart":"/Date(1764000010000)/","LogEnd":"/Date(1764000011500)/"}]}}"#;

fn tab(tab: &str, spec: RequestSpec, response: Option<PluginResponse>) -> ViewReply {
    testing::call(
        &Integration,
        &Call::RenderTab {
            tab: tab.into(),
            target: TabTarget {
                request_path: "r.yml".into(),
                response_id: response.as_ref().map(|_| 1),
            },
            spec,
            response,
            state: None,
            context: None,
        },
    )
    .unwrap()
}

fn act(
    tab: &str,
    action: &str,
    spec: RequestSpec,
    response: Option<PluginResponse>,
    input: &[(&str, &str)],
) -> Result<ViewReply, String> {
    testing::call(
        &Integration,
        &Call::Action {
            tab: tab.into(),
            action: action.into(),
            target: TabTarget {
                request_path: "r.yml".into(),
                response_id: response.as_ref().map(|_| 1),
            },
            spec,
            response,
            state: None,
            input: input
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            context: None,
        },
    )
}

#[test]
fn the_trigger_answer_names_the_log() {
    let n = notes(
        "https://t.example/http/orders",
        response(200, &[("SAP_MessageProcessingLogID", "AG123")], "{}"),
    );
    assert_eq!(n.notes.len(), 1);
    assert!(n.notes[0].text.contains("AG123"));
}

#[test]
fn access_errors_say_which_role_is_missing() {
    let api = notes(
        "https://t.example/api/v1/MessageProcessingLogs",
        response(403, &[], "{}"),
    );
    assert!(api.notes[0].text.contains("MonitoringDataRead"));
    let flow = notes("https://t.example/http/orders", response(403, &[], "{}"));
    assert!(flow.notes[0].text.contains("ESBMessaging.send"));
    let other = notes("https://t.example/x", response(401, &[], "{}"));
    assert!(other.notes[0].text.contains("401"));
}

#[test]
fn a_log_answer_is_explained_by_status() {
    let failed = notes("https://t.example/api", response(200, &[], FAILED));
    assert_eq!(failed.notes[0].level, NoteLevel::Warning);
    assert!(failed.notes[0].text.contains("FAILED after 3 s"));
    assert!(failed.notes[0].text.contains("MessageProcessingLogs('G1')"));
    let list = notes("https://t.example/api", response(200, &[], LIST));
    assert!(
        list.notes[0]
            .text
            .contains("2 message processing logs: 1 failed")
    );
    // JSON that is no log gets no note.
    assert!(
        notes("https://t.example/api", response(200, &[], r#"{"a":1}"#))
            .notes
            .is_empty()
    );
    // A plain answer without a body hook note.
    assert!(
        notes("https://t.example/x", response(200, &[], "not json"))
            .notes
            .is_empty()
    );
}

#[test]
fn the_response_tab_reads_one_log_and_offers_the_error_text() {
    let view = tab(
        "integration-log",
        RequestSpec::default(),
        Some(response(200, &[], FAILED)),
    );
    let blocks = &view.view.blocks;
    assert!(blocks.iter().any(|b| matches!(
        b,
        Block::Notice {
            level: NoteLevel::Error,
            ..
        }
    )));
    let rows = blocks
        .iter()
        .find_map(|b| match b {
            Block::KeyValue { rows } => Some(rows.clone()),
            _ => None,
        })
        .unwrap();
    assert!(rows.contains(&("Took".into(), "3 s".into())));
    assert!(rows.contains(&("Message id".into(), "G1".into())));
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, Block::Actions { actions } if actions[0].id == "error:G1"))
    );

    let reply = act(
        "integration-log",
        "error:G1",
        RequestSpec::default(),
        Some(response(200, &[], FAILED)),
        &[],
    )
    .unwrap();
    let new = &reply.create_requests[0];
    assert_eq!(new.folder, "message-processing-log");
    assert_eq!(
        new.spec.url,
        "{{apiUrl}}/api/v1/MessageProcessingLogs('G1')/ErrorInformation/$value"
    );
    // Only a message of this answer can be asked for.
    assert!(
        act(
            "integration-log",
            "error:other",
            RequestSpec::default(),
            Some(response(200, &[], FAILED)),
            &[]
        )
        .is_err()
    );
}

#[test]
fn the_response_tab_lists_many_and_not_a_log_says_so() {
    let view = tab(
        "integration-log",
        RequestSpec::default(),
        Some(response(200, &[], LIST)),
    );
    let table = view
        .view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table { rows, .. } => Some(rows.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.len(), 2);
    assert_eq!(table[1][3], "1.5 s");
    assert!(
        view.view
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Tree { nodes } if nodes.len() == 1))
    );
    let other = tab(
        "integration-log",
        RequestSpec::default(),
        Some(response(200, &[], "{\"a\":1}")),
    );
    assert!(
        matches!(&other.view.blocks[0], Block::Notice { text, .. } if text.contains("not a message processing log"))
    );
}

fn spec() -> RequestSpec {
    RequestSpec {
        name: "Order flow".into(),
        method: "POST".into(),
        url: "{{runtimeUrl}}/http/orders".into(),
        ..RequestSpec::default()
    }
}

#[test]
fn the_request_tab_keeps_the_log_id_and_creates_the_check() {
    let view = tab("integration", spec(), None);
    assert!(
        view.view
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Actions { actions } if actions[0].id == "capture"))
    );
    // Capture: the action is added to the request (unsaved), once.
    let reply = act("integration", "capture", spec(), None, &[]).unwrap();
    let updated = reply.update_request.unwrap();
    assert_eq!(updated.actions.len(), 1);
    assert_eq!(updated.actions[0].variable, "mplId");
    assert_eq!(updated.actions[0].expression, "SAP_MessageProcessingLogID");
    let again = act("integration", "capture", updated.clone(), None, &[]).unwrap();
    assert!(again.update_request.is_none());
    // Check: a new request with the status assertion; the trigger gets the capture action.
    let reply = act(
        "integration",
        "check",
        spec(),
        None,
        &[("expected", "FAILED"), ("name", "Log of the failing order")],
    )
    .unwrap();
    let new = &reply.create_requests[0];
    assert_eq!(new.name, "Log of the failing order");
    let status = new
        .spec
        .assertions
        .iter()
        .find(|a| a.expression == "res.body.d.Status")
        .unwrap();
    assert_eq!(
        (status.operator.as_str(), status.value.as_str()),
        ("eq", "FAILED")
    );
    assert!(new.spec.url.contains("('{{mplId}}')"));
    assert_eq!(reply.update_request.unwrap().actions.len(), 1);
    // A status that is none of the log's is refused.
    assert!(
        act(
            "integration",
            "check",
            spec(),
            None,
            &[("expected", "DONE")]
        )
        .is_err()
    );
    assert!(act("integration", "nope", spec(), None, &[]).is_err());
}
