use std::fmt::Write as _;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use crate::Error;

/// Generate a random hex string of `len` bytes (output is `2*len` chars).
///
/// # Panics
///
/// Panics if the operating system's random source is unavailable, which should
/// not happen on a supported platform.
#[must_use]
pub fn generate_hex(len: usize) -> String {
    let mut buf = vec![0u8; len];
    getrandom::fill(&mut buf).expect("OS random source should be available");

    buf.iter().fold(String::with_capacity(len * 2), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Generate `len` random bytes encoded as standard, padded base64.
///
/// # Panics
///
/// Panics if the operating system's random source is unavailable, which should
/// not happen on a supported platform.
#[must_use]
pub fn generate_base64(len: usize) -> String {
    let mut buf = vec![0u8; len];
    getrandom::fill(&mut buf).expect("OS random source should be available");

    STANDARD.encode(buf)
}

/// Write key=value pairs to an env file, one per line.
///
/// The file holds generated secrets and saved inputs, so it is readable by its
/// owner only (mode `0600` on Unix), including when an existing file is rewritten.
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

    write_private(path, content.as_bytes()).map_err(|source| Error::WriteFile {
        path: path.to_owned(),
        source,
    })
}

/// Rewrite an existing env file: drop every entry whose key matches `drop`,
/// keep the rest (generated secrets in particular) in order, then append `add`.
///
/// # Errors
///
/// Returns `Error::ReadFile` or `Error::WriteFile` if the file cannot be read or written.
pub fn update_env_file(path: &Path, drop: impl Fn(&str) -> bool, add: &[(&str, &str)]) -> crate::Result<()> {
    let existing = load_env_file(path)?;

    let mut entries: Vec<(&str, &str)> = existing
        .iter()
        .filter(|(k, _)| !drop(k))
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    entries.extend_from_slice(add);

    write_env_file(path, &entries)
}

/// Write a file readable and writable by its owner only.
#[cfg(unix)]
fn write_private(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;

    // `mode` only applies on create, so tighten a file left by an older version
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(content)
}

/// Write a file (no Unix permission bits to set on this platform).
#[cfg(not(unix))]
fn write_private(path: &Path, content: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, content)
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

    #[cfg(unix)]
    #[test]
    fn env_file_is_owner_only_even_when_rewritten() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".env");
        std::fs::write(&path, "OLD=1").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");

        write_env_file(&path, &[("SECRET", "value")]).expect("write env");

        let mode = std::fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn update_env_file_replaces_matching_entries_and_keeps_the_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".env");
        write_env_file(&path, &[("DB_PASSWORD", "secret"), ("RUN_INPUT_TZ", "UTC")]).expect("write");

        update_env_file(
            &path,
            |k| k.starts_with("RUN_INPUT_"),
            &[("RUN_INPUT_TZ", "Europe/Paris")],
        )
        .expect("update");

        let env = load_env_file(&path).expect("load");
        assert_eq!(
            env,
            [
                ("DB_PASSWORD".to_owned(), "secret".to_owned()),
                ("RUN_INPUT_TZ".to_owned(), "Europe/Paris".to_owned()),
            ]
        );
    }

    #[test]
    fn generate_base64_decodes_to_the_requested_bytes() {
        let encoded = generate_base64(32);
        let decoded = STANDARD.decode(&encoded).expect("valid base64");

        assert_eq!(decoded.len(), 32);
        assert_ne!(generate_base64(32), encoded, "each call should be random");
    }

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
