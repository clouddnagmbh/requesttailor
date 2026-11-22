// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Client certificates (mTLS, wave 5, strand D): picks the certificate for a request's host
//! (environment first, then collection; first enabled match wins) and reads its files.
//!
//! Matching (`ClientCertificate::domain`): only `https://` URLs get a certificate. The
//! domain is a host name, case-insensitive, optionally with `:port` (then the URL's port,
//! 443 by default, must be that port) and optionally written with a leading `https://`.
//! `*.example.com` matches every subdomain of `example.com` (any depth), not `example.com`
//! itself; `*` alone matches every host.
//!
//! Files are read through the request's [`FileSource`] (collection-relative paths), so the
//! same rules hold natively and in the browser. Key material is only parsed by the
//! transport that presents it (the native one); the browser's `fetch` cannot present client
//! certificates and fails instead of sending without one.

use bytes::Bytes;

use crate::model::{CertificateSource, ClientCertificate, RequestContext};
use crate::prepare::FileSource;
use crate::transport::ClientIdentity;
use crate::vars::Scope;

/// The identity to present for `url`, or `None` if no certificate matches. Paths are read
/// through `files`; the passphrase template is resolved against `scope`.
pub fn client_identity(
    context: &RequestContext<'_>,
    environment: Option<&str>,
    url: &str,
    scope: &Scope,
    files: &dyn FileSource,
) -> Result<Option<ClientIdentity>, String> {
    let environment = environment.and_then(|name| context.collection.environment(name));
    let candidates = environment
        .into_iter()
        .flat_map(|e| &e.client_certificates)
        .chain(&context.collection.config.client_certificates);
    let Some(certificate) = select(candidates, url) else {
        return Ok(None);
    };
    load(certificate, scope, files).map(Some)
}

/// The first enabled certificate whose domain matches `url`.
pub fn select<'a>(
    certificates: impl IntoIterator<Item = &'a ClientCertificate>,
    url: &str,
) -> Option<&'a ClientCertificate> {
    let target = HttpsTarget::parse(url)?;
    certificates
        .into_iter()
        .find(|c| c.enabled && domain_matches(&c.domain, &target))
}

struct HttpsTarget {
    host: String,
    port: u16,
}

impl HttpsTarget {
    fn parse(url: &str) -> Option<Self> {
        let url = url::Url::parse(url).ok()?;
        if url.scheme() != "https" {
            return None;
        }
        let host = match url.host()? {
            url::Host::Domain(d) => d.to_ascii_lowercase(),
            url::Host::Ipv4(ip) => ip.to_string(),
            url::Host::Ipv6(ip) => format!("[{ip}]"),
        };
        Some(Self {
            host,
            port: url.port_or_known_default()?,
        })
    }
}

fn domain_matches(domain: &str, target: &HttpsTarget) -> bool {
    let domain = domain.trim();
    let domain = domain
        .strip_prefix("https://")
        .unwrap_or(domain)
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if domain.is_empty() {
        return false;
    }
    // `host:port`, but not the colons of a bracketed IPv6 address.
    let (host, port) = match domain.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') && !port.is_empty() => {
            match port.parse::<u16>() {
                Ok(port) => (host.to_owned(), Some(port)),
                Err(_) => return false,
            }
        }
        _ => (domain, None),
    };
    if port.is_some_and(|p| p != target.port) {
        return false;
    }
    if host == "*" {
        return true;
    }
    match host.strip_prefix("*.") {
        Some(parent) => target
            .host
            .strip_suffix(parent)
            .is_some_and(|sub| sub.len() > 1 && sub.ends_with('.')),
        None => host == target.host,
    }
}

/// Reads the files of `certificate` and resolves its passphrase.
fn load(
    certificate: &ClientCertificate,
    scope: &Scope,
    files: &dyn FileSource,
) -> Result<ClientIdentity, String> {
    let read = |what: &str, path: &str| -> Result<Bytes, String> {
        if path.trim().is_empty() {
            return Err(format!(
                "client certificate for `{}`: no {what} file is set",
                certificate.domain
            ));
        }
        files.read(path).map(Bytes::from).map_err(|e| {
            format!(
                "client certificate for `{}`: cannot read the {what} file {path}: {e}",
                certificate.domain
            )
        })
    };
    let resolved = scope.interpolate(&certificate.passphrase);
    if let Some(name) = resolved.unresolved.first() {
        return Err(format!(
            "client certificate for `{}`: the passphrase uses the variable `{name}`, which has \
             no value",
            certificate.domain
        ));
    }
    let passphrase = resolved.value;
    Ok(match &certificate.source {
        CertificateSource::Pem {
            certificate_path,
            private_key_path,
        } => {
            let cert = read("certificate", certificate_path)?;
            // A missing key path: the key is in the certificate file.
            let key = if private_key_path.trim().is_empty() {
                cert.clone()
            } else {
                read("private key", private_key_path)?
            };
            ClientIdentity::Pem {
                certificate: cert,
                private_key: key,
                passphrase: (!passphrase.is_empty()).then_some(passphrase),
            }
        }
        CertificateSource::Pkcs12 { path } => ClientIdentity::Pkcs12 {
            archive: read("PKCS#12", path)?,
            passphrase,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::model::{
        Collection, CollectionConfig, Environment, Item, RequestEntry, RequestSpec,
    };
    use crate::vars::{VarLayer, VarSource};

    fn cert(domain: &str, path: &str) -> ClientCertificate {
        ClientCertificate {
            domain: domain.into(),
            source: CertificateSource::Pkcs12 { path: path.into() },
            passphrase: String::new(),
            enabled: true,
        }
    }

    fn matches(domain: &str, url: &str) -> bool {
        select([&cert(domain, "x.p12")], url).is_some()
    }

    #[test]
    fn domain_matching() {
        assert!(matches("api.example.com", "https://api.example.com/x"));
        assert!(matches("API.example.com", "https://api.EXAMPLE.com/"));
        assert!(matches(
            "https://api.example.com",
            "https://api.example.com/"
        ));
        assert!(
            !matches("api.example.com", "http://api.example.com/"),
            "https only"
        );
        assert!(!matches("api.example.com", "https://example.com/"));
        assert!(!matches("example.com", "https://api.example.com/"));
        assert!(!matches("example.com", "https://example.com.evil.test/"));
        assert!(matches("*.example.com", "https://a.b.example.com/"));
        assert!(!matches("*.example.com", "https://example.com/"));
        assert!(!matches("*.example.com", "https://badexample.com/"));
        assert!(matches("*", "https://anything.test/"));
        assert!(matches("localhost:8443", "https://localhost:8443/"));
        assert!(!matches("localhost:8443", "https://localhost:9443/"));
        assert!(matches("example.com:443", "https://example.com/"));
        assert!(matches("127.0.0.1", "https://127.0.0.1:1234/"));
        assert!(matches("[::1]:8443", "https://[::1]:8443/"));
        assert!(!matches("example.com:x", "https://example.com/"));
        assert!(!matches("", "https://example.com/"));
    }

    #[test]
    fn first_enabled_match_wins() {
        let mut off = cert("example.com", "off.p12");
        off.enabled = false;
        let a = cert("*.com", "a.p12");
        let b = cert("example.com", "b.p12");
        let chosen = select([&off, &b, &a], "https://example.com/").unwrap();
        assert_eq!(chosen, &b);
    }

    struct Files(BTreeMap<&'static str, &'static [u8]>);

    impl FileSource for Files {
        fn read(&self, path: &str) -> Result<Vec<u8>, String> {
            self.0
                .get(path)
                .map(|b| b.to_vec())
                .ok_or_else(|| "not found".to_owned())
        }
    }

    fn collection() -> Collection {
        Collection {
            name: "c".into(),
            defaults: Default::default(),
            items: vec![Item::Request(RequestEntry {
                path: "r.yml".into(),
                spec: RequestSpec::default(),
            })],
            environments: vec![Environment {
                name: "Local".into(),
                variables: Vec::new(),
                client_certificates: vec![ClientCertificate {
                    domain: "*.example.com".into(),
                    source: CertificateSource::Pem {
                        certificate_path: "env.crt".into(),
                        private_key_path: String::new(),
                    },
                    passphrase: "{{pass}}".into(),
                    enabled: true,
                }],
                external_secrets: None,
            }],
            config: CollectionConfig {
                client_certificates: vec![cert("api.example.com", "col.p12")],
            },
        }
    }

    fn scope(pass: Option<&str>) -> Scope {
        Scope {
            layers: vec![VarLayer {
                source: VarSource::Runtime,
                vars: pass
                    .map(|p| vec![("pass".to_owned(), p.to_owned(), true)])
                    .unwrap_or_default(),
            }],
        }
    }

    #[test]
    fn environment_before_collection_and_files_read() {
        let c = collection();
        let ctx = c.request("r.yml").unwrap();
        let files = Files(BTreeMap::from([
            ("env.crt", &b"CERT AND KEY"[..]),
            ("col.p12", &b"P12"[..]),
        ]));
        let url = "https://api.example.com/";
        let id = client_identity(&ctx, Some("Local"), url, &scope(Some("pw")), &files)
            .unwrap()
            .unwrap();
        match id {
            ClientIdentity::Pem {
                certificate,
                private_key,
                passphrase,
            } => {
                assert_eq!(&certificate[..], b"CERT AND KEY");
                assert_eq!(private_key, certificate, "key from the certificate file");
                assert_eq!(passphrase.as_deref(), Some("pw"));
            }
            other => panic!("{other:?}"),
        }
        // Without the environment: the collection's certificate.
        let id = client_identity(&ctx, None, url, &scope(None), &files)
            .unwrap()
            .unwrap();
        assert!(
            matches!(id, ClientIdentity::Pkcs12 { ref archive, ref passphrase }
            if &archive[..] == b"P12" && passphrase.is_empty())
        );
        // No match, or plain http: none.
        assert!(
            client_identity(&ctx, None, "https://other.test/", &scope(None), &files)
                .unwrap()
                .is_none()
        );
        assert!(
            client_identity(&ctx, None, "http://api.example.com/", &scope(None), &files)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn errors_name_the_problem_without_secrets() {
        let c = collection();
        let ctx = c.request("r.yml").unwrap();
        let none = Files(BTreeMap::new());
        let url = "https://api.example.com/";
        let err = client_identity(&ctx, Some("Local"), url, &scope(None), &none).unwrap_err();
        assert!(err.contains("`pass`"), "{err}");
        let err = client_identity(&ctx, Some("Local"), url, &scope(Some("pw")), &none).unwrap_err();
        assert!(
            err.contains("env.crt") && err.contains("not found"),
            "{err}"
        );
        assert!(!err.contains("pw"), "{err}");
        let err = client_identity(&ctx, None, url, &scope(None), &none).unwrap_err();
        assert!(err.contains("col.p12"), "{err}");
    }
}
