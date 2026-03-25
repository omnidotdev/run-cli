use std::path::{Path, PathBuf};

use tokio::process::Command;

/// TLS mode resolved at startup.
#[derive(Debug, Clone)]
pub enum TlsMode {
    Https { cert: PathBuf, key: PathBuf },
    Http,
}

/// Check whether `mkcert` is available on PATH.
#[must_use]
pub fn detect_mkcert() -> bool {
    which::which("mkcert").is_ok()
}

/// Generate localhost TLS certs via `mkcert`, falling back to HTTP if unavailable.
///
/// Never crashes on missing `mkcert` -- returns `TlsMode::Http` with a warning instead.
///
/// # Errors
///
/// Returns `Error::WriteFile` if the certs directory cannot be created.
pub async fn generate_certs(certs_dir: &Path) -> crate::Result<TlsMode> {
    if !detect_mkcert() {
        tracing::warn!("mkcert not found -- falling back to HTTP (install mkcert for trusted HTTPS)");

        return Ok(TlsMode::Http);
    }

    std::fs::create_dir_all(certs_dir).map_err(|source| crate::Error::WriteFile {
        path: certs_dir.to_owned(),
        source,
    })?;

    let cert = certs_dir.join("localhost.pem");
    let key = certs_dir.join("localhost-key.pem");

    // Install the local CA (best-effort, ignore errors)
    let _ = Command::new("mkcert")
        .arg("-install")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    let status = Command::new("mkcert")
        .args(["-cert-file"])
        .arg(&cert)
        .arg("-key-file")
        .arg(&key)
        .args(["localhost", "127.0.0.1", "::1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| crate::Error::Docker {
            message: format!("mkcert failed: {e}"),
        })?;

    if !status.success() {
        tracing::warn!("mkcert cert generation failed -- falling back to HTTP");

        return Ok(TlsMode::Http);
    }

    Ok(TlsMode::Https { cert, key })
}

/// Return the URL scheme for the resolved TLS mode.
#[must_use]
pub const fn protocol(mode: &TlsMode) -> &'static str {
    match mode {
        TlsMode::Https { .. } => "https",
        TlsMode::Http => "http",
    }
}
