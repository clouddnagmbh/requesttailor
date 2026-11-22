// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Auth beyond the simple header schemes (wave 5): OAuth 2.0, AWS Signature V4, client
//! certificates and external secret sources. All values are templates (`{{var}}`), resolved
//! like every other field when sending.
//!
//! File mapping (OpenCollection 1.0): `oauth2` with the flows `client_credentials`,
//! `resource_owner_password_credentials`, `authorization_code` and `implicit`; `awsv4`;
//! `config.clientCertificates` and `environments[].clientCertificates`;
//! `environments[].externalSecrets`. Device code and SAML 2.0 bearer have no OpenCollection
//! form: they can only be the collection's default auth and are stored in
//! `extensions.requesttailor.auth` of `opencollection.yml` (see `rt-format`).
//!
//! Owners in wave 5: OAuth 2.0 strand C, SigV4 and certificates strand D, external secrets
//! strand E. Changes to these types are interface changes.

use serde::{Deserialize, Serialize};

/// OAuth 2.0 as declared on a request, folder or collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuth2 {
    /// Flattened: the JSON is `{"type": "oauth2", "flow": "authorization-code", ...}`.
    #[serde(flatten)]
    pub flow: OAuth2Flow,
    /// Token endpoint. Empty for `Implicit`.
    pub access_token_url: String,
    /// Refresh endpoint; empty means the token endpoint.
    pub refresh_token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub credentials_placement: CredentialsPlacement,
    /// Space-separated scopes.
    pub scope: String,
    pub parameters: Vec<OAuth2Param>,
    pub token: TokenUse,
    /// Fetch a token when none is cached (otherwise sending fails with "no token").
    pub auto_fetch: bool,
    /// Refresh an expired token with its refresh token before sending.
    pub auto_refresh: bool,
}

impl Default for OAuth2 {
    fn default() -> Self {
        Self {
            flow: OAuth2Flow::ClientCredentials,
            access_token_url: String::new(),
            refresh_token_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            credentials_placement: CredentialsPlacement::BasicAuthHeader,
            scope: String::new(),
            parameters: Vec::new(),
            token: TokenUse::default(),
            auto_fetch: true,
            auto_refresh: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "flow", rename_all = "kebab-case")]
pub enum OAuth2Flow {
    ClientCredentials,
    /// Resource owner password credentials.
    Password {
        username: String,
        password: String,
    },
    AuthorizationCode {
        authorization_url: String,
        /// Redirect URI registered at the provider. A loopback URL (`http://127.0.0.1:<port>/…`
        /// or `localhost`) is served by the shell itself while the browser flow runs.
        callback_url: String,
        state: String,
        /// `None`: PKCE disabled.
        pkce: Option<PkceMethod>,
    },
    /// Declared in files, not executed (deprecated by OAuth 2.1): sending fails clearly.
    Implicit {
        authorization_url: String,
        callback_url: String,
        state: String,
    },
    /// RFC 8628. Collection default only (stored in `extensions`).
    DeviceCode {
        device_authorization_url: String,
    },
    /// RFC 7522: exchanges a SAML 2.0 assertion (base64url, usually a variable) for a token.
    /// Collection default only (stored in `extensions`).
    Saml2Bearer {
        assertion: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PkceMethod {
    S256,
    #[serde(rename = "plain")]
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialsPlacement {
    /// `Authorization: Basic base64(client_id:client_secret)`.
    BasicAuthHeader,
    /// `client_id` and `client_secret` as form fields.
    Body,
}

/// Which request of the flow an extra parameter belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OAuth2Step {
    Authorization,
    AccessToken,
    Refresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamPlacement {
    Header,
    Query,
    Body,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuth2Param {
    pub step: OAuth2Step,
    pub name: String,
    pub value: String,
    pub placement: ParamPlacement,
}

/// How the token is put on the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUse {
    /// Cache key part chosen by the user (OpenCollection `tokenConfig.id`), so several
    /// requests can share one token. Empty: derived from the flow settings.
    pub id: String,
    pub placement: TokenPlacement,
    pub source: TokenSource,
}

impl Default for TokenUse {
    fn default() -> Self {
        Self {
            id: String::new(),
            placement: TokenPlacement::Header {
                prefix: "Bearer".to_owned(),
            },
            source: TokenSource::AccessToken,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "in", rename_all = "lowercase")]
pub enum TokenPlacement {
    /// `Authorization: <prefix> <token>` (no space if `prefix` is empty).
    Header { prefix: String },
    /// `?<name>=<token>`.
    Query { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenSource {
    AccessToken,
    IdToken,
}

/// AWS Signature Version 4 (OpenCollection `awsv4`). An empty `access_key_id` with a
/// `profile_name` means "credentials from that profile" (native only).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AwsSigV4 {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    pub service: String,
    pub region: String,
    pub profile_name: String,
}

/// A TLS client certificate for one host (`domain`, `*.` wildcards allowed, port optional).
/// Paths are relative to the collection root. The passphrase is a template, normally a
/// secret variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCertificate {
    pub domain: String,
    pub source: CertificateSource,
    pub passphrase: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CertificateSource {
    Pem {
        certificate_path: String,
        private_key_path: String,
    },
    Pkcs12 {
        path: String,
    },
}

/// Collection-wide settings (OpenCollection `config`) that the model knows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CollectionConfig {
    pub client_certificates: Vec<ClientCertificate>,
    /// `config.protobuf` (wave 8, gRPC).
    #[serde(default)]
    pub protobuf: super::ProtobufConfig,
    /// `extensions.requesttailor.agents` (wave 8, MCP server).
    #[serde(default)]
    pub agents: super::AgentPolicy,
    /// `extensions.requesttailor.http3` (wave 8): try HTTP/3 natively (QUIC). Off by default;
    /// with it on, the native transport uses HTTP/3 for `https` URLs and falls back to
    /// HTTP/2 or 1.1 when QUIC fails. The browser decides on its own.
    #[serde(default)]
    pub http3: bool,
}

/// Secrets of an environment held by an external manager (OpenCollection `externalSecrets`).
/// Each entry provides the value of the variable `name`; the file holds only the reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalSecrets {
    pub provider: SecretProvider,
    pub variables: Vec<ExternalSecret>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretProvider {
    HashicorpVaultCloud,
    HashicorpVaultServer,
    AwsSecretsManager,
    AzureKeyVault,
    GcpSecretsManager,
    OnePassword,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalSecret {
    pub name: String,
    /// Provider-specific locator: Vault `path`, AWS `secretName`, Azure `vaultName`, GCP
    /// `projectId`, 1Password `secretReference` (`op://vault/item/field`).
    pub reference: String,
    pub enabled: bool,
}
