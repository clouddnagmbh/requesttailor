// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Re-indents XML text for display: one element per line, two spaces per level, text-only
//! elements on one line. It works on the tags as written (prefixes stay), so it needs no
//! namespace handling and never fails: text that is not XML comes back trimmed.

#[derive(Debug, PartialEq)]
enum Token<'a> {
    Open(&'a str),
    Close(&'a str),
    /// Self-closing tag, declaration, comment, CDATA, doctype.
    Other(&'a str),
    Text(&'a str),
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<') {
            let end = if stripped.starts_with("!--") {
                rest.find("-->").map(|i| i + 3)
            } else if stripped.starts_with("![CDATA[") {
                rest.find("]]>").map(|i| i + 3)
            } else {
                // `>` inside a quoted attribute value does not end the tag.
                let mut quote = None;
                let mut found = None;
                for (i, c) in rest.char_indices() {
                    match (quote, c) {
                        (Some(q), c) if c == q => quote = None,
                        (None, '"' | '\'') => quote = Some(c),
                        (None, '>') => {
                            found = Some(i + 1);
                            break;
                        }
                        _ => {}
                    }
                }
                found
            };
            let Some(end) = end else {
                out.push(Token::Text(rest));
                break;
            };
            let tag = &rest[..end];
            out.push(if tag.starts_with("</") {
                Token::Close(tag)
            } else if tag.starts_with("<?") || tag.starts_with("<!") || tag.ends_with("/>") {
                Token::Other(tag)
            } else {
                Token::Open(tag)
            });
            rest = &rest[end..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let t = &rest[..end];
            if !t.trim().is_empty() {
                out.push(Token::Text(t.trim()));
            }
            rest = &rest[end..];
        }
    }
    out
}

pub fn pretty(text: &str) -> String {
    let toks = tokens(text);
    let mut out = String::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < toks.len() {
        let pad = "  ".repeat(depth);
        match &toks[i] {
            Token::Open(tag) => {
                if let (Some(Token::Text(t)), Some(Token::Close(c))) =
                    (toks.get(i + 1), toks.get(i + 2))
                {
                    out.push_str(&format!("{pad}{tag}{t}{c}\n"));
                    i += 3;
                    continue;
                }
                if let Some(Token::Close(c)) = toks.get(i + 1) {
                    out.push_str(&format!("{pad}{tag}{c}\n"));
                    i += 2;
                    continue;
                }
                out.push_str(&format!("{pad}{tag}\n"));
                depth += 1;
            }
            Token::Close(tag) => {
                depth = depth.saturating_sub(1);
                out.push_str(&format!("{}{tag}\n", "  ".repeat(depth)));
            }
            Token::Other(tag) => out.push_str(&format!("{pad}{tag}\n")),
            Token::Text(t) => out.push_str(&format!("{pad}{t}\n")),
        }
        i += 1;
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_and_inlines_text() {
        let p =
            pretty("<?xml version=\"1.0\"?><a x=\"1>2\"><b>t</b><c/><d></d><e><f>1</f></e></a>");
        assert_eq!(
            p,
            "<?xml version=\"1.0\"?>\n<a x=\"1>2\">\n  <b>t</b>\n  <c/>\n  <d></d>\n  <e>\n    <f>1</f>\n  </e>\n</a>"
        );
    }

    #[test]
    fn plain_text_survives() {
        assert_eq!(pretty("  hello "), "hello");
        assert_eq!(pretty("<a"), "<a");
    }
}
