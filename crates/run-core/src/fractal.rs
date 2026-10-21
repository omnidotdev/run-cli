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
}
