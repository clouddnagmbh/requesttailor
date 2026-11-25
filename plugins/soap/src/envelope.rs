// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! SOAP 1.1 and 1.2 envelopes: writing one from sample nodes, reading one (header blocks,
//! body, faults), and the HTTP headers each version needs.

use crate::node::{self, Child, Node, Prefixes};
use crate::xml::{self, Element};

pub const ENV11: &str = "http://schemas.xmlsoap.org/soap/envelope/";
pub const ENV12: &str = "http://www.w3.org/2003/05/soap-envelope";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoapVersion {
    V11,
    V12,
}

impl SoapVersion {
    pub fn namespace(self) -> &'static str {
        match self {
            SoapVersion::V11 => ENV11,
            SoapVersion::V12 => ENV12,
        }
    }

    pub fn from_namespace(ns: &str) -> Option<Self> {
        match ns {
            ENV11 => Some(SoapVersion::V11),
            ENV12 => Some(SoapVersion::V12),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SoapVersion::V11 => "1.1",
            SoapVersion::V12 => "1.2",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label.trim().trim_start_matches("SOAP ") {
            "1.1" => Some(SoapVersion::V11),
            "1.2" => Some(SoapVersion::V12),
            _ => None,
        }
    }

    /// `Content-Type` for a request: 1.1 `text/xml`; 1.2 `application/soap+xml` with the
    /// action as a parameter.
    pub fn content_type(self, action: &str) -> String {
        match self {
            SoapVersion::V11 => "text/xml; charset=utf-8".into(),
            SoapVersion::V12 if action.is_empty() => "application/soap+xml; charset=utf-8".into(),
            SoapVersion::V12 => format!("application/soap+xml; charset=utf-8; action=\"{action}\""),
        }
    }

    /// SOAP 1.1 needs a `SOAPAction` header (possibly empty and quoted).
    pub fn action_header(self, action: &str) -> Option<String> {
        (self == SoapVersion::V11).then(|| format!("\"{action}\""))
    }
}

/// A message to write.
#[derive(Debug, Clone, Default)]
pub struct Message {
    /// Header blocks, as nodes, in order. Raw header text (the security header) goes in
    /// `raw_header` and is written first.
    pub header: Vec<Node>,
    pub raw_header: Vec<String>,
    pub body: Vec<Node>,
}

/// The envelope text. Namespaces of the content are declared on the `Envelope`.
pub fn render(version: SoapVersion, message: &Message) -> String {
    let mut prefixes = Prefixes::default();
    prefixes.bind(version.namespace(), "soapenv");
    prefixes.collect(&message.header);
    prefixes.collect(&message.body);
    let mut out = String::new();
    out.push_str("<soapenv:Envelope");
    for (prefix, ns) in prefixes.declarations() {
        out.push_str(&format!(" xmlns:{prefix}=\"{}\"", node::escape_attr(ns)));
    }
    out.push_str(">\n");
    if message.header.is_empty() && message.raw_header.is_empty() {
        out.push_str("  <soapenv:Header/>\n");
    } else {
        out.push_str("  <soapenv:Header>\n");
        for raw in &message.raw_header {
            for line in raw.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        for n in &message.header {
            node::render(n, 2, &prefixes, &mut out);
        }
        out.push_str("  </soapenv:Header>\n");
    }
    out.push_str("  <soapenv:Body>\n");
    for n in &message.body {
        node::render(n, 2, &prefixes, &mut out);
    }
    out.push_str("  </soapenv:Body>\n");
    out.push_str("</soapenv:Envelope>");
    out
}

/// A parsed envelope.
#[derive(Debug, Clone)]
pub struct Envelope {
    pub version: SoapVersion,
    pub header: Vec<Element>,
    pub body: Vec<Element>,
    pub fault: Option<Fault>,
}

/// A SOAP fault in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fault {
    /// `faultcode` / `Code/Value`, with subcodes appended (`Sender / m:NotFound`).
    pub code: String,
    pub reason: String,
    /// `faultactor` / `Role`.
    pub actor: Option<String>,
    /// The detail's text, one line per element with text.
    pub detail: Option<String>,
}

impl Fault {
    /// One sentence: `Client: Amount must be positive (amount: -5)`.
    pub fn sentence(&self) -> String {
        let mut s = format!("{}: {}", self.code, self.reason);
        if let Some(d) = &self.detail {
            s.push_str(&format!(" ({})", d.replace('\n', "; ")));
        }
        s
    }
}

/// Reads an envelope; `None` when the text is not XML or has another root.
pub fn parse(text: &str) -> Option<Envelope> {
    let root = xml::parse(text).ok()?;
    from_element(&root)
}

pub fn from_element(root: &Element) -> Option<Envelope> {
    if root.name != "Envelope" {
        return None;
    }
    let version = SoapVersion::from_namespace(&root.ns)?;
    let header = root
        .child("Header")
        .map(|h| h.children.clone())
        .unwrap_or_default();
    let body = root
        .child("Body")
        .map(|b| b.children.clone())
        .unwrap_or_default();
    let fault = body
        .iter()
        .find(|e| e.name == "Fault" && e.ns == version.namespace())
        .map(|f| read_fault(f, version));
    Some(Envelope {
        version,
        header,
        body,
        fault,
    })
}

fn text_of(e: Option<&Element>) -> String {
    e.map(|e| e.text.trim().to_owned()).unwrap_or_default()
}

fn read_fault(f: &Element, version: SoapVersion) -> Fault {
    match version {
        SoapVersion::V11 => Fault {
            code: text_of(f.child("faultcode")),
            reason: text_of(f.child("faultstring")),
            actor: f
                .child("faultactor")
                .map(|a| a.text.trim().to_owned())
                .filter(|s| !s.is_empty()),
            detail: f.child("detail").and_then(detail_text),
        },
        SoapVersion::V12 => {
            let mut code = Vec::new();
            let mut cur = f.child("Code");
            while let Some(c) = cur {
                let v = text_of(c.child("Value"));
                if !v.is_empty() {
                    code.push(v);
                }
                cur = c.child("Subcode");
            }
            let reason = f
                .child("Reason")
                .map(|r| {
                    r.children_named("Text")
                        .map(|t| t.text.trim().to_owned())
                        .collect::<Vec<_>>()
                        .join(" / ")
                })
                .unwrap_or_default();
            Fault {
                code: code.join(" / "),
                reason,
                actor: f
                    .child("Role")
                    .map(|a| a.text.trim().to_owned())
                    .filter(|s| !s.is_empty()),
                detail: f.child("Detail").and_then(detail_text),
            }
        }
    }
}

/// The text inside a detail element: `name: value` per element that has text.
fn detail_text(detail: &Element) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let own = detail.text.trim();
    if !own.is_empty() {
        lines.push(own.to_owned());
    }
    for e in detail.descendants() {
        let t = e.text.trim();
        if !t.is_empty() {
            lines.push(format!("{}: {t}", e.name));
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The first element of the body, if it is not a fault: what the call is called.
pub fn operation_name(envelope: &Envelope) -> Option<&str> {
    envelope
        .body
        .iter()
        .find(|e| e.name != "Fault")
        .map(|e| e.name.as_str())
}

/// The `Content-Type` base type (`text/xml`) without parameters, lowercase.
pub fn media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// A child node helper for samples.
pub fn comment(text: &str) -> Child {
    Child::Comment(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_an_envelope() {
        let mut body = Node::new("urn:x", "Add");
        body.children
            .push(Child::Node(Node::new("", "a").with_text("1 < 2")));
        let text = render(
            SoapVersion::V12,
            &Message {
                body: vec![body],
                ..Message::default()
            },
        );
        assert!(
            text.contains("xmlns:soapenv=\"http://www.w3.org/2003/05/soap-envelope\""),
            "{text}"
        );
        assert!(text.contains("<tns:Add>"));
        assert!(text.contains("<a>1 &lt; 2</a>"));
        let env = parse(&text).unwrap();
        assert_eq!(env.version, SoapVersion::V12);
        assert_eq!(operation_name(&env), Some("Add"));
        assert!(env.fault.is_none());
    }

    #[test]
    fn faults_of_both_versions() {
        let f11 = parse(
            r#"<e:Envelope xmlns:e="http://schemas.xmlsoap.org/soap/envelope/"><e:Body><e:Fault>
            <faultcode>e:Client</faultcode><faultstring>Bad amount</faultstring>
            <detail><amount>-5</amount></detail></e:Fault></e:Body></e:Envelope>"#,
        )
        .unwrap()
        .fault
        .unwrap();
        assert_eq!(f11.sentence(), "e:Client: Bad amount (amount: -5)");
        let f12 = parse(
            r#"<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"><e:Body><e:Fault>
            <e:Code><e:Value>e:Sender</e:Value><e:Subcode><e:Value>m:NotFound</e:Value></e:Subcode></e:Code>
            <e:Reason><e:Text xml:lang="en">Nothing</e:Text></e:Reason></e:Fault></e:Body></e:Envelope>"#,
        )
        .unwrap()
        .fault
        .unwrap();
        assert_eq!(f12.code, "e:Sender / m:NotFound");
        assert_eq!(f12.reason, "Nothing");
    }

    #[test]
    fn headers_per_version() {
        assert_eq!(
            SoapVersion::V11.action_header("urn:a"),
            Some("\"urn:a\"".into())
        );
        assert_eq!(SoapVersion::V12.action_header("urn:a"), None);
        assert_eq!(
            SoapVersion::V12.content_type("urn:a"),
            "application/soap+xml; charset=utf-8; action=\"urn:a\""
        );
        assert_eq!(media_type("Text/XML; charset=utf-8"), "text/xml");
    }
}
