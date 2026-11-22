// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The policy in force for `rt mcp`, and what agents may pass as variables.
//!
//! **Pinned at start.** An agent that can edit files could otherwise give itself write
//! access by editing `opencollection.yml`. `rt mcp` reads the policy once when it starts;
//! later the stricter of that and the file's current policy applies per environment (lower
//! access, production if either says so). Loosening a rule needs a restart of `rt mcp`.
//!
//! **Variables from the agent** (`vars` of `send_request` and `run_folder`) may fill in
//! names nothing else defines and override request or folder variables (ids, filters,
//! pages). They may not override variables of the collection, the environment or `--var`
//! (base URLs, hosts and credentials live there), nor any name declared secret: otherwise
//! an agent could point a request that carries a token at its own server.

use rt_core::model::{AgentAccess, AgentPolicy, Collection, EnvironmentPolicy};

fn rank(a: AgentAccess) -> u8 {
    match a {
        AgentAccess::None => 0,
        AgentAccess::Read => 1,
        AgentAccess::Write => 2,
    }
}

fn lower(a: AgentAccess, b: AgentAccess) -> AgentAccess {
    if rank(a) <= rank(b) { a } else { b }
}

/// Per environment the stricter of `pinned` and `current`.
pub fn tighten(pinned: &AgentPolicy, current: &AgentPolicy) -> AgentPolicy {
    let mut names: Vec<&str> = Vec::new();
    for e in pinned.environments.iter().chain(&current.environments) {
        if !names.contains(&e.environment.as_str()) {
            names.push(&e.environment);
        }
    }
    AgentPolicy {
        access: lower(pinned.access, current.access),
        environments: names
            .into_iter()
            .map(|name| {
                let (a, pa) = pinned.for_environment(Some(name));
                let (b, pb) = current.for_environment(Some(name));
                EnvironmentPolicy {
                    environment: name.to_owned(),
                    access: lower(a, b),
                    production: pa || pb,
                }
            })
            .collect(),
    }
}

/// Checks the agent's variables for a send of `request_path` in `environment`. `given` are
/// the `--var` names. `Err` is the reason the agent sees.
pub fn check_vars(
    collection: &Collection,
    request_path: Option<&str>,
    environment: Option<&str>,
    given: &[(String, String)],
    vars: &[(String, String)],
) -> Result<(), String> {
    let env = environment.and_then(|n| collection.environment(n));
    let context = request_path.and_then(|p| collection.request(p));
    for (name, _) in vars {
        let in_collection = collection.defaults.vars.iter().any(|v| v.name == *name);
        let in_environment = env.is_some_and(|e| e.variables.iter().any(|v| v.name == *name));
        let from_cli = given.iter().any(|(n, _)| n == name);
        if in_collection || in_environment || from_cli {
            return Err(format!(
                "the variable {name} is set by the collection, the environment or the user; \
                 agents can only set request variables and variables nothing else defines"
            ));
        }
        let secret_somewhere = collection
            .environments
            .iter()
            .flat_map(|e| &e.variables)
            .chain(context.iter().flat_map(|c| {
                c.folders
                    .iter()
                    .flat_map(|f| &f.defaults.vars)
                    .chain(&c.request.spec.vars)
            }))
            .any(|v| v.secret && v.name == *name);
        if secret_somewhere {
            return Err(format!(
                "the variable {name} is secret; agents cannot set secret variables"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::model::{Environment, Variable};

    fn env(name: &str, access: AgentAccess, production: bool) -> EnvironmentPolicy {
        EnvironmentPolicy {
            environment: name.into(),
            access,
            production,
        }
    }

    #[test]
    fn later_edits_can_only_tighten() {
        let pinned = AgentPolicy {
            access: AgentAccess::Read,
            environments: vec![
                env("PRD", AgentAccess::Write, true),
                env("Local", AgentAccess::Write, false),
            ],
        };
        // The agent tries to loosen everything.
        let loosened = AgentPolicy {
            access: AgentAccess::Write,
            environments: vec![
                env("PRD", AgentAccess::Write, false),
                env("Local", AgentAccess::Write, false),
                env("Vault", AgentAccess::Write, false),
            ],
        };
        let p = tighten(&pinned, &loosened);
        assert_eq!(p.access, AgentAccess::Read);
        assert_eq!(p.for_environment(Some("PRD")), (AgentAccess::Write, true));
        assert_eq!(p.for_environment(Some("Vault")), (AgentAccess::Read, false));
        assert_eq!(p.for_environment(Some("Other")), (AgentAccess::Read, false));
        // The user tightens: applies at once.
        let tightened = AgentPolicy {
            access: AgentAccess::None,
            environments: vec![env("Local", AgentAccess::Read, true)],
        };
        let p = tighten(&pinned, &tightened);
        assert_eq!(p.access, AgentAccess::None);
        assert_eq!(p.for_environment(Some("Local")), (AgentAccess::Read, true));
        assert_eq!(p.for_environment(Some("PRD")), (AgentAccess::None, true));
    }

    #[test]
    fn agents_cannot_redirect_or_set_secrets() {
        let mut c = Collection::default();
        c.defaults.vars.push(Variable::new("host", "h"));
        c.environments.push(Environment {
            name: "PRD".into(),
            variables: vec![
                Variable::new("base", "https://prd"),
                Variable {
                    name: "token".into(),
                    value: None,
                    enabled: true,
                    secret: true,
                },
            ],
            client_certificates: Vec::new(),
            external_secrets: None,
        });
        let v = |n: &str| vec![(n.to_owned(), "x".to_owned())];
        let given = vec![("apikey".to_owned(), "k".to_owned())];
        for bad in ["host", "base", "apikey", "token"] {
            assert!(
                check_vars(&c, None, Some("PRD"), &given, &v(bad)).is_err(),
                "{bad}"
            );
        }
        // `token` is secret in PRD; also refused when sending without an environment.
        assert!(check_vars(&c, None, None, &given, &v("token")).is_err());
        assert!(check_vars(&c, None, Some("PRD"), &given, &v("orderId")).is_ok());
        assert!(check_vars(&c, None, Some("PRD"), &given, &[]).is_ok());
    }
}
