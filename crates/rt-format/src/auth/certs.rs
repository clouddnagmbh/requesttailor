// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `clientCertificates` (in `config` and in environments) ↔ `ClientCertificate`.
//!
//! An entry as Bruno writes it (OpenCollection `ClientCertificate`):
//!
//! ```yaml
//! - domain: localhost
//!   type: pem                      # or pkcs12
//!   certificateFilePath: certs/client.crt
//!   privateKeyFilePath: certs/client.key
//!   pkcs12FilePath: certs/client.p12   # pkcs12 only
//!   passphrase: "{{certPassphrase}}"  # left out when empty
//!   disabled: true                 # left out when enabled
//! ```
//!
//! Writing goes through the list sync of `write`: unchanged entries stay byte for byte,
//! changed ones are edited in place. Entries of a type the reader does not know are not part
//! of the model; when the list changes they are dropped (the schema knows only `pem` and
//! `pkcs12`, Bruno's older `pfx` reads as `pkcs12`).

use rt_core::model::{CertificateSource, ClientCertificate, CollectionConfig};

use crate::read::{enabled, text};
use crate::write::{Empty, ListSpec, set_enabled, sync_list};
use crate::yaml::{Map, Node, Val};
use crate::{COLLECTION_FILE, CollectionDoc, FormatError, Source};

const ITEM_ORDER: &[&str] = &[
    "domain",
    "type",
    "certificateFilePath",
    "privateKeyFilePath",
    "pkcs12FilePath",
    "pfxFilePath",
    "passphrase",
    "disabled",
];

/// Where `clientCertificates` goes in an environment and in `config`.
const OWNER_ORDER: &[&str] = &[
    "name",
    "color",
    "variables",
    "environments",
    "clientCertificates",
    "externalSecrets",
    "protobuf",
    "proxy",
];

/// Top-level keys of `opencollection.yml` in Bruno's order.
const ROOT_ORDER: &[&str] = &[
    "opencollection",
    "info",
    "config",
    "request",
    "docs",
    "bundled",
    "extensions",
];

fn read_one(c: &Node) -> Option<ClientCertificate> {
    let source = match c.get("type").and_then(Node::text)? {
        "pem" => CertificateSource::Pem {
            certificate_path: text(c.get("certificateFilePath")),
            private_key_path: text(c.get("privateKeyFilePath")),
        },
        // Bruno wrote `pfx` before the schema settled on `pkcs12`.
        "pkcs12" | "pfx" => CertificateSource::Pkcs12 {
            path: c
                .get("pkcs12FilePath")
                .or_else(|| c.get("pfxFilePath"))
                .and_then(Node::text)
                .unwrap_or_default()
                .to_owned(),
        },
        _ => return None,
    };
    Some(ClientCertificate {
        domain: text(c.get("domain")),
        source,
        passphrase: text(c.get("passphrase")),
        enabled: enabled(c),
    })
}

/// Reads a `clientCertificates` list. Entries of unknown `type` are skipped.
pub(crate) fn read(list: Option<&Node>) -> Vec<ClientCertificate> {
    let Some(list) = list else {
        return Vec::new();
    };
    list.items().filter_map(read_one).collect()
}

fn render(c: &ClientCertificate) -> Val {
    let mut e = vec![("domain", Val::str(&c.domain))];
    match &c.source {
        CertificateSource::Pem {
            certificate_path,
            private_key_path,
        } => {
            e.push(("type", Val::str("pem")));
            e.push(("certificateFilePath", Val::str(certificate_path)));
            e.push(("privateKeyFilePath", Val::str(private_key_path)));
        }
        CertificateSource::Pkcs12 { path } => {
            e.push(("type", Val::str("pkcs12")));
            e.push(("pkcs12FilePath", Val::str(path)));
        }
    }
    if !c.passphrase.is_empty() {
        e.push(("passphrase", Val::str(&c.passphrase)));
    }
    if !c.enabled {
        e.push(("disabled", Val::bool(true)));
    }
    Val::Map(e)
}

fn edit(m: &mut Map, old: &ClientCertificate, new: &ClientCertificate, nl: &str) {
    if old.domain != new.domain {
        m.put("domain", &Val::str(&new.domain), ITEM_ORDER, false, nl);
    }
    match (&old.source, &new.source) {
        (a, b) if a == b => {}
        (
            CertificateSource::Pem { .. },
            CertificateSource::Pem {
                certificate_path,
                private_key_path,
            },
        ) => {
            m.put(
                "certificateFilePath",
                &Val::str(certificate_path),
                ITEM_ORDER,
                false,
                nl,
            );
            m.put(
                "privateKeyFilePath",
                &Val::str(private_key_path),
                ITEM_ORDER,
                false,
                nl,
            );
        }
        (CertificateSource::Pkcs12 { .. }, CertificateSource::Pkcs12 { path }) => {
            m.remove("pfxFilePath");
            m.put("pkcs12FilePath", &Val::str(path), ITEM_ORDER, false, nl);
        }
        (_, new_source) => {
            for key in [
                "certificateFilePath",
                "privateKeyFilePath",
                "pkcs12FilePath",
                "pfxFilePath",
            ] {
                m.remove(key);
            }
            let Val::Map(entries) = render(&ClientCertificate {
                source: new_source.clone(),
                ..new.clone()
            }) else {
                unreachable!("render gives a mapping")
            };
            for (key, value) in entries {
                if matches!(
                    key,
                    "type" | "certificateFilePath" | "privateKeyFilePath" | "pkcs12FilePath"
                ) {
                    m.put(key, &value, ITEM_ORDER, false, nl);
                }
            }
        }
    }
    if old.passphrase != new.passphrase {
        if new.passphrase.is_empty() {
            m.remove("passphrase");
        } else {
            m.put(
                "passphrase",
                &Val::str(&new.passphrase),
                ITEM_ORDER,
                false,
                nl,
            );
        }
    }
    if old.enabled != new.enabled {
        set_enabled(m, new.enabled, ITEM_ORDER, nl);
    }
}

/// Writes `certs` as the `clientCertificates` of `map` (an environment or `config`).
/// Called on every environment edit; leaves `map` untouched when nothing changed.
pub(crate) fn apply(map: &mut Map, certs: &[ClientCertificate], nl: &str) {
    let known = read(map.get("clientCertificates"));
    if known == certs {
        return;
    }
    // Items of unknown type would shift the alignment; the model has none of them.
    let read_item = |n: &Node| {
        read_one(n).unwrap_or_else(|| ClientCertificate {
            domain: String::new(),
            source: CertificateSource::Pkcs12 {
                path: String::new(),
            },
            passphrase: String::new(),
            enabled: false,
        })
    };
    sync_list(
        map,
        &ListSpec {
            key: "clientCertificates",
            order: OWNER_ORDER,
            empty: Empty::Remove,
            read: &read_item,
            same: &|a: &ClientCertificate, b: &ClientCertificate| a.domain == b.domain,
            render: &render,
            edit: &edit,
        },
        certs,
        nl,
    );
}

impl CollectionDoc {
    /// Replaces the collection-wide settings the model knows (`config` of
    /// `opencollection.yml`: client certificates). Unchanged parts of the file stay byte for
    /// byte; the edit is read back and refused if it does not give `config`.
    ///
    /// Owner: wave 5, strand D.
    pub fn set_collection_config(&mut self, config: &CollectionConfig) -> Result<(), FormatError> {
        let invalid = |message: String| FormatError::Invalid {
            path: COLLECTION_FILE.to_owned(),
            message,
        };
        if self.model.config == *config {
            return Ok(());
        }
        let Some(Source::Yaml(doc)) = self.files.get(COLLECTION_FILE) else {
            return Err(invalid("the collection file cannot be edited".to_owned()));
        };
        let mut doc = doc.clone();
        let nl = doc.newline;
        let root = doc
            .root
            .block_map_mut()
            .ok_or_else(|| invalid("the file is not a YAML mapping".to_owned()))?;
        match root.get_mut("config") {
            Some(node) => {
                let Some(map) = node.block_map_mut() else {
                    return Err(invalid(
                        "`config` is not a block mapping and cannot be edited".to_owned(),
                    ));
                };
                apply(map, &config.client_certificates, nl);
                if map.is_empty() {
                    root.remove("config");
                }
            }
            None if config.client_certificates.is_empty() => {}
            None => root.put(
                "config",
                &Val::Map(vec![(
                    "clientCertificates",
                    Val::Seq(config.client_certificates.iter().map(render).collect()),
                )]),
                ROOT_ORDER,
                true,
                nl,
            ),
        }
        let doc = crate::yaml::reparse(&doc).map_err(invalid)?;
        let got = CollectionConfig {
            agents: Default::default(),
            http3: false,
            protobuf: Default::default(),
            client_certificates: read(
                doc.root
                    .get("config")
                    .and_then(|c| c.get("clientCertificates")),
            ),
        };
        if got != *config {
            return Err(invalid(format!(
                "internal error: the edited configuration does not read back as written \
                 (wanted {config:?}, got {got:?})"
            )));
        }
        self.files
            .insert(COLLECTION_FILE.to_owned(), Source::Yaml(doc));
        self.model.config = got;
        Ok(())
    }
}
