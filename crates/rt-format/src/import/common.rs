// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Helpers shared by the importers: collecting losses, finding dynamic variables and literal
//! credentials, deriving parameters from a URL, and finishing an import.

use std::collections::BTreeSet;

use rt_core::model::{Auth, KeyValue, Param, ParamKind};

use super::out::{self, CollectionOut};
use super::shims::{self, Dialect};
use super::{Import, ImportError, ImportFormat, Loss, LossEffect, LossKind, LossReport};

/// Losses of one import, in the order they were found.
#[derive(Debug, Default)]
pub(crate) struct Losses(pub Vec<Loss>);

impl Losses {
    pub fn add(
        &mut self,
        location: &str,
        kind: LossKind,
        effect: LossEffect,
        detail: impl Into<String>,
    ) {
        self.0.push(Loss {
            location: location.to_owned(),
            kind,
            effect,
            detail: detail.into(),
        });
    }

    pub fn dropped(&mut self, location: &str, kind: LossKind, detail: impl Into<String>) {
        self.add(location, kind, LossEffect::Dropped, detail);
    }

    pub fn approximated(&mut self, location: &str, kind: LossKind, detail: impl Into<String>) {
        self.add(location, kind, LossEffect::Approximated, detail);
    }

    /// A script imported as it is: reports each API it uses that the shims do not support
    /// (`what` is e.g. `pre-request script`). The script stays in the collection; such a call
    /// fails when it runs.
    pub fn script_kept(&mut self, location: &str, what: &str, code: &str, dialect: Dialect) {
        // Post-response scripts usually hold the tests (`pm.test(...)`, `insomnia.test(...)`).
        let kind = if what.contains("test") || code.contains(".test(") {
            LossKind::Test
        } else {
            LossKind::Script
        };
        for gap in shims::gaps(code, dialect) {
            let why = if gap.note.is_empty() {
                String::new()
            } else {
                format!(" ({})", gap.note)
            };
            self.approximated(
                location,
                kind,
                format!(
                    "{what} kept, but it uses `{}`, which RequestTailor does not support{why}; \
                     see SHIM-COVERAGE.md",
                    gap.api
                ),
            );
        }
    }

    /// A script that is not imported (the `.http` importer: JetBrains handler scripts use
    /// another API).
    pub fn script(&mut self, location: &str, what: &str, code: &str) {
        let n = line_count(code);
        if n == 0 {
            return;
        }
        let kind = if what.contains("test") || code.contains(".test(") {
            LossKind::Test
        } else {
            LossKind::Script
        };
        self.dropped(
            location,
            kind,
            format!("{what} ({n} {}) not imported", plural(n, "line", "lines")),
        );
    }

    /// Reports `{{$name}}` placeholders in `texts`, which the core does not resolve.
    pub fn dynamic_vars<'a>(&mut self, location: &str, texts: impl IntoIterator<Item = &'a str>) {
        let names = dynamic_names(texts);
        if names.is_empty() {
            return;
        }
        let list: Vec<String> = names.iter().map(|n| format!("{{{{{n}}}}}")).collect();
        self.approximated(
            location,
            LossKind::Variable,
            format!(
                "dynamic {} {} kept as text; RequestTailor does not generate dynamic values yet",
                plural(list.len(), "variable", "variables"),
                list.join(", ")
            ),
        );
    }

    /// Reports credentials typed directly into the request (not a `{{variable}}`).
    pub fn literal_credentials(&mut self, location: &str, auth: &Auth, headers: &[KeyValue]) {
        let mut found: Vec<&str> = Vec::new();
        match auth {
            Auth::Basic { password, .. } if is_literal(password) => found.push("password"),
            Auth::Bearer { token } if is_literal(token) => found.push("bearer token"),
            Auth::ApiKey { value, .. } if is_literal(value) => found.push("API key"),
            _ => {}
        }
        for h in headers {
            let name = h.name.to_ascii_lowercase();
            let sensitive = matches!(
                name.as_str(),
                "authorization" | "proxy-authorization" | "cookie"
            ) || name.contains("api-key")
                || name.contains("apikey")
                || name.contains("token");
            if sensitive && is_literal(&h.value) {
                found.push("credential header");
            }
        }
        found.dedup();
        for what in found {
            self.approximated(
                location,
                LossKind::Auth,
                format!(
                    "the {what} is stored in the request file as typed; consider a secret \
                     variable (set its value in Environments) and use {{{{name}}}} here"
                ),
            );
        }
    }

    /// A secret whose value is not written to any file.
    pub fn secret_value(&mut self, location: &str, name: &str) {
        self.dropped(
            location,
            LossKind::Variable,
            format!("secret value of `{name}` not imported; set it in Environments"),
        );
    }
}

pub(crate) fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

/// Non-empty lines of a script.
pub(crate) fn line_count(code: &str) -> usize {
    code.lines().filter(|l| !l.trim().is_empty()).count()
}

/// A value typed by hand rather than taken from a variable.
pub(crate) fn is_literal(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains("{{")
}

/// Names of `{{$name}}` placeholders.
pub(crate) fn dynamic_names<'a>(texts: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for text in texts {
        let mut rest = text;
        while let Some(start) = rest.find("{{") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find('}') else { break };
            let name = rest[..end].trim();
            if name.starts_with('$') && name.len() > 1 {
                out.insert(name.to_owned());
            }
            rest = &rest[end..];
        }
    }
    out
}

/// `a / b / c`, the location of an item as the user knows it.
pub(crate) fn location(path: &[String], name: &str) -> String {
    let mut parts: Vec<&str> = path.iter().map(String::as_str).collect();
    parts.push(name);
    parts.join(" / ")
}

/// Query parameters (mirroring the URL's query string, as written) and `:name` path
/// parameters of `url`. `path_values` gives values for path parameters by name.
pub(crate) fn url_params(url: &str, path_values: &[(String, String)]) -> Vec<Param> {
    let without_fragment = url.split('#').next().unwrap_or(url);
    let (path, query) = match without_fragment.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (without_fragment, None),
    };
    let mut params = Vec::new();
    for seg in path.split('/') {
        if let Some(name) = seg.strip_prefix(':')
            && !name.is_empty()
            && !params.iter().any(|p: &Param| p.name == name)
        {
            let value = path_values
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            params.push(Param {
                name: name.to_owned(),
                value,
                kind: ParamKind::Path,
                enabled: true,
            });
        }
    }
    let mut query_params = Vec::new();
    if let Some(q) = query {
        for pair in q.split('&').filter(|p| !p.is_empty()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            query_params.push(Param {
                name: name.to_owned(),
                value: value.to_owned(),
                kind: ParamKind::Query,
                enabled: true,
            });
        }
    }
    // Bruno lists query parameters first.
    query_params.extend(params);
    query_params
}

/// Writes the collection and builds the import.
pub(crate) fn finish(
    format: ImportFormat,
    collection: CollectionOut,
    losses: Losses,
) -> Result<Import, ImportError> {
    let (files, counts) =
        out::write_collection(&collection).map_err(|message| ImportError::Invalid {
            format,
            message: format!("internal error while writing the collection: {message}"),
        })?;
    let mut losses = losses.0;
    losses.sort_by(|a, b| {
        let rank = |l: &str| u8::from(l != "collection");
        rank(&a.location)
            .cmp(&rank(&b.location))
            .then_with(|| a.location.cmp(&b.location))
            .then(a.kind.cmp(&b.kind))
    });
    losses.dedup();
    Ok(Import {
        collection_name: out::collection_name(&collection.name).to_owned(),
        files,
        report: LossReport {
            format,
            requests: counts.requests,
            folders: counts.folders,
            environments: counts.environments,
            losses,
        },
    })
}

/// Text of a file: UTF-8 (with or without a byte order mark) or UTF-16 with a byte order
/// mark (as Windows tools write it).
pub(crate) fn utf8(
    format: ImportFormat,
    data: &[u8],
) -> Result<std::borrow::Cow<'_, str>, ImportError> {
    text(data).ok_or_else(|| ImportError::Invalid {
        format,
        message: "the file is not UTF-8 or UTF-16 text".into(),
    })
}

/// See [`utf8`]; `None` if the bytes are no text.
pub(crate) fn text(data: &[u8]) -> Option<std::borrow::Cow<'_, str>> {
    use std::borrow::Cow;
    let utf16 = |rest: &[u8], le: bool| -> Option<Cow<'_, str>> {
        if !rest.len().is_multiple_of(2) {
            return None;
        }
        let units = rest.chunks_exact(2).map(|c| {
            if le {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        });
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .ok()
            .map(Cow::Owned)
    };
    if let Some(rest) = data.strip_prefix(b"\xFF\xFE") {
        return utf16(rest, true);
    }
    if let Some(rest) = data.strip_prefix(b"\xFE\xFF") {
        return utf16(rest, false);
    }
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    std::str::from_utf8(data).ok().map(Cow::Borrowed)
}

pub(crate) fn invalid(format: ImportFormat, message: impl Into<String>) -> ImportError {
    ImportError::Invalid {
        format,
        message: message.into(),
    }
}

/// Raw body kind for a content type.
pub(crate) fn raw_kind(content_type: &str) -> rt_core::model::RawKind {
    use rt_core::model::RawKind;
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("json") {
        RawKind::Json
    } else if ct.contains("xml") {
        RawKind::Xml
    } else if ct.contains("sparql") {
        RawKind::Sparql
    } else {
        RawKind::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_from_urls() {
        let p = url_params(
            "https://h:8080/users/:id/x?a=1&b&c=%20#frag",
            &[("id".into(), "7".into())],
        );
        let names: Vec<(&str, &str, ParamKind)> = p
            .iter()
            .map(|p| (p.name.as_str(), p.value.as_str(), p.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("a", "1", ParamKind::Query),
                ("b", "", ParamKind::Query),
                ("c", "%20", ParamKind::Query),
                ("id", "7", ParamKind::Path)
            ]
        );
    }

    #[test]
    fn dynamic_variables() {
        let n = dynamic_names(["{{$guid}} {{ $timestamp }} {{x}} {{$", "{{$}}"]);
        assert_eq!(n.into_iter().collect::<Vec<_>>(), ["$guid", "$timestamp"]);
    }
}
