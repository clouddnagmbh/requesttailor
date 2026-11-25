// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Small helpers to write OpenCollection YAML by hand (the plugin API has no writer): scalars
//! the way `rt-format` writes them, and file names.

/// A YAML scalar: plain when that reads back as the same string, else single-quoted, else
/// double-quoted (control characters).
pub fn quote(text: &str) -> String {
    if text.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}')
        || text.starts_with(['{', '[', '*', '&', '!', '|', '>', '%', '@', '`'])
    {
        return double_quoted(text);
    }
    if is_plain(text) {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', "''"))
}

fn is_plain(text: &str) -> bool {
    let Some(first) = text.chars().next() else {
        return false;
    };
    if text.starts_with("- ") || text.starts_with("? ") || text.starts_with(": ") {
        return false;
    }
    if "-?:,[]{}#&*!|>'\"%@`".contains(first)
        && !(first == '-' && text.len() > 1 && !text.starts_with("- "))
    {
        return false;
    }
    if text != text.trim() || text.contains(": ") || text.contains(" #") || text.ends_with(':') {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
    ) {
        return false;
    }
    text.parse::<f64>().is_err()
}

fn double_quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A file name part: lower case words joined by `-`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(60).collect();
    let out = out.trim_end_matches('-');
    match out {
        "" => "request".to_owned(),
        "opencollection" | "folder" => format!("{out}-1"),
        o => o.to_owned(),
    }
}

/// A file name for an environment: the name with path characters replaced.
pub fn file_stem(name: &str) -> String {
    let out: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .take(60)
        .collect();
    let out = out.trim_end_matches(['.', ' ']);
    if out.is_empty() || out.starts_with('.') {
        format!("_{out}")
    } else {
        out.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_that_need_quotes_get_them() {
        assert_eq!(quote("plain text"), "plain text");
        assert_eq!(quote("{{apiUrl}}/x"), "\"{{apiUrl}}/x\"");
        assert_eq!(quote("200"), "'200'");
        assert_eq!(quote("a: b"), "'a: b'");
        assert_eq!(quote("true"), "'true'");
    }

    #[test]
    fn slugs_are_file_names() {
        assert_eq!(slug("Log of the last trigger"), "log-of-the-last-trigger");
        assert_eq!(slug("folder"), "folder-1");
    }
}
