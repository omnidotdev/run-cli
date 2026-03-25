use std::fmt::Write as _;

use crate::config::{ProductManifest, ServiceDef};
use crate::orchestrator::tls::TlsMode;

/// Generate a Docker Compose YAML from a product manifest and TLS mode.
///
/// Builds the YAML via string formatting to avoid a `serde_yaml` dependency.
/// The output matches the structure expected by `docker compose`.
#[must_use]
pub fn generate_compose(manifest: &ProductManifest, tls_mode: &TlsMode) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "name: {}", manifest.name);
    out.push('\n');
    out.push_str("services:\n");

    for svc in &manifest.services {
        if svc.name == "caddy" {
            write_caddy_service(&mut out, svc, manifest, tls_mode);
        } else {
            write_service(&mut out, svc);
        }
    }

    // Collect all named volumes
    let volumes = collect_named_volumes(manifest);

    if !volumes.is_empty() {
        out.push_str("\nvolumes:\n");

        for vol in &volumes {
            let _ = writeln!(out, "  {vol}:");
        }
    }

    out
}

/// Write a standard (non-caddy) service block.
fn write_service(out: &mut String, svc: &ServiceDef) {
    let _ = writeln!(out, "  {}:", svc.name);
    let _ = writeln!(out, "    image: {}", svc.image);

    write_environment(out, svc);
    write_volumes(out, svc);
    write_extra_hosts(out, svc);
    write_depends_on(out, svc);
    write_healthcheck(out, svc);

    out.push('\n');
}

/// Write the caddy service with ports and volumes derived from proxy routes.
fn write_caddy_service(out: &mut String, svc: &ServiceDef, manifest: &ProductManifest, tls_mode: &TlsMode) {
    let _ = writeln!(out, "  {}:", svc.name);
    let _ = writeln!(out, "    image: {}", svc.image);

    // Ports derived from proxy routes + TLS mode
    if let Some(proxy) = &manifest.proxy {
        out.push_str("    ports:\n");

        for route in &proxy.routes {
            let var = route.name.to_uppercase();

            if route.name == "app" {
                let default = match tls_mode {
                    TlsMode::Https { .. } => "443:443",
                    TlsMode::Http => "80:80",
                };

                let _ = writeln!(out, "      - \"${{APP_PORT_MAP:-{default}}}\"");
            } else {
                let _ = writeln!(
                    out,
                    "      - \"${{{var}_PORT:-{port}}}:{port}\"",
                    port = route.listen_port,
                );
            }
        }
    }

    // Caddy volumes: Caddyfile, certs, data, config
    out.push_str("    volumes:\n");
    out.push_str("      - ./Caddyfile:/etc/caddy/Caddyfile:ro\n");
    out.push_str("      - ./.certs:/certs:ro\n");
    out.push_str("      - caddy_data:/data\n");
    out.push_str("      - caddy_config:/config\n");

    write_depends_on(out, svc);

    out.push('\n');
}

fn write_environment(out: &mut String, svc: &ServiceDef) {
    let Some(env) = &svc.environment else {
        return;
    };

    if env.is_empty() {
        return;
    }

    out.push_str("    environment:\n");

    for (key, value) in env {
        let _ = writeln!(out, "      {key}: {value}");
    }
}

fn write_volumes(out: &mut String, svc: &ServiceDef) {
    let Some(volumes) = &svc.volumes else {
        return;
    };

    if volumes.is_empty() {
        return;
    }

    out.push_str("    volumes:\n");

    for vol in volumes {
        let _ = writeln!(out, "      - {vol}");
    }
}

fn write_extra_hosts(out: &mut String, svc: &ServiceDef) {
    let Some(hosts) = &svc.extra_hosts else {
        return;
    };

    if hosts.is_empty() {
        return;
    }

    out.push_str("    extra_hosts:\n");

    for host in hosts {
        let _ = writeln!(out, "      - \"{host}\"");
    }
}

fn write_depends_on(out: &mut String, svc: &ServiceDef) {
    let Some(deps) = &svc.depends_on else {
        return;
    };

    if deps.is_empty() {
        return;
    }

    out.push_str("    depends_on:\n");

    let has_conditions = deps.iter().any(|d| d.condition.is_some());

    if has_conditions {
        for dep in deps {
            let _ = writeln!(out, "      {}:", dep.service);

            let cond = dep.condition.as_deref().unwrap_or("service_started");
            let _ = writeln!(out, "        condition: {cond}");
        }
    } else {
        for dep in deps {
            let _ = writeln!(out, "      - {}", dep.service);
        }
    }
}

fn write_healthcheck(out: &mut String, svc: &ServiceDef) {
    let Some(hc) = &svc.healthcheck else {
        return;
    };

    out.push_str("    healthcheck:\n");

    // Format test as a JSON array
    let test_items: Vec<String> = hc.test.iter().map(|t| format!("\"{t}\"")).collect();
    let _ = writeln!(out, "      test: [{}]", test_items.join(", "));

    if let Some(interval) = &hc.interval {
        let _ = writeln!(out, "      interval: {interval}");
    }

    if let Some(timeout) = &hc.timeout {
        let _ = writeln!(out, "      timeout: {timeout}");
    }

    if let Some(retries) = hc.retries {
        let _ = writeln!(out, "      retries: {retries}");
    }

    if let Some(start_period) = &hc.start_period {
        let _ = writeln!(out, "      start_period: {start_period}");
    }
}

/// Extract named volume names from all services plus caddy's implicit volumes.
fn collect_named_volumes(manifest: &ProductManifest) -> Vec<String> {
    let mut volumes = Vec::new();

    for svc in &manifest.services {
        if let Some(vols) = &svc.volumes {
            for vol in vols {
                // Named volumes use "name:path" syntax (no leading . or /)
                if let Some(name) = vol.split(':').next() {
                    if !name.starts_with('.') && !name.starts_with('/') && !volumes.contains(&name.to_owned()) {
                        volumes.push(name.to_owned());
                    }
                }
            }
        }
    }

    // Caddy always contributes these named volumes
    for vol in ["caddy_data", "caddy_config"] {
        let vol = vol.to_owned();

        if !volumes.contains(&vol) {
            volumes.push(vol);
        }
    }

    volumes
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::catalog::{Catalog, default_catalog};
    use crate::orchestrator::tls::TlsMode;

    /// Parse the embedded Runa manifest for reuse across tests
    fn runa_manifest() -> ProductManifest {
        default_catalog().get("runa").expect("runa manifest should parse")
    }

    fn tls_https() -> TlsMode {
        TlsMode::Https {
            cert: PathBuf::from("/certs/localhost.pem"),
            key: PathBuf::from("/certs/localhost-key.pem"),
        }
    }

    /// Extract the block for a given service from compose output.
    /// Splits on double-newline-separated service blocks.
    fn service_block(output: &str, name: &str) -> String {
        let header = format!("  {name}:\n");
        let start = output
            .find(&header)
            .unwrap_or_else(|| panic!("service {name} not found in output"));
        let rest = &output[start..];
        // A service block ends at the next blank line or end of string
        let end = rest.find("\n\n").map_or(rest.len(), |i| i);

        rest[..end].to_owned()
    }

    #[test]
    fn runa_compose_has_extra_hosts() {
        let manifest = runa_manifest();
        let output = generate_compose(&manifest, &tls_https());

        let api_block = service_block(&output, "api");
        assert!(
            api_block.contains("localhost:host-gateway"),
            "api service should have extra_hosts"
        );

        let app_block = service_block(&output, "app");
        assert!(
            app_block.contains("localhost:host-gateway"),
            "app service should have extra_hosts"
        );
    }

    #[test]
    fn runa_compose_has_node_tls_reject() {
        let manifest = runa_manifest();
        let output = generate_compose(&manifest, &tls_https());

        let api_block = service_block(&output, "api");
        assert!(
            api_block.contains("NODE_TLS_REJECT_UNAUTHORIZED: 0"),
            "api should have NODE_TLS_REJECT_UNAUTHORIZED"
        );

        let app_block = service_block(&output, "app");
        assert!(
            app_block.contains("NODE_TLS_REJECT_UNAUTHORIZED: 0"),
            "app should have NODE_TLS_REJECT_UNAUTHORIZED"
        );
    }

    #[test]
    fn runa_compose_has_vite_overrides() {
        let manifest = runa_manifest();
        let output = generate_compose(&manifest, &tls_https());

        let app_block = service_block(&output, "app");

        assert!(
            app_block.contains("VITE_BILLING_BASE_URL"),
            "app should have VITE_BILLING_BASE_URL"
        );
        assert!(
            app_block.contains("VITE_CONSOLE_URL"),
            "app should have VITE_CONSOLE_URL"
        );
    }

    #[test]
    fn runa_compose_caddy_https_ports() {
        let manifest = runa_manifest();
        let output = generate_compose(&manifest, &tls_https());

        assert!(
            output.contains("443:443"),
            "HTTPS mode caddy should have 443:443 port mapping"
        );
    }

    #[test]
    fn runa_compose_caddy_http_ports() {
        let manifest = runa_manifest();
        let output = generate_compose(&manifest, &TlsMode::Http);

        assert!(
            output.contains("80:80"),
            "HTTP mode caddy should have 80:80 port mapping"
        );
    }

    #[test]
    fn runa_compose_has_named_volumes() {
        let manifest = runa_manifest();
        let output = generate_compose(&manifest, &tls_https());

        for vol in ["db_data", "auth_db_data", "caddy_data", "caddy_config"] {
            assert!(
                output.contains(&format!("  {vol}:")),
                "volumes section should include {vol}"
            );
        }
    }
}
