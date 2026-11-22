// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The cookie jar for `rt send --cookie-jar FILE` (wave 5, strand D).
//!
//! FILE holds [`CookieJars`] as JSON (`{"version": 1, "scopes": {"<environment>": [...]}}`,
//! `""` without `--env`), the same format the desktop app keeps per collection. A missing
//! file is an empty jar and is created on the first cookie; a file that is not a jar is
//! refused (rather than overwritten). Cookies the response sets or expires are saved right
//! after the send (temporary file, then rename; readable by the user only on Unix). Without
//! `--cookie-jar` no cookies are sent or kept. Values are masked in the report.

use std::io;
use std::path::{Path, PathBuf};

use rt_core::cookies::{CookieJars, JARS_VERSION, SharedJars, now_ms};
use rt_core::services::{CookieJar, LocalBoxFuture};

use crate::SendArgs;

pub struct CliCookies {
    jars: SharedJars,
    file: PathBuf,
}

impl CliCookies {
    /// `None` without `--cookie-jar`.
    pub fn new(args: &SendArgs) -> Result<Option<Self>, String> {
        let Some(file) = &args.cookie_jar else {
            return Ok(None);
        };
        let jars = load(file)?;
        Ok(Some(Self {
            jars: SharedJars::new(jars),
            file: file.clone(),
        }))
    }

    pub fn jar(&self) -> &dyn CookieJar {
        self
    }

    fn save(&self, jars: &CookieJars) -> io::Result<()> {
        let tmp = self.file.with_extension("rt-tmp");
        write_private(&tmp, &jars.to_json())?;
        std::fs::rename(&tmp, &self.file)
    }
}

fn load(file: &Path) -> Result<CookieJars, String> {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(CookieJars::new()),
        Err(e) => {
            return Err(format!(
                "cannot read the cookie jar {}: {e}",
                file.display()
            ));
        }
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(CookieJars::new());
    }
    match serde_json::from_slice::<CookieJars>(&bytes) {
        Ok(jars) if jars.version == JARS_VERSION => Ok(jars),
        Ok(jars) => Err(format!(
            "the cookie jar {} has version {}; this rt reads version {JARS_VERSION}",
            file.display(),
            jars.version
        )),
        Err(e) => Err(format!(
            "{} is not a RequestTailor cookie jar ({e}); refusing to overwrite it",
            file.display()
        )),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

impl CookieJar for CliCookies {
    fn cookie_header<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
    ) -> LocalBoxFuture<'a, Option<String>> {
        self.jars.cookie_header(scope, url)
    }

    fn store<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
        set_cookie: &'a [String],
    ) -> LocalBoxFuture<'a, ()> {
        let now = now_ms();
        let (changed, jars) = self.jars.with(|jars| {
            let before = jars.clone();
            jars.store(scope, url, set_cookie, now);
            (*jars != before, jars.clone())
        });
        if changed && let Err(e) = self.save(&jars) {
            // The send itself succeeded; say why the jar did not change.
            eprintln!(
                "rt: cannot save the cookie jar {}: {e}",
                self.file.display()
            );
        }
        Box::pin(std::future::ready(()))
    }
}
