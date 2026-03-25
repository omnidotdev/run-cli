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

impl Catalog for EmbeddedCatalog {
    fn get(&self, product: &str) -> crate::Result<ProductManifest> {
        match product {
            "runa" => toml::from_str(RUNA_TOML).map_err(|source| crate::Error::ParseManifest { source }),
            _ => Err(crate::Error::ProductNotFound(product.to_owned())),
        }
    }

    fn list(&self) -> Vec<String> {
        vec!["runa".to_owned()]
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
    fn list_contains_runa() {
        let catalog = default_catalog();
        let products = catalog.list();

        assert_eq!(products, vec!["runa"]);
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
