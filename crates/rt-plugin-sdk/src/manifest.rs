// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Checking a manifest before it goes into the `rt-plugin` custom section. The host applies
//! the same rules when it loads a plugin (`API.md`, "What a plugin is"); checking in a native
//! test catches mistakes before the plugin is built.

use rt_plugin::api::{API_VERSION, Manifest};

use crate::{Error, Result};

/// Parses the manifest JSON and checks it: id format, API version, non-empty name and
/// version, unique tab and importer ids, lowercase content types and extensions.
pub fn parse_manifest(json: &[u8]) -> Result<Manifest> {
    let manifest: Manifest =
        serde_json::from_slice(json).map_err(|e| Error::new(format!("invalid manifest: {e}")))?;
    check_manifest(&manifest)?;
    Ok(manifest)
}

pub fn check_manifest(m: &Manifest) -> Result<()> {
    if !valid_id(&m.id) {
        crate::bail!(
            "manifest id {:?} must match [a-z0-9][a-z0-9-]* and have at most 64 characters",
            m.id
        );
    }
    if !(1..=API_VERSION).contains(&m.api) {
        crate::bail!(
            "manifest api {} is not supported by this SDK (1 to {API_VERSION})",
            m.api
        );
    }
    if m.name.trim().is_empty() {
        crate::bail!("manifest name is empty");
    }
    if m.version.trim().is_empty() {
        crate::bail!("manifest version is empty");
    }
    let tabs = &m.contributes.tabs;
    for (i, tab) in tabs.iter().enumerate() {
        if tab.id.is_empty() || tabs[..i].iter().any(|t| t.id == tab.id) {
            crate::bail!("tab id {:?} is empty or not unique", tab.id);
        }
        if let Some(ct) = tab.content_types.iter().find(|c| has_upper(c)) {
            crate::bail!("content type {ct:?} of tab {:?} must be lowercase", tab.id);
        }
    }
    let importers = &m.contributes.importers;
    for (i, imp) in importers.iter().enumerate() {
        if imp.id.is_empty() || importers[..i].iter().any(|x| x.id == imp.id) {
            crate::bail!("importer id {:?} is empty or not unique", imp.id);
        }
        if let Some(ext) = imp
            .extensions
            .iter()
            .find(|e| has_upper(e) || e.starts_with('.'))
        {
            crate::bail!(
                "extension {ext:?} of importer {:?} must be lowercase, without the dot",
                imp.id
            );
        }
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    first_ok
        && id.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn has_upper(s: &str) -> bool {
    s.chars().any(|c| c.is_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(extra: &str) -> String {
        format!(r#"{{"id": "demo", "name": "Demo", "version": "0.1.0", "api": 1{extra}}}"#)
    }

    #[test]
    fn accepts_a_minimal_manifest() {
        let m = parse_manifest(manifest("").as_bytes()).unwrap();
        assert_eq!(m.id, "demo");
    }

    #[test]
    fn rejects_bad_ids_versions_and_duplicates() {
        let bad = [
            r#"{"id": "Demo", "name": "D", "version": "1", "api": 1}"#.to_owned(),
            r#"{"id": "-x", "name": "D", "version": "1", "api": 1}"#.to_owned(),
            format!(
                r#"{{"id": "{}", "name": "D", "version": "1", "api": 1}}"#,
                "a".repeat(65)
            ),
            r#"{"id": "x", "name": "D", "version": "1", "api": 0}"#.to_owned(),
            format!(
                r#"{{"id": "x", "name": "D", "version": "1", "api": {}}}"#,
                API_VERSION + 1
            ),
            r#"{"id": "x", "name": " ", "version": "1", "api": 1}"#.to_owned(),
            manifest(
                r#", "contributes": {"tabs": [
                    {"id": "t", "title": "T", "place": "response"},
                    {"id": "t", "title": "U", "place": "request"}]}"#,
            ),
            manifest(
                r#", "contributes": {"tabs": [
                    {"id": "t", "title": "T", "place": "response", "content_types": ["Application/JSON"]}]}"#,
            ),
            manifest(
                r#", "contributes": {"importers": [{"id": "i", "label": "I", "extensions": [".txt"]}]}"#,
            ),
            "not json".to_owned(),
        ];
        for json in bad {
            assert!(parse_manifest(json.as_bytes()).is_err(), "{json}");
        }
    }
}
