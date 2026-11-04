//! The local web dashboard served by `run serve`.
//!
//! A single self-contained binary: an axum server, bound to localhost by
//! default, that drives the embedded run engine (the same `run-core` the CLI
//! uses) and serves a static UI compiled into the binary. This is the
//! device-agnostic, appliance-style surface for owning a self-hosted stack,
//! reachable from any browser on the machine.

use std::net::SocketAddr;

use axum::{
    Json, Router,
    extract::Path,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rust_embed::RustEmbed;
use serde::Serialize;

use run_core::catalog::{Catalog, default_catalog};
use run_core::orchestrator;

/// The dashboard UI, compiled into the binary so a host needs no Node runtime.
#[derive(RustEmbed)]
#[folder = "src/server/ui/"]
struct Assets;

/// A catalog app as the dashboard lists it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEntry {
    id: String,
    name: String,
    description: Option<String>,
    service_count: usize,
}

/// A known stack and its last-saved status.
#[derive(Serialize)]
struct StackEntry {
    name: String,
    status: String,
}

/// An engine error rendered as a 500 with its message.
struct ApiError(String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0).into_response()
    }
}

/// Validate a stack id from the client before it reaches the engine: a short
/// lowercase slug, never something that could be read as a path or image ref.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Start the dashboard on `host:port`, serving until the process is stopped.
///
/// # Errors
///
/// Returns an error if the address is invalid or the port cannot be bound.
pub async fn serve(host: &str, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("run dashboard listening on http://{addr}");
    if host == "127.0.0.1" || host == "localhost" {
        tracing::info!("open http://localhost:{port} in your browser");
    }

    axum::serve(listener, router()).await?;

    Ok(())
}

/// Build the dashboard's routes.
fn router() -> Router {
    Router::new()
        .route("/api/catalog", get(catalog))
        .route("/api/stacks", get(stacks))
        .route("/api/stacks/{id}/up", post(up))
        .route("/api/stacks/{id}/down", post(down))
        .fallback(assets)
}

/// List the catalog apps the dashboard can start.
async fn catalog() -> Json<Vec<CatalogEntry>> {
    let catalog = default_catalog();

    let entries = catalog
        .list()
        .into_iter()
        .filter_map(|id| {
            let manifest = catalog.get(&id).ok()?;
            Some(CatalogEntry {
                id,
                name: manifest.display_name.unwrap_or(manifest.name),
                description: manifest.description,
                service_count: manifest.services.len(),
            })
        })
        .collect();

    Json(entries)
}

/// List known stacks and their last-saved status.
async fn stacks() -> Result<Json<Vec<StackEntry>>, ApiError> {
    let projects = orchestrator::list_projects().map_err(|e| ApiError(e.to_string()))?;

    let entries = projects
        .into_iter()
        .map(|(name, status)| StackEntry {
            name,
            status: format!("{status:?}").to_lowercase(),
        })
        .collect();

    Ok(Json(entries))
}

/// Start a catalog app locally (equivalent to `run up <id>`).
async fn up(Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if !valid_id(&id) {
        return Err(ApiError(format!("invalid stack id: {id}")));
    }

    run_engine(move || async move {
        // Create and use the catalog inside the future so it owns its data
        let catalog = default_catalog();
        catalog.get(&id)?;
        orchestrator::up(&id, &catalog).await
    })
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Stop a running stack (equivalent to `run down <id>`), leaving its data intact.
async fn down(Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if !valid_id(&id) {
        return Err(ApiError(format!("invalid stack id: {id}")));
    }

    run_engine(move || async move { orchestrator::down(&id, false, false).await }).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Run an engine operation that borrows the non-`Send` catalog on a blocking
/// thread, so the handler future stays `Send` for the multi-threaded runtime.
async fn run_engine<F, Fut>(op: F) -> Result<(), ApiError>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = run_core::Result<()>>,
{
    tokio::task::spawn_blocking(move || tokio::runtime::Handle::current().block_on(op()))
        .await
        .map_err(|e| ApiError(e.to_string()))?
        .map_err(|e| ApiError(e.to_string()))
}

/// Serve an embedded UI asset, falling back to `index.html` for client routes.
async fn assets(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = Assets::get(path) {
        let mime = file.metadata.mimetype().to_owned();
        return ([(header::CONTENT_TYPE, mime)], file.data).into_response();
    }

    match Assets::get("index.html") {
        Some(file) => ([(header::CONTENT_TYPE, "text/html")], file.data).into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::valid_id;

    #[test]
    fn accepts_catalog_slugs() {
        assert!(valid_id("runa"));
        assert!(valid_id("uptime-kuma"));
    }

    #[test]
    fn rejects_paths_and_images() {
        assert!(!valid_id(""));
        assert!(!valid_id("../etc"));
        assert!(!valid_id("ghcr.io/acme/app:1"));
        assert!(!valid_id("Runa"));
    }
}
