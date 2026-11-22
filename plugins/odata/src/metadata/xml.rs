// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A small, non-validating XML reader for EDMX documents: enough XML 1.0 for `$metadata`,
//! without a dependency (the plugin builds for wasm32 and must stay small).
//!
//! It builds a tree of [`Element`]s with resolved namespaces (`xmlns`, `xmlns:p`), decodes the
//! predefined and numeric entities, keeps text and CDATA, and skips the XML declaration,
//! processing instructions, comments and a DOCTYPE (its internal subset is not interpreted,
//! so entities declared there are not expanded).

/// The namespace of the `xml:` prefix (e.g. `xml:lang`).
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Nesting deeper than this is refused (EDMX nests about ten levels).
const MAX_DEPTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Element {
    /// Namespace URI, `""` for none.
    pub ns: String,
    /// Local name (without prefix).
    pub name: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Element>,
    /// Text and CDATA directly inside this element, concatenated.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    /// Namespace URI; unprefixed attributes have none (`""`), as XML namespaces define.
    pub ns: String,
    pub name: String,
    pub value: String,
}

impl Element {
    /// The value of the unprefixed attribute `name`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attr_ns("", name)
    }

    /// The value of attribute `name` in namespace `ns`.
    pub fn attr_ns(&self, ns: &str, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.ns == ns && a.name == name)
            .map(|a| a.value.as_str())
    }

    /// Child elements with local name `name` (any namespace).
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The first child element with local name `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Every element below this one (depth first, pre-order), this one excluded.
    pub fn descendants(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        let mut stack: Vec<&Element> = self.children.iter().rev().collect();
        while let Some(e) = stack.pop() {
            out.push(e);
            stack.extend(e.children.iter().rev());
        }
        out
    }
}

/// Parses a document and returns its root element.
pub fn parse(text: &str) -> Result<Element, String> {
    Parser {
        src: text.strip_prefix('\u{feff}').unwrap_or(text),
        pos: 0,
    }
    .document()
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

/// An element being read: the element so far and the namespace declarations it made.
struct Open {
    element: Element,
    qname: String,
    scope: Vec<(String, String)>,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn error(&self, message: impl std::fmt::Display) -> String {
        let line = self.src[..self.pos].matches('\n').count() + 1;
        format!("XML line {line}: {message}")
    }

    /// Moves past `end`, or fails with `what`.
    fn skip_past(&mut self, end: &str, what: &str) -> Result<&'a str, String> {
        match self.rest().find(end) {
            Some(i) => {
                let skipped = &self.rest()[..i];
                self.pos += i + end.len();
                Ok(skipped)
            }
            None => Err(self.error(format!("unterminated {what}"))),
        }
    }

    fn skip_whitespace(&mut self) {
        let rest = self.rest();
        self.pos += rest.len() - rest.trim_start().len();
    }

    fn document(mut self) -> Result<Element, String> {
        let mut stack: Vec<Open> = Vec::new();
        let mut root: Option<Element> = None;
        while self.pos < self.src.len() {
            let rest = self.rest();
            if !rest.starts_with('<') {
                let end = rest.find('<').unwrap_or(rest.len());
                let raw = &rest[..end];
                match stack.last_mut() {
                    Some(open) => open.element.text.push_str(&decode(raw)),
                    None if raw.trim().is_empty() => {}
                    None => return Err(self.error("text outside the root element")),
                }
                self.pos += end;
            } else if rest.starts_with("<?") {
                self.skip_past("?>", "processing instruction")?;
            } else if rest.starts_with("<!--") {
                self.skip_past("-->", "comment")?;
            } else if rest.starts_with("<![CDATA[") {
                self.pos += "<![CDATA[".len();
                let data = self.skip_past("]]>", "CDATA section")?;
                match stack.last_mut() {
                    Some(open) => open.element.text.push_str(data),
                    None => return Err(self.error("CDATA outside the root element")),
                }
            } else if rest.starts_with("<!") {
                self.doctype()?;
            } else if rest.starts_with("</") {
                self.pos += 2;
                let name = self.skip_past(">", "end tag")?.trim();
                let Some(open) = stack.pop() else {
                    return Err(self.error(format!("unexpected </{name}>")));
                };
                if open.qname != name {
                    return Err(self.error(format!("</{name}> does not close <{}>", open.qname)));
                }
                match stack.last_mut() {
                    Some(parent) => parent.element.children.push(open.element),
                    None => root = Some(open.element),
                }
            } else {
                if root.is_some() {
                    return Err(self.error("a second root element"));
                }
                if stack.len() >= MAX_DEPTH {
                    return Err(self.error("elements nested too deeply"));
                }
                let (open, empty) = self.start_tag(&stack)?;
                if empty {
                    match stack.last_mut() {
                        Some(parent) => parent.element.children.push(open.element),
                        None => root = Some(open.element),
                    }
                } else {
                    stack.push(open);
                }
            }
        }
        if let Some(open) = stack.last() {
            return Err(self.error(format!("<{}> is not closed", open.qname)));
        }
        root.ok_or_else(|| self.error("no root element"))
    }

    /// `<!DOCTYPE ...>`, with an internal subset in brackets.
    fn doctype(&mut self) -> Result<(), String> {
        let mut depth = 0usize;
        let mut quote: Option<char> = None;
        for (i, c) in self.rest().char_indices() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '[') => depth += 1,
                (None, ']') => depth = depth.saturating_sub(1),
                (None, '>') if depth == 0 => {
                    self.pos += i + 1;
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(self.error("unterminated declaration"))
    }

    /// Reads `<name attr="value" ...>` or `.../>`; returns the element and whether it was
    /// empty (`/>`).
    fn start_tag(&mut self, stack: &[Open]) -> Result<(Open, bool), String> {
        self.pos += 1;
        let qname = self.name()?;
        let mut raw_attrs: Vec<(String, String)> = Vec::new();
        let empty = loop {
            self.skip_whitespace();
            let rest = self.rest();
            if rest.starts_with("/>") {
                self.pos += 2;
                break true;
            }
            if rest.starts_with('>') {
                self.pos += 1;
                break false;
            }
            if rest.is_empty() {
                return Err(self.error(format!("unterminated <{qname}>")));
            }
            let name = self.name()?;
            self.skip_whitespace();
            if !self.rest().starts_with('=') {
                return Err(self.error(format!("attribute {name} without a value")));
            }
            self.pos += 1;
            self.skip_whitespace();
            let quote = match self.rest().chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => return Err(self.error(format!("attribute {name}: value not quoted"))),
            };
            self.pos += 1;
            let value = self.skip_past(&quote.to_string(), "attribute value")?;
            if value.contains('<') {
                return Err(self.error(format!("attribute {name}: `<` in value")));
            }
            if raw_attrs.iter().any(|(n, _)| *n == name) {
                return Err(self.error(format!("attribute {name} twice")));
            }
            raw_attrs.push((name, decode(value)));
        };

        let mut scope = Vec::new();
        for (name, value) in &raw_attrs {
            if name == "xmlns" {
                scope.push((String::new(), value.clone()));
            } else if let Some(prefix) = name.strip_prefix("xmlns:") {
                scope.push((prefix.to_owned(), value.clone()));
            }
        }
        let lookup = |prefix: &str| -> Option<String> {
            if prefix == "xml" {
                return Some(XML_NS.to_owned());
            }
            scope
                .iter()
                .chain(stack.iter().rev().flat_map(|o| o.scope.iter()))
                .find(|(p, _)| p == prefix)
                .map(|(_, uri)| uri.clone())
        };
        let (prefix, local) = split_qname(&qname);
        let ns = match prefix {
            Some(p) => lookup(p).ok_or_else(|| self.error(format!("undeclared prefix {p}:")))?,
            None => lookup("").unwrap_or_default(),
        };
        let mut attrs = Vec::new();
        for (name, value) in raw_attrs {
            if name == "xmlns" || name.starts_with("xmlns:") {
                continue;
            }
            let (prefix, local) = split_qname(&name);
            let ns = match prefix {
                Some(p) => {
                    lookup(p).ok_or_else(|| self.error(format!("undeclared prefix {p}:")))?
                }
                None => String::new(),
            };
            attrs.push(Attr {
                ns,
                name: local.to_owned(),
                value,
            });
        }
        let element = Element {
            ns,
            name: local.to_owned(),
            attrs,
            children: Vec::new(),
            text: String::new(),
        };
        Ok((
            Open {
                element,
                qname,
                scope,
            },
            empty,
        ))
    }

    fn name(&mut self) -> Result<String, String> {
        let rest = self.rest();
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '>' | '=' | '<'))
            .unwrap_or(rest.len());
        if end == 0 {
            return Err(self.error("a name expected"));
        }
        self.pos += end;
        Ok(rest[..end].to_owned())
    }
}

fn split_qname(qname: &str) -> (Option<&str>, &str) {
    match qname.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, qname),
    }
}

/// Decodes `&lt;` `&gt;` `&amp;` `&quot;` `&apos;` and `&#..;`/`&#x..;`. Anything else
/// (an unknown entity, a lone `&`) is kept as written.
pub fn decode(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let entity = rest[1..].find(';').map(|end| &rest[1..=end]);
        let decoded = entity.and_then(|e| match e {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e => {
                let code = if let Some(hex) = e.strip_prefix("#x").or(e.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    e.strip_prefix('#').and_then(|d| d.parse().ok())
                };
                code.and_then(char::from_u32)
            }
        });
        match (entity, decoded) {
            (Some(e), Some(c)) => {
                out.push(c);
                rest = &rest[e.len() + 2..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_entities_and_skipped_parts() {
        let doc = parse(
            "\u{feff}<?xml version=\"1.0\"?>\n<!DOCTYPE x [<!ENTITY a \"b>\">]>\n<!-- c -->\
             <a:Root xmlns:a=\"urn:a\" xmlns=\"urn:d\" xml:lang='en'>\
               <Child x=\"1 &lt; 2 &amp;&#65;&#x42;\" a:y='q\"'/>\
               <!-- <Not/> -->\
               <Other>t &amp; <![CDATA[<raw>]]></Other>\
               <a:Empty></a:Empty>\
             </a:Root>",
        )
        .unwrap();
        assert_eq!((doc.ns.as_str(), doc.name.as_str()), ("urn:a", "Root"));
        assert_eq!(doc.attr_ns(XML_NS, "lang"), Some("en"));
        assert_eq!(doc.children.len(), 3);
        let child = &doc.children[0];
        assert_eq!(child.ns, "urn:d");
        assert_eq!(child.attr("x"), Some("1 < 2 &AB"));
        assert_eq!(child.attr_ns("urn:a", "y"), Some("q\""));
        assert_eq!(child.attr("y"), None);
        assert_eq!(doc.children[1].text, "t & <raw>");
        assert_eq!(doc.children[2].ns, "urn:a");
        assert_eq!(doc.descendants().len(), 3);
    }

    #[test]
    fn broken_documents_are_errors() {
        assert!(parse("<a><b></a>").unwrap_err().contains("does not close"));
        assert!(parse("<a>").unwrap_err().contains("not closed"));
        assert!(parse("<a/><b/>").unwrap_err().contains("second root"));
        assert!(parse("<p:a/>").unwrap_err().contains("undeclared prefix"));
        assert!(parse("<a x=1/>").unwrap_err().contains("not quoted"));
        assert!(parse("<a x='1' x='2'/>").unwrap_err().contains("twice"));
        assert!(parse("<!-- open").unwrap_err().contains("comment"));
        assert!(parse("").unwrap_err().contains("no root"));
        assert!(parse("text").unwrap_err().contains("outside"));
        let deep = "<a>".repeat(MAX_DEPTH + 1);
        assert!(parse(&deep).unwrap_err().contains("deeply"));
        assert!(parse("<a>\n\n</b>").unwrap_err().starts_with("XML line 3"));
    }

    #[test]
    fn unknown_entities_stay() {
        assert_eq!(decode("a & b &nbsp; &#xZZ; &#38;"), "a & b &nbsp; &#xZZ; &");
    }
}
