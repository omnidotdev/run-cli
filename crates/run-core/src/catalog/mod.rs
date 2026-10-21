use std::path::Path;

use crate::config::{ProductManifest, ServiceDef};

mod ingest;

/// Resolve a product manifest by name.
pub trait Catalog {
    /// Look up a product manifest by name.
    ///
    /// # Errors
    ///
    /// Returns `Error::ProductNotFound` if the product is not in the catalog.
    fn get(&self, product: &str) -> crate::Result<ProductManifest>;

    /// List all available product names.
    fn list(&self) -> Vec<String>;
}

/// Catalog backed by TOML manifests embedded at compile time.
#[derive(Debug)]
pub struct EmbeddedCatalog;

/// Embedded Runa manifest TOML.
const RUNA_TOML: &str = include_str!("runa.toml");

/// Embedded Vortex manifest TOML.
const VORTEX_TOML: &str = include_str!("vortex.toml");

/// Embedded Uptime Kuma manifest TOML (a third-party, non-Omni app).
const UPTIME_KUMA_TOML: &str = include_str!("uptime-kuma.toml");

/// Embedded Pi-hole manifest TOML (a third-party, non-Omni app).
const PIHOLE_TOML: &str = include_str!("pihole.toml");

/// Embedded Backfeed manifest TOML.
const BACKFEED_TOML: &str = include_str!("backfeed.toml");

impl Catalog for EmbeddedCatalog {
    fn get(&self, product: &str) -> crate::Result<ProductManifest> {
        match product {
            "runa" => toml::from_str(RUNA_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            "vortex" => toml::from_str(VORTEX_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            "uptime-kuma" => toml::from_str(UPTIME_KUMA_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            "pihole" => toml::from_str(PIHOLE_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            "backfeed" => toml::from_str(BACKFEED_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            _ => Err(crate::Error::ProductNotFound(product.to_owned())),
        }
    }

    fn list(&self) -> Vec<String> {
        vec![
            "backfeed".to_owned(),
            "pihole".to_owned(),
            "runa".to_owned(),
            "uptime-kuma".to_owned(),
            "vortex".to_owned(),
        ]
    }
}

/// Return the default catalog with all embedded product manifests.
#[must_use]
pub fn default_catalog() -> impl Catalog {
    EmbeddedCatalog
}

/// Resolve a target into a product manifest.
///
/// A target that names an existing file on disk is loaded as a recipe, and a
/// target naming a directory loads `run.toml` from inside it. Anything else is
/// looked up by name in the embedded catalog. This is the entry point for the
/// resolver cascade: bring-your-own sources (compose, git, bare image) hang off
/// the same function as they land.
///
/// # Errors
///
/// Returns `Error::ReadFile` if a resolved path cannot be read,
/// `Error::ParseManifest` if it does not parse, or `Error::ProductNotFound`
/// when nothing matches.
pub fn resolve(target: &str, catalog: &dyn Catalog) -> crate::Result<ProductManifest> {
    resolve_path(target).unwrap_or_else(|| {
        catalog.get(target).or_else(|error| match error {
            // a name that is not a path and not in the catalog is a bare image
            crate::Error::ProductNotFound(_) => Ok(synthesize_image(target)),
            other => Err(other),
        })
    })
}

/// Resolve a filesystem target to a manifest, or `None` if it is not a path.
///
/// A file loads as a recipe, or is ingested when it is a compose file; a
/// directory loads a `run.{toml,yaml,yml}` recipe, else a compose file inside
/// it. A directory with neither resolves to `Some(Err(ProductNotFound))`.
fn resolve_path(target: &str) -> Option<crate::Result<ProductManifest>> {
    let path = Path::new(target);

    if path.is_file() {
        return Some(if is_compose_file(path) {
            load_compose_file(path)
        } else {
            load_recipe_file(path)
        });
    }

    if path.is_dir() {
        // A run recipe in the directory takes priority over a bare compose file
        for candidate in ["run.toml", "run.yaml", "run.yml"] {
            let recipe = path.join(candidate);

            if recipe.is_file() {
                return Some(load_recipe_file(&recipe));
            }
        }

        for candidate in [
            "compose.yaml",
            "compose.yml",
            "docker-compose.yaml",
            "docker-compose.yml",
        ] {
            let compose = path.join(candidate);

            if compose.is_file() {
                return Some(load_compose_file(&compose));
            }
        }

        return Some(Err(crate::Error::ProductNotFound(target.to_owned())));
    }

    None
}

/// Return the project name for a target.
///
/// A path target resolves to its manifest's name; a bare name is returned
/// verbatim, so `run down <name>` and `run logs <name>` work for both catalog
/// and bring-your-own apps without re-reading the catalog.
///
/// # Errors
///
/// Returns an error if a path target cannot be resolved to a manifest.
pub fn project_name(target: &str) -> crate::Result<String> {
    resolve_path(target).map_or_else(|| Ok(target.to_owned()), |result| result.map(|manifest| manifest.name))
}

/// Read and parse a recipe file from disk, picking the parser by extension.
///
/// `.yaml`/`.yml` files parse as YAML; anything else parses as TOML.
fn load_recipe_file(path: &Path) -> crate::Result<ProductManifest> {
    let contents = std::fs::read_to_string(path).map_err(|source| crate::Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;

    let is_yaml = matches!(path.extension().and_then(|ext| ext.to_str()), Some("yaml" | "yml"));

    if is_yaml {
        serde_yaml::from_str(&contents).map_err(|source| crate::Error::ParseYaml { source })
    } else {
        toml::from_str(&contents).map_err(|source| crate::Error::ParseManifest { source })
    }
}

/// Whether a path is a conventionally-named docker-compose file.
fn is_compose_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("compose.yaml" | "compose.yml" | "docker-compose.yaml" | "docker-compose.yml")
    )
}

/// Read a docker-compose file and ingest it into a manifest named after its directory.
fn load_compose_file(path: &Path) -> crate::Result<ProductManifest> {
    let contents = std::fs::read_to_string(path).map_err(|source| crate::Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;

    ingest::ingest_compose(&contents, &derive_stack_name(path))
}

/// Derive a stack name from the directory that contains a compose file.
fn derive_stack_name(compose_path: &Path) -> String {
    compose_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .and_then(|parent| parent.canonicalize().ok())
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map_or_else(|| "app".to_owned(), ToOwned::to_owned)
}

/// Synthesize a single-service recipe from a bare image reference.
///
/// This is the final step of the cascade, so `run up nginx` or
/// `run up ghcr.io/acme/app:1.2` just works. The stack is named for the image
/// basename; TLS, proxy, and port mapping are left to a recipe when the user
/// wants them.
fn synthesize_image(reference: &str) -> ProductManifest {
    let name = image_basename(reference);

    ProductManifest {
        name: name.clone(),
        display_name: None,
        description: None,
        services: vec![ServiceDef {
            name,
            image: reference.to_owned(),
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
        }],
        secrets: None,
        tls: None,
        proxy: None,
        hooks: None,
    }
}

/// Extract an image's basename, dropping any registry, tag, and digest.
fn image_basename(reference: &str) -> String {
    let without_digest = reference.split('@').next().unwrap_or(reference);
    let last_segment = without_digest.rsplit('/').next().unwrap_or(without_digest);
    let bare = last_segment.split(':').next().unwrap_or(last_segment);

    bare.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runa_manifest_parses() {
        let catalog = default_catalog();
        let manifest = catalog.get("runa").expect("runa manifest should parse");

        assert_eq!(manifest.name, "runa");
        assert_eq!(manifest.services.len(), 6);
    }

    #[test]
    fn pihole_manifest_parses() {
        let catalog = default_catalog();
        let manifest = catalog.get("pihole").expect("pihole manifest should parse");

        assert_eq!(manifest.name, "pihole");
        assert_eq!(manifest.services.len(), 1);
        assert!(manifest.secrets.is_some(), "pihole should auto-generate a web password");
    }

    #[test]
    fn unknown_product_returns_error() {
        let catalog = default_catalog();
        assert!(catalog.get("nonexistent").is_err());
    }

    const MINIMAL_MANIFEST: &str = r#"
name = "myapp"

[[services]]
name = "web"
image = "nginx:latest"
"#;

    const MINIMAL_MANIFEST_YAML: &str = "
name: myapp
services:
  - name: web
    image: nginx:latest
";

    #[test]
    fn resolve_local_yaml_recipe_returns_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("custom.yaml");
        std::fs::write(&path, MINIMAL_MANIFEST_YAML).expect("write manifest");

        let catalog = default_catalog();
        let manifest = resolve(path.to_str().expect("utf8 path"), &catalog).expect("yaml recipe should resolve");

        assert_eq!(manifest.name, "myapp");
        assert_eq!(manifest.services.len(), 1);
    }

    #[test]
    fn resolve_directory_with_run_yaml_returns_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("run.yaml"), MINIMAL_MANIFEST_YAML).expect("write manifest");

        let catalog = default_catalog();
        let manifest = resolve(dir.path().to_str().expect("utf8 path"), &catalog).expect("dir run.yaml should resolve");

        assert_eq!(manifest.name, "myapp");
    }

    #[test]
    fn resolve_embedded_name_returns_manifest() {
        let catalog = default_catalog();
        let manifest = resolve("runa", &catalog).expect("embedded name should resolve");

        assert_eq!(manifest.name, "runa");
    }

    #[test]
    fn resolve_local_recipe_file_returns_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("custom.toml");
        std::fs::write(&path, MINIMAL_MANIFEST).expect("write manifest");

        let catalog = default_catalog();
        let manifest = resolve(path.to_str().expect("utf8 path"), &catalog).expect("local file should resolve");

        assert_eq!(manifest.name, "myapp");
        assert_eq!(manifest.services.len(), 1);
    }

    #[test]
    fn resolve_directory_with_run_toml_returns_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("run.toml"), MINIMAL_MANIFEST).expect("write manifest");

        let catalog = default_catalog();
        let manifest = resolve(dir.path().to_str().expect("utf8 path"), &catalog).expect("directory should resolve");

        assert_eq!(manifest.name, "myapp");
    }

    #[test]
    fn resolve_bare_image_name_synthesizes_a_service() {
        let catalog = default_catalog();
        let manifest = resolve("nginx", &catalog).expect("bare image should synthesize");

        assert_eq!(manifest.services.len(), 1);
        assert_eq!(manifest.services[0].image, "nginx");
    }

    #[test]
    fn synthesized_stack_is_named_for_the_image_basename() {
        let catalog = default_catalog();
        let manifest = resolve("ghcr.io/acme/widget:1.2", &catalog).expect("bare image should synthesize");

        assert_eq!(manifest.name, "widget");
        assert_eq!(manifest.services[0].name, "widget");
        assert_eq!(manifest.services[0].image, "ghcr.io/acme/widget:1.2");
    }

    #[test]
    fn resolve_catalog_name_is_not_treated_as_an_image() {
        let catalog = default_catalog();
        let manifest = resolve("runa", &catalog).expect("catalog name resolves");

        // runa has a full stack, not a single synthesized service
        assert!(manifest.services.len() > 1);
    }

    #[test]
    fn project_name_passes_bare_names_through() {
        // a catalog name and an unknown name both return verbatim, so `run down
        // <name>` tears down both catalog and bring-your-own apps
        assert_eq!(project_name("runa").expect("bare name"), "runa");
        assert_eq!(project_name("some-byo-app").expect("bare name"), "some-byo-app");
    }

    #[test]
    fn project_name_resolves_a_recipe_file_to_its_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app.toml");
        std::fs::write(&path, MINIMAL_MANIFEST).expect("write recipe");

        assert_eq!(
            project_name(path.to_str().expect("utf8 path")).expect("resolve"),
            "myapp"
        );
    }

    #[test]
    fn project_name_resolves_a_compose_directory_to_its_dir_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("compose.yaml"), COMPOSE).expect("write compose");

        let expected = dir
            .path()
            .canonicalize()
            .expect("canonicalize")
            .file_name()
            .expect("file name")
            .to_str()
            .expect("utf8")
            .to_owned();

        assert_eq!(
            project_name(dir.path().to_str().expect("utf8 path")).expect("resolve"),
            expected
        );
    }

    #[test]
    fn resolve_directory_without_run_toml_returns_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");

        let catalog = default_catalog();
        assert!(resolve(dir.path().to_str().expect("utf8 path"), &catalog).is_err());
    }

    const COMPOSE: &str = "
services:
  web:
    image: nginx:latest
";

    #[test]
    fn resolve_compose_file_ingests_services() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("docker-compose.yml");
        std::fs::write(&path, COMPOSE).expect("write compose");

        let catalog = default_catalog();
        let manifest = resolve(path.to_str().expect("utf8 path"), &catalog).expect("compose file should resolve");

        assert_eq!(manifest.services.len(), 1);
        assert_eq!(manifest.services[0].image, "nginx:latest");
    }

    #[test]
    fn resolve_directory_with_compose_file_ingests() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("compose.yaml"), COMPOSE).expect("write compose");

        let catalog = default_catalog();
        let manifest = resolve(dir.path().to_str().expect("utf8 path"), &catalog).expect("dir compose should resolve");

        assert_eq!(manifest.services[0].image, "nginx:latest");
    }

    #[test]
    fn resolve_prefers_run_recipe_over_compose_in_same_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("run.toml"), MINIMAL_MANIFEST).expect("write recipe");
        std::fs::write(dir.path().join("compose.yaml"), COMPOSE).expect("write compose");

        let catalog = default_catalog();
        let manifest = resolve(dir.path().to_str().expect("utf8 path"), &catalog).expect("should resolve");

        // run.toml wins, so the stack is named from the recipe, not ingested from compose
        assert_eq!(manifest.name, "myapp");
    }

    #[test]
    fn list_contains_all_products() {
        let catalog = default_catalog();
        let products = catalog.list();

        assert_eq!(products, vec!["backfeed", "pihole", "runa", "uptime-kuma", "vortex"]);
    }

    #[test]
    fn uptime_kuma_manifest_parses() {
        let catalog = default_catalog();
        let manifest = catalog.get("uptime-kuma").expect("uptime-kuma manifest should parse");

        assert_eq!(manifest.name, "uptime-kuma");
        assert_eq!(manifest.services.len(), 1);
        // a standalone third-party app needs no Omni auth/proxy/secrets
        assert!(manifest.secrets.is_none());
        assert!(manifest.proxy.is_none());
    }

    #[test]
    fn vortex_manifest_parses() {
        let catalog = default_catalog();
        let manifest = catalog.get("vortex").expect("vortex manifest should parse");

        assert_eq!(manifest.name, "vortex");
        // db, cache, events, api, app, worker, caddy
        assert_eq!(manifest.services.len(), 7);
    }

    #[test]
    fn vortex_events_has_capabilities() {
        let catalog = default_catalog();
        let manifest = catalog.get("vortex").unwrap();

        let events = manifest
            .services
            .iter()
            .find(|s| s.name == "events")
            .expect("vortex should define an events service");

        assert_eq!(events.cap_add.as_deref(), Some(&["SYS_NICE".to_owned()][..]));
        assert_eq!(
            events.security_opt.as_deref(),
            Some(&["seccomp:unconfined".to_owned()][..])
        );

        let ulimits = events.ulimits.as_ref().expect("events should set ulimits");
        let memlock = ulimits.get("memlock").expect("events should set a memlock ulimit");
        assert_eq!(memlock.soft, -1);
        assert_eq!(memlock.hard, -1);
    }

    #[test]
    fn vortex_cache_has_command() {
        let catalog = default_catalog();
        let manifest = catalog.get("vortex").unwrap();

        let cache = manifest
            .services
            .iter()
            .find(|s| s.name == "cache")
            .expect("vortex should define a cache service");

        let command = cache.command.as_ref().expect("cache should override its command");
        assert_eq!(command.first().map(String::as_str), Some("valkey-server"));
    }

    #[test]
    fn vortex_secrets_defined() {
        let catalog = default_catalog();
        let manifest = catalog.get("vortex").unwrap();
        let secrets = manifest.secrets.expect("secrets should be defined");

        assert_eq!(secrets.auto.len(), 6);
    }

    #[test]
    fn backfeed_manifest_parses() {
        let catalog = default_catalog();
        let manifest = catalog.get("backfeed").expect("backfeed manifest should parse");

        assert_eq!(manifest.name, "backfeed");
        assert_eq!(manifest.services.len(), 4);
        let secrets = manifest.secrets.expect("secrets should be defined");
        assert_eq!(secrets.auto.len(), 2);
    }

    #[test]
    fn runa_secrets_defined() {
        let catalog = default_catalog();
        let manifest = catalog.get("runa").unwrap();
        let secrets = manifest.secrets.expect("secrets should be defined");

        assert_eq!(secrets.auto.len(), 4);
    }

    #[test]
    fn runa_proxy_routes_defined() {
        let catalog = default_catalog();
        let manifest = catalog.get("runa").unwrap();
        let proxy = manifest.proxy.expect("proxy should be defined");

        assert_eq!(proxy.provider, "caddy");
        assert_eq!(proxy.routes.len(), 3);
    }

    #[test]
    fn runa_hooks_defined() {
        let catalog = default_catalog();
        let manifest = catalog.get("runa").unwrap();
        let hooks = manifest.hooks.expect("hooks should be defined");

        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "seed-oauth-client");
    }
}
