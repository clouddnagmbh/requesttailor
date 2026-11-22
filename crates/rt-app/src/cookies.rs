// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The cookie jar as the UI lists it (wave 5, strand D). One jar per collection and
//! environment, persisted by the shell next to the history (not in collection files: cookies
//! are session state and often credentials). Changes here are interface changes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CookieInfo {
    pub name: String,
    /// Shown in the cookie view only; never recorded in the history or given to agents.
    pub value: String,
    pub domain: String,
    pub path: String,
    /// Unix milliseconds; `None` for a session cookie.
    pub expires: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
}

impl From<&rt_core::cookies::Cookie> for CookieInfo {
    fn from(c: &rt_core::cookies::Cookie) -> Self {
        Self {
            name: c.name.clone(),
            value: c.value.clone(),
            domain: c.domain.clone(),
            path: c.path.clone(),
            expires: c.expires,
            secure: c.secure,
            http_only: c.http_only,
            same_site: c.same_site.clone(),
        }
    }
}

/// The live cookies of `environment` (`""`: none) at `now` (Unix ms), sorted by domain,
/// path and name: what `CookieApi.list` returns in every shell.
pub fn list(jars: &rt_core::cookies::CookieJars, environment: &str, now: i64) -> Vec<CookieInfo> {
    let Some(store) = jars.scope(environment) else {
        return Vec::new();
    };
    let mut out: Vec<CookieInfo> = store
        .cookies()
        .iter()
        .filter(|c| c.expires.is_none_or(|e| e > now))
        .map(CookieInfo::from)
        .collect();
    out.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
    out
}

#[cfg(test)]
mod tests {
    use rt_core::cookies::CookieJars;

    use super::*;

    #[test]
    fn lists_live_cookies_of_one_environment_sorted() {
        let mut jars = CookieJars::new();
        let set = [
            "z=1; Path=/".to_owned(),
            "a=2; Path=/; Secure; HttpOnly; SameSite=Lax".to_owned(),
            "old=3; Path=/; Max-Age=10".to_owned(),
        ];
        jars.store("Local", "https://api.example.com/x", &set, 1_000);
        jars.store(
            "Prod",
            "https://api.example.com/",
            &["p=1".to_owned()],
            1_000,
        );

        let listed = list(&jars, "Local", 5_000);
        let names: Vec<&str> = listed.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "old", "z"]);
        let a = &listed[0];
        assert_eq!(
            (a.value.as_str(), a.domain.as_str(), a.path.as_str()),
            ("2", "api.example.com", "/")
        );
        assert!(a.secure && a.http_only);
        assert_eq!(a.same_site.as_deref(), Some("Lax"));
        assert_eq!(listed[1].expires, Some(11_000));

        // Expired cookies are not listed; other environments have their own jar.
        let later: Vec<String> = list(&jars, "Local", 20_000)
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(later, ["a", "z"]);
        assert_eq!(list(&jars, "Prod", 0).len(), 1);
        assert_eq!(list(&jars, "", 0), Vec::new());
    }
}
