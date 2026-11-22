// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `clientCertificates` (in `config` and in environments) ↔ `ClientCertificate`.
//! Reading is complete; writing is strand D's.

use rt_core::model::{CertificateSource, ClientCertificate};

use crate::read::{enabled, text};
use crate::yaml::{Map, Node};

/// Reads a `clientCertificates` list. Entries of unknown `type` are skipped.
pub(crate) fn read(list: Option<&Node>) -> Vec<ClientCertificate> {
    let Some(list) = list else {
        return Vec::new();
    };
    list.items()
        .filter_map(|c| {
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
        })
        .collect()
}

/// Writes `certs` as the `clientCertificates` of `map` (an environment or `config`).
/// Called on every environment edit; must leave `map` untouched when nothing changed.
/// Interface commit: not yet (the read-back check refuses changed certificates).
pub(crate) fn apply(map: &mut Map, certs: &[ClientCertificate], nl: &str) {
    let _ = (map, certs, nl);
}
