use std::fmt::Write as _;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng as _;
use sha2::{Digest, Sha256};

use crate::Error;

/// Generate a random hex string of `len` bytes (output is `2*len` chars).
#[must_use]
pub fn generate_hex(len: usize) -> String {
    let mut buf = vec![0u8; len];
    rand::rng().fill(&mut buf[..]);

    buf.iter().fold(String::with_capacity(len * 2), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Write key=value pairs to an env file, one per line.
///
/// # Errors
///
/// Returns `Error::WriteFile` if the file cannot be written.
pub fn write_env_file(path: &Path, secrets: &[(&str, &str)]) -> crate::Result<()> {
    let content: String = secrets
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(path, content).map_err(|source| Error::WriteFile {
        path: path.to_owned(),
        source,
    })
}

/// Parse key=value lines from an env file, skipping blanks and comments.
///
/// # Errors
///
/// Returns `Error::ReadFile` if the file cannot be read.
pub fn load_env_file(path: &Path) -> crate::Result<Vec<(String, String)>> {
    let content = std::fs::read_to_string(path).map_err(|source| Error::ReadFile {
        path: path.to_owned(),
        source,
    })?;

    let pairs = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_owned(), value.trim().to_owned()))
        })
        .collect();

    Ok(pairs)
}

/// Check whether an env file exists at the given path.
#[must_use]
pub fn env_file_exists(path: &Path) -> bool {
    path.exists()
}

/// SHA-256 hash with base64url encoding (no padding).
///
/// Matches the shell equivalent:
/// `echo -n "$SECRET" | openssl dgst -sha256 -binary | openssl base64 -A | tr '+/' '-_' | tr -d '='`
#[must_use]
pub fn hash_secret(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());

    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_hex_correct_length() {
        let hex = generate_hex(32);

        assert_eq!(
            hex.len(),
            64,
            "generate_hex(32) should produce 64 hex chars (2 per byte)"
        );
    }

    #[test]
    fn generate_hex_is_valid_hex() {
        let hex = generate_hex(32);

        assert!(
            hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "output should only contain lowercase hex chars [0-9a-f]"
        );
    }

    #[test]
    fn env_file_round_trip() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join(".env");

        let secrets = [("DB_PASSWORD", "hunter2"), ("AUTH_SECRET", "s3cret")];
        write_env_file(&path, &secrets).expect("should write env file");

        let loaded = load_env_file(&path).expect("should read env file");

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], ("DB_PASSWORD".to_owned(), "hunter2".to_owned()));
        assert_eq!(loaded[1], ("AUTH_SECRET".to_owned(), "s3cret".to_owned()));
    }

    #[test]
    fn hash_secret_matches_expected() {
        // SHA-256("test") = 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
        // base64url-no-pad of those bytes = n4bQgYhMfWWaL-qgxVrQFaO_TxsrC4Is0V1sFbDwCgg
        let result = hash_secret("test");

        assert_eq!(
            result, "n4bQgYhMfWWaL-qgxVrQFaO_TxsrC4Is0V1sFbDwCgg",
            "hash_secret should produce SHA-256 + base64url-no-pad"
        );
    }
}
