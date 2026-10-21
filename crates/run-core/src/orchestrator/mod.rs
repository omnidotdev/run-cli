pub mod compose;
pub mod healthcheck;
pub mod hooks;
pub mod proxy;
pub mod secrets;
pub mod tls;

use std::fmt::Write as _;
use std::path::Path;

use crate::catalog::Catalog;
use crate::config::ProductManifest;
use crate::config::state::{ProjectDirs, ProjectState, ProjectStatus};
use crate::docker;

/// Full startup lifecycle for a product.
///
/// Mirrors the original `start.sh` flow: check docker, generate config,
/// bring up containers, wait for health, run hooks, and print a summary.
///
/// # Errors
///
/// Returns errors from any step in the lifecycle (docker checks, file I/O,
/// health timeout, hook failures).
#[allow(clippy::future_not_send)] // Catalog is only used synchronously before any awaits
pub async fn up(product: &str, catalog: &dyn Catalog) -> crate::Result<()> {
    // Resolve manifest before any awaits so the non-Send catalog ref is dropped
    let manifest = crate::catalog::resolve(product, catalog)?;
    // A path target resolves to a recipe whose own name keys the project dirs
    let product = manifest.name.as_str();

    docker::check_docker().await?;
    let dirs = ProjectDirs::new(product);
    dirs.ensure_dirs()?;

    let env_file = dirs.env_file();
    let first_run = !secrets::env_file_exists(&env_file);

    let tls_mode = if first_run {
        // Wipe stale volumes so old secrets don't conflict
        tracing::info!("first run -- generating secrets");
        docker::wipe_volumes(product).await?;

        let mode = tls::generate_certs(&dirs.certs_dir()).await?;

        // Generate secrets and write .env with PROTOCOL
        let secret_pairs = generate_secrets(&manifest);
        let protocol = tls::protocol(&mode);

        let mut env_entries: Vec<(&str, &str)> = secret_pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        env_entries.push(("PROTOCOL", protocol));

        secrets::write_env_file(&env_file, &env_entries)?;

        mode
    } else {
        // Recover TLS mode from existing .env
        let existing = secrets::load_env_file(&env_file)?;
        let protocol = existing.iter().find(|(k, _)| k == "PROTOCOL").map(|(_, v)| v.as_str());

        match protocol {
            Some("https") => {
                let certs_dir = dirs.certs_dir();

                tls::TlsMode::Https {
                    cert: certs_dir.join("localhost.pem"),
                    key: certs_dir.join("localhost-key.pem"),
                }
            }
            _ => tls::TlsMode::Http,
        }
    };

    // Generate and write Caddyfile
    let routes = manifest.proxy.as_ref().map_or(&[][..], |p| &p.routes);
    let caddyfile = proxy::generate_caddyfile(&tls_mode, routes);
    write_file(&dirs.caddyfile(), &caddyfile)?;

    // Generate and write compose file
    let compose_yaml = compose::generate_compose(&manifest, &tls_mode);
    write_file(&dirs.compose_file(), &compose_yaml)?;

    // Compute runtime env vars based on TLS mode
    let runtime_env = runtime_env_vars(&tls_mode);
    let runtime_refs: Vec<(&str, &str)> = runtime_env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    tracing::info!("starting {product}");
    docker::compose_up(dirs.root(), &env_file, &runtime_refs).await?;

    tracing::info!("waiting for services to become healthy");
    healthcheck::wait_for_healthy(dirs.root(), &env_file, 90).await?;

    // Run post-start hooks
    if let Some(hook_defs) = &manifest.hooks {
        tracing::info!("running post-start hooks");

        let all_env = build_hook_env(&env_file, &runtime_env)?;
        hooks::run_hooks(hook_defs, dirs.root(), &env_file, &all_env).await?;
    }

    // Save running state
    save_state(&dirs, product, ProjectStatus::Running)?;

    // Print summary
    let base_url = runtime_env_value(&runtime_env, "BASE_URL");
    let auth_url = runtime_env_value(&runtime_env, "AUTH_BASE_URL");
    let api_url = runtime_env_value(&runtime_env, "API_BASE_URL");
    let display = manifest.display_name.as_deref().unwrap_or(product);

    eprintln!();
    eprintln!("{display} is running at {base_url}");
    eprintln!("  Auth:  {auth_url}");
    eprintln!("  API:   {api_url}");
    eprintln!("  Stop:  run down {product}");
    eprintln!("  Logs:  run logs {product}");
    eprintln!("  Wipe:  run down {product} --fresh");

    Ok(())
}

/// Stop a running product, optionally wiping all data.
///
/// # Errors
///
/// Returns `Error::Docker` if compose down fails (non-fresh mode).
pub async fn down(product: &str, fresh: bool) -> crate::Result<()> {
    let dirs = ProjectDirs::new(product);
    let env_file = dirs.env_file();

    if fresh {
        tracing::info!("stopping {product} and wiping all data");

        // Best-effort compose down (env file may not exist)
        let _ = docker::compose_down(dirs.root(), &env_file).await;
        let _ = docker::wipe_volumes(product).await;

        // Remove generated files
        let _ = std::fs::remove_file(&env_file);
        let _ = std::fs::remove_dir_all(dirs.certs_dir());
        let _ = std::fs::remove_file(dirs.state_file());
        let _ = std::fs::remove_file(dirs.compose_file());
        let _ = std::fs::remove_file(dirs.caddyfile());

        tracing::info!("{product} wiped");
    } else {
        tracing::info!("stopping {product}");
        docker::compose_down(dirs.root(), &env_file).await?;
        save_state(&dirs, product, ProjectStatus::Stopped)?;

        tracing::info!("{product} stopped");
    }

    Ok(())
}

/// Dry-run: return a human-readable plan without making any changes.
///
/// # Errors
///
/// Returns an error if the target cannot be resolved to a manifest: not in the
/// catalog, or an unreadable or invalid local recipe.
pub fn plan(product: &str, catalog: &dyn Catalog) -> crate::Result<String> {
    let manifest = crate::catalog::resolve(product, catalog)?;
    let display = manifest.display_name.as_deref().unwrap_or(&manifest.name);

    let mut out = String::new();

    let _ = writeln!(out, "Product: {display} ({})", manifest.name);

    if let Some(desc) = &manifest.description {
        let _ = writeln!(out, "  {desc}");
    }

    out.push_str("\nServices:\n");

    for svc in &manifest.services {
        let _ = writeln!(out, "  - {} ({})", svc.name, svc.image);
    }

    if let Some(secrets_config) = &manifest.secrets {
        out.push_str("\nSecrets (auto-generated on first run):\n");

        for secret in &secrets_config.auto {
            let _ = writeln!(out, "  - {}", secret.name);
        }
    }

    let has_mkcert = tls::detect_mkcert();
    let _ = write!(
        out,
        "\nTLS: {} (mkcert {})\n",
        if has_mkcert { "HTTPS" } else { "HTTP" },
        if has_mkcert { "detected" } else { "not found" },
    );

    if let Some(proxy) = &manifest.proxy {
        out.push_str("\nProxy routes:\n");

        for route in &proxy.routes {
            let _ = writeln!(out, "  - :{} -> {}", route.listen_port, route.target);
        }
    }

    Ok(out)
}

/// Generate secret values from the manifest's secrets config.
fn generate_secrets(manifest: &ProductManifest) -> Vec<(String, String)> {
    let Some(secrets_config) = &manifest.secrets else {
        return Vec::new();
    };

    secrets_config
        .auto
        .iter()
        .map(|def| {
            let value = match def.method {
                crate::config::SecretMethod::Hex => secrets::generate_hex(def.length),
                crate::config::SecretMethod::Uuid => uuid_v4(),
            };

            (def.name.clone(), value)
        })
        .collect()
}

/// Generate a UUID v4 string.
fn uuid_v4() -> String {
    // Reuse hex generation and format as UUID
    let hex = secrets::generate_hex(16);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32],
    )
}

/// Compute runtime environment variables based on TLS mode.
fn runtime_env_vars(tls_mode: &tls::TlsMode) -> Vec<(String, String)> {
    match tls_mode {
        tls::TlsMode::Https { .. } => vec![
            ("BASE_URL".to_owned(), "https://localhost".to_owned()),
            ("AUTH_BASE_URL".to_owned(), "https://localhost:3001".to_owned()),
            ("API_BASE_URL".to_owned(), "https://localhost:4000".to_owned()),
            ("APP_PORT_MAP".to_owned(), "443:443".to_owned()),
        ],
        tls::TlsMode::Http => vec![
            ("BASE_URL".to_owned(), "http://localhost".to_owned()),
            ("AUTH_BASE_URL".to_owned(), "http://localhost:3001".to_owned()),
            ("API_BASE_URL".to_owned(), "http://localhost:4000".to_owned()),
            ("APP_PORT_MAP".to_owned(), "80:80".to_owned()),
        ],
    }
}

/// Merge .env file contents with runtime env vars for hook execution.
fn build_hook_env(env_file: &Path, runtime_env: &[(String, String)]) -> crate::Result<Vec<(String, String)>> {
    let mut env = secrets::load_env_file(env_file)?;

    for (key, value) in runtime_env {
        // Override or add runtime vars
        if let Some(entry) = env.iter_mut().find(|(k, _)| k == key) {
            entry.1.clone_from(value);
        } else {
            env.push((key.clone(), value.clone()));
        }
    }

    Ok(env)
}

/// Look up a value from the runtime env vec.
fn runtime_env_value(env: &[(String, String)], key: &str) -> String {
    env.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// Persist project state to disk.
fn save_state(dirs: &ProjectDirs, product: &str, status: ProjectStatus) -> crate::Result<()> {
    let now = timestamp_now();
    let existing = ProjectState::load(dirs)?;

    let state = ProjectState {
        product: product.to_owned(),
        status,
        created_at: existing.as_ref().map_or_else(|| now.clone(), |s| s.created_at.clone()),
        updated_at: now,
    };

    state.save(dirs)
}

/// Return a Unix timestamp as a string (avoids a chrono dependency).
fn timestamp_now() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    format!("{}", duration.as_secs())
}

/// Write content to a file, wrapping I/O errors.
fn write_file(path: &Path, content: &str) -> crate::Result<()> {
    std::fs::write(path, content).map_err(|source| crate::Error::WriteFile {
        path: path.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::default_catalog;

    const MINIMAL_MANIFEST: &str = r#"
name = "myapp"

[[services]]
name = "web"
image = "nginx:latest"
"#;

    #[test]
    fn plan_resolves_a_local_recipe_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("custom.toml");
        std::fs::write(&path, MINIMAL_MANIFEST).expect("write manifest");

        let catalog = default_catalog();
        let output = plan(path.to_str().expect("utf8 path"), &catalog).expect("plan should resolve the recipe");

        assert!(
            output.contains("myapp"),
            "plan output should name the recipe, got:\n{output}"
        );
        assert!(
            output.contains("web (nginx:latest)"),
            "plan output should list the service, got:\n{output}"
        );
    }
}
