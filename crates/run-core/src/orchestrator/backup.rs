//! Local backup and restore of a stack's Docker volumes.
//!
//! A backup snapshots every `<project>_*` volume into a directory of gzip
//! tarballs under the project's `backups/` dir; a restore replaces those volume
//! contents from a snapshot. Volumes are archived and restored through a
//! throwaway `alpine` container, so nothing needs to be installed on the host.

use std::path::Path;

use crate::docker;

/// The gzip tarball name used for a volume.
fn archive_name(volume: &str) -> String {
    format!("{volume}.tar.gz")
}

/// Recover the volume name from an archive file name.
fn volume_from_archive(file_name: &str) -> Option<&str> {
    file_name.strip_suffix(".tar.gz")
}

/// A timestamped default snapshot name (seconds since the epoch sort newest-last).
fn default_snapshot_name() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    format!("backup-{secs}")
}

/// List snapshot names under a backups directory, sorted ascending.
///
/// Returns an empty list when the directory does not exist yet.
///
/// # Errors
///
/// Returns `Error::ReadFile` if the directory cannot be read.
pub fn list_snapshots(backups_dir: &Path) -> crate::Result<Vec<String>> {
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(backups_dir).map_err(|source| crate::Error::ReadFile {
        path: backups_dir.to_path_buf(),
        source,
    })?;

    let mut names = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|source| crate::Error::ReadFile {
            path: backups_dir.to_path_buf(),
            source,
        })?;

        if entry.path().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_owned());
            }
        }
    }

    names.sort();
    Ok(names)
}

/// Back up a project's volumes into a new snapshot, returning the snapshot name.
///
/// # Errors
///
/// Returns `Error::Docker` if the project has no volumes or archiving fails, or
/// `Error::WriteFile` if the snapshot directory cannot be created.
pub async fn backup(project: &str, backups_dir: &Path, name: Option<&str>) -> crate::Result<String> {
    let volumes = docker::list_volumes(project).await?;

    if volumes.is_empty() {
        return Err(crate::Error::Docker {
            message: format!("no volumes found for `{project}` (has it been started yet?)"),
        });
    }

    let snapshot = name.map_or_else(default_snapshot_name, ToOwned::to_owned);
    let dir = backups_dir.join(&snapshot);

    std::fs::create_dir_all(&dir).map_err(|source| crate::Error::WriteFile {
        path: dir.clone(),
        source,
    })?;

    for volume in &volumes {
        docker::archive_volume(volume, &dir, &archive_name(volume)).await?;
    }

    Ok(snapshot)
}

/// Restore volumes from a snapshot (destructive: existing volume contents are
/// replaced). The caller is responsible for confirming first.
///
/// # Errors
///
/// Returns an error if the snapshot is missing, holds no archives, or a restore
/// container fails.
pub async fn restore(backups_dir: &Path, snapshot: &str) -> crate::Result<()> {
    let dir = backups_dir.join(snapshot);

    if !dir.is_dir() {
        return Err(crate::Error::ProjectNotInitialized(format!("snapshot `{snapshot}`")));
    }

    let entries = std::fs::read_dir(&dir).map_err(|source| crate::Error::ReadFile {
        path: dir.clone(),
        source,
    })?;

    let mut restored = 0;

    for entry in entries {
        let entry = entry.map_err(|source| crate::Error::ReadFile {
            path: dir.clone(),
            source,
        })?;

        let file_name = entry.file_name();
        let Some(file_str) = file_name.to_str() else {
            continue;
        };
        let Some(volume) = volume_from_archive(file_str) else {
            continue;
        };

        docker::extract_volume(volume, &dir, file_str).await?;
        restored += 1;
    }

    if restored == 0 {
        return Err(crate::Error::Docker {
            message: format!("snapshot `{snapshot}` contains no volume archives"),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_name_round_trips() {
        assert_eq!(archive_name("pihole_etc"), "pihole_etc.tar.gz");
        assert_eq!(volume_from_archive("pihole_etc.tar.gz"), Some("pihole_etc"));
        assert_eq!(volume_from_archive("not-an-archive"), None);
    }

    #[test]
    fn default_snapshot_name_is_prefixed() {
        assert!(default_snapshot_name().starts_with("backup-"));
    }

    #[test]
    fn list_snapshots_is_empty_when_dir_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("backups");

        assert!(list_snapshots(&missing).expect("list").is_empty());
    }

    #[test]
    fn list_snapshots_returns_sorted_subdirectories_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("backup-2")).expect("mkdir");
        std::fs::create_dir_all(dir.path().join("backup-1")).expect("mkdir");
        std::fs::write(dir.path().join("loose-file"), "x").expect("write");

        let snapshots = list_snapshots(dir.path()).expect("list");

        assert_eq!(snapshots, vec!["backup-1", "backup-2"]);
    }
}
