use std::path::PathBuf;

/// Errors that can occur during Run operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("product not found: {0}")]
    ProductNotFound(String),
    #[error("docker not found -- install Docker and ensure it's running")]
    DockerNotFound,
    #[error("docker compose not available")]
    ComposeNotFound,
    #[error("project not initialized: {0}")]
    ProjectNotInitialized(String),
    #[error("failed to read {path}: {source}")]
    ReadFile { path: PathBuf, source: std::io::Error },
    #[error("failed to write {path}: {source}")]
    WriteFile { path: PathBuf, source: std::io::Error },
    #[error("failed to parse manifest: {source}")]
    ParseManifest { source: toml::de::Error },
    #[error("failed to parse recipe: {source}")]
    ParseYaml { source: serde_yaml::Error },
    #[error("cannot ingest compose file: {message}")]
    ComposeIngest { message: String },
    #[error("required input not provided: {key}")]
    MissingInput { key: String },
    #[error("docker command failed: {message}")]
    Docker { message: String },
    #[error("healthcheck timed out after {seconds}s")]
    HealthTimeout { seconds: u64 },
    #[error("hook failed: {name}: {message}")]
    HookFailed { name: String, message: String },
}

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;
