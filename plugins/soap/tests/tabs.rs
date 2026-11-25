// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The SOAP tabs: the request tab changes version, action and WS-Security of an envelope; the
//! response tab reads MTOM packages (read only).

mod common;

use common::{body, header, import, read, request, testdata};
use rt_plugin_sdk::api::{Block, Call, PluginBody, PluginResponse, TabTarget, ViewReply};
use rt_plugin_sdk::testing;
use rt_plugin_soap::Soap;

fn target() -> TabTarget {
    TabTarget {
        request_path: "r.yml".into(),
        response_id: None,
    }
}

fn apply(
    spec: rt_plugin_sdk::model::RequestSpec,
    input: &[(&str, &str)],
) -> Result<ViewReply, String> {
    testing::call(
        &Soap,
        &Call::Action {
            tab: "soap".into(),
            action: "apply".into(),
            target: target(),
            spec,
            response: None,
            state: None,
            input: input
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            context: None,
        },
    )
}

fn add() -> rt_plugin_sdk::model::RequestSpec {
    request(
        &read(&import(
            "wsdl",
            vec![testdata("calculator.wsdl"), testdata("types.xsd")],
        )),
        "CalculatorSoap11",
        "Add",
    )
}

#[test]
fn the_form_shows_the_current_settings() {
    let reply: ViewReply = testing::call(
        &Soap,
        &Call::RenderTab {
            tab: "soap".into(),
            target: target(),
            spec: add(),
            response: None,
            state: None,
            context: None,
        },
    )
    .unwrap();
    let form = reply.view.blocks.iter().find_map(|b| match b {
        Block::Form { fields, .. } => Some(fields.clone()),
        _ => None,
    });
    let fields = form.expect("a form");
    let value = |n: &str| fields.iter().find(|f| f.name == n).unwrap().value.clone();
    assert_eq!(value("version"), "1.1");
    assert_eq!(value("action"), "urn:requesttailor:test:calc/Add");
    assert_eq!(value("username_token"), "false");
}

#[test]
fn apply_switches_to_1_2_and_adds_ws_security_with_placeholders() {
    let reply = apply(
        add(),
        &[
            ("version", "1.2"),
            ("action", "urn:requesttailor:test:calc/Add"),
            ("username_token", "true"),
            ("user_var", "{{svc_user}}"),
            ("password_var", "svc_pass"),
            ("timestamp", "true"),
            ("ttl", "120"),
        ],
    )
    .unwrap();
    let spec = reply.update_request.unwrap();
    let text = body(&spec);
    assert!(text.contains("http://www.w3.org/2003/05/soap-envelope"));
    assert!(
        text.contains("<wsse:Username>{{svc_user}}</wsse:Username>"),
        "{text}"
    );
    assert!(text.contains(">{{svc_pass}}</wsse:Password>"));
    assert_eq!(header(&spec, "SOAPAction"), None);
    assert!(
        header(&spec, "Content-Type")
            .unwrap()
            .starts_with("application/soap+xml")
    );
    assert!(rt_plugin_soap::envelope::parse(text).is_some());
}

#[test]
fn bad_input_is_refused_and_changes_nothing() {
    let err = apply(
        add(),
        &[("version", "1.1"), ("timestamp", "true"), ("ttl", "soon")],
    )
    .unwrap_err();
    assert!(err.contains("time to live"), "{err}");
    let err = apply(
        add(),
        &[
            ("version", "1.1"),
            ("username_token", "true"),
            ("user_var", "a b"),
        ],
    )
    .unwrap_err();
    assert!(err.contains("variable name"), "{err}");
}

#[test]
fn mtom_attachments_are_listed_read_only() {
    let envelope = "<e:Envelope xmlns:e=\"http://schemas.xmlsoap.org/soap/envelope/\"><e:Body><r:Doc xmlns:r=\"urn:r\"/></e:Body></e:Envelope>";
    let raw = format!(
        "--bnd\r\nContent-Type: application/xop+xml\r\nContent-ID: <root>\r\n\r\n{envelope}\r\n--bnd\r\nContent-Type: application/pdf\r\nContent-ID: <doc1>\r\n\r\n%PDF-1.4 12345\r\n--bnd--\r\n"
    );
    let ct = "multipart/related; type=\"application/xop+xml\"; boundary=bnd";
    let response = PluginResponse {
        status: 200,
        headers: vec![("Content-Type".into(), ct.into())],
        url: "http://127.0.0.1/x".into(),
        body: PluginBody {
            content_type: Some(ct.into()),
            text: Some(raw),
            base64: None,
        },
        truncated: false,
    };
    let reply: ViewReply = testing::call(
        &Soap,
        &Call::RenderTab {
            tab: "soap-response".into(),
            target: target(),
            spec: Default::default(),
            response: Some(response),
            state: None,
            context: None,
        },
    )
    .unwrap();
    let table = reply.view.blocks.iter().find_map(|b| match b {
        Block::Table { rows, .. } => Some(rows.clone()),
        _ => None,
    });
    assert_eq!(
        table.unwrap(),
        vec![vec![
            "doc1".to_owned(),
            "application/pdf".into(),
            "14".into()
        ]]
    );
    assert!(
        reply
            .view
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Code { text, .. } if text.contains("<r:Doc")))
    );
}
