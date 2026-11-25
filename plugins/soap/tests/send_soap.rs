// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sending the imported requests to the local SOAP test service (port 48812): SOAP 1.1 and 1.2,
//! headers made to fit by `prepare`, faults as plain text, the response tab.

mod common;

use common::{body, import, read, request, send, start, testdata};
use rt_plugin_sdk::api::{Block, Call, TabTarget, ViewReply};
use rt_plugin_sdk::model::{Body, RawKind};
use rt_plugin_sdk::testing;
use rt_plugin_soap::Soap;

fn collection() -> rt_plugin_sdk::model::Collection {
    read(&import(
        "wsdl",
        vec![testdata("calculator.wsdl"), testdata("types.xsd")],
    ))
}

fn with_numbers(
    mut spec: rt_plugin_sdk::model::RequestSpec,
    a: i32,
    b: i32,
) -> rt_plugin_sdk::model::RequestSpec {
    let text = body(&spec)
        .replace("<tns:a>0</tns:a>", &format!("<tns:a>{a}</tns:a>"))
        .replace("<tns:b>0</tns:b>", &format!("<tns:b>{b}</tns:b>"));
    spec.body = Body::Raw {
        kind: RawKind::Xml,
        data: text,
    };
    spec
}

#[test]
fn sends_soap_1_1_and_1_2() {
    let log = start(48812);
    let c = collection();
    for (folder, ct_start) in [
        ("CalculatorSoap11", "text/xml"),
        ("CalculatorSoap12", "application/soap+xml"),
    ] {
        let sent = send(&with_numbers(request(&c, folder, "Add"), 2, 3), &[]);
        assert_eq!(sent.response.status, 200, "{folder}");
        assert!(common::body_of(&sent.response).contains("<c:result>5</c:result>"));
        assert!(sent.notes.notes.is_empty(), "{:?}", sent.notes);
        assert!(
            sent.prepare.notes.is_empty(),
            "the imported headers already fit"
        );
        let seen = log
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|s| {
                s.body.contains("<tns:a>2</tns:a>")
                    && s.path
                        == if folder.ends_with("11") {
                            "/soap11"
                        } else {
                            "/soap12"
                        }
            })
            .cloned()
            .expect("the service saw the request");
        assert!(seen.header("Content-Type").unwrap().starts_with(ct_start));
        if folder.ends_with("11") {
            assert_eq!(
                seen.header("SOAPAction"),
                Some("\"urn:requesttailor:test:calc/Add\"")
            );
        } else {
            assert!(
                seen.header("Content-Type")
                    .unwrap()
                    .contains("action=\"urn:requesttailor:test:calc/Add\"")
            );
        }
    }
}

#[test]
fn prepare_repairs_missing_headers() {
    start(48812);
    let c = collection();
    for folder in ["CalculatorSoap11", "CalculatorSoap12"] {
        let mut spec = with_numbers(request(&c, folder, "Add"), 1, 1);
        spec.headers.clear();
        let sent = send(&spec, &[]);
        assert_eq!(
            sent.response.status,
            200,
            "{folder}: {}",
            common::body_of(&sent.response)
        );
        assert!(!sent.prepare.notes.is_empty());
    }
}

#[test]
fn faults_become_plain_text() {
    start(48812);
    let c = collection();
    let s11 = send(&request(&c, "CalculatorSoap11", "Divide"), &[]);
    assert_eq!(s11.response.status, 500);
    let text = &s11.notes.notes[0].text;
    assert_eq!(
        text,
        "SOAP fault: env:Client: Division by zero (operand: 0)"
    );
    let s12 = send(&request(&c, "CalculatorSoap12", "Divide"), &[]);
    assert_eq!(
        s12.notes.notes[0].text,
        "SOAP fault: env:Sender: Division by zero (operand: 0)"
    );

    // The response tab says it too.
    let reply: ViewReply = testing::call(
        &Soap,
        &Call::RenderTab {
            tab: "soap-response".into(),
            target: TabTarget {
                request_path: "x.yml".into(),
                response_id: Some(1),
            },
            spec: Default::default(),
            response: Some(s11.response.clone()),
            state: None,
            context: None,
        },
    )
    .unwrap();
    let blocks = &reply.view.blocks;
    assert!(
        matches!(&blocks[0], Block::Notice { text, .. } if text.contains("Division by zero")),
        "{blocks:?}"
    );
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, Block::Code { text, .. } if text.contains("<env:Fault>")))
    );
    assert!(blocks.iter().any(|b| matches!(b, Block::KeyValue { rows } if rows.iter().any(|(k, v)| k == "Detail" && v.contains("operand: 0")))));
}
