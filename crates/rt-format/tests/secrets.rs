// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `externalSecrets` of environments (wave 5, strand E): written with only references, read
//! back as written, unchanged parts byte-identical (descriptions and comments included).

use rt_core::model::{Environment, ExternalSecret, ExternalSecrets, SecretProvider, Variable};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

fn files(list: &[(&str, &str)]) -> FileSet {
    FileSet {
        files: list
            .iter()
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect(),
    }
}

fn text(set: &FileSet, path: &str) -> String {
    String::from_utf8(set.files[path].clone()).unwrap()
}

const ROOT: &str = "opencollection: \"1.0.0\"\ninfo:\n  name: x\n";
const ENV: &str = "environments/Dev.yml";

fn secret(name: &str, reference: &str) -> ExternalSecret {
    ExternalSecret {
        name: name.into(),
        reference: reference.into(),
        enabled: true,
    }
}

/// Sets `env` and returns the environment file's text (the edit's read-back check passed).
fn written(input: &FileSet, env: &Environment) -> String {
    let mut doc = read_collection(input).unwrap();
    assert_eq!(doc.set_environment(env).unwrap(), ENV);
    let out = doc.write();
    let back = read_collection(&out).unwrap();
    assert_eq!(back.model().environment(&env.name), Some(env));
    text(&out, ENV)
}

fn edited(file: &str, f: impl FnOnce(&mut Environment)) -> String {
    let input = files(&[(COLLECTION_FILE, ROOT), (ENV, file)]);
    let mut env = read_collection(&input)
        .unwrap()
        .model()
        .environment("Dev")
        .unwrap()
        .clone();
    f(&mut env);
    written(&input, &env)
}

#[test]
fn adds_external_secrets_after_the_variables() {
    let out = edited(
        "name: Dev\nvariables:\n  - name: host\n    value: x\n",
        |env| {
            env.external_secrets = Some(ExternalSecrets {
                provider: SecretProvider::OnePassword,
                variables: vec![
                    secret("token", "op://Dev/api/token"),
                    ExternalSecret {
                        enabled: false,
                        ..secret("old", "op://Dev/api/old")
                    },
                ],
            });
        },
    );
    assert_eq!(
        out,
        "name: Dev\nvariables:\n  - name: host\n    value: x\nexternalSecrets:\n  type: one-password\n  variables:\n    - name: token\n      secretReference: op://Dev/api/token\n    - name: old\n      secretReference: op://Dev/api/old\n      disabled: true\n"
    );
}

#[test]
fn every_provider_uses_its_field() {
    for (provider, ty, key) in [
        (
            SecretProvider::HashicorpVaultCloud,
            "hashicorp-vault-cloud",
            "path",
        ),
        (
            SecretProvider::HashicorpVaultServer,
            "hashicorp-vault-server",
            "path",
        ),
        (
            SecretProvider::AwsSecretsManager,
            "aws-secrets-manager",
            "secretName",
        ),
        (
            SecretProvider::AzureKeyVault,
            "azure-key-vault",
            "vaultName",
        ),
        (
            SecretProvider::GcpSecretsManager,
            "gcp-secrets-manager",
            "projectId",
        ),
        (
            SecretProvider::OnePassword,
            "one-password",
            "secretReference",
        ),
    ] {
        let out = edited("name: Dev\nvariables: []\n", |env| {
            env.external_secrets = Some(ExternalSecrets {
                provider,
                variables: vec![secret("pw", "ref/1")],
            });
        });
        assert_eq!(
            out,
            format!(
                "name: Dev\nvariables: []\nexternalSecrets:\n  type: {ty}\n  variables:\n    - name: pw\n      {key}: ref/1\n"
            )
        );
    }
}

const BRUNO_STYLE: &str = "\
name: Dev
variables:
  - name: host
    value: x
externalSecrets:
  type: hashicorp-vault-server
  variables:
    # the database
    - name: dbPassword
      description: Primary database
      path: secret/db#password
    - name: apiKey
      description: |-
        Partner API
        (rotated monthly)
      path: secret/partner#key
      disabled: true
";

#[test]
fn unchanged_secrets_stay_byte_identical() {
    let out = edited(BRUNO_STYLE, |env| {
        // Another part of the environment changes.
        env.variables.push(Variable::new("port", "8080"));
    });
    assert!(
        out.ends_with(&BRUNO_STYLE[BRUNO_STYLE.find("externalSecrets").unwrap()..]),
        "{out}"
    );
}

#[test]
fn editing_one_entry_keeps_the_others_and_descriptions() {
    let out = edited(BRUNO_STYLE, |env| {
        let s = env.external_secrets.as_mut().unwrap();
        s.variables[0].reference = "secret/db2#password".into();
        s.variables[1].enabled = true;
    });
    assert_eq!(
        out,
        BRUNO_STYLE
            .replace("secret/db#password", "secret/db2#password")
            .replace("      disabled: true\n", "")
    );
}

#[test]
fn adding_removing_and_reordering_entries() {
    let out = edited(BRUNO_STYLE, |env| {
        let s = env.external_secrets.as_mut().unwrap();
        let first = s.variables.remove(0);
        s.variables.push(secret("token", "secret/api#token"));
        s.variables.push(first);
    });
    // Moved entries keep their text; the comment above the list stays on top (`Seq::rebuild`).
    assert_eq!(
        out,
        "name: Dev\nvariables:\n  - name: host\n    value: x\nexternalSecrets:\n  type: hashicorp-vault-server\n  variables:\n    # the database\n    - name: apiKey\n      description: |-\n        Partner API\n        (rotated monthly)\n      path: secret/partner#key\n      disabled: true\n    - name: token\n      path: secret/api#token\n    - name: dbPassword\n      description: Primary database\n      path: secret/db#password\n"
    );
    let out = edited(BRUNO_STYLE, |env| {
        env.external_secrets.as_mut().unwrap().variables.clear();
    });
    assert!(
        out.ends_with("externalSecrets:\n  type: hashicorp-vault-server\n  variables: []\n"),
        "{out}"
    );
    let out = edited(BRUNO_STYLE, |env| env.external_secrets = None);
    assert_eq!(out, "name: Dev\nvariables:\n  - name: host\n    value: x\n");
}

#[test]
fn another_provider_rewrites_the_block() {
    let out = edited(BRUNO_STYLE, |env| {
        let s = env.external_secrets.as_mut().unwrap();
        s.provider = SecretProvider::AzureKeyVault;
        s.variables.truncate(1);
        s.variables[0].reference = "prod-vault/db-password".into();
    });
    assert!(
        out.ends_with("externalSecrets:\n  type: azure-key-vault\n  variables:\n    - name: dbPassword\n      vaultName: prod-vault/db-password\n"),
        "{out}"
    );
}

#[test]
fn inline_environments_and_crlf() {
    let root = "opencollection: \"1.0.0\"\r\ninfo:\r\n  name: x\r\nconfig:\r\n  environments:\r\n    - name: Dev\r\n      variables: []\r\n";
    let input = files(&[(COLLECTION_FILE, root)]);
    let mut doc = read_collection(&input).unwrap();
    let mut env = doc.model().environment("Dev").unwrap().clone();
    env.external_secrets = Some(ExternalSecrets {
        provider: SecretProvider::AwsSecretsManager,
        variables: vec![secret("token", "prod/api#token")],
    });
    assert_eq!(doc.set_environment(&env).unwrap(), COLLECTION_FILE);
    let out = doc.write();
    assert_eq!(
        text(&out, COLLECTION_FILE),
        "opencollection: \"1.0.0\"\r\ninfo:\r\n  name: x\r\nconfig:\r\n  environments:\r\n    - name: Dev\r\n      variables: []\r\n      externalSecrets:\r\n        type: aws-secrets-manager\r\n        variables:\r\n          - name: token\r\n            secretName: prod/api#token\r\n"
    );
    assert_eq!(
        read_collection(&out).unwrap().model().environment("Dev"),
        Some(&env)
    );
}

#[test]
fn an_unknown_provider_in_the_file_is_left_alone() {
    let file = "name: Dev\nvariables: []\nexternalSecrets:\n  type: some-future-vault\n  variables:\n    - name: a\n      where: x\n";
    let out = edited(file, |env| env.variables.push(Variable::new("b", "1")));
    assert!(out.ends_with("externalSecrets:\n  type: some-future-vault\n  variables:\n    - name: a\n      where: x\n"), "{out}");
}
