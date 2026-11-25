// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The `wsdl` importer: detection, one request per operation and binding with the envelope of
//! the binding's SOAP version, sample values from the schema, the loss report, WS-Security
//! placeholders.

mod common;

use common::{PASSWORD, body, header, import, read, request, testdata};
use rt_plugin_sdk::api::{Call, DetectReply};
use rt_plugin_sdk::testing;
use rt_plugin_sdk::{LossEffect, LossKind};
use rt_plugin_soap::Soap;

fn detect(files: Vec<rt_plugin_sdk::api::PluginFile>) -> bool {
    let reply: DetectReply = testing::call(
        &Soap,
        &Call::Detect {
            importer: "wsdl".into(),
            files,
        },
    )
    .unwrap();
    reply.matches
}

fn calculator() -> rt_plugin_sdk::api::ImportReply {
    import(
        "wsdl",
        vec![testdata("calculator.wsdl"), testdata("types.xsd")],
    )
}

#[test]
fn detects_wsdl_only() {
    assert!(detect(vec![testdata("calculator.wsdl")]));
    assert!(detect(vec![testdata("weather20.wsdl")]));
    assert!(!detect(vec![testdata("types.xsd")]));
    assert!(!detect(vec![rt_plugin_sdk::text_file("a.xml", "<a/>")]));
    assert!(!detect(vec![rt_plugin_sdk::text_file("a.wsdl", "not xml")]));
}

#[test]
fn one_request_per_operation_and_soap_binding() {
    let reply = calculator();
    let collection = read(&reply);
    assert_eq!(reply.report.folders, 2);
    assert_eq!(reply.report.requests, 5 + 2);
    assert_eq!(collection.name, "Calculator");

    let add11 = request(&collection, "CalculatorSoap11", "Add");
    assert_eq!(add11.method, "POST");
    assert_eq!(add11.url, "http://127.0.0.1:48812/soap11");
    assert_eq!(
        header(&add11, "Content-Type"),
        Some("text/xml; charset=utf-8")
    );
    assert_eq!(
        header(&add11, "SOAPAction"),
        Some("\"urn:requesttailor:test:calc/Add\"")
    );
    let text = body(&add11);
    assert!(
        text.contains("xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\""),
        "{text}"
    );
    assert!(text.contains("xmlns:tns=\"urn:requesttailor:test:calc\""));
    assert!(
        text.contains("<tns:a>0</tns:a>") && text.contains("<tns:b>0</tns:b>"),
        "{text}"
    );
    assert!(text.contains("<soapenv:Header/>"));

    let add12 = request(&collection, "CalculatorSoap12", "Add");
    assert_eq!(add12.url, "http://127.0.0.1:48812/soap12");
    assert_eq!(
        header(&add12, "Content-Type"),
        Some("application/soap+xml; charset=utf-8; action=\"urn:requesttailor:test:calc/Add\"")
    );
    assert_eq!(header(&add12, "SOAPAction"), None);
    assert!(body(&add12).contains("xmlns:soapenv=\"http://www.w3.org/2003/05/soap-envelope\""));
}

#[test]
fn samples_follow_the_schema() {
    let collection = read(&calculator());
    let quote = request(&collection, "CalculatorSoap11", "Quote");
    let text = body(&quote);
    // Header part, attribute and value of a simple-content type, enumeration (the attribute),
    // optional and repeated elements as comments, the first choice alternative, a cut recursion.
    assert!(text.contains("<tns:TraceId>string</tns:TraceId>"), "{text}");
    assert!(
        text.contains("<tns:price currency=\"EUR\">0.0</tns:price>"),
        "{text}"
    );
    assert!(text.contains("<!--Optional:-->"));
    assert!(text.contains("<!--Zero or more repetitions:-->"));
    assert!(text.contains("<!--You have a CHOICE of the next 2 items at this level-->"));
    assert!(text.contains("<tns:coupon>string</tns:coupon>") && !text.contains("voucher"));
    assert!(text.contains(":label>string</"));
    assert!(
        text.contains("<!--recursive type Node omitted-->"),
        "{text}"
    );
    assert!(text.contains("<!--any element is allowed here-->"));
    // The Money attribute `note` is optional: left out.
    assert!(!text.contains("note="));
    // The reply is well-formed XML.
    assert!(rt_plugin_soap::xml::parse(text).is_ok());
}

#[test]
fn rpc_style_wraps_the_parts_in_the_operation_element() {
    let collection = read(&calculator());
    let echo = request(&collection, "CalculatorSoap11", "Echo");
    let text = body(&echo);
    assert!(
        text.contains("xmlns:tns=\"urn:requesttailor:test:echo\""),
        "{text}"
    );
    assert!(text.contains("<tns:Echo>"));
    assert!(
        text.contains("<text>string</text>") && text.contains("<times>0</times>"),
        "{text}"
    );
}

#[test]
fn the_loss_report_names_what_was_left_out() {
    let reply = calculator();
    let losses = &reply.report.losses;
    let find = |needle: &str| {
        losses
            .iter()
            .find(|l| l.detail.contains(needle) || l.location.contains(needle))
            .unwrap_or_else(|| panic!("no loss mentioning {needle}: {losses:#?}"))
    };
    let http = find("CalculatorHttp");
    assert_eq!(http.effect, LossEffect::Dropped);
    assert_eq!(find("use=\"encoded\"").effect, LossEffect::Approximated);
    assert_eq!(find("one-way").kind, LossKind::Protocol);
    assert_eq!(find("recursive").kind, LossKind::Body);
    assert_eq!(find("xs:any").effect, LossEffect::Approximated);
    // Sorted and without repeats.
    let mut sorted = losses.clone();
    sorted.sort_by(|a, b| (&a.location, a.kind).cmp(&(&b.location, b.kind)));
    sorted.dedup();
    assert_eq!(&sorted, losses);
}

#[test]
fn a_missing_schema_file_is_reported_not_fatal() {
    let reply = import("wsdl", vec![testdata("calculator.wsdl")]);
    assert_eq!(reply.report.requests, 7);
    let losses = &reply.report.losses;
    assert!(
        losses
            .iter()
            .any(|l| l.location == "types.xsd" && l.effect == LossEffect::Dropped),
        "{losses:#?}"
    );
    assert!(
        losses
            .iter()
            .any(|l| l.detail.contains("not defined in any schema"))
    );
}

#[test]
fn wsdl_2_0_gives_both_versions() {
    let reply = import("wsdl", vec![testdata("weather20.wsdl")]);
    let collection = read(&reply);
    let v12 = request(&collection, "WeatherSoap12", "Forecast");
    assert_eq!(v12.url, "http://127.0.0.1:48812/soap12");
    assert!(
        body(&v12).contains("<tns:unit>celsius</tns:unit>"),
        "{}",
        body(&v12)
    );
    assert!(body(&v12).contains("http://www.w3.org/2003/05/soap-envelope"));
    let v11 = request(&collection, "WeatherSoap11", "Forecast");
    assert_eq!(v11.url, "{{baseUrl}}", "no endpoint for this binding");
    assert_eq!(
        header(&v11, "SOAPAction"),
        Some("\"urn:requesttailor:test:weather/Forecast\"")
    );
    assert!(
        reply
            .files
            .iter()
            .any(|(p, t)| p == "environments/Default.yml" && t.contains("baseUrl"))
    );
    assert!(
        reply
            .report
            .losses
            .iter()
            .any(|l| l.location == "binding WeatherSoap11")
    );
}

#[test]
fn username_token_policy_becomes_placeholders_and_a_secret_without_value() {
    let reply = import("wsdl", vec![testdata("secure.wsdl")]);
    let collection = read(&reply);
    let req = request(&collection, "VaultSoap", "GetSecret");
    let text = body(&req);
    assert!(
        text.contains("<wsse:Username>{{wsse_username}}</wsse:Username>"),
        "{text}"
    );
    assert!(text.contains("<wsse:Password Type="), "{text}");
    assert!(text.contains(">{{wsse_password}}</wsse:Password>"));
    assert!(
        text.contains("<wsu:Timestamp"),
        "IncludeTimestamp asks for a timestamp"
    );
    let env = reply
        .files
        .iter()
        .find(|(p, _)| p == "environments/Default.yml")
        .map(|(_, t)| t.clone())
        .expect("an environment for the credentials");
    assert!(
        env.contains("name: wsse_password\n    secret: true"),
        "{env}"
    );
    assert!(!env.contains("value: s"), "no value for the secret");
    for (_, content) in &reply.files {
        assert!(!content.contains(PASSWORD));
    }
    assert!(
        reply
            .report
            .losses
            .iter()
            .any(|l| l.kind == LossKind::Auth && l.location == "WS-Policy")
    );
}

#[test]
fn no_wsdl_is_an_error() {
    let err = testing::call::<rt_plugin_sdk::api::ImportReply>(
        &Soap,
        &Call::Import {
            importer: "wsdl".into(),
            files: vec![testdata("types.xsd")],
        },
    )
    .unwrap_err();
    assert!(err.contains("no WSDL"), "{err}");
}
