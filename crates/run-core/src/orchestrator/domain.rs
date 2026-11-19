//! Parse and validate the public domain a stack is served at.
//!
//! A stack runs at `localhost` by default. Passing a real domain (e.g.
//! `admin.example.com`) switches it to public HTTPS: Caddy obtains a trusted
//! certificate automatically via ACME, and every route URL uses that domain.

/// The host every stack is served at unless a custom domain is set.
pub const LOCALHOST: &str = "localhost";

/// Normalize a user-supplied domain, returning `None` for `localhost` (the
/// default, mkcert-backed mode) and the lowercased domain otherwise.
///
/// # Errors
///
/// Returns `Error::InvalidDomain` when the value is not a bare, publicly
/// resolvable hostname: a scheme, port, path, IP address, single-label name, or
/// malformed label is rejected so the generated Caddyfile and URLs stay valid.
pub fn normalize(raw: &str) -> crate::Result<Option<String>> {
    let domain = raw.trim().trim_end_matches('.').to_ascii_lowercase();

    let invalid = |reason: &str| crate::Error::InvalidDomain {
        domain: raw.trim().to_owned(),
        reason: reason.to_owned(),
    };

    if domain.is_empty() {
        return Err(invalid("the domain is empty"));
    }

    if domain == LOCALHOST {
        return Ok(None);
    }

    if domain.contains("://") {
        return Err(invalid(
            "pass a bare hostname without a scheme (e.g. admin.example.com)",
        ));
    }

    if domain.contains(['/', ':', '?', '#', '@']) {
        return Err(invalid("pass a bare hostname without a port or path"));
    }

    if domain.len() > 253 {
        return Err(invalid("the domain is longer than 253 characters"));
    }

    let labels: Vec<&str> = domain.split('.').collect();

    if labels.len() < 2 {
        return Err(invalid(
            "use a fully qualified domain (e.g. admin.example.com) so a certificate can be issued",
        ));
    }

    for label in &labels {
        let valid = !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');

        if !valid {
            return Err(invalid("each label must be 1-63 letters, digits, or inner hyphens"));
        }
    }

    // An all-numeric top-level label means an IP address, which ACME will not certify
    if labels.last().is_some_and(|tld| tld.chars().all(|c| c.is_ascii_digit())) {
        return Err(invalid("use a domain name, not an IP address"));
    }

    Ok(Some(domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localhost_means_no_custom_domain() {
        assert_eq!(normalize("localhost").expect("valid"), None);
        assert_eq!(normalize(" LocalHost ").expect("valid"), None);
    }

    #[test]
    fn a_domain_is_lowercased_and_trimmed() {
        assert_eq!(
            normalize(" Admin.Example.COM. ").expect("valid").as_deref(),
            Some("admin.example.com")
        );
    }

    #[test]
    fn a_hyphenated_subdomain_is_accepted() {
        assert_eq!(
            normalize("ops-console.my-company.co.uk").expect("valid").as_deref(),
            Some("ops-console.my-company.co.uk")
        );
    }

    #[test]
    fn rejects_a_scheme() {
        assert!(matches!(
            normalize("https://admin.example.com"),
            Err(crate::Error::InvalidDomain { .. })
        ));
    }

    #[test]
    fn rejects_a_port_or_path() {
        assert!(normalize("admin.example.com:8443").is_err());
        assert!(normalize("admin.example.com/console").is_err());
    }

    #[test]
    fn rejects_a_single_label_name() {
        assert!(normalize("intranet").is_err());
    }

    #[test]
    fn rejects_an_ip_address() {
        assert!(normalize("203.0.113.7").is_err());
    }

    #[test]
    fn rejects_malformed_labels() {
        assert!(normalize("").is_err());
        assert!(normalize("admin..example.com").is_err());
        assert!(normalize("-admin.example.com").is_err());
        assert!(normalize("admin_console.example.com").is_err());
    }
}
