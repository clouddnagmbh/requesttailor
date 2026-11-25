// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The hosts the user allowed on this device (`egress/hosts.json` in the data folder, in the
//! browser's storage on the web). A host is allowed once, for the extension and purpose that
//! asked; the entry is kept until the user removes it. The list is plain data: the shells load
//! and store it, [`HostList::allows`] is the only question the call asks.

use super::EgressHost;

/// The most hosts a device keeps; a list this long is not a list a person made.
pub const MAX_HOSTS: usize = 1000;

/// Why a host could not be added.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    #[error("{0} is not a host name (no scheme, path or spaces)")]
    Invalid(String),
    #[error("the list of allowed hosts is full")]
    Full,
    #[error("the list of allowed hosts cannot be read: {0}")]
    Unreadable(String),
}

/// The allowed hosts, in the order they were added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostList {
    hosts: Vec<EgressHost>,
}

/// A host name as it is compared: lower case, no trailing dot.
fn normal(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn valid(host: &str) -> bool {
    // A bracketed IPv6 literal or a name; no scheme, port, path, query or credentials.
    !host.is_empty()
        && host.len() <= 253
        && !host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "/\\?#@".contains(c))
        && (host.starts_with('[') && host.ends_with(']') || !host.contains(':'))
}

impl HostList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the file's JSON (`[EgressHost, ...]`). An empty text is an empty list; anything
    /// else that is not a list of hosts is an error (the caller decides: the shells treat it
    /// as "nothing allowed", which is the safe side).
    pub fn from_json(text: &str) -> Result<Self, HostError> {
        if text.trim().is_empty() {
            return Ok(Self::new());
        }
        let mut hosts: Vec<EgressHost> =
            serde_json::from_str(text).map_err(|e| HostError::Unreadable(e.to_string()))?;
        for h in &mut hosts {
            h.host = normal(&h.host);
        }
        hosts.retain(|h| valid(&h.host));
        hosts.truncate(MAX_HOSTS);
        Ok(Self { hosts })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.hosts).unwrap_or_else(|_| "[]".into())
    }

    pub fn list(&self) -> &[EgressHost] {
        &self.hosts
    }

    /// Whether a call to `host` on `port` (the URL's explicit port; `None` is the default of
    /// `scheme`) is allowed. An entry without a port is port 443, the default of `https`: a
    /// host allowed for `https://` is not reachable over plain `http://` (that needs an entry
    /// with port 80).
    pub fn allows(&self, host: &str, port: Option<u16>, scheme: &str) -> bool {
        let want = port.unwrap_or(if scheme.eq_ignore_ascii_case("http") {
            80
        } else {
            443
        });
        let host = normal(host);
        self.hosts
            .iter()
            .any(|h| h.host == host && h.port.unwrap_or(443) == want)
    }

    /// Allows `host` (and `port`; `None` is port 443) for `extension` and `purpose`. Allowing a host that is already on the list keeps the first entry.
    pub fn allow(
        &mut self,
        host: &str,
        port: Option<u16>,
        extension: &str,
        purpose: &str,
        now_ms: i64,
    ) -> Result<EgressHost, HostError> {
        let host = normal(host);
        if !valid(&host) {
            return Err(HostError::Invalid(host));
        }
        if let Some(existing) = self.hosts.iter().find(|h| h.host == host && h.port == port) {
            return Ok(existing.clone());
        }
        if self.hosts.len() >= MAX_HOSTS {
            return Err(HostError::Full);
        }
        let entry = EgressHost {
            host,
            port,
            extension: extension.trim().to_owned(),
            purpose: purpose.trim().to_owned(),
            allowed_at: now_ms,
        };
        self.hosts.push(entry.clone());
        Ok(entry)
    }

    /// Removes the entry for `host` and `port`; whether there was one.
    pub fn remove(&mut self, host: &str, port: Option<u16>) -> bool {
        let host = normal(host);
        let before = self.hosts.len();
        self.hosts.retain(|h| !(h.host == host && h.port == port));
        self.hosts.len() != before
    }
}

/// Loading and storing the list as a file (desktop: `egress/hosts.json` in the data folder).
#[cfg(not(target_arch = "wasm32"))]
pub mod file {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::{HostError, HostList};

    /// One writer at a time: allow and remove read, change and write the whole file.
    static WRITE: Mutex<()> = Mutex::new(());

    /// `egress/hosts.json` below `data_dir`.
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("egress").join("hosts.json")
    }

    /// The list stored in `data_dir`. A missing file is an empty list; a file that cannot be
    /// read or parsed is an error (nothing is allowed then, and nothing is overwritten
    /// unseen: [`update`] refuses too).
    pub fn load(data_dir: &Path) -> Result<HostList, HostError> {
        match std::fs::read_to_string(path(data_dir)) {
            Ok(text) => HostList::from_json(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HostList::new()),
            Err(e) => Err(HostError::Unreadable(e.to_string())),
        }
    }

    /// Reads the list, lets `change` edit it and writes it back (also what `change` returns).
    pub fn update<T>(
        data_dir: &Path,
        change: impl FnOnce(&mut HostList) -> Result<T, HostError>,
    ) -> Result<T, HostError> {
        let _one = WRITE.lock().unwrap_or_else(|p| p.into_inner());
        let mut list = load(data_dir)?;
        let out = change(&mut list)?;
        let target = path(data_dir);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| HostError::Unreadable(e.to_string()))?;
        }
        let tmp = target.with_extension("json.tmp");
        std::fs::write(&tmp, list.to_json())
            .and_then(|()| std::fs::rename(&tmp, &target))
            .map_err(|e| HostError::Unreadable(e.to_string()))?;
        Ok(out)
    }
}
