//! Map a run recipe into a Fractal `fractal.yaml` manifest.
//!
//! This is the bridge into Fractal cloud: `run deploy` emits a `fractal.yaml`
//! the user reviews and imports at fractal.omni.dev. Local-only concerns (the
//! Caddy proxy, mkcert TLS, hooks, Linux capabilities) have no cloud equivalent,
//! so they are dropped with a warning; Fractal provides its own ingress and TLS.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::{ProductManifest, ServiceDef};

/// Default persistent-volume size, since run recipes do not declare sizes.
const DEFAULT_VOLUME_SIZE: &str = "5Gi";

#[derive(Serialize)]
struct FractalManifest {
    version: String,
    services: BTreeMap<String, FractalService>,
}

#[derive(Serialize)]
struct FractalService {
    source: FractalSource,
    #[serde(rename = "serviceType")]
    service_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expose: Option<FractalExpose>,
    deploy: FractalDeploy,
}

#[derive(Serialize)]
struct FractalSource {
    image: FractalImage,
}

#[derive(Serialize)]
struct FractalImage {
    repository: String,
    tag: String,
}

#[derive(Serialize)]
struct FractalExpose {
    port: u16,
    tls: bool,
}

#[derive(Serialize)]
struct FractalDeploy {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    env: Vec<FractalEnv>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    volumes: Vec<FractalVolume>,
}

#[derive(Serialize)]
struct FractalEnv {
    name: String,
    value: String,
}

#[derive(Serialize)]
struct FractalVolume {
    #[serde(rename = "mountPath")]
    mount_path: String,
    size: String,
}

/// Map a run recipe into a `fractal.yaml` string plus warnings for anything that
/// does not translate to Fractal.
///
/// # Errors
///
/// Returns `Error::ParseYaml` if the manifest cannot be serialized.
pub fn to_fractal_yaml(manifest: &ProductManifest) -> crate::Result<(String, Vec<String>)> {
    let mut warnings = Vec::new();
    let mut services = BTreeMap::new();

    for svc in &manifest.services {
        let (repository, tag, had_digest) = split_image(&svc.image);
        if had_digest {
            warnings.push(format!(
                "service `{}`: image digest dropped; Fractal deploys `{repository}:{tag}`",
                svc.name
            ));
        }

        let port = exposed_port(manifest, svc);
        let service_type = if port.is_some() { "web" } else { "worker" };

        let env = svc.environment.as_ref().map_or_else(Vec::new, |env| {
            env.iter()
                .map(|(name, value)| FractalEnv {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect()
        });

        let volumes: Vec<FractalVolume> = svc.volumes.as_ref().map_or_else(Vec::new, |vols| {
            vols.iter()
                .filter_map(|vol| mount_path(vol))
                .map(|mount_path| FractalVolume {
                    mount_path,
                    size: DEFAULT_VOLUME_SIZE.to_owned(),
                })
                .collect()
        });
        if !volumes.is_empty() {
            warnings.push(format!(
                "service `{}`: volume sizes defaulted to {DEFAULT_VOLUME_SIZE}; adjust in Fractal",
                svc.name
            ));
        }

        if svc.healthcheck.is_some() {
            warnings.push(format!(
                "service `{}`: healthcheck dropped (run uses command probes; Fractal uses an HTTP path)",
                svc.name
            ));
        }
        if svc.cap_add.is_some() || svc.security_opt.is_some() || svc.ulimits.is_some() {
            warnings.push(format!(
                "service `{}`: cap_add/security_opt/ulimits dropped (not available on Fractal)",
                svc.name
            ));
        }

        services.insert(
            svc.name.clone(),
            FractalService {
                source: FractalSource {
                    image: FractalImage { repository, tag },
                },
                service_type: service_type.to_owned(),
                expose: port.map(|port| FractalExpose { port, tls: true }),
                deploy: FractalDeploy {
                    env,
                    command: svc.command.clone(),
                    volumes,
                },
            },
        );
    }

    if manifest.proxy.is_some() {
        warnings.push("proxy routes dropped; Fractal provides ingress and TLS via `expose`".to_owned());
    }
    if manifest.hooks.is_some() {
        warnings.push("hooks dropped; port any setup to a Fractal deploy release command".to_owned());
    }
    if manifest.secrets.is_some() {
        warnings.push("auto-generated secrets are emitted as plain env; set them as Fractal secrets".to_owned());
    }
    if manifest.services.iter().any(|svc| svc.depends_on.is_some()) {
        warnings.push("depends_on dropped; Fractal has no ordering field (create datastores first)".to_owned());
    }

    let document = FractalManifest {
        version: "1".to_owned(),
        services,
    };
    let yaml = serde_yaml::to_string(&document).map_err(|source| crate::Error::ParseYaml { source })?;

    Ok((yaml, warnings))
}

/// A structured plan a cloud target (Fractal) executes to deploy a recipe.
///
/// `run` resolves these locally; the `fractal.yaml` alone drops them as
/// local-only, so this exposes them (secrets to generate, URL env bindings,
/// hooks, the local proxy to skip) for the cloud to reproduce instead of
/// forking run's orchestration.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploySpec {
    /// Auto-generated secrets the target must create and inject as env.
    pub secrets: Vec<DeploySecret>,
    /// Which service owns a public URL, and the env vars to set to it.
    pub url_bindings: Vec<UrlBinding>,
    /// Post-deploy hooks (e.g. seeding an OAuth client) to run once healthy.
    pub hooks: Vec<DeployHook>,
    /// Services that are run's local-only reverse proxy, not to be deployed.
    pub skip_services: Vec<String>,
}

/// An auto-generated secret: a name, a generation method, and a byte length.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploySecret {
    pub name: String,
    /// Generation method, `"hex"` or `"uuid"`.
    pub method: String,
    pub length: usize,
}

/// A service that must be assigned a public URL, and the env vars bound to it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlBinding {
    pub service: String,
    pub env_vars: Vec<String>,
}

/// A post-deploy hook to run once the stack's services are healthy.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployHook {
    pub name: String,
    #[serde(rename = "type")]
    pub hook_type: String,
    pub target: String,
    pub database: Option<String>,
    pub sql: Option<String>,
    pub wait_for: Option<String>,
}

/// Build the cloud deploy spec from a recipe.
///
/// Mirrors how `run up` resolves secrets (`[secrets.auto]`), proxy-route URLs
/// (`runtime_env_vars`: route `app` -> `BASE_URL`, every route ->
/// `<NAME>_BASE_URL`, keyed by the service the route targets), hooks, and the
/// local proxy service.
#[must_use]
pub fn deploy_spec(manifest: &ProductManifest) -> DeploySpec {
    use crate::config::{HookType, SecretMethod};

    let secrets = manifest.secrets.as_ref().map_or_else(Vec::new, |config| {
        config
            .auto
            .iter()
            .map(|secret| DeploySecret {
                name: secret.name.clone(),
                method: match secret.method {
                    SecretMethod::Hex => "hex".to_owned(),
                    SecretMethod::Uuid => "uuid".to_owned(),
                },
                length: secret.length,
            })
            .collect()
    });

    let mut url_bindings: Vec<UrlBinding> = Vec::new();
    if let Some(proxy) = &manifest.proxy {
        for route in &proxy.routes {
            // The route target is `service:port`; the service owns the URL.
            let service = route.target.split(':').next().unwrap_or(&route.target).to_owned();

            let mut env_vars = Vec::new();
            if route.name == "app" {
                env_vars.push("BASE_URL".to_owned());
            }
            env_vars.push(format!("{}_BASE_URL", route.name.to_uppercase()));

            if let Some(existing) = url_bindings.iter_mut().find(|b| b.service == service) {
                existing.env_vars.extend(env_vars);
            } else {
                url_bindings.push(UrlBinding { service, env_vars });
            }
        }
    }

    let hooks = manifest.hooks.as_ref().map_or_else(Vec::new, |hooks| {
        hooks
            .iter()
            .map(|hook| DeployHook {
                name: hook.name.clone(),
                hook_type: match hook.hook_type {
                    HookType::Sql => "sql".to_owned(),
                },
                target: hook.target.clone(),
                database: hook.database.clone(),
                sql: hook.sql.clone(),
                wait_for: hook.wait_for.clone(),
            })
            .collect()
    });

    // run's local reverse proxy (e.g. the `caddy` service) is replaced by the
    // cloud's own ingress, so it is not deployed.
    let skip_services = manifest.proxy.as_ref().map_or_else(Vec::new, |proxy| {
        manifest
            .services
            .iter()
            .filter(|svc| {
                svc.name == proxy.provider || svc.image.split(':').next().unwrap_or(&svc.image) == proxy.provider
            })
            .map(|svc| svc.name.clone())
            .collect()
    });

    DeploySpec {
        secrets,
        url_bindings,
        hooks,
        skip_services,
    }
}

/// Split an image reference into `(repository, tag, had_digest)`.
fn split_image(image: &str) -> (String, String, bool) {
    let (without_digest, had_digest) = image.split_once('@').map_or((image, false), |(repo, _)| (repo, true));

    let (repository, tag) = match without_digest.rsplit_once(':') {
        Some((repo, tag)) if !tag.contains('/') => (repo.to_owned(), tag.to_owned()),
        _ => (without_digest.to_owned(), "latest".to_owned()),
    };

    (repository, tag, had_digest)
}

/// The container port from a compose port spec (`"8080:80"`, `"53:53/udp"`, `"3000"`).
fn container_port(spec: &str) -> Option<u16> {
    spec.split('/')
        .next()
        .unwrap_or(spec)
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
}

/// The port a service is reachable on: a published container port, else a proxy route target.
fn exposed_port(manifest: &ProductManifest, svc: &ServiceDef) -> Option<u16> {
    if let Some(port) = svc
        .ports
        .as_ref()
        .and_then(|ports| ports.iter().find_map(|spec| container_port(spec)))
    {
        return Some(port);
    }

    manifest.proxy.as_ref().and_then(|proxy| {
        proxy.routes.iter().find_map(|route| {
            let (name, port) = route.target.rsplit_once(':')?;

            if name == svc.name { port.parse().ok() } else { None }
        })
    })
}

/// The mount path from a compose volume spec (`"db_data:/var/lib/postgresql"`).
fn mount_path(volume: &str) -> Option<String> {
    volume.split(':').nth(1).map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProductManifest;

    fn manifest(toml_str: &str) -> ProductManifest {
        toml::from_str(toml_str).expect("manifest should parse")
    }

    #[test]
    fn maps_a_simple_service() {
        let m = manifest(
            r#"
name = "svc"

[[services]]
name = "web"
image = "nginx:1.27"
ports = ["8080:80"]
"#,
        );

        let (yaml, warnings) = to_fractal_yaml(&m).expect("map");

        assert!(yaml.contains("web:"));
        assert!(yaml.contains("repository: nginx"));
        assert!(yaml.contains("1.27"));
        assert!(yaml.contains("serviceType: web"));
        assert!(yaml.contains("port: 80"));
        assert!(warnings.is_empty(), "simple service should not warn, got {warnings:?}");
    }

    #[test]
    fn worker_when_not_exposed() {
        let m = manifest(
            r#"
name = "svc"

[[services]]
name = "job"
image = "busybox:latest"
"#,
        );

        let (yaml, _) = to_fractal_yaml(&m).expect("map");

        assert!(yaml.contains("serviceType: worker"));
        assert!(!yaml.contains("expose"));
    }

    #[test]
    fn exposes_via_proxy_route_target() {
        let m = manifest(
            r#"
name = "app"

[[services]]
name = "web"
image = "myapp:1"

[proxy]
provider = "caddy"
routes = [{ name = "web", listen_port = 443, target = "web:3000" }]
"#,
        );

        let (yaml, warnings) = to_fractal_yaml(&m).expect("map");

        assert!(yaml.contains("port: 3000"));
        assert!(yaml.contains("serviceType: web"));
        assert!(warnings.iter().any(|w| w.contains("proxy")));
    }

    #[test]
    fn warns_on_lossy_bits() {
        let m = manifest(
            r#"
name = "app"

[[services]]
name = "db"
image = "api.manifold.omni.dev/omni/thing@sha256:abc123"
volumes = ["db_data:/var/lib/data"]

[services.healthcheck]
test = ["CMD", "true"]

[secrets]
auto = [{ name = "KEY", method = "hex", length = 16 }]
"#,
        );

        let (yaml, warnings) = to_fractal_yaml(&m).expect("map");

        assert!(yaml.contains("mountPath: /var/lib/data"));
        assert!(yaml.contains("size: 5Gi"));
        assert!(warnings.iter().any(|w| w.contains("digest")));
        assert!(warnings.iter().any(|w| w.contains("volume sizes")));
        assert!(warnings.iter().any(|w| w.contains("healthcheck")));
        assert!(warnings.iter().any(|w| w.contains("secrets")));
    }

    #[test]
    fn splits_image_forms() {
        assert_eq!(
            split_image("nginx:1.27"),
            ("nginx".to_owned(), "1.27".to_owned(), false)
        );
        assert_eq!(split_image("nginx"), ("nginx".to_owned(), "latest".to_owned(), false));
        assert_eq!(
            split_image("ghcr.io/acme/app:2"),
            ("ghcr.io/acme/app".to_owned(), "2".to_owned(), false)
        );
        let (repo, tag, digest) = split_image("repo@sha256:abc");
        assert_eq!((repo.as_str(), tag.as_str(), digest), ("repo", "latest", true));
    }

    #[test]
    fn deploy_spec_derives_from_runa() {
        use crate::catalog::{Catalog, default_catalog};

        let m = default_catalog().get("runa").expect("runa recipe");
        let spec = deploy_spec(&m);

        // Secrets mirror [secrets.auto].
        let secret_names: Vec<&str> = spec.secrets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            secret_names,
            ["DB_PASSWORD", "AUTH_SECRET", "AUTH_CLIENT_ID", "AUTH_CLIENT_SECRET"]
        );

        // URL bindings mirror runtime_env_vars: app -> BASE_URL + APP_BASE_URL,
        // keyed by the service the proxy route targets.
        let app = spec
            .url_bindings
            .iter()
            .find(|b| b.service == "app")
            .expect("app binding");
        assert_eq!(app.env_vars, ["BASE_URL", "APP_BASE_URL"]);
        let auth = spec
            .url_bindings
            .iter()
            .find(|b| b.service == "auth")
            .expect("auth binding");
        assert_eq!(auth.env_vars, ["AUTH_BASE_URL"]);
        let api = spec
            .url_bindings
            .iter()
            .find(|b| b.service == "api")
            .expect("api binding");
        assert_eq!(api.env_vars, ["API_BASE_URL"]);

        // The local caddy proxy is skipped; the OAuth seed hook is carried.
        assert_eq!(spec.skip_services, ["caddy"]);
        assert_eq!(spec.hooks.len(), 1);
        assert_eq!(spec.hooks[0].name, "seed-oauth-client");
        assert_eq!(spec.hooks[0].hook_type, "sql");
    }

    #[test]
    fn deploy_spec_empty_for_plain_stack() {
        let m = manifest(
            r#"
name = "svc"

[[services]]
name = "web"
image = "nginx:1.27"
ports = ["8080:80"]
"#,
        );

        let spec = deploy_spec(&m);
        assert!(spec.secrets.is_empty());
        assert!(spec.url_bindings.is_empty());
        assert!(spec.hooks.is_empty());
        assert!(spec.skip_services.is_empty());
    }
}
