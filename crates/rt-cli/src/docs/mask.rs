// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What `rt docs` must not show: values the secret scanner (`rt_app::secret_scan`) recognises
//! as credentials. The scanner reads YAML lines, so a value is checked as the `value` of a
//! `name` (headers, parameters, variables) or as a `key: value` line, and texts (bodies) line
//! by line.

use rt_app::secret_scan::scan;

pub const MASK: &str = "***";

/// Counts what was masked.
#[derive(Debug, Default)]
pub struct Masker {
    pub masked: usize,
}

/// A YAML double-quoted scalar (JSON string syntax is valid YAML).
fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

impl Masker {
    /// `value` of an entry named `name` (a header, parameter or variable): `***` if it is a
    /// credential.
    pub fn named(&mut self, name: &str, value: &str) -> String {
        let yaml = format!("- name: {}\n  value: {}\n", quoted(name), quoted(value));
        self.check(&yaml, value)
    }

    /// The value of a field `key` (e.g. `password`, `token`, `username`).
    pub fn field(&mut self, key: &str, value: &str) -> String {
        let yaml = format!("{key}: {}\n", quoted(value));
        self.check(&yaml, value)
    }

    fn check(&mut self, yaml: &str, value: &str) -> String {
        if value.is_empty() || scan(&[("docs.yml", yaml)]).is_empty() {
            value.to_owned()
        } else {
            self.masked += 1;
            MASK.to_owned()
        }
    }

    /// A text (a body): each line with a credential has the credential replaced by `***`.
    pub fn text(&mut self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let findings = scan(&[("body.txt", text)]);
        if findings.is_empty() {
            return text.to_owned();
        }
        let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let mut done = Vec::new();
        for f in &findings {
            let i = f.line as usize - 1;
            if i >= lines.len() || done.contains(&i) {
                continue;
            }
            done.push(i);
            self.masked += 1;
            let line = &lines[i];
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            // The snippet is the line with the value masked, trimmed and cut at 160
            // characters (then it ends with "…" and cannot stand in for the line).
            lines[i] = if f.snippet.ends_with('…') || f.snippet.is_empty() {
                format!("{indent}{MASK}")
            } else {
                format!("{indent}{}", f.snippet)
            };
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Built at run time, so the repository's own secret scans do not see a credential.
    fn github_token() -> String {
        format!("{}{}", "ghp_", "Ab1Cd2Ef3Gh4Ij5Kl6Mn7Op8Qr9St0Uv1Wx2")
    }

    #[test]
    fn named_values() {
        let mut m = Masker::default();
        assert_eq!(m.named("Accept", "application/json"), "application/json");
        assert_eq!(
            m.named("Authorization", "Bearer {{token}}"),
            "Bearer {{token}}"
        );
        assert_eq!(m.named("X-Api-Key", "{{apiKey}}"), "{{apiKey}}");
        assert_eq!(m.masked, 0);
        assert_eq!(m.named("X-Api-Key", "plain-key-value-1234"), MASK);
        assert_eq!(
            m.named("Authorization", &format!("Bearer {}", github_token())),
            MASK
        );
        assert_eq!(m.named("X-Anything", &github_token()), MASK);
        assert_eq!(m.masked, 3);
    }

    #[test]
    fn fields() {
        let mut m = Masker::default();
        assert_eq!(m.field("username", "admin"), "admin");
        assert_eq!(
            m.field("password", "{{adminPassword}}"),
            "{{adminPassword}}"
        );
        assert_eq!(m.field("password", "correct-horse-battery"), MASK);
        assert_eq!(m.masked, 1);
    }

    #[test]
    fn texts_keep_everything_but_the_credential() {
        let mut m = Masker::default();
        let body = format!(
            "{{\n  \"user\": \"ada\",\n  \"token\": \"{}\"\n}}",
            github_token()
        );
        let masked = m.text(&body);
        assert!(!masked.contains(&github_token()), "{masked}");
        assert!(masked.contains("\"user\": \"ada\""), "{masked}");
        assert!(masked.contains(MASK), "{masked}");
        assert_eq!(m.masked, 1);
        assert_eq!(m.text("{\"name\": \"Grace\"}"), "{\"name\": \"Grace\"}");
    }
}
