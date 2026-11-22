// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! External secret sources with a fake runner (commands and their answers), and the native
//! runner against fake CLIs (shell scripts, Unix only).

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn secret(name: &str, reference: &str) -> ExternalSecret {
    ExternalSecret {
        name: name.into(),
        reference: reference.into(),
        enabled: true,
    }
}

fn args(provider: SecretProvider, name: &str, reference: &str) -> String {
    lookup(provider, &secret(name, reference))
        .unwrap()
        .command
        .display()
}

#[test]
fn references_become_the_providers_commands() {
    use SecretProvider::*;
    assert_eq!(
        args(OnePassword, "token", "op://Dev/api/token"),
        "op read --no-newline op://Dev/api/token"
    );
    assert_eq!(
        args(HashicorpVaultServer, "password", "secret/app"),
        "vault kv get -format=json secret/app"
    );
    assert_eq!(
        args(
            HashicorpVaultCloud,
            "password",
            "secret/data/app#db_password"
        ),
        "vault read -format=json secret/data/app"
    );
    assert_eq!(
        args(AwsSecretsManager, "token", "prod/api#token"),
        "aws secretsmanager get-secret-value --secret-id=prod/api --query SecretString --output text"
    );
    assert_eq!(
        args(AzureKeyVault, "api-key", "my-vault"),
        "az keyvault secret show --vault-name=my-vault --name=api-key --query value --output tsv"
    );
    assert_eq!(
        args(AzureKeyVault, "apiKey", "my-vault/api-key/0123abcd"),
        "az keyvault secret show --vault-name=my-vault --name=api-key --version=0123abcd --query value --output tsv"
    );
    assert_eq!(
        args(GcpSecretsManager, "token", "my-project"),
        "gcloud secrets versions access latest --secret=token --project=my-project"
    );
    assert_eq!(
        args(GcpSecretsManager, "token", "my-project/api-token/3"),
        "gcloud secrets versions access 3 --secret=api-token --project=my-project"
    );
    assert_eq!(
        args(
            GcpSecretsManager,
            "token",
            "projects/p1/secrets/s1/versions/7"
        ),
        "gcloud secrets versions access 7 --secret=s1 --project=p1"
    );
}

#[test]
fn references_cannot_inject_options_or_arguments() {
    use SecretProvider::*;
    for (provider, reference) in [
        (OnePassword, "vault/item"),
        (OnePassword, "op://a b/c"),
        (HashicorpVaultServer, "-address=https://evil#x"),
        (HashicorpVaultServer, "secret/app#-field"),
        (AwsSecretsManager, "--profile evil"),
        (AwsSecretsManager, "a\nb"),
        (AzureKeyVault, "-v/x"),
        (GcpSecretsManager, "p/-s"),
        (GcpSecretsManager, "projects/p/other/x"),
        (AwsSecretsManager, "  "),
    ] {
        let err = lookup(provider, &secret("v", reference)).unwrap_err();
        assert!(err.contains("`v`"), "{reference}: {err}");
    }
}

/// Answers each command from a table and counts the calls.
struct Fake {
    answers: Vec<(String, Result<Vec<u8>, RunError>)>,
    calls: AtomicUsize,
}

impl Fake {
    fn new(answers: &[(&str, Result<&str, RunError>)]) -> Arc<Self> {
        Arc::new(Self {
            answers: answers
                .iter()
                .map(|(cmd, out)| {
                    (
                        (*cmd).to_owned(),
                        out.clone().map(|s| s.as_bytes().to_vec()),
                    )
                })
                .collect(),
            calls: AtomicUsize::new(0),
        })
    }
}

impl CommandRunner for Fake {
    fn run(&self, command: &SecretCommand, _: Duration) -> Result<Vec<u8>, RunError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let line = command.display();
        self.answers
            .iter()
            .find(|(cmd, _)| *cmd == line)
            .map(|(_, out)| out.clone())
            .unwrap_or_else(|| panic!("unexpected command {line}"))
    }
}

fn one(
    provider: SecretProvider,
    s: ExternalSecret,
    answer: Result<&str, RunError>,
) -> Result<String, String> {
    let l = lookup(provider, &s).unwrap();
    let fake = Fake::new(&[(&l.command.display(), answer)]);
    resolve_one(fake.as_ref(), provider, &s, DEFAULT_TIMEOUT)
}

#[test]
fn outputs_become_values() {
    use SecretProvider::*;
    assert_eq!(
        one(OnePassword, secret("t", "op://a/b/c"), Ok("v4lue")),
        Ok("v4lue".into())
    );
    // The tools' final line break is not part of the value.
    assert_eq!(
        one(AzureKeyVault, secret("t", "kv"), Ok("v4lue\n")),
        Ok("v4lue".into())
    );
    assert_eq!(
        one(GcpSecretsManager, secret("t", "p"), Ok("line1\nline2\r\n")),
        Ok("line1\nline2".into())
    );
    // AWS: the whole string, or one key of a JSON secret.
    assert_eq!(
        one(AwsSecretsManager, secret("t", "s"), Ok("{\"a\":1}\n")),
        Ok("{\"a\":1}".into())
    );
    assert_eq!(
        one(
            AwsSecretsManager,
            secret("t", "s#password"),
            Ok("{\"user\":\"u\",\"password\":\"pw\"}\n")
        ),
        Ok("pw".into())
    );
    let err = one(
        AwsSecretsManager,
        secret("t", "s#nope"),
        Ok("{\"user\":\"u\"}"),
    )
    .unwrap_err();
    assert!(err.contains("no field `nope` (fields: user)"), "{err}");
    let err = one(AwsSecretsManager, secret("t", "s"), Ok("None\n")).unwrap_err();
    assert!(err.contains("binary"), "{err}");
}

#[test]
fn vault_reads_kv1_and_kv2_answers() {
    use SecretProvider::*;
    let kv2 = r#"{"data":{"data":{"password":"pw2","user":"u"},"metadata":{"version":3}}}"#;
    let kv1 = r#"{"data":{"password":"pw1"},"lease_duration":0}"#;
    assert_eq!(
        one(
            HashicorpVaultServer,
            secret("password", "secret/app"),
            Ok(kv2)
        ),
        Ok("pw2".into())
    );
    assert_eq!(
        one(
            HashicorpVaultServer,
            secret("db", "kv/app#password"),
            Ok(kv1)
        ),
        Ok("pw1".into())
    );
    let err = one(HashicorpVaultServer, secret("token", "secret/app"), Ok(kv2)).unwrap_err();
    assert!(
        err.contains("no field `token` (fields: password, user)"),
        "{err}"
    );
    assert!(!err.contains("pw2"), "{err}");
}

#[test]
fn errors_say_what_to_do() {
    use SecretProvider::*;
    let s = secret("token", "op://a/b/c");
    let err = one(OnePassword, s.clone(), Err(RunError::NotFound)).unwrap_err();
    assert!(err.contains("`op` was not found"), "{err}");
    assert!(err.contains("Install the 1Password CLI"), "{err}");
    let err = one(
        OnePassword,
        s.clone(),
        Err(RunError::Failed {
            code: Some(1),
            stderr:
                "[ERROR] 2026/11/22 You are not currently signed in. Please run `op signin --help`"
                    .into(),
        }),
    )
    .unwrap_err();
    assert!(err.starts_with("1Password: not signed in"), "{err}");
    assert!(err.contains("op signin"), "{err}");
    let err = one(OnePassword, s.clone(), Err(RunError::Timeout)).unwrap_err();
    assert!(err.contains("did not answer within 60 s"), "{err}");
    let err = one(
        AwsSecretsManager,
        secret("token", "prod/api"),
        Err(RunError::Failed {
            code: Some(255),
            stderr: "\nUnable to locate credentials. You can configure credentials by running \"aws configure\".\n".into(),
        }),
    )
    .unwrap_err();
    assert!(err.contains("aws sso login"), "{err}");
    let err = one(
        AzureKeyVault,
        secret("token", "kv"),
        Err(RunError::Failed {
            code: Some(1),
            stderr: "ERROR: (SecretNotFound) A secret with (name/id) token was not found in this key vault.".into(),
        }),
    )
    .unwrap_err();
    assert!(
        err.contains("failed for `token` (exit code 1): ERROR: (SecretNotFound)"),
        "{err}"
    );
    let err = one(
        GcpSecretsManager,
        secret("token", "p"),
        Err(RunError::Failed {
            code: Some(1),
            stderr: "ERROR: (gcloud.secrets.versions.access) You do not currently have an active account selected.".into(),
        }),
    )
    .unwrap_err();
    assert!(err.contains("gcloud auth login"), "{err}");
}

fn secrets(provider: SecretProvider, vars: &[ExternalSecret]) -> ExternalSecrets {
    ExternalSecrets {
        provider,
        variables: vars.to_vec(),
    }
}

#[test]
fn the_resolver_caches_per_session_and_skips_what_it_must() {
    let fake = Fake::new(&[
        ("op read --no-newline op://a/b/one", Ok("1")),
        ("op read --no-newline op://a/b/two", Ok("2")),
    ]);
    let resolver = SecretResolver::new(fake.clone());
    let mut off = secret("off", "op://a/b/off");
    off.enabled = false;
    let env = secrets(
        SecretProvider::OnePassword,
        &[
            secret("one", "op://a/b/one"),
            secret("two", "op://a/b/two"),
            off,
            secret("", "op://a/b/nameless"),
        ],
    );
    assert!(resolver.cached(&env).is_empty());
    let values = resolver.resolve(&env, &[]).unwrap();
    assert_eq!(
        values,
        [
            ("one".to_owned(), "1".to_owned()),
            ("two".into(), "2".into())
        ]
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    // Cached: no second run.
    assert_eq!(resolver.resolve(&env, &[]).unwrap(), values);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    assert_eq!(resolver.cached(&env), values);
    // A variable that has a value already (keychain) is not read.
    assert_eq!(
        resolver.resolve(&env, &["one".to_owned()]).unwrap(),
        [("two".to_owned(), "2".to_owned())]
    );
    // An edited reference is read anew; the debug output never shows values.
    resolver.clear();
    assert!(resolver.cached(&env).is_empty());
    assert!(!format!("{resolver:?}").contains('1'));
}

#[test]
fn the_resolver_stops_at_the_first_error() {
    let fake = Fake::new(&[("op read --no-newline op://a/b/one", Err(RunError::NotFound))]);
    let resolver = SecretResolver::new(fake.clone());
    let env = secrets(
        SecretProvider::OnePassword,
        &[secret("one", "op://a/b/one"), secret("two", "op://a/b/two")],
    );
    let err = resolver.resolve(&env, &[]).unwrap_err();
    assert!(err.contains("`one`"), "{err}");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(resolver.cached(&env).is_empty());
}

#[cfg(unix)]
mod process {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::PathBuf;

    use super::*;

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fake_cli(test: &str, program: &str, script: &str) -> (Dir, ProcessRunner) {
        let dir =
            std::env::temp_dir().join(format!("rt-app-secret-cli-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(program);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (Dir(dir.clone()), ProcessRunner { dir: Some(dir) })
    }

    fn op(args: &[&str]) -> SecretCommand {
        SecretCommand {
            program: "op",
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn runs_the_program_with_its_arguments_and_no_input() {
        let (_d, runner) = fake_cli(
            "args",
            "op",
            "read -r line || echo \"no input\" >&2\nprintf '%s|' \"$@\"",
        );
        let out = runner
            .run(&op(&["read", "op://a b/c"]), Duration::from_secs(10))
            .unwrap();
        // Arguments arrive unchanged (no shell splits them).
        assert_eq!(String::from_utf8(out).unwrap(), "read|op://a b/c|");
    }

    #[test]
    fn reports_failures_with_their_error_output() {
        let (_d, runner) = fake_cli("fail", "op", "echo 'not currently signed in' >&2\nexit 6");
        let err = runner.run(&op(&[]), Duration::from_secs(10)).unwrap_err();
        assert_eq!(
            err,
            RunError::Failed {
                code: Some(6),
                stderr: "not currently signed in\n".into()
            }
        );
    }

    #[test]
    fn a_missing_program_is_not_found() {
        let (_d, runner) = fake_cli("missing", "other", "exit 0");
        assert_eq!(
            runner.run(&op(&[]), Duration::from_secs(10)),
            Err(RunError::NotFound)
        );
    }

    #[test]
    fn a_hanging_program_is_stopped_at_the_timeout() {
        let (_d, runner) = fake_cli("hang", "op", "sleep 30");
        let started = web_time::Instant::now();
        assert_eq!(
            runner.run(&op(&[]), Duration::from_millis(300)),
            Err(RunError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn large_outputs_are_refused() {
        let (_d, runner) = fake_cli("large", "op", "head -c 1100000 /dev/zero | tr '\\0' a");
        assert!(matches!(
            runner.run(&op(&[]), Duration::from_secs(10)),
            Err(RunError::Other(_))
        ));
    }
}
