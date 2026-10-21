//! Magic-token expansion for recipes.
//!
//! Recipes can use namespaced tokens that run resolves before writing the
//! compose file:
//!
//! - `${secret.NAME}` auto-registers an auto-generated secret and rewrites to a
//!   plain `${NAME}`, so the value flows through the generated `.env` and never
//!   lands in the compose file.
//! - `${app.name}`, `${app.fqdn}`, `${app.data_dir}` substitute known facts.
//!
//! Plain `${VAR}` (no namespace) and compose defaults like `${VAR:-default}` are
//! left untouched for docker compose to interpolate. `${input.*}` is reserved
//! for typed inputs and is left literal until those land.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{ProductManifest, SecretDef, SecretMethod, SecretsConfig, ServiceDef};

/// Facts a recipe can reference through `${app.*}` and `${input.*}` tokens.
pub struct AppContext {
    pub name: String,
    pub fqdn: String,
    pub data_dir: String,
    /// Resolved typed-input values, keyed by input key.
    pub inputs: BTreeMap<String, String>,
}

/// Expand magic tokens across a manifest, registering any `${secret.*}` the
/// recipe references so they are generated alongside declared secrets.
pub fn expand(manifest: &mut ProductManifest, app: &AppContext) {
    let mut needed_secrets: BTreeSet<String> = BTreeSet::new();

    for service in &mut manifest.services {
        expand_service(service, app, &mut needed_secrets);
    }

    if needed_secrets.is_empty() {
        return;
    }

    let secrets = manifest
        .secrets
        .get_or_insert_with(|| SecretsConfig { auto: Vec::new() });

    for name in needed_secrets {
        if secrets.auto.iter().any(|existing| existing.name == name) {
            continue;
        }

        secrets.auto.push(SecretDef {
            name,
            method: SecretMethod::Hex,
            length: 32,
        });
    }
}

/// Expand tokens in a single service's environment values and command args.
fn expand_service(service: &mut ServiceDef, app: &AppContext, secrets: &mut BTreeSet<String>) {
    if let Some(env) = &mut service.environment {
        for value in env.values_mut() {
            *value = substitute(value, app, secrets);
        }
    }

    if let Some(command) = &mut service.command {
        for arg in command {
            *arg = substitute(arg, app, secrets);
        }
    }
}

/// Replace every recognized `${namespace.key}` token in a string, leaving any
/// other `${...}` untouched for compose.
fn substitute(input: &str, app: &AppContext, secrets: &mut BTreeSet<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];

        let Some(end) = after.find('}') else {
            // no closing brace, emit the remainder verbatim
            out.push_str("${");
            rest = after;
            continue;
        };

        let inner = &after[..end];

        if let Some(replacement) = resolve_token(inner, app, secrets) {
            out.push_str(&replacement);
        } else {
            out.push_str("${");
            out.push_str(inner);
            out.push('}');
        }

        rest = &after[end + 1..];
    }

    out.push_str(rest);
    out
}

/// Resolve one token's inner text, or `None` to leave it literal.
fn resolve_token(inner: &str, app: &AppContext, secrets: &mut BTreeSet<String>) -> Option<String> {
    let (namespace, key) = inner.split_once('.')?;

    match namespace {
        "secret" if !key.is_empty() => {
            secrets.insert(key.to_owned());
            Some(format!("${{{key}}}"))
        }
        "app" => match key {
            "name" => Some(app.name.clone()),
            "fqdn" => Some(app.fqdn.clone()),
            "data_dir" => Some(app.data_dir.clone()),
            _ => None,
        },
        "input" => app.inputs.get(key).cloned(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn ctx() -> AppContext {
        AppContext {
            name: "myapp".to_owned(),
            fqdn: "localhost".to_owned(),
            data_dir: "/data/myapp".to_owned(),
            inputs: BTreeMap::new(),
        }
    }

    fn service(name: &str) -> ServiceDef {
        ServiceDef {
            name: name.to_owned(),
            image: "img".to_owned(),
            ports: None,
            command: None,
            environment: None,
            volumes: None,
            depends_on: None,
            extra_hosts: None,
            cap_add: None,
            security_opt: None,
            ulimits: None,
            healthcheck: None,
        }
    }

    fn with_env(name: &str, key: &str, value: &str) -> ServiceDef {
        let mut env = BTreeMap::new();
        env.insert(key.to_owned(), value.to_owned());

        let mut svc = service(name);
        svc.environment = Some(env);
        svc
    }

    fn manifest(services: Vec<ServiceDef>) -> ProductManifest {
        ProductManifest {
            name: "myapp".to_owned(),
            display_name: None,
            description: None,
            services,
            secrets: None,
            tls: None,
            proxy: None,
            hooks: None,
            inputs: None,
        }
    }

    fn env_value<'a>(manifest: &'a ProductManifest, service: &str, key: &str) -> &'a str {
        manifest
            .services
            .iter()
            .find(|s| s.name == service)
            .and_then(|s| s.environment.as_ref())
            .and_then(|env| env.get(key))
            .map(String::as_str)
            .expect("env value present")
    }

    #[test]
    fn secret_token_registers_and_rewrites_to_plain_var() {
        let mut m = manifest(vec![with_env("api", "PASSWORD", "${secret.db_password}")]);

        expand(&mut m, &ctx());

        assert_eq!(env_value(&m, "api", "PASSWORD"), "${db_password}");
        let secrets = m.secrets.expect("a secret was registered");
        assert!(secrets.auto.iter().any(|s| s.name == "db_password"));
    }

    #[test]
    fn app_tokens_substitute_known_values() {
        let mut m = manifest(vec![with_env("web", "URL", "https://${app.fqdn}/${app.name}")]);

        expand(&mut m, &ctx());

        assert_eq!(env_value(&m, "web", "URL"), "https://localhost/myapp");
    }

    #[test]
    #[allow(clippy::literal_string_with_formatting_args)] // the ${...} are compose vars, not format args
    fn plain_compose_vars_are_left_untouched() {
        let dsn = "postgres://${DB_USER:-postgres}:${DB_PASSWORD}@db";
        let mut m = manifest(vec![with_env("db", "DSN", dsn)]);

        expand(&mut m, &ctx());

        assert_eq!(env_value(&m, "db", "DSN"), dsn);
        assert!(m.secrets.is_none(), "plain vars register no secrets");
    }

    #[test]
    fn unknown_namespace_is_left_literal() {
        let mut m = manifest(vec![with_env("x", "V", "${weird.thing}")]);

        expand(&mut m, &ctx());

        assert_eq!(env_value(&m, "x", "V"), "${weird.thing}");
    }

    #[test]
    fn input_token_substitutes_a_resolved_value() {
        let mut inputs = BTreeMap::new();
        inputs.insert("TZ".to_owned(), "Europe/Paris".to_owned());
        let app = AppContext {
            name: "myapp".to_owned(),
            fqdn: "localhost".to_owned(),
            data_dir: "/d".to_owned(),
            inputs,
        };
        let mut m = manifest(vec![with_env("web", "TZ", "${input.TZ}")]);

        expand(&mut m, &app);

        assert_eq!(env_value(&m, "web", "TZ"), "Europe/Paris");
    }

    #[test]
    fn unresolved_input_is_left_literal() {
        let mut m = manifest(vec![with_env("x", "V", "${input.absent}")]);

        expand(&mut m, &ctx());

        assert_eq!(env_value(&m, "x", "V"), "${input.absent}");
    }

    #[test]
    fn a_secret_referenced_twice_is_registered_once() {
        let mut m = manifest(vec![
            with_env("a", "TOKEN", "${secret.shared}"),
            with_env("b", "TOKEN", "${secret.shared}"),
        ]);

        expand(&mut m, &ctx());

        let secrets = m.secrets.expect("a secret was registered");
        assert_eq!(secrets.auto.iter().filter(|s| s.name == "shared").count(), 1);
    }

    #[test]
    fn secret_token_in_command_is_expanded() {
        let mut svc = service("svc");
        svc.command = Some(vec!["--pass".to_owned(), "${secret.token}".to_owned()]);
        let mut m = manifest(vec![svc]);

        expand(&mut m, &ctx());

        let command = m.services[0].command.as_ref().expect("command present");
        assert_eq!(command[1], "${token}");
        assert!(
            m.secrets
                .expect("secret registered")
                .auto
                .iter()
                .any(|s| s.name == "token")
        );
    }

    #[test]
    fn an_already_declared_secret_is_not_duplicated() {
        let mut m = manifest(vec![with_env("api", "PASSWORD", "${secret.db}")]);
        m.secrets = Some(SecretsConfig {
            auto: vec![SecretDef {
                name: "db".to_owned(),
                method: SecretMethod::Hex,
                length: 16,
            }],
        });

        expand(&mut m, &ctx());

        let secrets = m.secrets.expect("secrets present");
        assert_eq!(secrets.auto.iter().filter(|s| s.name == "db").count(), 1);
        // the pre-existing declaration (length 16) is kept, not overwritten
        assert_eq!(secrets.auto.iter().find(|s| s.name == "db").map(|s| s.length), Some(16));
    }
}
