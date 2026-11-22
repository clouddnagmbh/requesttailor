// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The cookie jar of the desktop app (wave 5, strand D): one jar per collection and
//! environment, persisted in the data folder, passed to `send_with` as `CookieJar`. Commands:
//! `cookie_list`, `cookie_delete`, `cookie_clear`.
//!
//! - `cookies.json` in the app's data folder maps each collection id (its canonical root) to
//!   its [`CookieJars`] (`{"version": 1, "scopes": {"<environment>": [...]}}`, `""` for no
//!   environment). Never inside the collection folder: cookies are session state and often
//!   credentials. On Unix the file is readable by the user only.
//! - Every change (a send that sets or expires cookies, delete, clear) rewrites the file
//!   (temporary file, then rename). A write error is logged; the jar stays in memory.
//! - An unreadable file gives empty jars (losing cookies is not an error).
//! - Without a data folder (`None`, tests) the jars live in memory only.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rt_app::AppError;
use rt_app::cookies::CookieInfo;
use rt_core::cookies::{CookieJars, now_ms};
use rt_core::services::{CookieJar, LocalBoxFuture};
use tauri::State;

use crate::commands::{AppCore, CommandError};

/// File name of the cookie jars in the app's data folder.
pub const COOKIES_FILE: &str = "cookies.json";

#[tauri::command]
pub async fn cookie_list(
    core: State<'_, AppCore>,
    environment: String,
) -> Result<Vec<CookieInfo>, CommandError> {
    let Some(id) = core.collection_id() else {
        return Ok(Vec::new());
    };
    Ok(core.services.cookies.list(&id, &environment))
}

#[tauri::command]
pub async fn cookie_delete(
    core: State<'_, AppCore>,
    environment: String,
    domain: String,
    path: String,
    name: String,
) -> Result<(), CommandError> {
    let Some(id) = core.collection_id() else {
        return Ok(());
    };
    Ok(core
        .services
        .cookies
        .delete(&id, &environment, &domain, &path, &name)?)
}

#[tauri::command]
pub async fn cookie_clear(
    core: State<'_, AppCore>,
    environment: String,
) -> Result<(), CommandError> {
    let Some(id) = core.collection_id() else {
        return Ok(());
    };
    Ok(core.services.cookies.clear(&id, &environment)?)
}

/// The cookie jars of all collections, kept by `Core`.
pub struct CookieParts {
    jars: Mutex<BTreeMap<String, CookieJars>>,
    /// `None`: in memory only.
    file: Option<PathBuf>,
}

impl CookieParts {
    pub fn new(data_dir: Option<&Path>) -> Self {
        let file = data_dir.map(|d| d.join(COOKIES_FILE));
        let jars = file.as_deref().map(load).unwrap_or_default();
        Self {
            jars: Mutex::new(jars),
            file,
        }
    }

    pub fn for_send(&self, collection: &str) -> CookieSend<'_> {
        CookieSend {
            parts: self,
            collection: collection.to_owned(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, CookieJars>> {
        self.jars.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The live cookies of `environment` (`""`: none) in `collection`.
    pub fn list(&self, collection: &str, environment: &str) -> Vec<CookieInfo> {
        match self.lock().get(collection) {
            Some(jars) => rt_app::cookies::list(jars, environment, now_ms()),
            None => Vec::new(),
        }
    }

    pub fn delete(
        &self,
        collection: &str,
        environment: &str,
        domain: &str,
        path: &str,
        name: &str,
    ) -> Result<(), AppError> {
        self.change(collection, |jars| {
            jars.remove(environment, domain, path, name);
        })
    }

    pub fn clear(&self, collection: &str, environment: &str) -> Result<(), AppError> {
        self.change(collection, |jars| jars.clear(environment))
    }

    /// Applies `f` to the jars of `collection` and saves them if anything changed.
    fn change(&self, collection: &str, f: impl FnOnce(&mut CookieJars)) -> Result<(), AppError> {
        let mut all = self.lock();
        let jars = all
            .entry(collection.to_owned())
            .or_insert_with(CookieJars::new);
        let before = jars.clone();
        f(jars);
        let changed = *jars != before;
        if jars.scopes.is_empty() {
            all.remove(collection);
        }
        if !changed {
            return Ok(());
        }
        self.save(&all)
            .map_err(|e| AppError::Shell(format!("cannot save the cookies: {e}")))
    }

    fn save(&self, all: &BTreeMap<String, CookieJars>) -> io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec(all).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        write_private(&tmp, &bytes)?;
        fs::rename(&tmp, path)
    }
}

/// Writes `bytes` to a new file only the user can read (Unix; elsewhere the data folder's
/// permissions apply).
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

fn load(path: &Path) -> BTreeMap<String, CookieJars> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(e) => {
            eprintln!("RequestTailor: cannot read the cookies: {e}");
            return BTreeMap::new();
        }
    };
    let Ok(raw) = serde_json::from_slice::<BTreeMap<String, serde_json::Value>>(&bytes) else {
        eprintln!("RequestTailor: the cookie file is damaged; starting with empty jars");
        return BTreeMap::new();
    };
    raw.into_iter()
        .map(|(id, value)| {
            let jars = CookieJars::from_json(&serde_json::to_vec(&value).unwrap_or_default());
            (id, jars)
        })
        .filter(|(_, jars)| !jars.scopes.is_empty())
        .collect()
}

/// The jar of one send (the scope inside it is the environment).
pub struct CookieSend<'a> {
    parts: &'a CookieParts,
    collection: String,
}

impl CookieSend<'_> {
    pub fn jar(&self) -> Option<&dyn rt_core::services::CookieJar> {
        Some(self)
    }
}

impl CookieJar for CookieSend<'_> {
    fn cookie_header<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
    ) -> LocalBoxFuture<'a, Option<String>> {
        let header = self
            .parts
            .lock()
            .get(&self.collection)
            .and_then(|jars| jars.header(scope, url, now_ms()));
        Box::pin(std::future::ready(header))
    }

    fn store<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
        set_cookie: &'a [String],
    ) -> LocalBoxFuture<'a, ()> {
        let now = now_ms();
        if let Err(e) = self.parts.change(&self.collection, |jars| {
            jars.store(scope, url, set_cookie, now)
        }) {
            // Never fails the send: the cookies stay in memory.
            eprintln!("RequestTailor: {e}");
        }
        Box::pin(std::future::ready(()))
    }
}
