// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reading a BTP service key of the Integration Suite (Cloud Integration). Only what goes into
//! an environment is kept: the client secret is checked for presence and dropped on the spot,
//! so no value of it can reach a file, a report or a note.

use serde_json::Value;

/// Which of the two interfaces of a tenant a key is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The tenant's management and monitoring API (`/api/v1/...`).
    Api,
    /// The runtime, where the iFlow endpoints (`/http/...`) are.
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// The file it came from.
    pub file: String,
    pub client_id: String,
    /// Whether the key had a client secret (its value is gone).
    pub has_secret: bool,
    pub token_url: String,
    /// The service URL without a trailing slash.
    pub url: String,
}

impl Key {
    /// A runtime key has `-rt` in the first label of its host; every other key is for the API.
    pub fn side(&self) -> Side {
        if self.host().split('.').take(2).any(|l| l.ends_with("-rt")) {
            Side::Runtime
        } else {
            Side::Api
        }
    }

    fn host(&self) -> &str {
        let rest = self.url.split("://").nth(1).unwrap_or(&self.url);
        rest.split(['/', '?', '#']).next().unwrap_or("")
    }

    /// The first label of the host (`rt-test` for `rt-test.it-cpi001...`).
    fn tenant_label(&self) -> &str {
        self.host().split('.').next().unwrap_or("")
    }

    /// The tenant's name: the first host label.
    pub fn tenant(&self) -> String {
        self.tenant_label().to_owned()
    }

    /// The URL of the other interface when it follows the usual host pattern
    /// (`<tenant>.it-cpi001` and `<tenant>.it-cpi001-rt`); `None` for other hosts.
    pub fn other_url(&self) -> Option<String> {
        let (scheme, rest) = self.url.split_once("://")?;
        let (host, path) = rest.split_once('/').map_or((rest, ""), |(h, p)| (h, p));
        let mut labels: Vec<&str> = host.split('.').collect();
        let second = *labels.get(1)?;
        if !second.starts_with("it-cpi") {
            return None;
        }
        let swapped = match second.strip_suffix("-rt") {
            Some(base) => base.to_owned(),
            None => format!("{second}-rt"),
        };
        labels[1] = &swapped;
        let host = labels.join(".");
        Some(if path.is_empty() {
            format!("{scheme}://{host}")
        } else {
            format!("{scheme}://{host}/{path}")
        })
    }
}

/// Reads a key from a JSON file; `None` when the file is not a service key. Understood shapes:
/// the `oauth` object of the Cloud Integration plans, and the generic BTP shape with a `uaa`
/// object and a top-level `url`.
pub fn parse(file: &str, text: &str) -> Option<Key> {
    let value: Value = serde_json::from_str(text).ok()?;
    let str_of = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let oauth = value.get("oauth").filter(|o| o.is_object());
    let uaa = value.get("uaa").filter(|o| o.is_object());
    let (client, token_url, url) = if let Some(o) = oauth {
        let token = str_of(o, "tokenurl");
        (o, token, str_of(o, "url").or_else(|| str_of(&value, "url")))
    } else if let Some(u) = uaa {
        let token =
            str_of(u, "url").map(|base| format!("{}/oauth/token", base.trim_end_matches('/')));
        (u, token, str_of(&value, "url"))
    } else if value.get("clientid").is_some() {
        (&value, str_of(&value, "tokenurl"), str_of(&value, "url"))
    } else {
        return None;
    };
    let client_id = str_of(client, "clientid")?;
    let url = url?;
    let token_url = token_url?;
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return None;
    }
    let token_url = if token_url.ends_with("/oauth/token") {
        token_url
    } else {
        format!("{}/oauth/token", token_url.trim_end_matches('/'))
    };
    Some(Key {
        file: file.to_owned(),
        client_id,
        has_secret: str_of(client, "clientsecret").is_some(),
        token_url,
        url: url.trim_end_matches('/').to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_shapes_and_the_side() {
        let api = r#"{"oauth":{"clientid":"c","clientsecret":"s","url":"https://t.it-cpi001.cfapps.eu10.hana.ondemand.com/","tokenurl":"https://t.authentication.eu10.hana.ondemand.com/oauth/token"}}"#;
        let k = parse("a.json", api).unwrap();
        assert_eq!(k.side(), Side::Api);
        assert!(k.has_secret);
        assert_eq!(k.url, "https://t.it-cpi001.cfapps.eu10.hana.ondemand.com");
        assert_eq!(
            k.other_url().as_deref(),
            Some("https://t.it-cpi001-rt.cfapps.eu10.hana.ondemand.com")
        );
        let rt = r#"{"oauth":{"clientid":"c","clientsecret":"s","url":"https://t.it-cpi001-rt.cfapps.eu10.hana.ondemand.com","tokenurl":"https://t.authentication.eu10.hana.ondemand.com"}}"#;
        let k = parse("b.json", rt).unwrap();
        assert_eq!(k.side(), Side::Runtime);
        assert!(k.token_url.ends_with("/oauth/token"));
        assert_eq!(
            k.other_url().as_deref(),
            Some("https://t.it-cpi001.cfapps.eu10.hana.ondemand.com")
        );
        let uaa = r#"{"url":"https://u.example.com","uaa":{"clientid":"c","clientsecret":"s","url":"https://u.auth.example.com"}}"#;
        let k = parse("c.json", uaa).unwrap();
        assert_eq!(k.token_url, "https://u.auth.example.com/oauth/token");
        assert_eq!(k.other_url(), None);
        assert!(parse("d.json", r#"{"openapi":"3.0.0"}"#).is_none());
        assert!(parse("e.json", "[1]").is_none());
    }
}
