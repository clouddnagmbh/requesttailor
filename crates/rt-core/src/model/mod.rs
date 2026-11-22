// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The internal request model. Format-neutral: `rt-format` maps files to these types, and
//! everything else (variables, transport, CLI, WASM, UI) works only on them.
//!
//! Changes here are interface changes (working rule 1): they go in their own commit.

mod agents;
mod auth;
mod collection;
mod env;
mod protocol;
mod request;
mod script;

pub use agents::{AgentAccess, AgentPolicy, EnvironmentPolicy};
pub use auth::{
    AwsSigV4, CertificateSource, ClientCertificate, CollectionConfig, CredentialsPlacement,
    ExternalSecret, ExternalSecrets, OAuth2, OAuth2Flow, OAuth2Param, OAuth2Step, ParamPlacement,
    PkceMethod, SecretProvider, TokenPlacement, TokenSource, TokenUse,
};
pub use collection::{Collection, Folder, Item, RequestContext, RequestDefaults, RequestEntry};
pub use env::{Environment, Variable};
pub use protocol::{
    GrpcMethodType, GrpcSpec, MessageKind, MessageVariant, ProtobufConfig, Protocol, WebSocketSpec,
};
pub use request::{
    ApiKeyPlacement, Auth, Body, KeyValue, MultipartPart, Param, ParamKind, PartKind, RawKind,
    RequestSpec, Settings,
};
pub use script::{Assertion, Script, ScriptPhase, VarAction, VarScope};
