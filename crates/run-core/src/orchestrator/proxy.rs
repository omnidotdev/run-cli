use std::fmt::Write as _;

use crate::config::ProxyRoute;
use crate::orchestrator::tls::TlsMode;

/// Generate a Caddyfile from the resolved TLS mode and proxy routes.
#[must_use]
pub fn generate_caddyfile(tls_mode: &TlsMode, routes: &[ProxyRoute]) -> String {
    let mut out = String::new();

    match tls_mode {
        TlsMode::Https { .. } => {
            out.push_str("# Auto-generated -- trusted HTTPS via mkcert\n");

            for route in routes {
                let var = route.name.to_uppercase();

                let _ = write!(
                    out,
                    "\nhttps://localhost:{{${var}_PORT:{port}}} {{\n\
                     \ttls /certs/localhost.pem /certs/localhost-key.pem\n\
                     \treverse_proxy {target}\n\
                     }}\n",
                    port = route.listen_port,
                    target = route.target,
                );
            }
        }
        TlsMode::Acme { domain } => {
            out.push_str("# Auto-generated -- public HTTPS via ACME (certificates are obtained automatically)\n");

            // The app is served at the bare domain and every other route at its
            // own subdomain, all on 443 (with an automatic HTTP->HTTPS redirect on
            // 80), each with its own certificate
            for route in routes {
                let _ = write!(
                    out,
                    "\n{host} {{\n\
                     \treverse_proxy {target}\n\
                     }}\n",
                    host = public_host(route, routes, domain),
                    target = route.target,
                );
            }
        }
        TlsMode::Http => {
            out.push_str("# Auto-generated -- HTTP mode (install mkcert for trusted HTTPS)\n");

            for route in routes {
                let var = route.name.to_uppercase();
                let port = route.listen_port_http.unwrap_or(route.listen_port);

                let _ = write!(
                    out,
                    "\nhttp://localhost:{{${var}_PORT:{port}}} {{\n\
                     \treverse_proxy {target}\n\
                     }}\n",
                    target = route.target,
                );
            }
        }
    }

    out
}

/// The route a stack's primary URL points at: the one named `app`, else the
/// first route.
#[must_use]
pub fn primary_route(routes: &[ProxyRoute]) -> Option<&ProxyRoute> {
    routes
        .iter()
        .find(|route| route.name == "app")
        .or_else(|| routes.first())
}

/// The public hostname of a route under a custom domain: the primary route is
/// served at the bare domain, every other route at `<route>.<domain>`.
#[must_use]
pub fn public_host(route: &ProxyRoute, routes: &[ProxyRoute], domain: &str) -> String {
    if primary_route(routes).is_some_and(|primary| primary.name == route.name) {
        domain.to_owned()
    } else {
        format!("{}.{domain}", route.name)
    }
}

/// Every public hostname a stack answers on under a custom domain, primary
/// first: the DNS names that must point at the host.
#[must_use]
pub fn public_hosts(routes: &[ProxyRoute], domain: &str) -> Vec<String> {
    let mut hosts: Vec<String> = routes.iter().map(|route| public_host(route, routes, domain)).collect();

    // Keep the bare domain first so it reads as the main address
    hosts.sort_by_key(|host| host != domain);
    hosts.dedup();

    hosts
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::orchestrator::tls::TlsMode;

    fn sample_routes() -> Vec<ProxyRoute> {
        vec![
            ProxyRoute {
                name: "app".to_owned(),
                listen_port: 443,
                listen_port_http: Some(80),
                target: "app:3000".to_owned(),
            },
            ProxyRoute {
                name: "api".to_owned(),
                listen_port: 4000,
                listen_port_http: None,
                target: "api:4000".to_owned(),
            },
        ]
    }

    fn tls_https() -> TlsMode {
        TlsMode::Https {
            cert: PathBuf::from("/certs/localhost.pem"),
            key: PathBuf::from("/certs/localhost-key.pem"),
        }
    }

    #[test]
    fn caddyfile_https_uses_tls() {
        let output = generate_caddyfile(&tls_https(), &sample_routes());

        assert!(
            output.contains("tls /certs/localhost.pem"),
            "HTTPS mode should contain tls directive with cert path"
        );
        assert!(
            output.contains("https://localhost:"),
            "HTTPS mode should use https:// scheme"
        );
    }

    #[test]
    fn caddyfile_http_no_tls() {
        let output = generate_caddyfile(&TlsMode::Http, &sample_routes());

        assert!(!output.contains("tls"), "HTTP mode should not contain tls directive");
        assert!(
            output.contains("http://localhost:"),
            "HTTP mode should use http:// scheme"
        );
    }

    fn tls_acme() -> TlsMode {
        TlsMode::Acme {
            domain: "admin.example.com".to_owned(),
        }
    }

    #[test]
    fn caddyfile_acme_serves_app_at_the_bare_domain() {
        let output = generate_caddyfile(&tls_acme(), &sample_routes());

        assert!(
            output.contains("\nadmin.example.com {\n\treverse_proxy app:3000\n}"),
            "app route should be a bare-domain site block, got:\n{output}"
        );
    }

    #[test]
    fn caddyfile_acme_serves_other_routes_on_subdomains() {
        let output = generate_caddyfile(&tls_acme(), &sample_routes());

        assert!(
            output.contains("\napi.admin.example.com {\n\treverse_proxy api:4000\n}"),
            "api route should be its own subdomain site block, got:\n{output}"
        );
        assert!(
            !output.contains(":4000 {"),
            "no route should need a custom port, got:\n{output}"
        );
    }

    #[test]
    fn caddyfile_acme_uses_automatic_certificates() {
        let output = generate_caddyfile(&tls_acme(), &sample_routes());

        // No manual tls directive (Caddy manages ACME certs on its own), no
        // localhost addresses, and no explicit scheme forcing plain HTTP
        assert!(
            !output.contains("tls "),
            "ACME mode should not pin a cert, got:\n{output}"
        );
        assert!(
            !output.contains("localhost"),
            "ACME mode should not mention localhost, got:\n{output}"
        );
        assert!(
            !output.contains("http://"),
            "ACME mode should not force HTTP, got:\n{output}"
        );
    }

    #[test]
    fn public_hosts_lists_the_apex_then_subdomains() {
        assert_eq!(
            public_hosts(&sample_routes(), "admin.example.com"),
            ["admin.example.com", "api.admin.example.com"]
        );
    }

    #[test]
    fn primary_route_falls_back_to_the_first_route() {
        let routes = vec![
            ProxyRoute {
                name: "web".to_owned(),
                listen_port: 443,
                listen_port_http: None,
                target: "web:8080".to_owned(),
            },
            ProxyRoute {
                name: "admin".to_owned(),
                listen_port: 8443,
                listen_port_http: None,
                target: "web:9090".to_owned(),
            },
        ];

        assert_eq!(public_host(&routes[0], &routes, "example.org"), "example.org");
        assert_eq!(public_host(&routes[1], &routes, "example.org"), "admin.example.org");
    }

    #[test]
    fn caddyfile_http_uses_alternate_port() {
        let routes = sample_routes();
        let output = generate_caddyfile(&TlsMode::Http, &routes);

        // The app route has listen_port=443 and listen_port_http=80,
        // so in HTTP mode it should use port 80
        assert!(
            output.contains("APP_PORT:80"),
            "HTTP mode should use listen_port_http (80) for app route, got:\n{output}"
        );
    }
}
