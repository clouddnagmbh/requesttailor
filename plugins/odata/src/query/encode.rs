// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Text helpers of the query builder: `{{template}}` ranges (opaque everywhere), splitting
//! outside templates, parentheses and string literals, and percent-decoding and -encoding of
//! query values.

/// Byte ranges `[start, end)` of `{{...}}` placeholders, the same matches as rt-core
/// (`\{\{([^}]+)\}\}`).
pub fn template_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let inner = i + 2;
            let close = text[inner..].find('}').map(|j| inner + j);
            match close {
                Some(c) if c > inner && bytes.get(c + 1) == Some(&b'}') => {
                    out.push((i, c + 2));
                    i = c + 2;
                    continue;
                }
                _ => {}
            }
        }
        i += 1;
    }
    out
}

/// Byte offsets of `sep` in `text` outside `{{...}}`.
fn indexes_outside(text: &str, sep: char) -> Vec<usize> {
    let ranges = template_ranges(text);
    text.char_indices()
        .filter(|&(i, c)| c == sep && !ranges.iter().any(|&(s, e)| s <= i && i < e))
        .map(|(i, _)| i)
        .collect()
}

/// Splits at `sep` outside `{{...}}`.
pub fn split_outside(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for i in indexes_outside(text, sep) {
        parts.push(&text[start..i]);
        start = i + sep.len_utf8();
    }
    parts.push(&text[start..]);
    parts
}

/// The first `sep` outside `{{...}}`.
pub fn find_outside(text: &str, sep: char) -> Option<usize> {
    indexes_outside(text, sep).first().copied()
}

/// Splits a list (`a,b($select=x,y),'c,d'`) at commas that are outside parentheses, string
/// literals (`'...'`, `''` inside) and templates. Pieces are returned as written.
pub fn split_list(text: &str) -> Vec<&str> {
    let ranges = template_ranges(text);
    let mut parts = Vec::new();
    let (mut depth, mut quoted, mut start) = (0usize, false, 0);
    for (i, c) in text.char_indices() {
        if ranges.iter().any(|&(s, e)| s <= i && i < e) {
            continue;
        }
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Percent-decodes a query value: `%XX` to bytes, `+` to a space (as servers read the query
/// string). Templates stay as written; invalid escapes and invalid UTF-8 stay literal.
pub fn decode(text: &str) -> String {
    let ranges = template_ranges(text);
    let mut out = String::with_capacity(text.len());
    let mut pending: Vec<u8> = Vec::new();
    let flush = |pending: &mut Vec<u8>, out: &mut String| {
        if !pending.is_empty() {
            out.push_str(&utf8_keep(pending));
            pending.clear();
        }
    };
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(&(_, end)) = ranges.iter().find(|&&(s, _)| s == i) {
            flush(&mut pending, &mut out);
            out.push_str(&text[i..end]);
            i = end;
            continue;
        }
        match bytes[i] {
            b'%' if i + 2 < bytes.len()
                && hex(bytes[i + 1]).is_some()
                && hex(bytes[i + 2]).is_some() =>
            {
                let (h, l) = (hex(bytes[i + 1]), hex(bytes[i + 2]));
                pending.push(h.unwrap_or_default() << 4 | l.unwrap_or_default());
                i += 3;
            }
            b'+' => {
                pending.push(b' ');
                i += 1;
            }
            _ => {
                // Copy one whole character.
                let c = text[i..].chars().next().unwrap_or_default();
                let mut buf = [0u8; 4];
                pending.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                i += c.len_utf8();
            }
        }
    }
    flush(&mut pending, &mut out);
    out
}

/// Bytes as UTF-8; invalid sequences come back percent-encoded, so nothing is lost.
fn utf8_keep(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let (good, bad) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(good).unwrap_or_default());
                let n = e.error_len().unwrap_or(bad.len());
                for b in &bad[..n] {
                    out.push_str(&format!("%{b:02X}"));
                }
                rest = &bad[n..];
            }
        }
    }
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-encodes a decoded value of a system query option for the query string: what would
/// change the query's structure or be read differently (`%`, `&`, `#`, `+`, space), control
/// characters and non-ASCII. OData punctuation (`'`, `(`, `)`, `,`, `$`, `/`, `:`, `=`, `;`,
/// `@`) stays readable. Templates stay as written.
pub fn encode(text: &str) -> String {
    escape(text, |c| matches!(c, '%' | '&' | '#' | '+' | ' '))
}

/// Minimal escaping for text that is kept as written (unknown parameters): only what cannot
/// stand in a query at all (space, `#`, `&`, control characters, non-ASCII); `%` escapes
/// already there stay, so this is a no-op on anything read from a URL.
pub fn escape_raw(text: &str, extra: &[char]) -> String {
    escape(text, |c| matches!(c, ' ' | '#' | '&') || extra.contains(&c))
}

fn escape(text: &str, special: impl Fn(char) -> bool) -> String {
    let ranges = template_ranges(text);
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some(&(_, end)) = ranges.iter().find(|&&(s, _)| s == i) {
            out.push_str(&text[i..end]);
            i = end;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or_default();
        if special(c) || c.is_control() || !c.is_ascii() {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
        i += c.len_utf8();
    }
    out
}

/// Replaces `{{name}}` by the value of `name` in `variables` (a few rounds, for variables
/// that use other variables). Unknown names stay.
pub fn resolve(text: &str, variables: &[(String, String)]) -> String {
    let mut text = text.to_owned();
    for _ in 0..5 {
        let ranges = template_ranges(&text);
        if ranges.is_empty() {
            break;
        }
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        let mut changed = false;
        for (s, e) in ranges {
            out.push_str(&text[last..s]);
            let name = text[s + 2..e - 2].trim();
            match variables.iter().find(|(n, _)| n == name) {
                Some((_, value)) => {
                    out.push_str(value);
                    changed = true;
                }
                None => out.push_str(&text[s..e]),
            }
            last = e;
        }
        out.push_str(&text[last..]);
        text = out;
        if !changed {
            break;
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_opaque() {
        assert_eq!(template_ranges("a{{b}}c{{}}d{{e}"), vec![(1, 6)]);
        assert_eq!(split_outside("a&{{x&y}}&b", '&'), vec!["a", "{{x&y}}", "b"]);
        assert_eq!(decode("{{a%20b}}%20"), "{{a%20b}} ");
        assert_eq!(encode("{{a b}} c"), "{{a b}}%20c");
    }

    #[test]
    fn decode_and_encode() {
        assert_eq!(decode("Name%20eq%20'x'+and%2B"), "Name eq 'x' and+");
        assert_eq!(decode("%C3%A4%zz%"), "ä%zz%");
        assert_eq!(decode("%FF"), "%FF");
        assert_eq!(encode("a eq 'ä&#'+1%"), "a%20eq%20'%C3%A4%26%23'%2B1%25");
        assert_eq!(escape_raw("a%20b c", &[]), "a%20b%20c");
    }

    #[test]
    fn lists_respect_parentheses_and_quotes() {
        assert_eq!(
            split_list("a($select=x,y),b,'c,d',{{e,f}}"),
            vec!["a($select=x,y)", "b", "'c,d'", "{{e,f}}"]
        );
    }

    #[test]
    fn resolve_variables() {
        let vars = vec![
            ("baseUrl".to_owned(), "{{host}}/svc".to_owned()),
            ("host".to_owned(), "http://h".to_owned()),
        ];
        assert_eq!(resolve("{{baseUrl}}/X{{y}}", &vars), "http://h/svc/X{{y}}");
    }
}
