use crate::config::ProductManifest;

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

impl Catalog for EmbeddedCatalog {
    fn get(&self, product: &str) -> crate::Result<ProductManifest> {
        match product {
            "runa" => toml::from_str(RUNA_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            "vortex" => toml::from_str(VORTEX_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            "uptime-kuma" => toml::from_str(UPTIME_KUMA_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            _ => Err(crate::Error::ProductNotFound(product.to_owned())),
        }
    }

    fn list(&self) -> Vec<String> {
        vec!["runa".to_owned(), "uptime-kuma".to_owned(), "vortex".to_owned()]
    }
}

/// Return the default catalog with all embedded product manifests.
#[must_use]
pub fn default_catalog() -> impl Catalog {
    EmbeddedCatalog
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
    fn unknown_product_returns_error() {
        let catalog = default_catalog();
        assert!(catalog.get("nonexistent").is_err());
    }

    #[test]
    fn list_contains_all_products() {
        let catalog = default_catalog();
        let products = catalog.list();

        assert_eq!(products, vec!["runa", "uptime-kuma", "vortex"]);
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
