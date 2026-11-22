// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! mTLS end to end (wave 5, strand D): `send` picks the client certificate of the
//! environment, reads its files, and the native transport presents it to rt-testserver's
//! HTTPS endpoint, which requires one. All key material is generated here, for `localhost`
//! only, and never written to disk.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::net::SocketAddr;

use base64::Engine as _;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rt_core::model::{
    CertificateSource, ClientCertificate, Collection, Environment, Item, RequestEntry, RequestSpec,
    Variable,
};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::send::{ReportBody, SendError, SendOptions, SendReport, send};
use rt_core::transport::TransportError;
use rt_core::transport::native::NativeTransport;
use sha2::Digest as _;

struct Ca {
    issuer: Issuer<'static, KeyPair>,
    der: Vec<u8>,
}

fn ca(name: &str) -> Ca {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name.push(DnType::CommonName, name);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let cert = params.self_signed(&key).unwrap();
    Ca {
        der: cert.der().to_vec(),
        issuer: Issuer::new(params, key),
    }
}

/// `(certificate DER, PKCS#8 key DER)` signed by `ca`.
fn leaf(ca: &Ca, names: &[&str], purpose: ExtendedKeyUsagePurpose) -> (Vec<u8>, Vec<u8>) {
    let key = KeyPair::generate().unwrap();
    let mut params =
        CertificateParams::new(names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>()).unwrap();
    params.distinguished_name.push(DnType::CommonName, names[0]);
    params.extended_key_usages = vec![purpose];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.use_authority_key_identifier_extension = true;
    let cert = params.signed_by(&key, &ca.issuer).unwrap();
    (cert.der().to_vec(), key.serialize_der())
}

fn pem(label: &str, der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        lines.join("\n")
    )
}

fn encrypted_pkcs8_pem(key_der: &[u8], passphrase: &str) -> String {
    use pkcs8::der::Encode as _;
    use pkcs8::pkcs5::pbes2;
    let params =
        pbes2::Parameters::generate_pbkdf2_sha256_aes256cbc(2048, b"rt-test-salt-123", [7; 16])
            .unwrap();
    let encrypted = params.encrypt(passphrase, key_der).unwrap();
    let info = pkcs8::EncryptedPrivateKeyInfoRef {
        encryption_algorithm: params.into(),
        encrypted_data: pkcs8::der::asn1::OctetStringRef::new(&encrypted).unwrap(),
    };
    pem("ENCRYPTED PRIVATE KEY", &info.to_der().unwrap())
}

fn pkcs12(cert_der: &[u8], key_der: &[u8], ca_der: &[u8], passphrase: &str) -> Vec<u8> {
    use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain};
    let chain = PrivateKeyChain::new(
        b"rt-test".as_slice(),
        PrivateKey::from_der(key_der).unwrap(),
        [
            Certificate::from_der(cert_der).unwrap(),
            Certificate::from_der(ca_der).unwrap(),
        ],
    );
    let mut store = KeyStore::new();
    store.add_entry("client", KeyStoreEntry::PrivateKeyChain(chain));
    store.writer(passphrase).write().unwrap()
}

struct Setup {
    addr: SocketAddr,
    transport: NativeTransport,
    client_ca: Ca,
}

async fn server() -> Setup {
    let server_ca = ca("RequestTailor test server CA");
    let client_ca = ca("RequestTailor test client CA");
    let (server_cert, server_key) = leaf(
        &server_ca,
        &["localhost", "127.0.0.1"],
        ExtendedKeyUsagePurpose::ServerAuth,
    );
    let addr = rt_testserver::spawn_mtls(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        rt_testserver::MtlsConfig {
            server_chain: vec![server_cert],
            server_key,
            client_ca: client_ca.der.clone(),
        },
    )
    .await
    .unwrap();
    let transport = NativeTransport::with_extra_root_certificates([server_ca.der.clone()]).unwrap();
    Setup {
        addr,
        transport,
        client_ca,
    }
}

struct MemFiles(BTreeMap<String, Vec<u8>>);

impl FileSource for MemFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| format!("{path}: not found"))
    }
}

fn collection(port: u16, certificates: Vec<ClientCertificate>) -> Collection {
    Collection {
        name: "mtls".into(),
        defaults: Default::default(),
        items: vec![Item::Request(RequestEntry {
            path: "echo.yml".into(),
            spec: RequestSpec {
                name: "echo".into(),
                method: "GET".into(),
                url: format!("https://localhost:{port}/echo"),
                ..Default::default()
            },
        })],
        environments: vec![Environment {
            name: "Local".into(),
            variables: vec![Variable {
                name: "certPass".into(),
                value: None,
                enabled: true,
                secret: true,
            }],
            client_certificates: certificates,
            external_secrets: None,
        }],
        config: Default::default(),
    }
}

async fn send_with_cert(
    setup: &Setup,
    certificates: Vec<ClientCertificate>,
    files: &MemFiles,
    passphrase: &str,
    environment: Option<&str>,
) -> SendReport {
    let collection = collection(setup.addr.port(), certificates);
    let input = PrepareInput {
        environment,
        globals: Vec::new(),
        runtime: vec![("certPass".into(), passphrase.into())],
    };
    send(
        &collection,
        "echo.yml",
        &input,
        files,
        &setup.transport,
        &SendOptions::default(),
    )
    .await
}

fn presented_fingerprint(report: &SendReport) -> String {
    let response = report
        .response
        .as_ref()
        .unwrap_or_else(|| panic!("{report:?}"));
    assert_eq!(response.status, 200, "{report:?}");
    let ReportBody::Utf8(body) = &response.body else {
        panic!("{:?}", response.body)
    };
    let echo: serde_json::Value = serde_json::from_str(body).unwrap();
    echo["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h[0] == "x-rt-client-cert-sha256")
        .map(|h| h[1].as_str().unwrap().to_owned())
        .unwrap_or_default()
}

fn hex_sha256(der: &[u8]) -> String {
    sha2::Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn pem_cert(domain: &str, cert: &str, key: &str, passphrase: &str) -> ClientCertificate {
    ClientCertificate {
        domain: domain.into(),
        source: CertificateSource::Pem {
            certificate_path: cert.into(),
            private_key_path: key.into(),
        },
        passphrase: passphrase.into(),
        enabled: true,
    }
}

#[tokio::test]
async fn pem_certificate_is_presented_and_required() {
    let setup = server().await;
    let (cert, key) = leaf(
        &setup.client_ca,
        &["rt-test-client"],
        ExtendedKeyUsagePurpose::ClientAuth,
    );
    let files = MemFiles(BTreeMap::from([
        (
            "certs/client.crt".to_owned(),
            pem("CERTIFICATE", &cert).into_bytes(),
        ),
        (
            "certs/client.key".to_owned(),
            pem("PRIVATE KEY", &key).into_bytes(),
        ),
    ]));
    let certs = vec![pem_cert(
        &format!("localhost:{}", setup.addr.port()),
        "certs/client.crt",
        "certs/client.key",
        "",
    )];

    let report = send_with_cert(&setup, certs.clone(), &files, "", Some("Local")).await;
    assert_eq!(report.error, None, "{report:?}");
    assert_eq!(presented_fingerprint(&report), hex_sha256(&cert));

    // Without the environment there is no certificate: the server refuses the connection.
    let report = send_with_cert(&setup, certs, &files, "", None).await;
    assert!(report.response.is_none(), "{report:?}");
    match &report.error {
        Some(SendError::Transport(_)) => {}
        other => panic!("expected a transport error, got {other:?}"),
    }
}

#[tokio::test]
async fn certificate_from_another_ca_is_refused() {
    let setup = server().await;
    let stranger = ca("Someone else");
    let (cert, key) = leaf(
        &stranger,
        &["intruder"],
        ExtendedKeyUsagePurpose::ClientAuth,
    );
    let files = MemFiles(BTreeMap::from([(
        "both.pem".to_owned(),
        format!("{}{}", pem("CERTIFICATE", &cert), pem("PRIVATE KEY", &key)).into_bytes(),
    )]));
    // One file with certificate and key (no key path).
    let certs = vec![pem_cert("localhost", "both.pem", "", "")];
    let report = send_with_cert(&setup, certs, &files, "", Some("Local")).await;
    assert!(report.response.is_none(), "{report:?}");
    assert!(
        matches!(report.error, Some(SendError::Transport(_))),
        "{report:?}"
    );
}

#[tokio::test]
async fn encrypted_pkcs8_key_with_passphrase() {
    let setup = server().await;
    let (cert, key) = leaf(
        &setup.client_ca,
        &["rt-test-client"],
        ExtendedKeyUsagePurpose::ClientAuth,
    );
    let pass = "pass phrase 1";
    let files = MemFiles(BTreeMap::from([
        ("c.pem".to_owned(), pem("CERTIFICATE", &cert).into_bytes()),
        (
            "k.pem".to_owned(),
            encrypted_pkcs8_pem(&key, pass).into_bytes(),
        ),
    ]));
    let certs = vec![pem_cert("*", "c.pem", "k.pem", "{{certPass}}")];
    let report = send_with_cert(&setup, certs.clone(), &files, pass, Some("Local")).await;
    assert_eq!(report.error, None, "{report:?}");
    assert_eq!(presented_fingerprint(&report), hex_sha256(&cert));

    let report = send_with_cert(&setup, certs, &files, "wrong", Some("Local")).await;
    match &report.error {
        Some(SendError::Transport(TransportError::Tls(m))) => {
            assert!(m.contains("cannot be decrypted"), "{m}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn pkcs12_archive_with_passphrase() {
    let setup = server().await;
    let (cert, key) = leaf(
        &setup.client_ca,
        &["rt-test-client"],
        ExtendedKeyUsagePurpose::ClientAuth,
    );
    let pass = "p12-secret";
    let files = MemFiles(BTreeMap::from([(
        "client.p12".to_owned(),
        pkcs12(&cert, &key, &setup.client_ca.der, pass),
    )]));
    let certs = vec![ClientCertificate {
        domain: "localhost".into(),
        source: CertificateSource::Pkcs12 {
            path: "client.p12".into(),
        },
        passphrase: "{{certPass}}".into(),
        enabled: true,
    }];
    let report = send_with_cert(&setup, certs.clone(), &files, pass, Some("Local")).await;
    assert_eq!(report.error, None, "{report:?}");
    assert_eq!(presented_fingerprint(&report), hex_sha256(&cert));

    let report = send_with_cert(&setup, certs, &files, "nope", Some("Local")).await;
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("PKCS#12"), "{json}");
    assert!(!json.contains(pass), "{json}");
}

#[tokio::test]
async fn missing_certificate_file_fails_before_sending() {
    let setup = server().await;
    let certs = vec![pem_cert("localhost", "missing.crt", "missing.key", "")];
    let report = send_with_cert(&setup, certs, &MemFiles(BTreeMap::new()), "", Some("Local")).await;
    match &report.error {
        Some(SendError::Auth(m)) => assert!(m.contains("missing.crt"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(report.request.is_none(), "nothing was sent");
}
