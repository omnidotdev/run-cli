pub mod state;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use state::{ProjectDirs, ProjectState, ProjectStatus};

/// Top-level `run.toml` product manifest.
#[derive(Debug, Deserialize, Serialize)]
pub struct ProductManifest {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub services: Vec<ServiceDef>,
    pub secrets: Option<SecretsConfig>,
    pub tls: Option<TlsConfig>,
    pub proxy: Option<ProxyConfig>,
    pub hooks: Option<Vec<Hook>>,
}

/// Individual container service definition.
#[derive(Debug, Deserialize, Serialize)]
pub struct ServiceDef {
    pub name: String,
    pub image: String,
    pub ports: Option<Vec<String>>,
    pub environment: Option<BTreeMap<String, String>>,
    pub volumes: Option<Vec<String>>,
    pub depends_on: Option<Vec<DependsOn>>,
    pub extra_hosts: Option<Vec<String>>,
    pub healthcheck: Option<HealthcheckDef>,
}

/// Service dependency with optional readiness condition.
#[derive(Debug, Deserialize, Serialize)]
pub struct DependsOn {
    pub service: String,
    pub condition: Option<String>,
}

/// Container healthcheck configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct HealthcheckDef {
    pub test: Vec<String>,
    pub interval: Option<String>,
    pub timeout: Option<String>,
    pub retries: Option<u32>,
    pub start_period: Option<String>,
}

/// Auto-generated secrets configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct SecretsConfig {
    pub auto: Vec<SecretDef>,
}

/// Individual secret definition.
#[derive(Debug, Deserialize, Serialize)]
pub struct SecretDef {
    pub name: String,
    pub method: SecretMethod,
    pub length: usize,
}

/// Secret generation method.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretMethod {
    Hex,
    Uuid,
}

/// TLS configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct TlsConfig {
    pub provider: String,
}

/// Reverse proxy configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct ProxyConfig {
    pub provider: String,
    pub routes: Vec<ProxyRoute>,
}

/// Individual proxy route.
#[derive(Debug, Deserialize, Serialize)]
pub struct ProxyRoute {
    pub name: String,
    pub listen_port: u16,
    pub target: String,
    pub listen_port_http: Option<u16>,
}

/// Post-start lifecycle hook.
#[derive(Debug, Deserialize, Serialize)]
pub struct Hook {
    pub name: String,
    pub wait_for: Option<String>,
    #[serde(rename = "type")]
    pub hook_type: HookType,
    pub target: String,
    pub database: Option<String>,
    pub sql: Option<String>,
}

/// Hook execution type.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookType {
    Sql,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_manifest() {
        let toml_str = r#"
name = "minimal"

[[services]]
name = "web"
image = "nginx:latest"
"#;

        let manifest: ProductManifest = toml::from_str(toml_str).expect("minimal manifest should parse");

        assert_eq!(manifest.name, "minimal");
        assert_eq!(manifest.services.len(), 1);
        assert_eq!(manifest.services[0].name, "web");
        assert_eq!(manifest.services[0].image, "nginx:latest");
        assert!(manifest.display_name.is_none());
        assert!(manifest.secrets.is_none());
        assert!(manifest.proxy.is_none());
        assert!(manifest.hooks.is_none());
    }

    #[test]
    fn deserialize_full_manifest() {
        let toml_str = include_str!("../catalog/runa.toml");
        let manifest: ProductManifest = toml::from_str(toml_str).expect("runa manifest should parse via TOML");

        assert_eq!(manifest.name, "runa");
        assert_eq!(manifest.display_name.as_deref(), Some("Runa"));
        assert_eq!(manifest.services.len(), 6);

        // Verify secrets
        let secrets = manifest.secrets.expect("secrets should be defined");
        assert_eq!(secrets.auto.len(), 4);

        // Verify proxy
        let proxy = manifest.proxy.expect("proxy should be defined");
        assert_eq!(proxy.routes.len(), 3);

        // Verify hooks
        let hooks = manifest.hooks.expect("hooks should be defined");
        assert_eq!(hooks.len(), 1);

        // Verify TLS config
        let tls = manifest.tls.expect("tls should be defined");
        assert_eq!(tls.provider, "mkcert");
    }
}
