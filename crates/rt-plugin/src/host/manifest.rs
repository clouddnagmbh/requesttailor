// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The manifest: UTF-8 JSON in the custom section `rt-plugin` of the module, checked before
//! anything of the plugin runs.

use std::collections::BTreeSet;

use super::PluginError;
use super::permissions::valid_network_entry;
use crate::api::{API_VERSION, MANIFEST_SECTION, Manifest};

/// Largest manifest accepted.
const MAX_MANIFEST_BYTES: usize = 256 * 1024;

/// Reads the manifest section of `wasm` and checks it. Does not validate the code.
pub fn read(wasm: &[u8]) -> Result<Manifest, PluginError> {
    let invalid = |m: &str| PluginError::Invalid(m.to_owned());
    let mut found: Option<&[u8]> = None;
    for section in custom_sections(wasm)? {
        let (name, data) = section;
        if name == MANIFEST_SECTION.as_bytes() {
            if found.is_some() {
                return Err(invalid("more than one rt-plugin section"));
            }
            found = Some(data);
        }
    }
    let data = found.ok_or_else(|| invalid("no rt-plugin section with a manifest"))?;
    if data.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("the manifest is too large"));
    }
    let text = std::str::from_utf8(data).map_err(|_| invalid("the manifest is not UTF-8"))?;
    let manifest: Manifest = serde_json::from_str(text)
        .map_err(|e| PluginError::Invalid(format!("invalid manifest: {e}")))?;
    check(&manifest)?;
    Ok(manifest)
}

/// The rules of `API.md` for a manifest.
pub fn check(m: &Manifest) -> Result<(), PluginError> {
    let invalid = |m: String| Err(PluginError::Invalid(m));
    if m.api < 1 || m.api > API_VERSION {
        return Err(PluginError::ApiVersion {
            needs: m.api,
            runs: API_VERSION,
        });
    }
    if !valid_id(&m.id) {
        return invalid(format!(
            "invalid id {:?}: use [a-z0-9][a-z0-9-]*, at most 64 characters",
            m.id
        ));
    }
    if m.name.trim().is_empty() {
        return invalid("the manifest has no name".into());
    }
    if m.version.trim().is_empty() {
        return invalid("the manifest has no version".into());
    }
    if let Some(bad) = m
        .permissions
        .network
        .iter()
        .find(|e| !valid_network_entry(e))
    {
        return invalid(format!("invalid network permission {bad:?}"));
    }
    let mut tabs = BTreeSet::new();
    for tab in &m.contributes.tabs {
        if tab.id.is_empty() || !tabs.insert(tab.id.as_str()) {
            return invalid(format!("tab id {:?} is empty or used twice", tab.id));
        }
    }
    let mut importers = BTreeSet::new();
    for importer in &m.contributes.importers {
        if importer.id.is_empty() || !importers.insert(importer.id.as_str()) {
            return invalid(format!(
                "importer id {:?} is empty or used twice",
                importer.id
            ));
        }
    }
    Ok(())
}

pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 64
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A custom section: name and data.
type Section<'a> = (&'a [u8], &'a [u8]);

/// `(name, data)` of every custom section, from the module's binary layout: the 8-byte
/// header, then sections of `id: u8, size: leb128 u32, payload`.
fn custom_sections(wasm: &[u8]) -> Result<Vec<Section<'_>>, PluginError> {
    let invalid = || PluginError::Invalid("not a WebAssembly module".into());
    if wasm.len() < 8 || &wasm[..4] != b"\0asm" || wasm[4..8] != [1, 0, 0, 0] {
        return Err(invalid());
    }
    let mut out = Vec::new();
    let mut pos = 8;
    while pos < wasm.len() {
        let id = wasm[pos];
        pos += 1;
        let size = leb128(wasm, &mut pos).ok_or_else(invalid)? as usize;
        let end = pos
            .checked_add(size)
            .filter(|&e| e <= wasm.len())
            .ok_or_else(invalid)?;
        if id == 0 {
            let mut p = pos;
            let len = leb128(&wasm[..end], &mut p).ok_or_else(invalid)? as usize;
            let name_end = p
                .checked_add(len)
                .filter(|&e| e <= end)
                .ok_or_else(invalid)?;
            out.push((&wasm[p..name_end], &wasm[name_end..end]));
        }
        pos = end;
    }
    Ok(out)
}

fn leb128(bytes: &[u8], pos: &mut usize) -> Option<u32> {
    let mut result: u32 = 0;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        result |= u32::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some(result);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        for ok in ["odata", "jwt-inspector", "a", "0x", &"a".repeat(64)] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "-a", "A", "a_b", "a.b", "ä", &"a".repeat(65)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn not_wasm() {
        assert!(matches!(read(b"PK\x03\x04"), Err(PluginError::Invalid(_))));
        assert!(matches!(
            read(b"\0asm\x01\0\0\0\x00\x7f"),
            Err(PluginError::Invalid(_))
        ));
        assert!(matches!(
            read(b"\0asm\x01\0\0\0"),
            Err(PluginError::Invalid(m)) if m.contains("no rt-plugin section")
        ));
    }
}
