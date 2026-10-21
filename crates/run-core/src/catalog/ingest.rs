//! Ingest a user's docker-compose file into run's internal manifest.
//!
//! This is what makes `run up ./docker-compose.yml` work: the user's own
//! compose becomes a `ProductManifest`, so the whole run pipeline (secrets,
//! TLS, proxy, health) applies to a bring-your-own stack. Polymorphic compose
//! fields (ports, environment, command, `depends_on`) are kept as raw YAML and
//! normalized here; the common shapes are covered, unusual ones are skipped
//! rather than failing the whole file.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::config::{DependsOn, ProductManifest, ServiceDef};

/// A docker-compose file, reduced to the fields run maps onto a manifest.
#[derive(Deserialize)]
struct ComposeFile {
    name: Option<String>,
    #[serde(default)]
    services: BTreeMap<String, ComposeService>,
}

/// A single compose service. The messy polymorphic fields stay as raw YAML and
/// are normalized during ingestion.
#[derive(Deserialize)]
struct ComposeService {
    image: Option<String>,
    ports: Option<Vec<serde_yaml::Value>>,
    environment: Option<serde_yaml::Value>,
    volumes: Option<Vec<String>>,
    command: Option<serde_yaml::Value>,
    depends_on: Option<serde_yaml::Value>,
}

/// Ingest a docker-compose document into a product manifest.
///
/// `default_name` names the stack when the compose file has no top-level
/// `name` (run derives it from the directory or file name).
///
/// # Errors
///
/// Returns `Error::ParseYaml` if the document is not valid YAML, or
/// `Error::ComposeIngest` if it has no runnable services (no services at all,
/// or a service with no `image`).
pub fn ingest_compose(contents: &str, default_name: &str) -> crate::Result<ProductManifest> {
    let compose: ComposeFile = serde_yaml::from_str(contents).map_err(|source| crate::Error::ParseYaml { source })?;

    let mut services = Vec::new();

    for (name, svc) in compose.services {
        let image = svc.image.ok_or_else(|| crate::Error::ComposeIngest {
            message: format!("service `{name}` has no image (build-only services are not supported yet)"),
        })?;

        services.push(ServiceDef {
            name,
            image,
            ports: svc.ports.map(|ports| ports.iter().filter_map(port_to_string).collect()),
            command: svc.command.and_then(convert_command),
            environment: svc.environment.and_then(convert_env),
            volumes: svc.volumes,
            depends_on: svc.depends_on.and_then(convert_depends_on),
            extra_hosts: None,
            cap_add: None,
            security_opt: None,
            ulimits: None,
            healthcheck: None,
        });
    }

    if services.is_empty() {
        return Err(crate::Error::ComposeIngest {
            message: "compose file defines no services".to_owned(),
        });
    }

    Ok(ProductManifest {
        name: compose.name.unwrap_or_else(|| default_name.to_owned()),
        display_name: None,
        description: None,
        services,
        secrets: None,
        tls: None,
        proxy: None,
        hooks: None,
    })
}

/// Render a scalar YAML value as a plain string, empty for non-scalars.
fn scalar_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Normalize one compose port entry (`"8080:80"`, `9090`, or long syntax).
fn port_to_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Mapping(mapping) => {
            let target = mapping.get("target").map(scalar_to_string)?;
            let published = mapping.get("published").map(scalar_to_string);

            match published {
                Some(port) if !port.is_empty() => Some(format!("{port}:{target}")),
                _ => Some(target),
            }
        }
        _ => None,
    }
}

/// Normalize compose `environment`, which is either a map or a `KEY=value` list.
fn convert_env(value: serde_yaml::Value) -> Option<BTreeMap<String, String>> {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            let mut env = BTreeMap::new();

            for (key, val) in mapping {
                if let serde_yaml::Value::String(key) = key {
                    env.insert(key, scalar_to_string(&val));
                }
            }

            Some(env)
        }
        serde_yaml::Value::Sequence(seq) => {
            let mut env = BTreeMap::new();

            for item in seq {
                if let serde_yaml::Value::String(entry) = item {
                    match entry.split_once('=') {
                        Some((key, val)) => env.insert(key.to_owned(), val.to_owned()),
                        None => env.insert(entry, String::new()),
                    };
                }
            }

            Some(env)
        }
        _ => None,
    }
}

/// Normalize compose `command`, which is either a string or an argv list.
fn convert_command(value: serde_yaml::Value) -> Option<Vec<String>> {
    match value {
        serde_yaml::Value::String(s) => Some(s.split_whitespace().map(ToOwned::to_owned).collect()),
        serde_yaml::Value::Sequence(seq) => Some(
            seq.iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect(),
        ),
        _ => None,
    }
}

/// Normalize compose `depends_on`, either a name list or a `name: {condition}` map.
fn convert_depends_on(value: serde_yaml::Value) -> Option<Vec<DependsOn>> {
    match value {
        serde_yaml::Value::Sequence(seq) => Some(
            seq.iter()
                .filter_map(|item| {
                    item.as_str().map(|service| DependsOn {
                        service: service.to_owned(),
                        condition: None,
                    })
                })
                .collect(),
        ),
        serde_yaml::Value::Mapping(mapping) => {
            let mut deps = Vec::new();

            for (key, val) in mapping {
                if let serde_yaml::Value::String(service) = key {
                    let condition = val.get("condition").and_then(|c| c.as_str()).map(ToOwned::to_owned);
                    deps.push(DependsOn { service, condition });
                }
            }

            Some(deps)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_maps_services_with_image_and_ports() {
        let yaml = "
services:
  web:
    image: nginx:1.27
    ports:
      - \"8080:80\"
      - 9090
";
        let manifest = ingest_compose(yaml, "mystack").expect("should ingest");

        assert_eq!(manifest.name, "mystack");
        assert_eq!(manifest.services.len(), 1);

        let web = &manifest.services[0];
        assert_eq!(web.name, "web");
        assert_eq!(web.image, "nginx:1.27");
        assert_eq!(
            web.ports.as_deref(),
            Some(&["8080:80".to_owned(), "9090".to_owned()][..])
        );
    }

    #[test]
    fn ingest_top_level_name_wins_over_default() {
        let yaml = "
name: realname
services:
  app:
    image: busybox
";
        let manifest = ingest_compose(yaml, "fallback").expect("should ingest");

        assert_eq!(manifest.name, "realname");
    }

    #[test]
    fn ingest_environment_map_and_list() {
        let yaml = "
services:
  a:
    image: busybox
    environment:
      FOO: bar
      NUM: 5
  b:
    image: busybox
    environment:
      - HELLO=world
      - FLAG=1
";
        let manifest = ingest_compose(yaml, "s").expect("should ingest");

        // services sort by key, so a precedes b
        let env_a = manifest.services[0].environment.as_ref().expect("a has env");
        assert_eq!(env_a.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(env_a.get("NUM").map(String::as_str), Some("5"));

        let env_b = manifest.services[1].environment.as_ref().expect("b has env");
        assert_eq!(env_b.get("HELLO").map(String::as_str), Some("world"));
        assert_eq!(env_b.get("FLAG").map(String::as_str), Some("1"));
    }

    #[test]
    fn ingest_command_and_depends_on_lists() {
        let yaml = "
services:
  api:
    image: myapi
    command: [\"serve\", \"--port\", \"80\"]
    depends_on:
      - db
  db:
    image: postgres:16
";
        let manifest = ingest_compose(yaml, "s").expect("should ingest");
        let api = manifest.services.iter().find(|s| s.name == "api").expect("api service");

        assert_eq!(
            api.command.as_deref(),
            Some(&["serve".to_owned(), "--port".to_owned(), "80".to_owned()][..])
        );

        let deps = api.depends_on.as_ref().expect("api depends on db");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].service, "db");
    }

    #[test]
    fn ingest_depends_on_map_keeps_condition() {
        let yaml = "
services:
  api:
    image: myapi
    depends_on:
      db:
        condition: service_healthy
  db:
    image: postgres:16
";
        let manifest = ingest_compose(yaml, "s").expect("should ingest");
        let api = manifest.services.iter().find(|s| s.name == "api").expect("api service");

        let deps = api.depends_on.as_ref().expect("api depends on db");
        assert_eq!(deps[0].service, "db");
        assert_eq!(deps[0].condition.as_deref(), Some("service_healthy"));
    }

    #[test]
    fn ingest_service_without_image_errors() {
        let yaml = "
services:
  web:
    build: .
";
        assert!(ingest_compose(yaml, "s").is_err());
    }

    #[test]
    fn ingest_empty_services_errors() {
        assert!(ingest_compose("services: {}", "s").is_err());
    }
}
