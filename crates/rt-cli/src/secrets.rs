// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! External secrets for `rt send` (wave 5, strand E): the values of the selected
//! environment's `external_secrets`, read with the manager's own CLI (`op`, `vault`, `aws`,
//! `az`, `gcloud`; `rt_app::secret_sources`) and put into the environment layer as secret
//! variables, like keychain values in the app: the report masks them.
//!
//! Variables given with `--var` are not read (the runtime value wins anyway), so a pipeline
//! can pass a value without the manager's CLI installed.
//!
//! [`resolve_all`] first fills secret variables without a value from the OS keychain, where
//! the desktop app keeps them (same entries: collection = canonical root), so `rt send`,
//! `rt run` and `rt mcp` send what the app sends. A keychain value wins over a secret
//! manager, as in the app; an unreachable keychain just leaves the value unset.

use rt_app::secret_sources::{CommandRunner, DEFAULT_TIMEOUT, ProcessRunner, active, resolve_one};
use rt_app::secrets::{SecretKey, SecretStore};
use rt_core::model::{Collection, Variable};

/// Keychain values for the secret variables of `environment` that have no value and are not
/// in `given`, then the external secrets of the rest. `collection_id` is the keychain's
/// collection key ([`crate::oauth::collection_id`] of the root).
pub async fn resolve_all(
    collection: &Collection,
    environment: Option<&str>,
    given: &[(String, String)],
    collection_id: &str,
) -> Result<Collection, String> {
    let store = rt_app::secrets::keychain::KeychainStore::default();
    let mut collection = collection.clone();
    let mut skip = given.to_vec();
    for name in fill_from_store(&mut collection, environment, given, collection_id, &store) {
        skip.push((name, String::new()));
    }
    resolve(&collection, environment, &skip).await
}

/// Fills secret variables without a value from `store`; returns the names it filled.
pub fn fill_from_store(
    collection: &mut Collection,
    environment: Option<&str>,
    given: &[(String, String)],
    collection_id: &str,
    store: &dyn SecretStore,
) -> Vec<String> {
    let mut filled = Vec::new();
    let Some(env) =
        environment.and_then(|n| collection.environments.iter_mut().find(|e| e.name == n))
    else {
        return filled;
    };
    for var in env
        .variables
        .iter_mut()
        .filter(|v| v.secret && v.value.is_none())
    {
        if given.iter().any(|(n, _)| *n == var.name) {
            continue;
        }
        let key = SecretKey {
            collection: collection_id.to_owned(),
            environment: env.name.clone(),
            variable: var.name.clone(),
        };
        if let Ok(Some(value)) = store.get(&key) {
            var.value = Some(value);
            filled.push(var.name.clone());
        }
    }
    filled
}

/// The collection with the external secrets of `environment` filled in, except the names in
/// `given` (runtime variables).
pub async fn resolve(
    collection: &Collection,
    environment: Option<&str>,
    given: &[(String, String)],
) -> Result<Collection, String> {
    resolve_with(collection, environment, given, &ProcessRunner::default())
}

/// [`resolve`] with another runner (tests).
pub fn resolve_with(
    collection: &Collection,
    environment: Option<&str>,
    given: &[(String, String)],
    runner: &dyn CommandRunner,
) -> Result<Collection, String> {
    let mut collection = collection.clone();
    // An unknown environment is reported by `prepare`.
    let Some(env) =
        environment.and_then(|name| collection.environments.iter_mut().find(|e| e.name == name))
    else {
        return Ok(collection);
    };
    let Some(secrets) = env.external_secrets.clone() else {
        return Ok(collection);
    };
    for secret in active(&secrets).filter(|s| !given.iter().any(|(n, _)| *n == s.name)) {
        let value = resolve_one(runner, secrets.provider, secret, DEFAULT_TIMEOUT)?;
        match env.variables.iter_mut().find(|v| v.name == secret.name) {
            Some(v) => {
                v.value = Some(value);
                v.secret = true;
                v.enabled = true;
            }
            None => env.variables.push(Variable {
                name: secret.name.clone(),
                value: Some(value),
                enabled: true,
                secret: true,
            }),
        }
    }
    Ok(collection)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use rt_app::secret_sources::{RunError, SecretCommand};
    use rt_core::model::{Environment, ExternalSecret, ExternalSecrets, SecretProvider};

    use super::*;

    #[test]
    fn keychain_values_fill_secret_variables_without_a_value() {
        use rt_app::auth::MemorySecrets;
        let store = MemorySecrets::default();
        let key = |v: &str| SecretKey {
            collection: "/c".into(),
            environment: "Dev".into(),
            variable: v.into(),
        };
        store.set(&key("token"), "from-keychain").unwrap();
        store.set(&key("given"), "ignored").unwrap();
        store.set(&key("plain"), "ignored").unwrap();
        let var = |name: &str, value: Option<&str>, secret: bool| Variable {
            name: name.into(),
            value: value.map(Into::into),
            enabled: true,
            secret,
        };
        let mut collection = Collection::default();
        collection.environments.push(Environment {
            name: "Dev".into(),
            variables: vec![
                var("token", None, true),
                var("given", None, true),
                var("plain", Some("file"), false),
                var("missing", None, true),
            ],
            ..Default::default()
        });
        let given = vec![("given".to_owned(), "cli".to_owned())];
        let filled = fill_from_store(&mut collection, Some("Dev"), &given, "/c", &store);
        assert_eq!(filled, ["token"]);
        let vars = &collection.environments[0].variables;
        assert_eq!(vars[0].value.as_deref(), Some("from-keychain"));
        assert_eq!(vars[1].value, None);
        assert_eq!(vars[2].value.as_deref(), Some("file"));
        assert_eq!(vars[3].value, None);
        // Other environments and no environment fill nothing.
        assert!(fill_from_store(&mut collection, Some("Prod"), &[], "/c", &store).is_empty());
        assert!(fill_from_store(&mut collection, None, &[], "/c", &store).is_empty());
    }

    #[derive(Default)]
    struct Fake(Mutex<Vec<String>>);

    impl CommandRunner for Fake {
        fn run(&self, command: &SecretCommand, _: Duration) -> Result<Vec<u8>, RunError> {
            let line = command.display();
            self.0.lock().unwrap().push(line.clone());
            match line.rsplit('/').next() {
                Some("missing") => Err(RunError::NotFound),
                Some(field) => Ok(format!("value-of-{field}\n").into_bytes()),
                None => unreachable!(),
            }
        }
    }

    fn collection(vars: &[(&str, &str)]) -> Collection {
        let mut c = Collection::default();
        c.environments.push(Environment {
            name: "Dev".into(),
            variables: vec![Variable::new("host", "h"), Variable::new("token", "stale")],
            client_certificates: Vec::new(),
            external_secrets: Some(ExternalSecrets {
                provider: SecretProvider::OnePassword,
                variables: vars
                    .iter()
                    .map(|(n, r)| ExternalSecret {
                        name: (*n).into(),
                        reference: (*r).into(),
                        enabled: true,
                    })
                    .collect(),
            }),
        });
        c
    }

    #[test]
    fn fills_the_environment_with_secret_variables() {
        let c = collection(&[("token", "op://v/i/token"), ("key", "op://v/i/key")]);
        let fake = Fake::default();
        let out = resolve_with(&c, Some("Dev"), &[], &fake).unwrap();
        let env = out.environment("Dev").unwrap();
        let get = |n: &str| env.variables.iter().find(|v| v.name == n).unwrap();
        assert_eq!(get("token").value.as_deref(), Some("value-of-token"));
        assert!(get("token").secret);
        assert_eq!(get("key").value.as_deref(), Some("value-of-key"));
        assert!(get("key").secret);
        assert!(!get("host").secret);
        assert_eq!(fake.0.lock().unwrap().len(), 2);
        // The input is unchanged.
        assert_eq!(c.environment("Dev").unwrap().variables.len(), 2);
    }

    #[test]
    fn skips_runtime_variables_other_environments_and_no_environment() {
        let c = collection(&[("token", "op://v/i/token"), ("key", "op://v/i/missing")]);
        let fake = Fake::default();
        let given = [("key".to_owned(), "from-the-pipeline".to_owned())];
        let out = resolve_with(&c, Some("Dev"), &given, &fake).unwrap();
        assert_eq!(
            *fake.0.lock().unwrap(),
            ["op read --no-newline op://v/i/token"]
        );
        assert!(
            out.environment("Dev")
                .unwrap()
                .variables
                .iter()
                .all(|v| v.name != "key")
        );
        assert_eq!(resolve_with(&c, None, &[], &fake).unwrap(), c);
        assert_eq!(resolve_with(&c, Some("Prod"), &[], &fake).unwrap(), c);
    }

    #[test]
    fn a_failing_secret_fails_with_its_message() {
        let c = collection(&[("key", "op://v/i/missing")]);
        let err = resolve_with(&c, Some("Dev"), &[], &Fake::default()).unwrap_err();
        assert!(err.contains("`op` was not found"), "{err}");
    }
}
