// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! WSDL 1.1 and 2.0 into a small model: per binding the SOAP version, the endpoint and the
//! operations with what goes into the body and the headers. What the model cannot carry is
//! collected as [`Note`]s for the loss report.

use crate::envelope::SoapVersion;
use crate::schema::Schema;
use crate::xml::Element;

pub const WSDL11_NS: &str = "http://schemas.xmlsoap.org/wsdl/";
pub const WSDL11_SOAP11: &str = "http://schemas.xmlsoap.org/wsdl/soap/";
pub const WSDL11_SOAP12: &str = "http://schemas.xmlsoap.org/wsdl/soap12/";
pub const WSDL20_NS: &str = "http://www.w3.org/ns/wsdl";
pub const WSDL20_SOAP: &str = "http://www.w3.org/ns/wsdl/soap";
const MIME_NS: &str = "http://schemas.xmlsoap.org/wsdl/mime/";
const HTTP_NS: &str = "http://schemas.xmlsoap.org/wsdl/http/";
const POLICY_NS: [&str; 2] = [
    "http://schemas.xmlsoap.org/ws/2004/09/policy",
    "http://www.w3.org/ns/ws-policy",
];

/// How much a [`Note`] costs the imported request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Dropped,
    Approximated,
}

/// Something the model leaves out or changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub location: String,
    pub effect: Effect,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Document,
    Rpc,
}

/// What a message part stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// A global element `{ns}name`.
    Element { ns: String, name: String },
    /// An RPC part `name` of type `{ns}type_name`.
    Typed {
        name: String,
        ns: String,
        type_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub name: String,
    pub soap_action: String,
    pub style: Style,
    /// RPC: the namespace of the wrapper element.
    pub rpc_ns: String,
    pub input: Vec<Part>,
    pub headers: Vec<Part>,
    pub one_way: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub version: SoapVersion,
    /// `soap:address`/`endpoint address`, if a service refers to the binding.
    pub endpoint: Option<String>,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Default)]
pub struct Wsdl {
    pub name: String,
    pub target_ns: String,
    pub bindings: Vec<Binding>,
    pub schema: Schema,
    /// A `UsernameToken` assertion was found in a policy.
    pub username_token: bool,
    pub notes: Vec<Note>,
}

/// Whether the text is a WSDL document (root `definitions` or `description`).
pub fn is_wsdl(root: &Element) -> bool {
    (root.ns == WSDL11_NS && root.name == "definitions")
        || (root.ns == WSDL20_NS && root.name == "description")
}

/// Whether the text is an XML Schema document.
pub fn is_schema(root: &Element) -> bool {
    root.ns == crate::schema::XSD_NS && root.name == "schema"
}

/// Reads the WSDL `root` and the schemas of `others` (XSD files and further WSDLs).
pub fn parse(root: &Element, others: &[(String, Element)]) -> Result<Wsdl, String> {
    let mut wsdl = Wsdl {
        target_ns: root.attr("targetNamespace").unwrap_or("").to_owned(),
        name: root.attr("name").unwrap_or("").to_owned(),
        ..Wsdl::default()
    };
    for (_, other) in others {
        if is_schema(other) {
            wsdl.schema.add(other);
        } else if is_wsdl(other) {
            for types in other.children_named("types") {
                for s in types.children_named("schema") {
                    wsdl.schema.add(s);
                }
            }
        }
    }
    for types in root.children_named("types") {
        for s in types.children_named("schema") {
            wsdl.schema.add(s);
        }
    }
    let known: Vec<&str> = others.iter().map(|(n, _)| n.as_str()).collect();
    let mut refs: Vec<String> = wsdl.schema.references.clone();
    for imp in root
        .children_named("import")
        .chain(root.children_named("include"))
    {
        if let Some(loc) = imp.attr("location") {
            refs.push(loc.to_owned());
        }
    }
    for r in refs {
        let file = r.rsplit('/').next().unwrap_or(&r);
        if !known
            .iter()
            .any(|k| k.rsplit('/').next().unwrap_or(k) == file)
        {
            wsdl.notes.push(Note {
                location: r.clone(),
                effect: Effect::Dropped,
                detail:
                    "imported file was not among the files; its types are missing from the samples"
                        .into(),
            });
        }
    }
    if root.ns == WSDL11_NS {
        wsdl11(root, &mut wsdl);
    } else {
        wsdl20(root, &mut wsdl);
    }
    wsdl.username_token = root
        .descendants()
        .iter()
        .any(|e| e.name == "UsernameToken" && !e.ns.is_empty())
        || wsdl.username_token;
    let policy = root
        .descendants()
        .iter()
        .filter(|e| e.name == "Policy" && POLICY_NS.contains(&e.ns.as_str()))
        .count();
    if policy > 0 {
        wsdl.notes.push(Note {
            location: "WS-Policy".into(),
            effect: Effect::Approximated,
            detail: if wsdl.username_token {
                "policies are not applied; only a UsernameToken assertion was recognized (credentials as placeholders)".into()
            } else {
                "policies are not applied (no UsernameToken assertion recognized)".into()
            },
        });
    }
    if wsdl.bindings.is_empty() {
        wsdl.notes.push(Note {
            location: "bindings".into(),
            effect: Effect::Dropped,
            detail: "no SOAP binding found".into(),
        });
    }
    Ok(wsdl)
}

fn qname(e: &Element, value: &str) -> Option<(String, String)> {
    e.resolve_qname(value).map(|(ns, l)| (ns, l.to_owned()))
}

fn wsdl11(root: &Element, wsdl: &mut Wsdl) {
    let tns = wsdl.target_ns.clone();
    let messages: Vec<&Element> = root.children_named("message").collect();
    let port_types: Vec<&Element> = root.children_named("portType").collect();
    for binding in root.children_named("binding") {
        let name = binding.attr("name").unwrap_or("").to_owned();
        let soap = binding
            .children
            .iter()
            .find(|c| c.name == "binding" && (c.ns == WSDL11_SOAP11 || c.ns == WSDL11_SOAP12));
        let Some(soap) = soap else {
            let what = binding
                .children
                .iter()
                .find(|c| c.name == "binding")
                .map(|c| {
                    if c.ns == HTTP_NS {
                        "HTTP GET/POST binding"
                    } else {
                        "non-SOAP binding"
                    }
                })
                .unwrap_or("binding without a SOAP extension");
            wsdl.notes.push(Note {
                location: format!("binding {name}"),
                effect: Effect::Dropped,
                detail: format!("{what} is not imported"),
            });
            continue;
        };
        let version = if soap.ns == WSDL11_SOAP12 {
            SoapVersion::V12
        } else {
            SoapVersion::V11
        };
        if let Some(t) = soap.attr("transport") {
            if !t.ends_with("/http") {
                wsdl.notes.push(Note {
                    location: format!("binding {name}"),
                    effect: Effect::Dropped,
                    detail: format!("transport {t} is not HTTP"),
                });
                continue;
            }
        }
        let default_style = if soap.attr("style") == Some("rpc") {
            Style::Rpc
        } else {
            Style::Document
        };
        let port_type = binding
            .attr("type")
            .and_then(|t| qname(binding, t))
            .and_then(|(_, l)| {
                port_types
                    .iter()
                    .find(|p| p.attr("name") == Some(l.as_str()))
            });
        let mut operations = Vec::new();
        for bop in binding.children_named("operation") {
            let op_name = bop.attr("name").unwrap_or("").to_owned();
            let sop = bop.children.iter().find(|c| {
                c.name == "operation" && (c.ns == WSDL11_SOAP11 || c.ns == WSDL11_SOAP12)
            });
            let style = match sop.and_then(|s| s.attr("style")) {
                Some("rpc") => Style::Rpc,
                Some(_) => Style::Document,
                None => default_style,
            };
            let soap_action = sop
                .and_then(|s| s.attr("soapAction"))
                .unwrap_or("")
                .to_owned();
            let input = bop.child("input");
            let body = input.and_then(|i| i.children.iter().find(|c| c.name == "body"));
            let rpc_ns = body
                .and_then(|b| b.attr("namespace"))
                .unwrap_or(&tns)
                .to_owned();
            if body.is_some_and(|b| b.attr("use") == Some("encoded")) {
                wsdl.notes.push(Note {
                    location: format!("{name}/{op_name}"),
                    effect: Effect::Approximated,
                    detail: "use=\"encoded\" is written as literal (no SOAP encoding attributes)"
                        .into(),
                });
            }
            if input.is_some_and(|i| i.children.iter().any(|c| c.ns == MIME_NS)) {
                wsdl.notes.push(Note {
                    location: format!("{name}/{op_name}"),
                    effect: Effect::Approximated,
                    detail: "MIME/MTOM parts of the input are not imported; the request is sent as a plain envelope".into(),
                });
            }
            let pt_op = port_type.and_then(|p| {
                p.children_named("operation")
                    .find(|o| o.attr("name") == Some(op_name.as_str()))
            });
            let Some(pt_op) = pt_op else {
                wsdl.notes.push(Note {
                    location: format!("{name}/{op_name}"),
                    effect: Effect::Dropped,
                    detail: "the operation is not in the port type".into(),
                });
                continue;
            };
            let pt_input = pt_op.child("input");
            let message = pt_input
                .and_then(|i| i.attr("message"))
                .and_then(|m| qname(pt_op, m))
                .and_then(|(_, l)| messages.iter().find(|m| m.attr("name") == Some(l.as_str())));
            let only: Option<Vec<&str>> = body
                .and_then(|b| b.attr("parts"))
                .map(|p| p.split_whitespace().collect());
            let mut parts = Vec::new();
            if let Some(m) = message {
                for part in m.children_named("part") {
                    let pname = part.attr("name").unwrap_or("");
                    if let Some(only) = &only {
                        if !only.contains(&pname) {
                            continue;
                        }
                    }
                    if let Some(p) = part_of(part, pname) {
                        parts.push(p);
                    }
                }
            } else if pt_input.is_some() {
                wsdl.notes.push(Note {
                    location: format!("{name}/{op_name}"),
                    effect: Effect::Approximated,
                    detail: "the input message was not found; the body is empty".into(),
                });
            }
            let mut headers = Vec::new();
            if let Some(i) = input {
                for h in i.children.iter().filter(|c| c.name == "header") {
                    let hm = h
                        .attr("message")
                        .and_then(|m| qname(h, m))
                        .and_then(|(_, l)| {
                            messages.iter().find(|m| m.attr("name") == Some(l.as_str()))
                        });
                    let hp = h.attr("part").unwrap_or("");
                    if let Some(part) = hm.and_then(|m| {
                        m.children_named("part")
                            .find(|p| p.attr("name") == Some(hp))
                    }) {
                        if let Some(p) = part_of(part, hp) {
                            headers.push(p);
                        }
                    }
                }
            }
            let one_way = pt_op.child("output").is_none();
            operations.push(Operation {
                name: op_name,
                soap_action,
                style,
                rpc_ns,
                input: parts,
                headers,
                one_way,
            });
        }
        wsdl.bindings.push(Binding {
            name,
            version,
            endpoint: None,
            operations,
        });
    }
    for service in root.children_named("service") {
        for port in service.children_named("port") {
            let Some(binding_name) = port
                .attr("binding")
                .and_then(|b| qname(port, b))
                .map(|(_, l)| l)
            else {
                continue;
            };
            let address = port
                .children
                .iter()
                .find(|c| c.name == "address" && (c.ns == WSDL11_SOAP11 || c.ns == WSDL11_SOAP12))
                .and_then(|a| a.attr("location"));
            if let (Some(b), Some(a)) = (
                wsdl.bindings.iter_mut().find(|b| b.name == binding_name),
                address,
            ) {
                if b.endpoint.is_none() {
                    b.endpoint = Some(a.to_owned());
                }
            }
        }
    }
}

fn part_of(part: &Element, name: &str) -> Option<Part> {
    if let Some(e) = part.attr("element") {
        let (ns, local) = qname(part, e)?;
        return Some(Part::Element { ns, name: local });
    }
    let t = part.attr("type")?;
    let (ns, local) = qname(part, t)?;
    Some(Part::Typed {
        name: name.to_owned(),
        ns,
        type_name: local,
    })
}

fn wsdl20(root: &Element, wsdl: &mut Wsdl) {
    let interfaces: Vec<&Element> = root.children_named("interface").collect();
    for binding in root.children_named("binding") {
        let name = binding.attr("name").unwrap_or("").to_owned();
        if binding.attr("type") != Some(WSDL20_SOAP) {
            wsdl.notes.push(Note {
                location: format!("binding {name}"),
                effect: Effect::Dropped,
                detail: "not a SOAP binding".into(),
            });
            continue;
        }
        let wsoap = |e: &Element, attr: &str| -> Option<String> {
            e.attrs
                .iter()
                .find(|a| a.name == attr && a.ns == WSDL20_SOAP)
                .map(|a| a.value.clone())
        };
        let version = match wsoap(binding, "version").as_deref() {
            Some("1.1") => SoapVersion::V11,
            _ => SoapVersion::V12,
        };
        if let Some(p) = wsoap(binding, "protocol") {
            if !p.contains("HTTP") {
                wsdl.notes.push(Note {
                    location: format!("binding {name}"),
                    effect: Effect::Dropped,
                    detail: format!("protocol {p} is not HTTP"),
                });
                continue;
            }
        }
        let interface = binding
            .attr("interface")
            .and_then(|i| qname(binding, i))
            .and_then(|(_, l)| {
                interfaces
                    .iter()
                    .find(|i| i.attr("name") == Some(l.as_str()))
            });
        let mut operations = Vec::new();
        if let Some(interface) = interface {
            for op in interface.children_named("operation") {
                let op_name = op.attr("name").unwrap_or("").to_owned();
                let bop = binding.children_named("operation").find(|b| {
                    b.attr("ref")
                        .and_then(|r| qname(b, r))
                        .is_some_and(|(_, l)| l == op_name)
                });
                let soap_action = bop.and_then(|b| wsoap(b, "action")).unwrap_or_default();
                let input = op.child("input");
                let mut parts = Vec::new();
                match input.and_then(|i| i.attr("element")) {
                    Some("#none") | None => {}
                    Some(e @ ("#any" | "#other")) => wsdl.notes.push(Note {
                        location: format!("{name}/{op_name}"),
                        effect: Effect::Approximated,
                        detail: format!("input element {e} has no sample; the body is empty"),
                    }),
                    Some(e) => {
                        if let Some((ns, local)) = input.and_then(|i| qname(i, e)) {
                            parts.push(Part::Element { ns, name: local });
                        }
                    }
                }
                if op.attr("style").is_some_and(|s| s.contains("rpc")) {
                    wsdl.notes.push(Note {
                        location: format!("{name}/{op_name}"),
                        effect: Effect::Approximated,
                        detail: "RPC style (wrpc:signature) is written as a document body".into(),
                    });
                }
                operations.push(Operation {
                    name: op_name,
                    soap_action,
                    style: Style::Document,
                    rpc_ns: String::new(),
                    input: parts,
                    headers: Vec::new(),
                    one_way: op.child("output").is_none(),
                });
            }
        } else {
            wsdl.notes.push(Note {
                location: format!("binding {name}"),
                effect: Effect::Dropped,
                detail: "the interface of the binding was not found".into(),
            });
        }
        wsdl.bindings.push(Binding {
            name,
            version,
            endpoint: None,
            operations,
        });
    }
    for service in root.children_named("service") {
        for ep in service.children_named("endpoint") {
            let Some(binding_name) = ep
                .attr("binding")
                .and_then(|b| qname(ep, b))
                .map(|(_, l)| l)
            else {
                continue;
            };
            if let (Some(b), Some(a)) = (
                wsdl.bindings.iter_mut().find(|b| b.name == binding_name),
                ep.attr("address"),
            ) {
                if b.endpoint.is_none() {
                    b.endpoint = Some(a.to_owned());
                }
            }
        }
    }
}
