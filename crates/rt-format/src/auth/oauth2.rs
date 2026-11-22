// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: oauth2` ↔ `Auth::OAuth2` (OpenCollection `AuthOAuth2`, four flows), and the
//! collection's default auth for flows OpenCollection lacks (device code, SAML 2.0 bearer)
//! in `extensions.requesttailor.auth` of `opencollection.yml`.
//!
//! File shape (Bruno's key order; empty strings are left out):
//!
//! ```yaml
//! type: oauth2
//! flow: authorization_code   # client_credentials | resource_owner_password_credentials | implicit
//! authorizationUrl: …        # authorization code, implicit
//! accessTokenUrl: …          # not implicit
//! refreshTokenUrl: …         # not implicit
//! callbackUrl: …             # authorization code, implicit
//! credentials: {clientId, clientSecret, placement: basic_auth_header | body}
//! resourceOwner: {username, password}   # password flow
//! scope: …
//! state: …                   # authorization code, implicit
//! additionalParameters:      # lists of {name, value, placement: header | query | body}
//!   authorizationRequest: […]
//!   accessTokenRequest: […]
//!   refreshTokenRequest: […]
//! pkce: {disabled: false, method: S256 | plain}   # authorization code
//! tokenConfig: {id, placement: {header: <prefix>} | {query: <name>}, source: access_token | id_token}
//! settings: {autoFetchToken, autoRefreshToken}
//! ```
//!
//! In `extensions.requesttailor.auth` the same mapping may also have `flow: device_code` with
//! `deviceAuthorizationUrl`, or `flow: saml2_bearer` with `assertion`.
//!
//! Reading defaults for keys a hand-written file leaves out: credentials in the Basic auth
//! header, PKCE off, token as `Authorization: Bearer`, auto fetch and auto refresh on. An
//! unknown flow reads as `Auth::Unsupported { kind: "oauth2" }` and stays in the file.
//!
//! Writing edits the existing mapping key by key (and inside `credentials`, `pkce`,
//! `tokenConfig`, `settings`), so what did not change stays byte for byte; the read-back
//! check of the request edit refuses anything this module cannot write.

use rt_core::model::{
    Auth, CredentialsPlacement, OAuth2, OAuth2Flow, OAuth2Param, OAuth2Step, ParamPlacement,
    PkceMethod, TokenPlacement, TokenSource, TokenUse,
};

use crate::read::{enabled, flag, text};
use crate::yaml::{Map, Node, Val};

/// Keys of an `oauth2` mapping in Bruno's order.
const ORDER: &[&str] = &[
    "type",
    "flow",
    "authorizationUrl",
    "deviceAuthorizationUrl",
    "accessTokenUrl",
    "refreshTokenUrl",
    "callbackUrl",
    "credentials",
    "resourceOwner",
    "assertion",
    "scope",
    "state",
    "additionalParameters",
    "pkce",
    "tokenConfig",
    "settings",
];

/// `additionalParameters` keys, in the order the reader lists the steps.
const STEPS: &[(OAuth2Step, &str)] = &[
    (OAuth2Step::Authorization, "authorizationRequest"),
    (OAuth2Step::AccessToken, "accessTokenRequest"),
    (OAuth2Step::Refresh, "refreshTokenRequest"),
];

/// Reads an `auth` mapping with `type: oauth2`.
pub(crate) fn read(node: &Node) -> Auth {
    read_any(node, false)
}

fn read_any(node: &Node, extended: bool) -> Auth {
    let field = |k: &str| text(node.get(k));
    let flow = match node.get("flow").and_then(Node::text).unwrap_or_default() {
        "client_credentials" => OAuth2Flow::ClientCredentials,
        "resource_owner_password_credentials" | "password" => {
            let owner = node.get("resourceOwner");
            OAuth2Flow::Password {
                username: text(owner.and_then(|o| o.get("username"))),
                password: text(owner.and_then(|o| o.get("password"))),
            }
        }
        "authorization_code" => OAuth2Flow::AuthorizationCode {
            authorization_url: field("authorizationUrl"),
            callback_url: field("callbackUrl"),
            state: field("state"),
            pkce: read_pkce(node.get("pkce")),
        },
        "implicit" => OAuth2Flow::Implicit {
            authorization_url: field("authorizationUrl"),
            callback_url: field("callbackUrl"),
            state: field("state"),
        },
        "device_code" if extended => OAuth2Flow::DeviceCode {
            device_authorization_url: field("deviceAuthorizationUrl"),
        },
        "saml2_bearer" if extended => OAuth2Flow::Saml2Bearer {
            assertion: field("assertion"),
        },
        _ => {
            return Auth::Unsupported {
                kind: "oauth2".to_owned(),
            };
        }
    };
    let implicit = matches!(flow, OAuth2Flow::Implicit { .. });
    let credentials = node.get("credentials");
    let cred = |k: &str| text(credentials.and_then(|c| c.get(k)));
    let settings = node.get("settings");
    let setting = |k: &str| flag(settings.and_then(|s| s.get(k))).unwrap_or(true);
    Auth::OAuth2(Box::new(OAuth2 {
        flow,
        access_token_url: if implicit {
            String::new()
        } else {
            field("accessTokenUrl")
        },
        refresh_token_url: if implicit {
            String::new()
        } else {
            field("refreshTokenUrl")
        },
        client_id: cred("clientId"),
        client_secret: cred("clientSecret"),
        credentials_placement: match cred("placement").as_str() {
            "body" => CredentialsPlacement::Body,
            _ => CredentialsPlacement::BasicAuthHeader,
        },
        scope: field("scope"),
        parameters: read_params(node.get("additionalParameters")),
        token: read_token(node.get("tokenConfig")),
        auto_fetch: setting("autoFetchToken"),
        auto_refresh: setting("autoRefreshToken"),
    }))
}

fn read_pkce(node: Option<&Node>) -> Option<PkceMethod> {
    let node = node?;
    if flag(node.get("disabled")) == Some(true) || flag(node.get("enabled")) == Some(false) {
        return None;
    }
    if !node.is_map() {
        // `pkce: true` / `pkce: false` as found in hand-written files.
        return node.as_bool().filter(|b| *b).map(|_| PkceMethod::S256);
    }
    Some(match node.get("method").and_then(Node::text) {
        Some("plain") => PkceMethod::Plain,
        _ => PkceMethod::S256,
    })
}

fn read_params(node: Option<&Node>) -> Vec<OAuth2Param> {
    let Some(node) = node else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (step, key) in STEPS {
        for p in node.get(key).into_iter().flat_map(Node::items) {
            if !enabled(p) {
                continue;
            }
            out.push(OAuth2Param {
                step: *step,
                name: text(p.get("name")),
                value: text(p.get("value")),
                placement: match p.get("placement").and_then(Node::text) {
                    Some("header") => ParamPlacement::Header,
                    Some("query") => ParamPlacement::Query,
                    Some("body") => ParamPlacement::Body,
                    // The browser step only has a query; token requests have a form body.
                    _ if *step == OAuth2Step::Authorization => ParamPlacement::Query,
                    _ => ParamPlacement::Body,
                },
            });
        }
    }
    out
}

fn read_token(node: Option<&Node>) -> TokenUse {
    let Some(node) = node else {
        return TokenUse::default();
    };
    let placement = node.get("placement");
    let placement = match (
        placement.and_then(|p| p.get("header")),
        placement.and_then(|p| p.get("query")),
    ) {
        (_, Some(q)) => TokenPlacement::Query {
            name: text(Some(q)),
        },
        (Some(h), None) => TokenPlacement::Header {
            prefix: text(Some(h)),
        },
        (None, None) => TokenUse::default().placement,
    };
    TokenUse {
        id: text(node.get("id")),
        placement,
        source: match node.get("source").and_then(Node::text) {
            Some("id_token") => TokenSource::IdToken,
            _ => TokenSource::AccessToken,
        },
    }
}

/// The collection default auth from `extensions.requesttailor.auth` of `opencollection.yml`
/// (`root` is that file's root), if set. It takes precedence over `request.auth`.
pub(crate) fn read_extension(root: &Node) -> Option<Auth> {
    let node = root
        .get("extensions")?
        .get("requesttailor")?
        .get("auth")
        .filter(|n| n.is_map())?;
    match node.get("type").and_then(Node::text) {
        Some("oauth2") => Some(read_any(node, true)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Writing

/// The mapping after `type` for `auth`, or an error for flows that have no OpenCollection
/// form (device code, SAML bearer: only as the collection default, see [`read_extension`]).
pub(crate) fn fields(auth: &OAuth2) -> Result<Vec<(&'static str, Val)>, String> {
    entries(auth, false)
}

/// The value of `extensions.requesttailor.auth` for `auth` (with `type`). Every flow,
/// including device code and SAML 2.0 bearer.
#[cfg(test)]
pub(crate) fn extension_val(auth: &OAuth2) -> Result<Val, String> {
    let mut e = vec![("type", Val::str("oauth2"))];
    e.extend(entries(auth, true)?);
    Ok(Val::Map(e))
}

fn put(e: &mut Vec<(&'static str, Val)>, key: &'static str, value: &str) {
    if !value.is_empty() {
        e.push((key, Val::str(value)));
    }
}

fn placement_str(p: ParamPlacement) -> &'static str {
    match p {
        ParamPlacement::Header => "header",
        ParamPlacement::Query => "query",
        ParamPlacement::Body => "body",
    }
}

fn entries(a: &OAuth2, extended: bool) -> Result<Vec<(&'static str, Val)>, String> {
    let only_default = || {
        format!(
            "the OAuth 2.0 {} flow can only be the collection's default auth \
             (`extensions.requesttailor.auth` in opencollection.yml)",
            match a.flow {
                OAuth2Flow::DeviceCode { .. } => "device code",
                _ => "SAML 2.0 bearer",
            }
        )
    };
    let (flow, implicit) = match &a.flow {
        OAuth2Flow::ClientCredentials => ("client_credentials", false),
        OAuth2Flow::Password { .. } => ("resource_owner_password_credentials", false),
        OAuth2Flow::AuthorizationCode { .. } => ("authorization_code", false),
        OAuth2Flow::Implicit { .. } => ("implicit", true),
        OAuth2Flow::DeviceCode { .. } if extended => ("device_code", false),
        OAuth2Flow::Saml2Bearer { .. } if extended => ("saml2_bearer", false),
        OAuth2Flow::DeviceCode { .. } | OAuth2Flow::Saml2Bearer { .. } => {
            return Err(only_default());
        }
    };
    if implicit && !(a.access_token_url.is_empty() && a.refresh_token_url.is_empty()) {
        return Err("the OAuth 2.0 implicit flow has no token URLs".to_owned());
    }
    let mut e = vec![("flow", Val::str(flow))];
    match &a.flow {
        OAuth2Flow::AuthorizationCode {
            authorization_url, ..
        }
        | OAuth2Flow::Implicit {
            authorization_url, ..
        } => put(&mut e, "authorizationUrl", authorization_url),
        OAuth2Flow::DeviceCode {
            device_authorization_url,
        } => put(&mut e, "deviceAuthorizationUrl", device_authorization_url),
        _ => {}
    }
    put(&mut e, "accessTokenUrl", &a.access_token_url);
    put(&mut e, "refreshTokenUrl", &a.refresh_token_url);
    if let OAuth2Flow::AuthorizationCode { callback_url, .. }
    | OAuth2Flow::Implicit { callback_url, .. } = &a.flow
    {
        put(&mut e, "callbackUrl", callback_url);
    }

    let mut cred = Vec::new();
    put(&mut cred, "clientId", &a.client_id);
    put(&mut cred, "clientSecret", &a.client_secret);
    // Implicit sends no credentials to a token endpoint; Bruno leaves the placement out.
    if !implicit || a.credentials_placement != CredentialsPlacement::BasicAuthHeader {
        cred.push((
            "placement",
            Val::str(match a.credentials_placement {
                CredentialsPlacement::BasicAuthHeader => "basic_auth_header",
                CredentialsPlacement::Body => "body",
            }),
        ));
    }
    if !cred.is_empty() {
        e.push(("credentials", Val::Map(cred)));
    }
    match &a.flow {
        OAuth2Flow::Password { username, password } => {
            let mut owner = Vec::new();
            put(&mut owner, "username", username);
            put(&mut owner, "password", password);
            if !owner.is_empty() {
                e.push(("resourceOwner", Val::Map(owner)));
            }
        }
        OAuth2Flow::Saml2Bearer { assertion } => put(&mut e, "assertion", assertion),
        _ => {}
    }
    put(&mut e, "scope", &a.scope);
    if let OAuth2Flow::AuthorizationCode { state, .. } | OAuth2Flow::Implicit { state, .. } =
        &a.flow
    {
        put(&mut e, "state", state);
    }

    if !a.parameters.is_empty() {
        let rank = |s: OAuth2Step| STEPS.iter().position(|(x, _)| *x == s).unwrap_or(0);
        if a.parameters
            .windows(2)
            .any(|w| rank(w[0].step) > rank(w[1].step))
        {
            return Err(
                "OAuth 2.0 extra parameters must be grouped by step (authorization, \
                        access token, refresh)"
                    .to_owned(),
            );
        }
        let mut groups = Vec::new();
        for (step, key) in STEPS {
            let list: Vec<Val> = a
                .parameters
                .iter()
                .filter(|p| p.step == *step)
                .map(|p| {
                    Val::Map(vec![
                        ("name", Val::str(&p.name)),
                        ("value", Val::str(&p.value)),
                        ("placement", Val::str(placement_str(p.placement))),
                    ])
                })
                .collect();
            if !list.is_empty() {
                groups.push((*key, Val::Seq(list)));
            }
        }
        e.push(("additionalParameters", Val::Map(groups)));
    }

    if let OAuth2Flow::AuthorizationCode { pkce, .. } = &a.flow {
        e.push((
            "pkce",
            Val::Map(match pkce {
                None => vec![("disabled", Val::bool(true))],
                Some(m) => vec![
                    ("disabled", Val::bool(false)),
                    (
                        "method",
                        Val::str(match m {
                            PkceMethod::S256 => "S256",
                            PkceMethod::Plain => "plain",
                        }),
                    ),
                ],
            }),
        ));
    }

    let mut token = Vec::new();
    put(&mut token, "id", &a.token.id);
    token.push((
        "placement",
        Val::Map(vec![match &a.token.placement {
            TokenPlacement::Header { prefix } => ("header", Val::str(prefix)),
            TokenPlacement::Query { name } => ("query", Val::str(name)),
        }]),
    ));
    token.push((
        "source",
        Val::str(match a.token.source {
            TokenSource::AccessToken => "access_token",
            TokenSource::IdToken => "id_token",
        }),
    ));
    e.push(("tokenConfig", Val::Map(token)));
    e.push((
        "settings",
        Val::Map(vec![
            ("autoFetchToken", Val::bool(a.auto_fetch)),
            ("autoRefreshToken", Val::bool(a.auto_refresh)),
        ]),
    ));
    Ok(e)
}

/// Edits the existing `auth` mapping (`type: oauth2`, read as `old`) into `new`: only keys
/// whose rendering changed are touched, recursively inside nested mappings, so comments,
/// quoting and keys the model does not know survive wherever nothing changed.
pub(crate) fn apply(map: &mut Map, old: &OAuth2, new: &OAuth2, nl: &str) -> Result<(), String> {
    let want = fields(new)?;
    // The file read as `old`, so `old` has a rendering; if not, rewrite every key.
    let had = fields(old).unwrap_or_default();
    sync(map, &had, &want, ORDER, nl);
    Ok(())
}

fn sync(
    map: &mut Map,
    had: &[(&'static str, Val)],
    want: &[(&'static str, Val)],
    order: &[&str],
    nl: &str,
) {
    let get = |list: &[(&'static str, Val)], key: &str| {
        list.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
    };
    let mut keys: Vec<&'static str> = had.iter().map(|(k, _)| *k).collect();
    for (k, _) in want {
        if !keys.contains(k) {
            keys.push(k);
        }
    }
    for key in keys {
        let (old, new) = (get(had, key), get(want, key));
        if old == new {
            continue;
        }
        match (old, new) {
            (_, None) => {
                map.remove(key);
            }
            (Some(Val::Map(o)), Some(Val::Map(n))) => {
                match map.get_mut(key).and_then(Node::block_map_mut) {
                    Some(inner) => {
                        let inner_order: Vec<&str> = n.iter().map(|(k, _)| *k).collect();
                        sync(inner, &o, &n, &inner_order, nl);
                    }
                    None => map.put(key, &Val::Map(n), order, false, nl),
                }
            }
            (_, Some(v)) => map.put(key, &v, order, false, nl),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::{document_text, parse, reparse};

    fn auth_of(yaml: &str) -> Auth {
        let doc = parse(yaml).unwrap();
        read(doc.root.get("auth").unwrap())
    }

    fn oauth_of(yaml: &str) -> OAuth2 {
        match auth_of(yaml) {
            Auth::OAuth2(o) => *o,
            other => panic!("{other:?}"),
        }
    }

    /// `auth` written as a new mapping and read back.
    fn round_trip(auth: &OAuth2) -> OAuth2 {
        let mut e = vec![("type", Val::str("oauth2"))];
        e.extend(fields(auth).unwrap());
        let text = document_text(&[("auth", Val::Map(e))], false);
        oauth_of(&text)
    }

    const BRUNO_CODE: &str = "auth:
  type: oauth2
  flow: authorization_code
  authorizationUrl: http://localhost:8081/authorize
  accessTokenUrl: http://localhost:8081/token
  callbackUrl: http://localhost:8081/callback
  credentials:
    clientId: inherited-client-id
    clientSecret: inherited-client-secret
    placement: body
  additionalParameters:
    authorizationRequest:
      - name: auth-header-key
        placement: header
        value: auth-header-value
    accessTokenRequest:
      - name: token-body-key
        placement: body
        value: token-body-value
    refreshTokenRequest:
      - name: refresh-query-key
        placement: query
        value: refresh-query-value
  pkce:
    disabled: true
  tokenConfig:
    id: credentials
    placement:
      header: Bearer
    source: access_token
  settings:
    autoFetchToken: true
    autoRefreshToken: false
";

    #[test]
    fn reads_brunos_authorization_code() {
        let o = oauth_of(BRUNO_CODE);
        assert_eq!(
            o.flow,
            OAuth2Flow::AuthorizationCode {
                authorization_url: "http://localhost:8081/authorize".into(),
                callback_url: "http://localhost:8081/callback".into(),
                state: String::new(),
                pkce: None,
            }
        );
        assert_eq!(o.access_token_url, "http://localhost:8081/token");
        assert_eq!(o.client_id, "inherited-client-id");
        assert_eq!(o.client_secret, "inherited-client-secret");
        assert_eq!(o.credentials_placement, CredentialsPlacement::Body);
        assert_eq!(
            o.parameters,
            vec![
                OAuth2Param {
                    step: OAuth2Step::Authorization,
                    name: "auth-header-key".into(),
                    value: "auth-header-value".into(),
                    placement: ParamPlacement::Header,
                },
                OAuth2Param {
                    step: OAuth2Step::AccessToken,
                    name: "token-body-key".into(),
                    value: "token-body-value".into(),
                    placement: ParamPlacement::Body,
                },
                OAuth2Param {
                    step: OAuth2Step::Refresh,
                    name: "refresh-query-key".into(),
                    value: "refresh-query-value".into(),
                    placement: ParamPlacement::Query,
                },
            ]
        );
        assert_eq!(o.token.id, "credentials");
        assert_eq!(
            o.token.placement,
            TokenPlacement::Header {
                prefix: "Bearer".into()
            }
        );
        assert!(o.auto_fetch);
        assert!(!o.auto_refresh);
        assert_eq!(round_trip(&o), o);
    }

    #[test]
    fn defaults_for_missing_keys() {
        let o = oauth_of("auth:\n  type: oauth2\n  flow: client_credentials\n");
        assert_eq!(o, OAuth2::default());
        let o = oauth_of(
            "auth:
  type: oauth2
  flow: authorization_code
  pkce:
    disabled: false
",
        );
        assert!(matches!(
            o.flow,
            OAuth2Flow::AuthorizationCode {
                pkce: Some(PkceMethod::S256),
                ..
            }
        ));
    }

    #[test]
    fn unknown_flows_and_extension_only_flows_in_requests_are_unsupported() {
        for flow in ["", "flow: magic", "flow: device_code", "flow: saml2_bearer"] {
            assert_eq!(
                auth_of(&format!("auth:\n  type: oauth2\n  {flow}\n")),
                Auth::Unsupported {
                    kind: "oauth2".into()
                },
                "{flow}"
            );
        }
    }

    fn every_flow() -> Vec<OAuth2Flow> {
        vec![
            OAuth2Flow::ClientCredentials,
            OAuth2Flow::Password {
                username: "alice".into(),
                password: "{{password}}".into(),
            },
            OAuth2Flow::AuthorizationCode {
                authorization_url: "https://idp/auth".into(),
                callback_url: "http://127.0.0.1:8765/callback".into(),
                state: "xyz".into(),
                pkce: Some(PkceMethod::Plain),
            },
            OAuth2Flow::AuthorizationCode {
                authorization_url: String::new(),
                callback_url: String::new(),
                state: String::new(),
                pkce: Some(PkceMethod::S256),
            },
            OAuth2Flow::Implicit {
                authorization_url: "https://idp/auth".into(),
                callback_url: "https://app/cb".into(),
                state: String::new(),
            },
        ]
    }

    fn full(flow: OAuth2Flow) -> OAuth2 {
        let implicit = matches!(flow, OAuth2Flow::Implicit { .. });
        let url = |s: &str| {
            if implicit {
                String::new()
            } else {
                s.to_owned()
            }
        };
        OAuth2 {
            flow,
            access_token_url: url("{{idp}}/token"),
            refresh_token_url: url("{{idp}}/refresh"),
            client_id: "client".into(),
            client_secret: "{{secret}}".into(),
            credentials_placement: CredentialsPlacement::Body,
            scope: "openid profile".into(),
            parameters: vec![
                OAuth2Param {
                    step: OAuth2Step::Authorization,
                    name: "prompt".into(),
                    value: "login".into(),
                    placement: ParamPlacement::Query,
                },
                OAuth2Param {
                    step: OAuth2Step::AccessToken,
                    name: "audience".into(),
                    value: "".into(),
                    placement: ParamPlacement::Header,
                },
            ],
            token: TokenUse {
                id: "shared".into(),
                placement: TokenPlacement::Query {
                    name: "access_token".into(),
                },
                source: TokenSource::IdToken,
            },
            auto_fetch: false,
            auto_refresh: true,
        }
    }

    #[test]
    fn every_flow_round_trips() {
        for flow in every_flow() {
            let o = full(flow.clone());
            assert_eq!(round_trip(&o), o, "{flow:?}");
            let minimal = OAuth2 {
                flow,
                ..OAuth2::default()
            };
            assert_eq!(round_trip(&minimal), minimal);
        }
        let empty_prefix = OAuth2 {
            token: TokenUse {
                placement: TokenPlacement::Header {
                    prefix: String::new(),
                },
                ..TokenUse::default()
            },
            ..OAuth2::default()
        };
        assert_eq!(round_trip(&empty_prefix), empty_prefix);
    }

    #[test]
    fn what_the_writer_refuses() {
        for flow in [
            OAuth2Flow::DeviceCode {
                device_authorization_url: "https://idp/device".into(),
            },
            OAuth2Flow::Saml2Bearer {
                assertion: "{{assertion}}".into(),
            },
        ] {
            let err = fields(&OAuth2 {
                flow,
                ..OAuth2::default()
            })
            .unwrap_err();
            assert!(err.contains("collection's default"), "{err}");
        }
        let implicit_with_token_url = OAuth2 {
            access_token_url: "https://idp/token".into(),
            ..full(every_flow().pop().unwrap())
        };
        assert!(fields(&implicit_with_token_url).is_err());
        let mut mixed = full(OAuth2Flow::ClientCredentials);
        mixed.parameters.reverse();
        assert!(fields(&mixed).unwrap_err().contains("grouped"));
    }

    #[test]
    fn the_collection_extension() {
        for flow in [
            OAuth2Flow::DeviceCode {
                device_authorization_url: "{{idp}}/device".into(),
            },
            OAuth2Flow::Saml2Bearer {
                assertion: "{{assertion}}".into(),
            },
            OAuth2Flow::ClientCredentials,
        ] {
            let o = OAuth2 {
                flow,
                ..full(OAuth2Flow::ClientCredentials)
            };
            let text = document_text(
                &[(
                    "extensions",
                    Val::Map(vec![(
                        "requesttailor",
                        Val::Map(vec![("auth", extension_val(&o).unwrap())]),
                    )]),
                )],
                false,
            );
            let doc = parse(&text).unwrap();
            assert_eq!(read_extension(&doc.root), Some(Auth::OAuth2(Box::new(o))));
        }
        let doc = parse("extensions: {}\n").unwrap();
        assert_eq!(read_extension(&doc.root), None);
        let doc = parse("extensions:\n  requesttailor:\n    auth:\n      type: basic\n").unwrap();
        assert_eq!(read_extension(&doc.root), None);
    }

    fn edited(yaml: &str, change: impl Fn(&mut OAuth2)) -> String {
        let old = oauth_of(yaml);
        let mut new = old.clone();
        change(&mut new);
        let mut doc = parse(yaml).unwrap();
        let nl = doc.newline;
        let map = doc
            .root
            .block_map_mut()
            .unwrap()
            .get_mut("auth")
            .unwrap()
            .block_map_mut()
            .unwrap();
        apply(map, &old, &new, nl).unwrap();
        let text = reparse(&doc).unwrap().to_text();
        assert_eq!(oauth_of(&text), new, "reads back:\n{text}");
        text
    }

    #[test]
    fn edits_touch_only_what_changed() {
        let text = edited(BRUNO_CODE, |o| o.scope = "openid".into());
        assert_eq!(
            text,
            BRUNO_CODE.replace(
                "    placement: body\n  additionalParameters:",
                "    placement: body\n  scope: openid\n  additionalParameters:"
            )
        );

        let text = edited(BRUNO_CODE, |o| o.client_id = "other".into());
        assert_eq!(text, BRUNO_CODE.replace("inherited-client-id", "other"));

        let text = edited(BRUNO_CODE, |o| o.auto_refresh = true);
        assert_eq!(
            text,
            BRUNO_CODE.replace("autoRefreshToken: false", "autoRefreshToken: true")
        );

        let text = edited(BRUNO_CODE, |o| {
            if let OAuth2Flow::AuthorizationCode { pkce, .. } = &mut o.flow {
                *pkce = Some(PkceMethod::S256);
            }
        });
        assert_eq!(
            text,
            BRUNO_CODE.replace(
                "    disabled: true\n",
                "    disabled: false\n    method: S256\n"
            )
        );

        // Comments and unknown keys elsewhere survive a change.
        let commented = BRUNO_CODE.replace(
            "  tokenConfig:",
            "  # how the token is used\n  tokenConfig:",
        );
        let text = edited(&commented, |o| {
            o.token.placement = TokenPlacement::Header {
                prefix: "Token".into(),
            }
        });
        assert_eq!(text, commented.replace("header: Bearer", "header: Token"));
    }

    #[test]
    fn edits_that_change_the_flow() {
        let text = edited(BRUNO_CODE, |o| {
            o.flow = OAuth2Flow::Password {
                username: "u".into(),
                password: "{{p}}".into(),
            }
        });
        assert!(text.contains("flow: resource_owner_password_credentials"));
        assert!(!text.contains("authorizationUrl"));
        assert!(!text.contains("pkce"));
        assert!(
            text.contains("resourceOwner:\n    username: u\n    password: "),
            "{text}"
        );

        edited(BRUNO_CODE, |o| o.parameters.clear());
        edited(BRUNO_CODE, |o| o.parameters.truncate(1));
    }

    #[test]
    fn refused_edits() {
        let old = oauth_of(BRUNO_CODE);
        let new = OAuth2 {
            flow: OAuth2Flow::DeviceCode {
                device_authorization_url: "x".into(),
            },
            ..old.clone()
        };
        let mut doc = parse(BRUNO_CODE).unwrap();
        let map = doc
            .root
            .block_map_mut()
            .unwrap()
            .get_mut("auth")
            .unwrap()
            .block_map_mut()
            .unwrap();
        assert!(apply(map, &old, &new, "\n").is_err());
    }

    /// Through the request writer (`write::apply_request`): switching a request to OAuth 2.0,
    /// editing it in place, refusing device code on a request.
    #[test]
    fn request_edits() {
        let path = "r.yml";
        let file = "info:\n  name: r\n  type: http\n\nhttp:\n  method: GET\n  url: https://api\n";
        let doc = parse(file).unwrap();
        let read = |doc: &crate::yaml::Document| {
            let docs = std::collections::BTreeMap::from([(path, doc)]);
            crate::read::Builder::new(&docs).request(path).unwrap().spec
        };
        let mut spec = read(&doc);
        spec.auth = Auth::OAuth2(Box::new(full(OAuth2Flow::ClientCredentials)));
        let doc = crate::write::apply_request(&doc, path, &spec).unwrap();
        assert_eq!(read(&doc), spec);
        let before = doc.to_text();

        if let Auth::OAuth2(o) = &mut spec.auth {
            o.client_id = "renamed".into();
        }
        let edited = crate::write::apply_request(&doc, path, &spec).unwrap();
        assert_eq!(
            edited.to_text(),
            before.replace("clientId: client", "clientId: renamed")
        );

        if let Auth::OAuth2(o) = &mut spec.auth {
            o.flow = OAuth2Flow::DeviceCode {
                device_authorization_url: "https://idp/device".into(),
            };
        }
        let err = crate::write::apply_request(&edited, path, &spec).unwrap_err();
        assert!(err.contains("collection's default"), "{err}");
    }
}
