// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Auth schemes beyond the simple ones. OAuth 2.0 and AWS Signature v4 map to the model and
//! are sent natively (wave 5); OAuth 1.0, Digest, NTLM and WSSE the core cannot execute yet:
//! importers keep their settings in the file (so nothing typed is lost and a later version
//! can use them) and report them.

use super::LossKind;
use super::common::{Losses, is_literal};
use super::out::AuthOut;
use crate::yaml::Val;

/// Settings of an OAuth 2.0 configuration, as the importers find them.
#[derive(Debug, Clone, Default)]
pub(crate) struct OAuth2 {
    /// `authorization_code`, `client_credentials`, `resource_owner_password_credentials` or
    /// `implicit`.
    pub flow: &'static str,
    pub authorization_url: String,
    pub access_token_url: String,
    pub refresh_token_url: String,
    pub callback_url: String,
    pub client_id: String,
    pub client_secret: String,
    /// Client credentials in the token request body instead of a Basic auth header.
    pub credentials_in_body: bool,
    pub scope: String,
    pub state: String,
    pub username: String,
    pub password: String,
    /// `Some(method)` (`S256` or `plain`) if PKCE is used.
    pub pkce: Option<&'static str>,
}

fn put(e: &mut Vec<(&'static str, Val)>, key: &'static str, value: &str) {
    if !value.is_empty() {
        e.push((key, Val::str(value)));
    }
}

/// The flow name for a grant type as Postman, Insomnia and OpenAPI spell it.
pub(crate) fn oauth2_flow(grant: &str) -> Option<&'static str> {
    match grant {
        "authorization_code" | "authorization_code_with_pkce" | "authorizationCode" => {
            Some("authorization_code")
        }
        "client_credentials" | "clientCredentials" => Some("client_credentials"),
        "password" | "password_credentials" | "resource_owner_password_credentials" => {
            Some("resource_owner_password_credentials")
        }
        "implicit" => Some("implicit"),
        _ => None,
    }
}

/// OAuth 2.0 as the model has it (sent natively since wave 5; implicit is kept but not run).
/// An unknown flow name is imported as authorization code (the importers report that).
pub(crate) fn oauth2(o: &OAuth2) -> AuthOut {
    use rt_core::model::{CredentialsPlacement, OAuth2Flow, PkceMethod};
    let implicit = o.flow == "implicit";
    let flow = match o.flow {
        "client_credentials" => OAuth2Flow::ClientCredentials,
        "resource_owner_password_credentials" => OAuth2Flow::Password {
            username: o.username.clone(),
            password: o.password.clone(),
        },
        "implicit" => OAuth2Flow::Implicit {
            authorization_url: o.authorization_url.clone(),
            callback_url: o.callback_url.clone(),
            state: o.state.clone(),
        },
        _ => OAuth2Flow::AuthorizationCode {
            authorization_url: o.authorization_url.clone(),
            callback_url: o.callback_url.clone(),
            state: o.state.clone(),
            pkce: o.pkce.map(|m| match m {
                "plain" => PkceMethod::Plain,
                _ => PkceMethod::S256,
            }),
        },
    };
    let unless_implicit = |s: &str| {
        if implicit {
            String::new()
        } else {
            s.to_owned()
        }
    };
    AuthOut::Model(rt_core::model::Auth::OAuth2(Box::new(
        rt_core::model::OAuth2 {
            flow,
            access_token_url: unless_implicit(&o.access_token_url),
            refresh_token_url: unless_implicit(&o.refresh_token_url),
            client_id: o.client_id.clone(),
            client_secret: unless_implicit(&o.client_secret),
            credentials_placement: if o.credentials_in_body {
                CredentialsPlacement::Body
            } else {
                CredentialsPlacement::BasicAuthHeader
            },
            scope: o.scope.clone(),
            ..rt_core::model::OAuth2::default()
        },
    )))
}

pub(crate) fn user_password(kind: &'static str, username: &str, password: &str) -> AuthOut {
    let mut e = Vec::new();
    put(&mut e, "username", username);
    put(&mut e, "password", password);
    AuthOut::Other { kind, fields: e }
}

pub(crate) fn ntlm(username: &str, password: &str, domain: &str) -> AuthOut {
    let mut e = Vec::new();
    put(&mut e, "username", username);
    put(&mut e, "password", password);
    put(&mut e, "domain", domain);
    AuthOut::Other {
        kind: "ntlm",
        fields: e,
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AwsV4 {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    pub service: String,
    pub region: String,
}

pub(crate) fn awsv4(a: &AwsV4) -> AuthOut {
    AuthOut::Model(rt_core::model::Auth::AwsSigV4(rt_core::model::AwsSigV4 {
        access_key_id: a.access_key_id.clone(),
        secret_access_key: a.secret_access_key.clone(),
        session_token: a.session_token.clone(),
        service: a.service.clone(),
        region: a.region.clone(),
        profile_name: String::new(),
    }))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OAuth1 {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
    pub callback_url: String,
    pub verifier: String,
    pub signature_method: String,
    pub timestamp: String,
    pub nonce: String,
    pub version: String,
    pub realm: String,
}

const SIGNATURE_METHODS: &[&str] = &[
    "HMAC-SHA1",
    "HMAC-SHA256",
    "HMAC-SHA512",
    "RSA-SHA1",
    "RSA-SHA256",
    "RSA-SHA512",
    "PLAINTEXT",
];

/// OAuth 1.0; an unknown signature method is left out (the caller reports it).
pub(crate) fn oauth1(o: &OAuth1) -> AuthOut {
    let mut e = Vec::new();
    put(&mut e, "consumerKey", &o.consumer_key);
    put(&mut e, "consumerSecret", &o.consumer_secret);
    put(&mut e, "accessToken", &o.access_token);
    put(&mut e, "accessTokenSecret", &o.access_token_secret);
    put(&mut e, "callbackUrl", &o.callback_url);
    put(&mut e, "verifier", &o.verifier);
    let method = o.signature_method.to_ascii_uppercase();
    if SIGNATURE_METHODS.contains(&method.as_str()) {
        e.push(("signatureMethod", Val::str(method)));
    }
    put(&mut e, "timestamp", &o.timestamp);
    put(&mut e, "nonce", &o.nonce);
    put(&mut e, "version", &o.version);
    put(&mut e, "realm", &o.realm);
    AuthOut::Other {
        kind: "oauth1",
        fields: e,
    }
}

/// `true` if OAuth 1.0's signature method cannot be written.
pub(crate) fn oauth1_method_unknown(o: &OAuth1) -> bool {
    !o.signature_method.is_empty()
        && !SIGNATURE_METHODS.contains(&o.signature_method.to_ascii_uppercase().as_str())
}

/// Fields of an `Other` auth that hold credentials.
const SECRET_FIELDS: &[&str] = &[
    "password",
    "clientSecret",
    "secretAccessKey",
    "sessionToken",
    "consumerSecret",
    "accessToken",
    "accessTokenSecret",
];

/// Reports an auth that is kept but cannot be sent yet, and literal credentials in it.
pub(crate) fn report(losses: &mut Losses, location: &str, auth: &AuthOut, label: &str) {
    let fields = match auth {
        AuthOut::Other { fields, .. } => {
            losses.approximated(
                location,
                LossKind::Auth,
                format!(
                    "{label} settings kept in the file, but RequestTailor cannot send with \
                     {label} yet"
                ),
            );
            fields.clone()
        }
        // Sent natively since wave 5; only literal credentials are worth a note.
        AuthOut::Model(rt_core::model::Auth::AwsSigV4(a)) => crate::auth::awsv4::fields(a),
        AuthOut::Model(rt_core::model::Auth::OAuth2(o)) => {
            if let rt_core::model::OAuth2Flow::Implicit { .. } = o.flow {
                losses.approximated(
                    location,
                    LossKind::Auth,
                    format!(
                        "{label} implicit flow kept in the file, but RequestTailor does not run \
                         it (OAuth 2.1 removes it); switch to authorization code with PKCE"
                    ),
                );
            }
            crate::auth::oauth2::fields(o).unwrap_or_default()
        }
        AuthOut::Model(_) => return,
    };
    let mut literal = Vec::new();
    collect_literals(&fields, &mut literal);
    if !literal.is_empty() {
        losses.approximated(
            location,
            LossKind::Auth,
            format!(
                "{label} {} stored in the request file as typed; consider secret variables \
                 (set their values in Environments)",
                literal.join(", ")
            ),
        );
    }
}

fn collect_literals(fields: &[(&'static str, Val)], out: &mut Vec<&'static str>) {
    for (k, v) in fields {
        match v {
            Val::Str(s) if SECRET_FIELDS.contains(k) && is_literal(s) => {
                if !out.contains(k) {
                    out.push(k);
                }
            }
            Val::Map(m) => collect_literals(m, out),
            _ => {}
        }
    }
}
