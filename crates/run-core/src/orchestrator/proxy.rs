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
