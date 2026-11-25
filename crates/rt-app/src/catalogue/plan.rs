// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What installing a catalogue entry would do, with every check. Pure: the shell downloaded the
//! bytes. The order matters: size, checksum (a mismatch refuses before anything else is read),
//! the manifest inside the module, then the comparison with what is installed.

use sha2::{Digest, Sha256};

use super::{
    CatalogueEntry, CatalogueUpdate, InstallPlan, InstalledSource, MAX_WASM_BYTES, Permissions,
    SourceIndex,
};
use crate::AppError;
use crate::plugins::newer;
use rt_plugin::host::PluginHost;
use rt_plugin::host::permissions::covers;

fn err(message: impl Into<String>) -> AppError {
    AppError::Plugin(message.into())
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// An installed plugin with the same id, as far as the plan needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledState {
    pub version: String,
    /// What the user granted.
    pub granted: Permissions,
    /// SHA-256 of the installed module when it came from a catalogue.
    pub sha256: Option<String>,
}

fn same_rights(a: &Permissions, b: &Permissions) -> bool {
    covers(a, b) && covers(b, a)
}

/// Checks the downloaded `wasm` against `entry` and builds the plan. Refuses (an error, no
/// plan) if the size or the SHA-256 differ from the index, if the module is not a plugin with
/// the entry's id, or if the index changed a release that is installed (same version, other
/// checksum). The rights in the plan come from the module's own manifest; if the index named
/// others, that is a warning.
pub fn plan_install(
    source: &str,
    wasm_url: &str,
    entry: &CatalogueEntry,
    wasm: &[u8],
    installed: Option<&InstalledState>,
) -> Result<InstallPlan, AppError> {
    if wasm.len() > MAX_WASM_BYTES {
        return Err(err(format!(
            "{}: the module is larger than {} MiB",
            entry.id,
            MAX_WASM_BYTES / (1024 * 1024)
        )));
    }
    let sha256 = sha256_hex(wasm);
    if !sha256.eq_ignore_ascii_case(&entry.wasm.sha256) {
        return Err(err(format!(
            "{}: the checksum does not match (the index says {}, the download is {sha256}). Nothing was installed.",
            entry.id, entry.wasm.sha256
        )));
    }
    if let Some(i) = installed
        && i.version == entry.version
        && i.sha256.as_deref().is_some_and(|s| s != sha256)
    {
        return Err(err(format!(
            "{} {}: the index changed a published release (installed {}, now {sha256}). Nothing was installed.",
            entry.id,
            entry.version,
            i.sha256.as_deref().unwrap_or_default()
        )));
    }
    let manifest = PluginHost::inspect(wasm)
        .map_err(|e| err(format!("{}: this is not a usable plugin: {e}", entry.id)))?;
    if manifest.id != entry.id {
        return Err(err(format!(
            "the module is the plugin {:?}, the index says {:?}. Nothing was installed.",
            manifest.id, entry.id
        )));
    }
    let mut warnings = Vec::new();
    if manifest.version != entry.version {
        warnings.push(format!(
            "The index says version {}, the module is version {}.",
            entry.version, manifest.version
        ));
    }
    let permissions_differ = !same_rights(&manifest.permissions, &entry.permissions);
    if permissions_differ {
        warnings.push(
            "The index names other rights than the plugin itself asks for; the plugin's own are shown."
                .to_owned(),
        );
    }
    if let Some(i) = installed
        && newer(&i.version, &manifest.version)
    {
        warnings.push(format!(
            "This is older than the installed version {}.",
            i.version
        ));
    }
    let more_permissions = installed.is_some_and(|i| !covers(&i.granted, &manifest.permissions));
    Ok(InstallPlan {
        source: source.to_owned(),
        entry: entry.clone(),
        wasm_url: wasm_url.to_owned(),
        sha256,
        permissions: manifest.permissions,
        permissions_differ,
        update: installed.is_some(),
        more_permissions,
        warnings,
    })
}

/// Whether the module on disk is the one that was installed (`rt plugin verify`).
pub fn verify_installed(recorded_sha256: &str, wasm: &[u8]) -> bool {
    sha256_hex(wasm).eq_ignore_ascii_case(recorded_sha256)
}

/// Newer versions in the refreshed sources than what was installed from them.
pub fn check_updates(
    sources: &[SourceIndex],
    installed: &[InstalledSource],
) -> Vec<CatalogueUpdate> {
    let mut out = Vec::new();
    for i in installed {
        let Some(found) = sources
            .iter()
            .filter(|s| s.source.url == i.source)
            .filter_map(|s| s.index.plugins.iter().find(|e| e.id == i.plugin))
            .find(|e| newer(&e.version, &i.version))
        else {
            continue;
        };
        out.push(CatalogueUpdate {
            plugin: i.plugin.clone(),
            source: i.source.clone(),
            installed: i.version.clone(),
            available: found.version.clone(),
        });
    }
    out
}
