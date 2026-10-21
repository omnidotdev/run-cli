pub mod backup;
pub mod compose;
pub mod healthcheck;
pub mod hooks;
mod inputs;
pub mod proxy;
pub mod secrets;
pub mod tls;
mod tokens;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{IsTerminal, Write as _};
use std::path::Path;

use crate::catalog::Catalog;
use crate::config::state::{ProjectDirs, ProjectState, ProjectStatus};
use crate::config::{ProductManifest, ProxyRoute};
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
    let mut manifest = crate::catalog::resolve(product, catalog)?;

    // A path target resolves to a recipe whose own name keys the project dirs
    let dirs = ProjectDirs::new(&manifest.name);

    // Resolve typed inputs (env overrides, then defaults), then expand magic
    // tokens before secrets are generated, so `${secret.*}` is registered for
    // generation and `${app.*}` / `${input.*}` are inlined
    let input_values = resolve_inputs(&manifest)?;
    let app_ctx = tokens::AppContext {
        name: manifest.name.clone(),
        fqdn: "localhost".to_owned(),
        data_dir: dirs.root().display().to_string(),
        inputs: input_values,
    };
    tokens::expand(&mut manifest, &app_ctx);

    docker::check_docker().await?;

    // A bare-image stack (one service, no ports, no proxy) publishes the image's
    // exposed ports so `run up <image>` is reachable
    infer_image_ports(&mut manifest).await;

    let product = manifest.name.as_str();
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
        // Recover TLS mode from the existing .env
        recover_tls_mode(&dirs, &env_file)?
    };

    // Generate and write Caddyfile
    let routes = manifest.proxy.as_ref().map_or(&[][..], |p| &p.routes);
    let caddyfile = proxy::generate_caddyfile(&tls_mode, routes);
    write_file(&dirs.caddyfile(), &caddyfile)?;

    // Generate and write compose file
    let compose_yaml = compose::generate_compose(&manifest, &tls_mode);
    write_file(&dirs.compose_file(), &compose_yaml)?;

    // Compute runtime env vars from the manifest's routing
    let runtime_env = runtime_env_vars(&manifest, &tls_mode);
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

    // Print a summary derived from the manifest's actual routing
    let display = manifest.display_name.as_deref().unwrap_or(product);
    let access = access_summary(&manifest, &tls_mode);

    eprintln!();

    match &access.primary {
        Some(url) => eprintln!("{display} is running at {url}"),
        None => eprintln!("{display} is running"),
    }

    for (label, url) in &access.extra {
        eprintln!("  {label}:  {url}");
    }

    if !access.ports.is_empty() {
        eprintln!("  Ports: {}", access.ports.join(", "));
    }

    eprintln!("  Stop:  run down {product}");
    eprintln!("  Logs:  run logs {product}");
    eprintln!("  Wipe:  run down {product} --fresh");

    Ok(())
}

/// Stop a running product, optionally wiping all data.
///
/// With `fresh`, a pre-wipe backup is taken first (unless `no_backup`) so an
/// explicit wipe still leaves a recovery point; the wipe is aborted rather than
/// destroying data if that backup fails.
///
/// # Errors
///
/// Returns `Error::Docker` if compose down fails (non-fresh mode) or the
/// pre-wipe backup fails.
pub async fn down(product: &str, fresh: bool, no_backup: bool) -> crate::Result<()> {
    let dirs = ProjectDirs::new(product);
    let env_file = dirs.env_file();

    if fresh {
        // Safety snapshot before wiping; abort rather than silently lose data
        if !no_backup {
            let volumes = docker::list_volumes(product).await.unwrap_or_default();

            if !volumes.is_empty() {
                tracing::info!("backing up {product} data before wiping");
                let snapshot = backup::backup(product, &dirs.backups_dir(), None).await?;
                tracing::info!("saved pre-wipe backup `{snapshot}` (restore with `run restore {product} {snapshot}`)");
            }
        }

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

/// Update a running stack in place, rolling back automatically if it breaks.
///
/// Snapshots, pulls the latest images for the recipe's tags, recreates, and
/// health-gates. If the new images fail to become healthy, rolls back to the
/// exact images that were running before (restoring the pre-update snapshot too),
/// so a bad upstream release never leaves a stack down.
///
/// # Errors
///
/// Returns an error if the stack was never started, docker is unavailable, the
/// pull or recreate fails, or a post-failure rollback itself fails.
#[allow(clippy::future_not_send)] // Catalog is only used synchronously before any awaits
pub async fn update(product: &str, catalog: &dyn Catalog, no_backup: bool) -> crate::Result<()> {
    let mut manifest = crate::catalog::resolve(product, catalog)?;
    let dirs = ProjectDirs::new(&manifest.name);
    let env_file = dirs.env_file();

    if !secrets::env_file_exists(&env_file) {
        return Err(crate::Error::ProjectNotInitialized(format!(
            "{} is not running; start it with `run up {}`",
            manifest.name, manifest.name
        )));
    }

    // Reproduce up()'s manifest preparation so the regenerated compose matches
    let input_values = resolve_inputs(&manifest)?;
    let app_ctx = tokens::AppContext {
        name: manifest.name.clone(),
        fqdn: "localhost".to_owned(),
        data_dir: dirs.root().display().to_string(),
        inputs: input_values,
    };
    tokens::expand(&mut manifest, &app_ctx);

    docker::check_docker().await?;
    infer_image_ports(&mut manifest).await;

    let product = manifest.name.as_str();
    let tls_mode = recover_tls_mode(&dirs, &env_file)?;

    // Capture the exact images running now so a failed update can roll back to them
    let pre_images = docker::service_image_ids(dirs.root(), &env_file)
        .await
        .unwrap_or_default();

    // Snapshot before touching anything, unless opted out or there is no data to
    // protect (a stateless stack has no volumes, so backup would have nothing to do)
    let has_volumes = !docker::list_volumes(product).await.unwrap_or_default().is_empty();
    let snapshot = if no_backup || !has_volumes {
        None
    } else {
        let snap = backup::backup(product, &dirs.backups_dir(), None).await?;
        tracing::info!("saved pre-update backup `{snap}`");
        Some(snap)
    };

    // Regenerate config from the current recipe, then pull the latest images
    let routes = manifest.proxy.as_ref().map_or(&[][..], |p| &p.routes);
    write_file(&dirs.caddyfile(), &proxy::generate_caddyfile(&tls_mode, routes))?;
    write_file(&dirs.compose_file(), &compose::generate_compose(&manifest, &tls_mode))?;

    let runtime_env = runtime_env_vars(&manifest, &tls_mode);
    let runtime_refs: Vec<(&str, &str)> = runtime_env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    tracing::info!("pulling latest images for {product}");
    docker::compose_pull(dirs.root(), &env_file, &runtime_refs).await?;

    tracing::info!("recreating {product}");
    docker::compose_up(dirs.root(), &env_file, &runtime_refs).await?;

    tracing::info!("waiting for services to become healthy");

    match healthcheck::wait_for_healthy(dirs.root(), &env_file, 120).await {
        Ok(()) => {
            save_state(&dirs, product, ProjectStatus::Running)?;

            eprintln!();
            eprintln!("{} updated", manifest.display_name.as_deref().unwrap_or(product));

            if let Some(snap) = &snapshot {
                eprintln!("  Pre-update backup: {snap} (restore with `run restore {product} {snap}`)");
            }

            Ok(())
        }
        Err(health_err) => {
            tracing::warn!("{product} failed to become healthy after update, rolling back");
            rollback(
                &manifest,
                &dirs,
                &env_file,
                &tls_mode,
                &pre_images,
                snapshot.as_deref(),
                &runtime_refs,
            )
            .await?;

            Err(health_err)
        }
    }
}

/// Roll a stack back to the images it ran before a failed update, pinning each
/// service to the captured image id and restoring the pre-update snapshot.
#[allow(clippy::future_not_send)] // mirrors update()'s synchronous-catalog constraint
#[allow(clippy::too_many_arguments)] // all state captured by update() is needed to rebuild the stack
async fn rollback(
    manifest: &ProductManifest,
    dirs: &ProjectDirs,
    env_file: &Path,
    tls_mode: &tls::TlsMode,
    pre_images: &[(String, String)],
    snapshot: Option<&str>,
    runtime_refs: &[(&str, &str)],
) -> crate::Result<()> {
    let product = manifest.name.as_str();

    // Pin each service back to the image id it was running before the update
    let pinned = apply_image_pins(manifest, pre_images);
    write_file(&dirs.compose_file(), &compose::generate_compose(&pinned, tls_mode))?;

    let _ = docker::compose_down(dirs.root(), env_file).await;

    // Restore the pre-update data so a destructive migration is undone too
    if let Some(snap) = snapshot {
        backup::restore(&dirs.backups_dir(), snap).await?;
    }

    docker::compose_up(dirs.root(), env_file, runtime_refs).await?;
    healthcheck::wait_for_healthy(dirs.root(), env_file, 120).await?;

    save_state(dirs, product, ProjectStatus::Running)?;

    eprintln!();
    eprintln!(
        "{} rolled back to the previous version",
        manifest.display_name.as_deref().unwrap_or(product)
    );

    Ok(())
}

/// Return a manifest clone with each service's image replaced by a captured image
/// id, for services named in `pins`. Unpinned services keep their recipe image.
fn apply_image_pins(manifest: &ProductManifest, pins: &[(String, String)]) -> ProductManifest {
    let mut pinned = manifest.clone();

    for svc in &mut pinned.services {
        if let Some((_, image_id)) = pins.iter().find(|(service, _)| service == &svc.name) {
            svc.image.clone_from(image_id);
        }
    }

    pinned
}

/// Recover the TLS mode of an already-started stack from its persisted `.env`.
fn recover_tls_mode(dirs: &ProjectDirs, env_file: &Path) -> crate::Result<tls::TlsMode> {
    let existing = secrets::load_env_file(env_file)?;
    let protocol = existing.iter().find(|(k, _)| k == "PROTOCOL").map(|(_, v)| v.as_str());

    Ok(match protocol {
        Some("https") => {
            let certs_dir = dirs.certs_dir();

            tls::TlsMode::Https {
                cert: certs_dir.join("localhost.pem"),
                key: certs_dir.join("localhost-key.pem"),
            }
        }
        _ => tls::TlsMode::Http,
    })
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

/// Resolve a manifest's typed inputs from `RUN_INPUT_<KEY>` env overrides, then
/// declared defaults.
fn resolve_inputs(manifest: &ProductManifest) -> crate::Result<BTreeMap<String, String>> {
    let Some(defs) = &manifest.inputs else {
        return Ok(BTreeMap::new());
    };

    let mut overrides = BTreeMap::new();

    for def in defs {
        let var = format!("RUN_INPUT_{}", def.key.to_ascii_uppercase());

        if let Ok(value) = std::env::var(&var) {
            overrides.insert(def.key.clone(), value);
        }
    }

    // Prompt interactively for any input without an env override; a blank answer
    // falls through to the input's default
    if std::io::stdin().is_terminal() {
        for def in defs {
            if overrides.contains_key(&def.key) {
                continue;
            }

            if let Some(value) = prompt_input(def) {
                overrides.insert(def.key.clone(), value);
            }
        }
    }

    inputs::resolve(defs, &overrides)
}

/// Prompt on stderr for one input, returning the trimmed answer or `None` when
/// it is blank or cannot be read.
fn prompt_input(def: &crate::config::InputDef) -> Option<String> {
    let label = def.label.as_deref().unwrap_or(&def.key);
    let hint = def
        .default
        .as_deref()
        .map_or_else(String::new, |default| format!(" [{default}]"));

    eprint!("{label}{hint}: ");
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return None;
    }

    let trimmed = line.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Show the running status of a single stack via `docker compose ps`.
///
/// # Errors
///
/// Returns `Error::Docker` if docker is unavailable or the command fails.
pub async fn status(product: &str) -> crate::Result<()> {
    let dirs = ProjectDirs::new(product);
    let env_file = dirs.env_file();

    if !secrets::env_file_exists(&env_file) {
        println!("{product}: not initialized (run `run up {product}`)");
        return Ok(());
    }

    docker::check_docker().await?;
    docker::compose_status(dirs.root(), &env_file).await
}

/// List every known project and its last-saved status.
///
/// # Errors
///
/// Returns `Error::ReadFile` if the projects directory cannot be read.
pub fn list_projects() -> crate::Result<Vec<(String, ProjectStatus)>> {
    list_projects_in(&ProjectDirs::projects_root())
}

/// List projects under a given root directory, each with its saved status.
fn list_projects_in(root: &Path) -> crate::Result<Vec<(String, ProjectStatus)>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(root).map_err(|source| crate::Error::ReadFile {
        path: root.to_path_buf(),
        source,
    })?;

    let mut projects = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|source| crate::Error::ReadFile {
            path: root.to_path_buf(),
            source,
        })?;

        let path = entry.path();

        if path.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                projects.push((name.to_owned(), read_status(&path)));
            }
        }
    }

    projects.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(projects)
}

/// Read a project's saved status from its `state.json`, defaulting to stopped.
fn read_status(project_dir: &Path) -> ProjectStatus {
    std::fs::read_to_string(project_dir.join("state.json"))
        .ok()
        .and_then(|content| serde_json::from_str::<ProjectState>(&content).ok())
        .map_or(ProjectStatus::Stopped, |state| state.status)
}

/// For a bare-image stack (one service, no ports, no proxy), pull the image and
/// publish its exposed TCP ports so `run up <image>` is reachable. Best-effort:
/// any failure leaves the manifest unchanged.
async fn infer_image_ports(manifest: &mut ProductManifest) {
    if manifest.proxy.is_some() || manifest.services.len() != 1 {
        return;
    }

    if manifest.services[0].ports.is_some() {
        return;
    }

    let image = manifest.services[0].image.clone();

    if docker::pull_image(&image).await.is_err() {
        return;
    }

    let ports = docker::image_exposed_ports(&image).await.unwrap_or_default();

    if !ports.is_empty() {
        let published = ports.iter().map(|port| format!("{port}:{port}")).collect();
        manifest.services[0].ports = Some(published);
        tracing::info!("publishing inferred ports for {image}: {ports:?}");
    }
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

/// Well-known container ports used to pick the primary URL for a stack that
/// publishes ports directly (no reverse proxy).
const WEB_PORTS: &[u16] = &[80, 443, 8080, 8000, 3000, 8888, 5000];

/// User-facing access info for a started stack, derived entirely from its
/// manifest rather than any product-specific assumption.
struct Access {
    /// The URL to open in a browser, if the stack is reachable over HTTP(S).
    primary: Option<String>,
    /// Additional labeled URLs (proxy routes beyond the primary one).
    extra: Vec<(String, String)>,
    /// Raw published port mappings, shown for a port-publishing stack.
    ports: Vec<String>,
}

/// Build the browser URL for a proxy route under the active TLS mode, omitting
/// the port when it is the scheme default.
fn proxy_route_url(route: &ProxyRoute, tls_mode: &tls::TlsMode) -> String {
    let (scheme, port, default_port) = match tls_mode {
        tls::TlsMode::Https { .. } => ("https", route.listen_port, 443),
        tls::TlsMode::Http => ("http", route.listen_port_http.unwrap_or(route.listen_port), 80),
    };

    if port == default_port {
        format!("{scheme}://localhost")
    } else {
        format!("{scheme}://localhost:{port}")
    }
}

/// Parse a compose port mapping (`host:container[/proto]` or a bare `port`) into
/// `(host, container, is_tcp)`.
fn parse_port_mapping(mapping: &str) -> Option<(u16, u16, bool)> {
    let (spec, proto) = mapping.split_once('/').map_or((mapping, "tcp"), |(s, p)| (s, p));
    let is_tcp = proto.eq_ignore_ascii_case("tcp");

    let mut parts = spec.split(':');
    let host: u16 = parts.next()?.parse().ok()?;
    let container = match parts.next() {
        Some(c) => c.parse().ok()?,
        None => host,
    };

    Some((host, container, is_tcp))
}

/// Pick the host port to surface as the primary URL for a port-publishing stack:
/// prefer a mapping whose container port is a well-known web port, else the first
/// published TCP port.
fn primary_published_port(manifest: &ProductManifest) -> Option<u16> {
    let mappings: Vec<(u16, u16, bool)> = manifest
        .services
        .iter()
        .filter_map(|svc| svc.ports.as_ref())
        .flatten()
        .filter_map(|mapping| parse_port_mapping(mapping))
        .filter(|&(_, _, is_tcp)| is_tcp)
        .collect();

    mappings
        .iter()
        .find(|&&(_, container, _)| WEB_PORTS.contains(&container))
        .or_else(|| mappings.first())
        .map(|&(host, _, _)| host)
}

/// Capitalize the first character of a route name for display.
fn title_case(name: &str) -> String {
    let mut chars = name.chars();

    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// Derive user-facing access info from a manifest: proxy routes map to URLs, and
/// a port-publishing stack maps its primary web port to `http://localhost`.
fn access_summary(manifest: &ProductManifest, tls_mode: &tls::TlsMode) -> Access {
    if let Some(proxy) = &manifest.proxy {
        let app = proxy
            .routes
            .iter()
            .find(|route| route.name == "app")
            .or_else(|| proxy.routes.first());
        let app_name = app.map(|route| route.name.as_str());

        let primary = app.map(|route| proxy_route_url(route, tls_mode));
        let extra = proxy
            .routes
            .iter()
            .filter(|route| Some(route.name.as_str()) != app_name)
            .map(|route| (title_case(&route.name), proxy_route_url(route, tls_mode)))
            .collect();

        return Access {
            primary,
            extra,
            ports: Vec::new(),
        };
    }

    let primary = primary_published_port(manifest).map(|port| {
        if port == 80 {
            "http://localhost".to_owned()
        } else {
            format!("http://localhost:{port}")
        }
    });
    let ports = manifest
        .services
        .iter()
        .filter_map(|svc| svc.ports.as_ref())
        .flatten()
        .cloned()
        .collect();

    Access {
        primary,
        extra: Vec::new(),
        ports,
    }
}

/// Compute runtime environment variables for compose substitution, derived from
/// the manifest's proxy routes (or its primary published port when there is no
/// proxy). A route named `app` also populates `BASE_URL` and `APP_PORT_MAP`.
fn runtime_env_vars(manifest: &ProductManifest, tls_mode: &tls::TlsMode) -> Vec<(String, String)> {
    let mut vars = Vec::new();

    if let Some(proxy) = &manifest.proxy {
        for route in &proxy.routes {
            let url = proxy_route_url(route, tls_mode);

            if route.name == "app" {
                vars.push(("BASE_URL".to_owned(), url.clone()));

                let port = match tls_mode {
                    tls::TlsMode::Https { .. } => route.listen_port,
                    tls::TlsMode::Http => route.listen_port_http.unwrap_or(route.listen_port),
                };
                vars.push(("APP_PORT_MAP".to_owned(), format!("{port}:{port}")));
            }

            vars.push((format!("{}_BASE_URL", route.name.to_uppercase()), url));
        }
    } else if let Some(primary) = access_summary(manifest, tls_mode).primary {
        vars.push(("BASE_URL".to_owned(), primary));
    }

    vars
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

    #[test]
    fn list_projects_is_empty_when_root_absent() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert!(list_projects_in(&dir.path().join("missing")).expect("list").is_empty());
    }

    #[test]
    fn list_projects_reads_saved_status_sorted() {
        let dir = tempfile::tempdir().expect("tempdir");

        let alpha = dir.path().join("alpha");
        std::fs::create_dir_all(&alpha).expect("mkdir");
        std::fs::write(
            alpha.join("state.json"),
            r#"{"product":"alpha","status":"running","created_at":"t","updated_at":"t"}"#,
        )
        .expect("write state");

        // a project with no state.json defaults to stopped
        std::fs::create_dir_all(dir.path().join("beta")).expect("mkdir");

        let projects = list_projects_in(dir.path()).expect("list");

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].0, "alpha");
        assert!(matches!(projects[0].1, ProjectStatus::Running));
        assert_eq!(projects[1].0, "beta");
        assert!(matches!(projects[1].1, ProjectStatus::Stopped));
    }

    fn tls_https() -> tls::TlsMode {
        tls::TlsMode::Https {
            cert: std::path::PathBuf::from("/certs/localhost.pem"),
            key: std::path::PathBuf::from("/certs/localhost-key.pem"),
        }
    }

    #[test]
    fn proxy_route_url_omits_default_port() {
        let route = ProxyRoute {
            name: "app".to_owned(),
            listen_port: 443,
            target: "app:3000".to_owned(),
            listen_port_http: Some(80),
        };

        assert_eq!(proxy_route_url(&route, &tls_https()), "https://localhost");
        assert_eq!(proxy_route_url(&route, &tls::TlsMode::Http), "http://localhost");
    }

    #[test]
    fn proxy_route_url_keeps_nondefault_port() {
        let route = ProxyRoute {
            name: "auth".to_owned(),
            listen_port: 3001,
            target: "auth:3000".to_owned(),
            listen_port_http: None,
        };

        assert_eq!(proxy_route_url(&route, &tls_https()), "https://localhost:3001");
    }

    #[test]
    #[allow(clippy::similar_names)] // auth/api mirror the real route names
    fn access_summary_for_proxy_uses_routes() {
        let manifest = default_catalog().get("runa").expect("runa manifest should parse");
        let access = access_summary(&manifest, &tls_https());

        assert_eq!(access.primary.as_deref(), Some("https://localhost"));
        assert!(
            access
                .extra
                .iter()
                .any(|(l, u)| l == "Auth" && u == "https://localhost:3001"),
            "expected an Auth route line, got {:?}",
            access.extra
        );
        assert!(
            access
                .extra
                .iter()
                .any(|(l, u)| l == "Api" && u == "https://localhost:4000"),
            "expected an Api route line, got {:?}",
            access.extra
        );
        assert!(access.ports.is_empty());
    }

    #[test]
    fn access_summary_for_published_ports_prefers_web_port() {
        let manifest = default_catalog().get("pihole").expect("pihole manifest should parse");
        // TLS mode is irrelevant for a port-publishing stack (no proxy, no caddy)
        let access = access_summary(&manifest, &tls_https());

        assert_eq!(access.primary.as_deref(), Some("http://localhost:8089"));
        assert!(access.extra.is_empty());
        assert!(
            access.ports.iter().any(|p| p == "53:53/tcp"),
            "expected raw port mappings listed, got {:?}",
            access.ports
        );
    }

    #[test]
    #[allow(clippy::similar_names)] // auth/api mirror the real route names
    fn runtime_env_vars_derive_from_runa_routes() {
        let manifest = default_catalog().get("runa").expect("runa manifest should parse");
        let env = runtime_env_vars(&manifest, &tls_https());
        let get = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

        assert_eq!(get("BASE_URL"), Some("https://localhost"));
        assert_eq!(get("AUTH_BASE_URL"), Some("https://localhost:3001"));
        assert_eq!(get("API_BASE_URL"), Some("https://localhost:4000"));
        assert_eq!(get("APP_PORT_MAP"), Some("443:443"));
    }

    #[test]
    fn apply_image_pins_overrides_matching_services() {
        let manifest = default_catalog()
            .get("uptime-kuma")
            .expect("uptime-kuma manifest should parse");
        let pins = vec![("app".to_owned(), "sha256:old".to_owned())];

        let pinned = apply_image_pins(&manifest, &pins);

        assert_eq!(
            pinned
                .services
                .iter()
                .find(|s| s.name == "app")
                .expect("app service")
                .image,
            "sha256:old"
        );
        // the original manifest is untouched
        assert_eq!(
            manifest
                .services
                .iter()
                .find(|s| s.name == "app")
                .expect("app service")
                .image,
            "louislam/uptime-kuma:1"
        );
    }

    #[test]
    fn apply_image_pins_leaves_unpinned_services() {
        let manifest = default_catalog().get("runa").expect("runa manifest should parse");
        let pins = vec![("app".to_owned(), "sha256:app-old".to_owned())];

        let pinned = apply_image_pins(&manifest, &pins);

        // db has no pin, so it keeps its recipe image
        let db = pinned.services.iter().find(|s| s.name == "db").expect("db service");
        assert_eq!(db.image, "postgres:16-alpine");
    }

    #[test]
    fn runtime_env_vars_http_mode() {
        let manifest = default_catalog().get("runa").expect("runa manifest should parse");
        let env = runtime_env_vars(&manifest, &tls::TlsMode::Http);
        let get = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

        assert_eq!(get("BASE_URL"), Some("http://localhost"));
        assert_eq!(get("AUTH_BASE_URL"), Some("http://localhost:3001"));
        assert_eq!(get("APP_PORT_MAP"), Some("80:80"));
    }
}
