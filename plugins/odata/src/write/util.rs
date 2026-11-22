// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Small helpers of the write strand: methods, URLs and templates, the cached model.

use rt_plugin_sdk::api::PluginRequest;
use rt_plugin_sdk::model::{Param, ParamKind};

use crate::edm::Model;
use crate::service::{self, Target};

/// Methods that change data; they need a CSRF token.
pub const MODIFYING: &[&str] = &["POST", "PUT", "PATCH", "MERGE", "DELETE"];

/// The mask hosts put in place of secret values; never copied into a patch.
pub const MASK: &str = "***";

/// The method the service sees: `X-HTTP-Method` tunnels `MERGE` etc. through `POST`.
pub fn effective_method(request: &PluginRequest) -> String {
    let method = request.method.to_ascii_uppercase();
    match rt_plugin_sdk::header(&request.headers, "X-HTTP-Method") {
        Some(tunnel) if method == "POST" => tunnel.trim().to_ascii_uppercase(),
        _ => method,
    }
}

/// [`service::split`] with the roots that have cached metadata.
pub fn split(url: &str) -> Option<Target> {
    let known = crate::metadata::known_roots().unwrap_or_default();
    service::split(url, &known)
}

/// Decodes `%XX` escapes (invalid ones stay).
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The value of a query parameter of `url`, decoded.
pub fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    let query = query.split_once('#').map_or(query, |(q, _)| q);
    query.split('&').find_map(|pair| {
        let (n, v) = pair.split_once('=').unwrap_or((pair, ""));
        (percent_decode(n) == name).then(|| percent_decode(&v.replace('+', " ")))
    })
}

/// An entity URL as a storage key: no query or fragment, escapes decoded, scheme and host
/// in lowercase.
pub fn entity_url(url: &str) -> String {
    let base = url.split(['?', '#']).next().unwrap_or(url);
    let decoded = percent_decode(base);
    match decoded.find("://") {
        Some(i) => {
            let after = i + 3;
            let host_end = decoded[after..]
                .find('/')
                .map_or(decoded.len(), |j| after + j);
            format!(
                "{}{}",
                decoded[..host_end].to_ascii_lowercase(),
                &decoded[host_end..]
            )
        }
        None => decoded,
    }
}

/// `{{name}}` replaced by the variable's value; unknown variables stay.
pub fn resolve(template: &str, variables: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let Some(end) = rest[start..].find("}}").map(|e| start + e) else {
            break;
        };
        out.push_str(&rest[..start]);
        let name = rest[start + 2..end].trim();
        match variables.iter().find(|(n, _)| n == name) {
            Some((_, v)) => out.push_str(v),
            None => out.push_str(&rest[start..end + 2]),
        }
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

/// A request URL template split like [`service::split`], keeping the template's
/// `{{variables}}`, plus the service root with the variables resolved (for storage keys).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateTarget {
    /// The root as written in the template, with trailing `/` (e.g. `{{baseUrl}}/`).
    pub root: String,
    pub resource: String,
    pub query: String,
    /// The root with variables resolved; `None` if that is not an absolute URL.
    pub resolved_root: Option<String>,
    pub version: Option<crate::edm::Version>,
}

const FAKE_ORIGIN: &str = "http://template.invalid/";

pub fn template_target(template: &str, variables: &[(String, String)]) -> Option<TemplateTarget> {
    let template = template.trim();
    if template.is_empty() {
        return None;
    }
    let resolved = split(&resolve(template, variables)).filter(|t| !t.service_root.contains("{{"));
    let (target, fake) = if template.contains("://") {
        (service::split(template, &[])?, false)
    } else {
        (
            service::split(
                &format!("{FAKE_ORIGIN}{}", template.trim_start_matches('/')),
                &[],
            )?,
            true,
        )
    };
    let mut root = target.service_root.clone();
    if fake {
        root = root[FAKE_ORIGIN.len()..].to_owned();
        if template.starts_with('/') {
            root.insert(0, '/');
        }
    }
    // When the resolved URL knows its root better (cached metadata), keep the template's
    // resource consistent with it.
    let (resource, query) = match &resolved {
        Some(r) if !template.contains("{{") => (r.resource.clone(), r.query.clone()),
        _ => (target.resource.clone(), target.query.clone()),
    };
    let root = match &resolved {
        Some(r) if !template.contains("{{") => r.service_root.clone(),
        _ => root,
    };
    Some(TemplateTarget {
        version: resolved.as_ref().and_then(|r| r.version).or(target.version),
        root,
        resource,
        query,
        resolved_root: resolved.map(|r| r.service_root),
    })
}

/// The query parameters a rewritten request keeps: `sap-client` and `sap-language`.
pub fn sap_query(query: &str) -> String {
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| {
            let name = p.split('=').next().unwrap_or_default();
            name == "sap-client" || name == "sap-language"
        })
        .collect();
    if kept.is_empty() {
        String::new()
    } else {
        format!("?{}", kept.join("&"))
    }
}

/// Params of a request whose URL now has only the [`sap_query`] parameters.
pub fn sap_params(params: &[Param]) -> Vec<Param> {
    params
        .iter()
        .filter(|p| p.kind == ParamKind::Path || p.name == "sap-client" || p.name == "sap-language")
        .cloned()
        .collect()
}

/// The cached model of `root` (plugin storage `service::storage_key("metadata", root)`, the
/// JSON of [`Model`]); `None` if there is none or it does not parse.
pub fn model(root: &str) -> Option<Model> {
    if let Ok(Some(model)) = crate::metadata::cached(root) {
        return Some(model);
    }
    let value = rt_plugin_sdk::host::storage_get(service::storage_key("metadata", root)).ok()??;
    serde_json::from_value(value).ok()
}

/// `Name(predicate)` → (`Name`, `Some(predicate)`).
pub fn split_segment(segment: &str) -> (&str, Option<&str>) {
    match segment.find('(') {
        Some(i) if segment.ends_with(')') => {
            (&segment[..i], Some(&segment[i + 1..segment.len() - 1]))
        }
        _ => (segment, None),
    }
}

/// Splits at commas outside single quotes.
pub fn split_key(predicate: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_quote = false;
    let mut start = 0;
    for (i, c) in predicate.char_indices() {
        match c {
            '\'' => in_quote = !in_quote,
            ',' if !in_quote => {
                out.push(&predicate[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&predicate[start..]);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// FNV-1a, for deterministic boundaries.
pub fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_templates() {
        assert_eq!(
            entity_url("HTTP://Host:1/a/Books(%27x%27)?$top=1"),
            "http://host:1/a/Books('x')"
        );
        assert_eq!(
            query_param("http://h/a?x=1&sap-client=200", "sap-client").as_deref(),
            Some("200")
        );
        assert_eq!(query_param("http://h/a", "sap-client"), None);
        let vars = vec![("baseUrl".to_owned(), "http://h:4004".to_owned())];
        assert_eq!(
            resolve("{{baseUrl}}/x/{{id}}", &vars),
            "http://h:4004/x/{{id}}"
        );
        let t = template_target("{{baseUrl}}/odata/v4/admin/Books(201)?$top=1", &vars).unwrap();
        assert_eq!(t.root, "{{baseUrl}}/odata/v4/admin/");
        assert_eq!(t.resource, "Books(201)");
        assert_eq!(t.query, "$top=1");
        assert_eq!(
            t.resolved_root.as_deref(),
            Some("http://h:4004/odata/v4/admin/")
        );
        let t = template_target(
            "http://h/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet",
            &[],
        )
        .unwrap();
        assert_eq!(t.root, "http://h/sap/opu/odata/sap/ZRT_SALES_SRV/");
        assert_eq!(
            sap_query("$top=1&sap-client=200&sap-language=EN"),
            "?sap-client=200&sap-language=EN"
        );
        assert_eq!(split_key("A='x,y',B=2"), vec!["A='x,y'", "B=2"]);
    }
}
