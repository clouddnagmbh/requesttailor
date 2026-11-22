// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `externalSecrets` of an environment ↔ `ExternalSecrets`. Reading is complete; writing is
//! strand E's.

use rt_core::model::{ExternalSecret, ExternalSecrets, SecretProvider};

use crate::read::{enabled, text};
use crate::yaml::{Map, Node};

pub(crate) fn provider(name: &str) -> Option<SecretProvider> {
    Some(match name {
        "hashicorp-vault-cloud" => SecretProvider::HashicorpVaultCloud,
        "hashicorp-vault-server" => SecretProvider::HashicorpVaultServer,
        "aws-secrets-manager" => SecretProvider::AwsSecretsManager,
        "azure-key-vault" => SecretProvider::AzureKeyVault,
        "gcp-secrets-manager" => SecretProvider::GcpSecretsManager,
        "one-password" => SecretProvider::OnePassword,
        _ => return None,
    })
}

/// The locator field of each provider's entries.
pub(crate) fn reference_key(provider: SecretProvider) -> &'static str {
    match provider {
        SecretProvider::HashicorpVaultCloud | SecretProvider::HashicorpVaultServer => "path",
        SecretProvider::AwsSecretsManager => "secretName",
        SecretProvider::AzureKeyVault => "vaultName",
        SecretProvider::GcpSecretsManager => "projectId",
        SecretProvider::OnePassword => "secretReference",
    }
}

pub(crate) fn read(node: Option<&Node>) -> Option<ExternalSecrets> {
    let node = node?;
    let provider = provider(node.get("type").and_then(Node::text)?)?;
    let key = reference_key(provider);
    let variables = node
        .get("variables")
        .map(|list| {
            list.items()
                .map(|v| ExternalSecret {
                    name: text(v.get("name")),
                    reference: text(v.get(key)),
                    enabled: enabled(v),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ExternalSecrets {
        provider,
        variables,
    })
}

/// Writes `secrets` as the `externalSecrets` of the environment `map`.
/// Called on every environment edit; must leave `map` untouched when nothing changed.
/// Interface commit: not yet (the read-back check refuses changed references).
pub(crate) fn apply(map: &mut Map, secrets: &Option<ExternalSecrets>, nl: &str) {
    let _ = (map, secrets, nl);
}
