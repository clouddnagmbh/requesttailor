// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A strict little XML reader for the JUnit tests (no dependency): elements, attributes,
//! text, the five predefined entities and character references. It rejects what JUnit
//! parsers reject: unbalanced tags, unquoted or duplicate attributes, unknown entities, raw
//! `<` or `&`, characters XML 1.0 does not allow (also as references), a missing or second
//! root element.

#![allow(dead_code)]

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn attr(&self, name: &str) -> &str {
        self.attrs
            .get(name)
            .map(String::as_str)
            .unwrap_or_else(|| panic!("<{}> has no {name}", self.name))
    }

    pub fn all<'a>(&'a self, name: &str) -> Vec<&'a Element> {
        let mut out = Vec::new();
        self.collect(name, &mut out);
        out
    }

    fn collect<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.collect(name, out);
        }
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }
}

fn allowed(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

struct Reader<'a> {
    s: &'a str,
    pos: usize,
}

impl Reader<'_> {
    fn rest(&self) -> &str {
        &self.s[self.pos..]
    }

    fn eat(&mut self, prefix: &str) -> bool {
        if self.rest().starts_with(prefix) {
            self.pos += prefix.len();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        let n = self.rest().len() - self.rest().trim_start().len();
        self.pos += n;
    }

    fn until(&mut self, end: &str) -> Result<&str, String> {
        let i = self
            .rest()
            .find(end)
            .ok_or_else(|| format!("missing {end:?} at {}", self.pos))?;
        let out = &self.s[self.pos..self.pos + i];
        self.pos += i + end.len();
        Ok(out)
    }

    fn name(&mut self) -> Result<String, String> {
        let n = self
            .rest()
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')))
            .unwrap_or(self.rest().len());
        if n == 0 {
            return Err(format!("expected a name at {}", self.pos));
        }
        let out = self.rest()[..n].to_owned();
        self.pos += n;
        Ok(out)
    }

    fn misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            if self.eat("<!--") {
                self.until("-->")?;
            } else if self.eat("<?") {
                self.until("?>")?;
            } else {
                return Ok(());
            }
        }
    }

    fn element(&mut self) -> Result<Element, String> {
        if !self.eat("<") {
            return Err(format!("expected '<' at {}", self.pos));
        }
        let name = self.name()?;
        let mut attrs = BTreeMap::new();
        loop {
            let before = self.pos;
            self.skip_ws();
            if self.eat("/>") {
                return Ok(Element {
                    name,
                    attrs,
                    children: Vec::new(),
                    text: String::new(),
                });
            }
            if self.eat(">") {
                break;
            }
            if self.pos == before {
                return Err(format!(
                    "expected whitespace before an attribute at {}",
                    self.pos
                ));
            }
            let key = self.name()?;
            self.skip_ws();
            if !self.eat("=") {
                return Err(format!("attribute {key} without '='"));
            }
            self.skip_ws();
            let quote = if self.eat("\"") {
                "\""
            } else if self.eat("'") {
                "'"
            } else {
                return Err(format!("attribute {key} is not quoted"));
            };
            let raw = self.until(quote)?.to_owned();
            if raw.contains('<') {
                return Err(format!("raw '<' in attribute {key}"));
            }
            // Attribute value normalization: literal whitespace becomes a space, so a
            // writer must spell tabs and line breaks as references to keep them.
            let value = unescape(&raw.replace(['\t', '\n', '\r'], " "))?;
            if attrs.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate attribute {key}"));
            }
        }
        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            if self.eat("</") {
                let end = self.name()?;
                self.skip_ws();
                if !self.eat(">") || end != name {
                    return Err(format!("</{end}> does not close <{name}>"));
                }
                return Ok(Element {
                    name,
                    attrs,
                    children,
                    text,
                });
            }
            if self.eat("<!--") {
                self.until("-->")?;
            } else if self.eat("<![CDATA[") {
                text.push_str(self.until("]]>")?);
            } else if self.rest().starts_with('<') {
                children.push(self.element()?);
            } else {
                let n = self.rest().find('<').ok_or("unexpected end")?;
                let raw = &self.s[self.pos..self.pos + n];
                text.push_str(&unescape(raw)?);
                self.pos += n;
            }
        }
    }
}

fn unescape(raw: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let end = rest[i..]
            .find(';')
            .ok_or_else(|| format!("unterminated entity in {raw:?}"))?;
        let entity = &rest[i + 1..i + end];
        let c = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("bad reference &{e};"))?,
            e if e.starts_with('#') => e[1..]
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("bad reference &{e};"))?,
            e => return Err(format!("unknown entity &{e};")),
        };
        if !allowed(c) {
            return Err(format!("reference to a character XML forbids: &{entity};"));
        }
        out.push(c);
        rest = &rest[i + end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Parses a whole document.
pub fn parse(doc: &str) -> Result<Element, String> {
    if let Some(c) = doc.chars().find(|c| !allowed(*c)) {
        return Err(format!("character XML forbids: {:?}", c));
    }
    let mut r = Reader { s: doc, pos: 0 };
    if !doc.starts_with("<?xml ") {
        return Err("no XML declaration".into());
    }
    r.misc()?;
    let root = r.element()?;
    r.misc()?;
    if !r.rest().is_empty() {
        return Err(format!("content after the root element: {:?}", r.rest()));
    }
    Ok(root)
}

#[test]
fn the_reader_is_strict() {
    let ok = parse("<?xml version=\"1.0\"?>\n<a x=\"1 &amp; &#x41;\"><b/>t&lt;</a>\n").unwrap();
    assert_eq!(ok.attr("x"), "1 & A");
    assert_eq!(ok.text, "t<");
    for bad in [
        "<?xml version=\"1.0\"?><a>",
        "<?xml version=\"1.0\"?><a></b>",
        "<?xml version=\"1.0\"?><a x=1/>",
        "<?xml version=\"1.0\"?><a x=\"1\" x=\"2\"/>",
        "<?xml version=\"1.0\"?><a>&nbsp;</a>",
        "<?xml version=\"1.0\"?><a>&#27;</a>",
        "<?xml version=\"1.0\"?><a>\u{1b}</a>",
        "<?xml version=\"1.0\"?><a/><b/>",
        "<?xml version=\"1.0\"?><a>x & y</a>",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}
