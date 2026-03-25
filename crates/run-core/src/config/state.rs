use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Runtime status of a deployed project.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Stopped,
    Running,
    Degraded,
}

/// Persisted runtime state for a project instance.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectState {
    pub product: String,
    pub status: ProjectStatus,
    pub created_at: String,
    pub updated_at: String,
}

impl ProjectState {
    /// Load state from disk, returning `None` if the file doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns `Error::ReadFile` if the file exists but cannot be read or parsed.
    pub fn load(dirs: &ProjectDirs) -> Result<Option<Self>> {
        let path = dirs.state_file();

        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path).map_err(|source| Error::ReadFile {
            path: path.clone(),
            source,
        })?;

        let state = serde_json::from_str(&content).map_err(|err| Error::ReadFile {
            path,
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, err),
        })?;

        Ok(Some(state))
    }

    /// Persist state to disk.
    ///
    /// # Errors
    ///
    /// Returns `Error::WriteFile` if the state cannot be serialized or written.
    pub fn save(&self, dirs: &ProjectDirs) -> Result<()> {
        let path = dirs.state_file();

        let content = serde_json::to_string_pretty(self).map_err(|err| Error::WriteFile {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, err),
        })?;

        std::fs::write(&path, content).map_err(|source| Error::WriteFile { path, source })
    }
}

/// XDG-based directory layout for a project's generated files.
#[derive(Debug, Clone)]
pub struct ProjectDirs {
    root: PathBuf,
}

impl ProjectDirs {
    /// Create a new `ProjectDirs` for the given product name.
    ///
    /// Resolves to `~/.config/run/projects/<product>/`.
    #[must_use]
    pub fn new(product: &str) -> Self {
        let root = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("run")
            .join("projects")
            .join(product);

        Self { root }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn compose_file(&self) -> PathBuf {
        self.root.join("compose.gen.yaml")
    }

    #[must_use]
    pub fn env_file(&self) -> PathBuf {
        self.root.join(".env")
    }

    #[must_use]
    pub fn caddyfile(&self) -> PathBuf {
        self.root.join("Caddyfile")
    }

    #[must_use]
    pub fn certs_dir(&self) -> PathBuf {
        self.root.join(".certs")
    }

    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    /// Create the project directory tree if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns `Error::WriteFile` if directory creation fails.
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root).map_err(|source| Error::WriteFile {
            path: self.root.clone(),
            source,
        })?;

        let certs = self.certs_dir();
        std::fs::create_dir_all(&certs).map_err(|source| Error::WriteFile { path: certs, source })?;

        Ok(())
    }
}
