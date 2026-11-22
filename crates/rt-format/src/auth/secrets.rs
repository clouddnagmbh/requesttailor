// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `externalSecrets` of an environment ↔ `ExternalSecrets` (wave 5, strand E). The file holds
//! only references (`path`, `secretName`, `vaultName`, `projectId` or `secretReference`, by
//! provider), never values.

use rt_core::model::{ExternalSecret, ExternalSecrets, SecretProvider};

use crate::read::{enabled, text};
use crate::yaml::{ItemOp, Map, Node, Val};

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

/// The OpenCollection `type` of a provider (inverse of [`provider`]).
fn provider_name(provider: SecretProvider) -> &'static str {
    match provider {
        SecretProvider::HashicorpVaultCloud => "hashicorp-vault-cloud",
        SecretProvider::HashicorpVaultServer => "hashicorp-vault-server",
        SecretProvider::AwsSecretsManager => "aws-secrets-manager",
        SecretProvider::AzureKeyVault => "azure-key-vault",
        SecretProvider::GcpSecretsManager => "gcp-secrets-manager",
        SecretProvider::OnePassword => "one-password",
    }
}

/// Where `externalSecrets` goes in an environment: after variables and certificates.
const ENV_ORDER: &[&str] = &["name", "variables", "clientCertificates", "externalSecrets"];
const SECRETS_ORDER: &[&str] = &["type", "variables"];
/// Key order of an entry (the schema's), with every provider's locator field.
const ENTRY_ORDER: &[&str] = &[
    "name",
    "description",
    "path",
    "secretName",
    "vaultName",
    "projectId",
    "secretReference",
    "disabled",
];

fn entry_val(key: &'static str, v: &ExternalSecret) -> Val {
    let mut e = vec![("name", Val::str(&v.name)), (key, Val::str(&v.reference))];
    if !v.enabled {
        e.push(("disabled", Val::bool(true)));
    }
    Val::Map(e)
}

fn secrets_val(s: &ExternalSecrets) -> Val {
    let key = reference_key(s.provider);
    Val::Map(vec![
        ("type", Val::str(provider_name(s.provider))),
        (
            "variables",
            Val::Seq(s.variables.iter().map(|v| entry_val(key, v)).collect()),
        ),
    ])
}

/// Edits one entry in place; `description` and comments stay.
fn edit_entry(
    m: &mut Map,
    key: &'static str,
    old: &ExternalSecret,
    new: &ExternalSecret,
    nl: &str,
) {
    if old.name != new.name {
        m.put("name", &Val::str(&new.name), ENTRY_ORDER, false, nl);
    }
    if old.reference != new.reference {
        m.put(key, &Val::str(&new.reference), ENTRY_ORDER, false, nl);
    }
    if old.enabled != new.enabled {
        m.remove("enabled");
        if new.enabled {
            m.remove("disabled");
        } else {
            m.put("disabled", &Val::bool(true), ENTRY_ORDER, false, nl);
        }
    }
}

/// Writes `secrets` as the `externalSecrets` of the environment `map`.
/// Called on every environment edit; leaves `map` untouched when nothing changed, and edits
/// only the entries that changed (each keeps its `description` and comments). A new block, or
/// one whose provider changed (its entries then need another locator field), is written
/// whole.
pub(crate) fn apply(map: &mut Map, secrets: &Option<ExternalSecrets>, nl: &str) {
    let old = read(map.get("externalSecrets"));
    if old == *secrets {
        return;
    }
    let Some(new) = secrets else {
        map.remove("externalSecrets");
        return;
    };
    let key = reference_key(new.provider);
    let editable = old.as_ref().is_some_and(|o| o.provider == new.provider)
        && map
            .get_mut("externalSecrets")
            .and_then(Node::block_map_mut)
            .is_some();
    if !editable {
        map.put("externalSecrets", &secrets_val(new), ENV_ORDER, false, nl);
        return;
    }
    let old = old.expect("checked").variables;
    let block = map
        .get_mut("externalSecrets")
        .and_then(Node::block_map_mut)
        .expect("checked");
    if new.variables.is_empty() {
        block.put("variables", &Val::Seq(Vec::new()), SECRETS_ORDER, false, nl);
        return;
    }
    let rendered: Vec<Val> = new.variables.iter().map(|v| entry_val(key, v)).collect();
    let Some(seq) = block.get_mut("variables").and_then(Node::block_seq_mut) else {
        block.put("variables", &Val::Seq(rendered), SECRETS_ORDER, false, nl);
        return;
    };
    // Each new entry takes the first unused old entry of the same name.
    let mut used = vec![false; old.len()];
    let mut pairs = Vec::new();
    let ops: Vec<ItemOp<'_>> = new
        .variables
        .iter()
        .enumerate()
        .map(|(n, v)| {
            let found = (0..old.len()).find(|&o| !used[o] && old[o].name == v.name);
            match found {
                Some(o) => {
                    used[o] = true;
                    if old[o] == *v {
                        ItemOp::Keep(o)
                    } else if matches!(seq.items[o].value, Node::Map(ref m) if !m.flow) {
                        pairs.push((o, n));
                        ItemOp::Edit(o)
                    } else {
                        ItemOp::New(&rendered[n])
                    }
                }
                None => ItemOp::New(&rendered[n]),
            }
        })
        .collect();
    let positions = seq.rebuild(&ops, nl);
    for (pos, (o, n)) in positions.into_iter().zip(pairs) {
        let item = seq.items[pos]
            .value
            .block_map_mut()
            .expect("checked editable");
        edit_entry(item, key, &old[o], &new.variables[n], nl);
    }
}
