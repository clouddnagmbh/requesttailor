// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Writing AWS SigV4 auth and client certificates (wave 5, strand D): unchanged parts stay
//! byte for byte, new parts are laid out the way Bruno writes them, edits read back.

use std::path::Path;

use rt_core::model::{
    Auth, AwsSigV4, CertificateSource, ClientCertificate, CollectionConfig, Environment,
};
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

const AWS_REQUEST: &str = "info:
  name: list
  type: http
  seq: 1

http:
  method: GET
  url: https://abc.execute-api.eu-central-1.amazonaws.com/prod/items
  auth:
    type: awsv4
    # keys come from the environment
    accessKeyId: \"{{awsKey}}\"
    secretAccessKey: \"{{awsSecret}}\"
    service: execute-api
    region: eu-central-1
";

fn aws_collection() -> FileSet {
    files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: aws\n",
        ),
        ("list.yml", AWS_REQUEST),
    ])
}

fn aws_of(spec: &rt_core::model::RequestSpec) -> AwsSigV4 {
    match &spec.auth {
        Auth::AwsSigV4(a) => a.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn awsv4_edits_change_only_the_changed_fields() {
    let mut doc = read_collection(&aws_collection()).unwrap();
    let mut spec = doc
        .model()
        .request("list.yml")
        .unwrap()
        .request
        .spec
        .clone();
    let mut aws = aws_of(&spec);
    assert_eq!(aws.access_key_id, "{{awsKey}}");
    aws.region = "us-east-1".into();
    aws.session_token = "{{awsToken}}".into();
    aws.secret_access_key = String::new();
    spec.auth = Auth::AwsSigV4(aws.clone());
    doc.set_request("list.yml", &spec).unwrap();
    let written = text(&doc.write(), "list.yml");
    assert_eq!(
        written,
        AWS_REQUEST
            .replace("    secretAccessKey: \"{{awsSecret}}\"\n", "")
            .replace(
                "    service: execute-api\n",
                "    sessionToken: \"{{awsToken}}\"\n    service: execute-api\n"
            )
            .replace("region: eu-central-1", "region: us-east-1")
    );
    let back = read_collection(&doc.write()).unwrap();
    assert_eq!(
        aws_of(&back.model().request("list.yml").unwrap().request.spec),
        aws
    );
}

#[test]
fn switching_to_awsv4_writes_bruno_layout() {
    let input = files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: aws\n",
        ),
        (
            "r.yml",
            "info:\n  name: r\n  type: http\n\nhttp:\n  method: GET\n  url: https://x\n  auth:\n    type: bearer\n    token: t\n",
        ),
    ]);
    let mut doc = read_collection(&input).unwrap();
    let mut spec = doc.model().request("r.yml").unwrap().request.spec.clone();
    spec.auth = Auth::AwsSigV4(AwsSigV4 {
        access_key_id: "{{k}}".into(),
        secret_access_key: "{{s}}".into(),
        service: "s3".into(),
        region: "eu-west-1".into(),
        profile_name: "dev".into(),
        ..Default::default()
    });
    doc.set_request("r.yml", &spec).unwrap();
    assert_eq!(
        text(&doc.write(), "r.yml"),
        "info:\n  name: r\n  type: http\n\nhttp:\n  method: GET\n  url: https://x\n  auth:\n    type: awsv4\n    accessKeyId: \"{{k}}\"\n    secretAccessKey: \"{{s}}\"\n    service: s3\n    region: eu-west-1\n    profileName: dev\n"
    );
}

fn pem(domain: &str, cert: &str, key: &str) -> ClientCertificate {
    ClientCertificate {
        domain: domain.into(),
        source: CertificateSource::Pem {
            certificate_path: cert.into(),
            private_key_path: key.into(),
        },
        passphrase: String::new(),
        enabled: true,
    }
}

const ENV: &str = "name: Local
variables:
  - name: baseUrl
    value: https://localhost:8443
clientCertificates:
  - domain: localhost
    type: pem
    certificateFilePath: certs/client.crt
    privateKeyFilePath: certs/client.key
  # the archive for the staging gateway
  - domain: \"*.staging.example.com\"
    type: pfx
    pfxFilePath: certs/staging.pfx
    passphrase: \"{{p12Pass}}\"
";

fn env_collection() -> FileSet {
    files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: certs\n",
        ),
        ("environments/Local.yml", ENV),
    ])
}

fn local(doc: &rt_format::CollectionDoc) -> Environment {
    doc.model().environment("Local").unwrap().clone()
}

#[test]
fn environment_certificates_unchanged_and_edited() {
    let mut doc = read_collection(&env_collection()).unwrap();
    let env = local(&doc);
    assert_eq!(env.client_certificates.len(), 2);
    doc.set_environment(&env).unwrap();
    assert_eq!(text(&doc.write(), "environments/Local.yml"), ENV);

    // Disable the first, new passphrase for the second: edited in place, comment kept,
    // Bruno's legacy `pfx` keys stay while the path is unchanged.
    let mut env = local(&doc);
    env.client_certificates[0].enabled = false;
    env.client_certificates[1].passphrase = "{{stagingPass}}".into();
    doc.set_environment(&env).unwrap();
    let written = text(&doc.write(), "environments/Local.yml");
    assert_eq!(
        written,
        ENV.replace(
            "    privateKeyFilePath: certs/client.key\n",
            "    privateKeyFilePath: certs/client.key\n    disabled: true\n"
        )
        .replace("{{p12Pass}}", "{{stagingPass}}")
    );
    assert_eq!(local(&read_collection(&doc.write()).unwrap()), env);
}

#[test]
fn environment_certificates_switch_type_add_and_remove() {
    let mut doc = read_collection(&env_collection()).unwrap();
    let mut env = local(&doc);
    // PEM -> PKCS#12, the legacy pfx path -> pkcs12FilePath, one new entry.
    env.client_certificates[0].source = CertificateSource::Pkcs12 {
        path: "certs/client.p12".into(),
    };
    env.client_certificates[1].source = CertificateSource::Pkcs12 {
        path: "certs/staging.p12".into(),
    };
    env.client_certificates
        .push(pem("api.example.com:8443", "a.crt", ""));
    doc.set_environment(&env).unwrap();
    let written = text(&doc.write(), "environments/Local.yml");
    assert!(
        written.contains(
            "  - domain: localhost\n    type: pkcs12\n    pkcs12FilePath: certs/client.p12\n"
        ),
        "{written}"
    );
    assert!(
        written.contains("    type: pfx\n    pkcs12FilePath: certs/staging.p12\n"),
        "{written}"
    );
    assert!(
        written.ends_with(
            "  - domain: api.example.com:8443\n    type: pem\n    certificateFilePath: a.crt\n    privateKeyFilePath: \"\"\n"
        ),
        "{written}"
    );
    assert_eq!(local(&read_collection(&doc.write()).unwrap()), env);

    // Removing all certificates removes the key.
    let mut env = local(&doc);
    env.client_certificates.clear();
    doc.set_environment(&env).unwrap();
    assert_eq!(
        text(&doc.write(), "environments/Local.yml"),
        "name: Local\nvariables:\n  - name: baseUrl\n    value: https://localhost:8443\n"
    );
}

#[test]
fn new_environment_with_certificates() {
    let mut doc = read_collection(&env_collection()).unwrap();
    let env = Environment {
        name: "Prod".into(),
        variables: Vec::new(),
        client_certificates: vec![ClientCertificate {
            domain: "api.example.com".into(),
            source: CertificateSource::Pkcs12 {
                path: "certs/prod.p12".into(),
            },
            passphrase: "{{prodPass}}".into(),
            enabled: true,
        }],
        external_secrets: None,
    };
    let path = doc.set_environment(&env).unwrap();
    assert_eq!(
        text(&doc.write(), &path),
        "name: Prod\nvariables: []\nclientCertificates:\n  - domain: api.example.com\n    type: pkcs12\n    pkcs12FilePath: certs/prod.p12\n    passphrase: \"{{prodPass}}\"\n"
    );
    assert_eq!(doc.model().environment("Prod").unwrap(), &env);
}

fn corpus(name: &str) -> FileSet {
    FileSet::from_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/collections/bruno")
            .join(name),
    )
    .unwrap()
}

#[test]
fn collection_config_certificates() {
    let input = corpus("ssl-client-certs-tests-collection-client-certs-enabled-yml");
    let original = text(&input, COLLECTION_FILE);
    let mut doc = read_collection(&input).unwrap();
    let config = doc.model().config.clone();
    assert_eq!(config.client_certificates.len(), 1);
    doc.set_collection_config(&config).unwrap();
    assert_eq!(text(&doc.write(), COLLECTION_FILE), original);

    let mut changed = config.clone();
    changed.client_certificates[0].domain = "127.0.0.1".into();
    doc.set_collection_config(&changed).unwrap();
    assert_eq!(
        text(&doc.write(), COLLECTION_FILE),
        original.replace("- domain: localhost", "- domain: 127.0.0.1")
    );
    assert_eq!(doc.model().config, changed);

    // Removing the certificates keeps the rest of `config` (protobuf, read since wave 8).
    let without = CollectionConfig {
        protobuf: config.protobuf.clone(),
        ..CollectionConfig::default()
    };
    doc.set_collection_config(&without).unwrap();
    let written = text(&doc.write(), COLLECTION_FILE);
    assert!(!written.contains("clientCertificates"), "{written}");
    assert!(written.contains("config:\n  protobuf:"), "{written}");
    let back = read_collection(&doc.write()).unwrap();
    assert_eq!(back.model().config, without);
}

#[test]
fn collection_config_is_added_and_removed() {
    let input = files(&[(
        COLLECTION_FILE,
        "opencollection: \"1.0.0\"\n\ninfo:\n  name: plain\n\nbundled: false\n",
    )]);
    let mut doc = read_collection(&input).unwrap();
    let config = CollectionConfig {
        ai: Default::default(),
        agents: Default::default(),
        http3: false,
        protobuf: Default::default(),
        client_certificates: vec![pem("localhost", "c.pem", "k.pem")],
    };
    doc.set_collection_config(&config).unwrap();
    assert_eq!(
        text(&doc.write(), COLLECTION_FILE),
        "opencollection: \"1.0.0\"\n\ninfo:\n  name: plain\n\nconfig:\n  clientCertificates:\n    - domain: localhost\n      type: pem\n      certificateFilePath: c.pem\n      privateKeyFilePath: k.pem\n\nbundled: false\n"
    );
    doc.set_collection_config(&CollectionConfig::default())
        .unwrap();
    assert_eq!(
        text(&doc.write(), COLLECTION_FILE),
        "opencollection: \"1.0.0\"\n\ninfo:\n  name: plain\n\nbundled: false\n"
    );
}
