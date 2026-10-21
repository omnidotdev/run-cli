use std::path::Path;
use std::process::Output;

use tokio::process::Command;

use crate::Error;

/// Verify `docker` is on PATH and `docker compose` is available.
///
/// # Errors
///
/// Returns `Error::DockerNotFound` if docker is missing, or
/// `Error::ComposeNotFound` if the compose plugin is not installed.
pub async fn check_docker() -> crate::Result<()> {
    which::which("docker").map_err(|_| Error::DockerNotFound)?;

    let status = Command::new("docker")
        .args(["compose", "version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|_| Error::DockerNotFound)?;

    if !status.success() {
        return Err(Error::ComposeNotFound);
    }

    Ok(())
}

/// Pull an image so it is present locally.
///
/// # Errors
///
/// Returns `Error::Docker` if the pull fails.
pub async fn pull_image(image: &str) -> crate::Result<()> {
    let output = Command::new("docker")
        .args(["pull", image])
        .output()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    if !output.status.success() {
        return Err(Error::Docker {
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(())
}

/// Return a locally-present image's exposed TCP ports.
///
/// # Errors
///
/// Returns `Error::Docker` only if the docker binary cannot be invoked; a
/// failed inspect (e.g. image absent) yields an empty list.
pub async fn image_exposed_ports(image: &str) -> crate::Result<Vec<u16>> {
    let output = Command::new("docker")
        .args(["image", "inspect", image, "--format", "{{json .Config.ExposedPorts}}"])
        .output()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    Ok(parse_exposed_ports(&String::from_utf8_lossy(&output.stdout)))
}

/// Parse docker's `ExposedPorts` map (e.g. `{"80/tcp":{},"53/udp":{}}`) into
/// sorted, de-duplicated TCP port numbers.
fn parse_exposed_ports(json: &str) -> Vec<u16> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(json.trim()) else {
        return Vec::new();
    };

    let mut ports: Vec<u16> = map
        .keys()
        .filter(|key| key.ends_with("/tcp") || !key.contains('/'))
        .filter_map(|key| key.split('/').next().and_then(|port| port.parse().ok()))
        .collect();

    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Run `docker compose pull` to fetch the latest images for the stack's tags.
///
/// # Errors
///
/// Returns `Error::Docker` if the pull fails.
pub async fn compose_pull(project_dir: &Path, env_file: &Path, extra_env: &[(&str, &str)]) -> crate::Result<()> {
    run_compose(project_dir, env_file, &["pull"], extra_env).await?;

    Ok(())
}

/// Return the currently-running image id for each service, as `(service, image_id)`.
///
/// Joins `docker compose images` (image ids keyed by container name) with
/// `docker compose ps` (service names keyed by container name) so a later update
/// can pin each service back to the exact image it is running now.
///
/// # Errors
///
/// Returns `Error::Docker` if either compose command fails.
pub async fn service_image_ids(project_dir: &Path, env_file: &Path) -> crate::Result<Vec<(String, String)>> {
    let images_out = run_compose(project_dir, env_file, &["images", "--format", "json"], &[]).await?;
    let ps_out = run_compose(project_dir, env_file, &["ps", "--format", "json"], &[]).await?;

    let images = parse_image_ids(&String::from_utf8_lossy(&images_out.stdout));
    let services = parse_ps_services(&String::from_utf8_lossy(&ps_out.stdout));

    Ok(join_service_images(&images, &services))
}

/// Parse `docker compose images --format json` (a JSON array) into
/// `(container_name, image_id)` pairs.
fn parse_image_ids(json: &str) -> Vec<(String, String)> {
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str::<serde_json::Value>(json.trim()) else {
        return Vec::new();
    };

    entries
        .iter()
        .filter_map(|entry| {
            let name = entry.get("ContainerName")?.as_str()?.to_owned();
            let id = entry.get("ID")?.as_str()?.to_owned();

            Some((name, id))
        })
        .collect()
}

/// Parse `docker compose ps --format json` (newline-delimited objects) into
/// `(container_name, service)` pairs.
fn parse_ps_services(json: &str) -> Vec<(String, String)> {
    json.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .filter_map(|entry| {
            let name = entry.get("Name")?.as_str()?.to_owned();
            let service = entry.get("Service")?.as_str()?.to_owned();

            Some((name, service))
        })
        .collect()
}

/// Join container-keyed image ids with container-keyed service names, yielding
/// `(service, image_id)`.
fn join_service_images(images: &[(String, String)], services: &[(String, String)]) -> Vec<(String, String)> {
    services
        .iter()
        .filter_map(|(container, service)| {
            images
                .iter()
                .find(|(name, _)| name == container)
                .map(|(_, id)| (service.clone(), id.clone()))
        })
        .collect()
}

/// Run `docker compose up -d` with the generated compose file.
///
/// # Errors
///
/// Returns `Error::Docker` if the command fails.
pub async fn compose_up(project_dir: &Path, env_file: &Path, extra_env: &[(&str, &str)]) -> crate::Result<()> {
    run_compose(project_dir, env_file, &["up", "-d"], extra_env).await?;

    Ok(())
}

/// Run `docker compose down` to stop and remove containers.
///
/// # Errors
///
/// Returns `Error::Docker` if the command fails.
pub async fn compose_down(project_dir: &Path, env_file: &Path) -> crate::Result<()> {
    run_compose(project_dir, env_file, &["down"], &[]).await?;

    Ok(())
}

/// Stream `docker compose logs` to stdout, optionally following a single service.
///
/// # Errors
///
/// Returns `Error::Docker` if the command fails.
pub async fn compose_logs(
    project_dir: &Path,
    env_file: &Path,
    service: Option<&str>,
    follow: bool,
) -> crate::Result<()> {
    let mut args = vec!["logs"];

    if follow {
        args.push("--follow");
    }

    if let Some(svc) = service {
        args.push(svc);
    }

    // Stream output directly to the terminal
    let status = Command::new("docker")
        .args(["compose", "-f", "compose.gen.yaml", "--env-file"])
        .arg(env_file)
        .args(&args)
        .current_dir(project_dir)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    if !status.success() {
        return Err(Error::Docker {
            message: format!("compose logs exited with {status}"),
        });
    }

    Ok(())
}

/// Stream `docker compose ps` to the terminal for a running project.
///
/// # Errors
///
/// Returns `Error::Docker` if the command fails.
pub async fn compose_status(project_dir: &Path, env_file: &Path) -> crate::Result<()> {
    let status = Command::new("docker")
        .args(["compose", "-f", "compose.gen.yaml", "--env-file"])
        .arg(env_file)
        .arg("ps")
        .current_dir(project_dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    if !status.success() {
        return Err(Error::Docker {
            message: format!("compose ps exited with {status}"),
        });
    }

    Ok(())
}

/// Return `docker compose ps` output as a JSON string.
///
/// # Errors
///
/// Returns `Error::Docker` if the command fails.
pub async fn compose_ps(project_dir: &Path, env_file: &Path) -> crate::Result<String> {
    let output = run_compose(project_dir, env_file, &["ps", "--format", "json"], &[]).await?;

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Remove all Docker volumes matching `<product>_*`.
///
/// Uses `docker volume ls/rm` directly instead of `docker compose down -v`
/// because the env file may not exist at wipe time.
///
/// # Errors
///
/// Returns `Error::Docker` if volume removal fails.
pub async fn wipe_volumes(product: &str) -> crate::Result<()> {
    let filter = format!("name={product}_");

    let output = Command::new("docker")
        .args(["volume", "ls", "-q", "--filter", &filter])
        .output()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let volumes: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

    if volumes.is_empty() {
        return Ok(());
    }

    let status = Command::new("docker")
        .args(["volume", "rm"])
        .args(&volumes)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    if !status.success() {
        return Err(Error::Docker {
            message: "failed to remove volumes".to_owned(),
        });
    }

    Ok(())
}

/// List Docker volumes belonging to a project (named `<project>_*`).
///
/// # Errors
///
/// Returns `Error::Docker` if the listing fails.
pub async fn list_volumes(project: &str) -> crate::Result<Vec<String>> {
    let filter = format!("name={project}_");

    let output = Command::new("docker")
        .args(["volume", "ls", "-q", "--filter", &filter])
        .output()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    if !output.status.success() {
        return Err(Error::Docker {
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

/// Archive a volume's contents to `<dest_dir>/<archive_name>` as a gzip tarball,
/// using a throwaway `alpine` container.
///
/// # Errors
///
/// Returns `Error::Docker` if the archive container fails.
pub async fn archive_volume(volume: &str, dest_dir: &Path, archive_name: &str) -> crate::Result<()> {
    let volume_mount = format!("{volume}:/data:ro");
    let backup_mount = format!("{}:/backup", dest_dir.display());
    let target = format!("/backup/{archive_name}");

    run_helper_container(&[
        "-v",
        volume_mount.as_str(),
        "-v",
        backup_mount.as_str(),
        "alpine",
        "tar",
        "czf",
        target.as_str(),
        "-C",
        "/data",
        ".",
    ])
    .await
}

/// Replace a volume's contents from `<src_dir>/<archive_name>` (destructive).
///
/// # Errors
///
/// Returns `Error::Docker` if the restore container fails.
pub async fn extract_volume(volume: &str, src_dir: &Path, archive_name: &str) -> crate::Result<()> {
    let volume_mount = format!("{volume}:/data");
    let backup_mount = format!("{}:/backup", src_dir.display());
    let script = format!(
        "set -e; rm -rf /data/* /data/..?* /data/.[!.]* 2>/dev/null || true; tar xzf /backup/{archive_name} -C /data"
    );

    run_helper_container(&[
        "-v",
        volume_mount.as_str(),
        "-v",
        backup_mount.as_str(),
        "alpine",
        "sh",
        "-c",
        script.as_str(),
    ])
    .await
}

/// Run a throwaway `docker run --rm` helper container with the given arguments.
async fn run_helper_container(args: &[&str]) -> crate::Result<()> {
    let output = Command::new("docker")
        .args(["run", "--rm"])
        .args(args)
        .output()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    if !output.status.success() {
        return Err(Error::Docker {
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(())
}

/// Shared helper for all `docker compose` subcommands.
async fn run_compose(
    project_dir: &Path,
    env_file: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> crate::Result<Output> {
    let mut cmd = Command::new("docker");

    cmd.args(["compose", "-f", "compose.gen.yaml", "--env-file"])
        .arg(env_file)
        .args(args)
        .current_dir(project_dir);

    for (key, val) in extra_env {
        cmd.env(key, val);
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| Error::Docker { message: e.to_string() })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        return Err(Error::Docker {
            message: stderr.trim().to_owned(),
        });
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{join_service_images, parse_exposed_ports, parse_image_ids, parse_ps_services};

    #[test]
    fn parse_exposed_ports_keeps_tcp_sorted() {
        assert_eq!(
            parse_exposed_ports(r#"{"443/tcp":{},"80/tcp":{},"53/udp":{}}"#),
            vec![80, 443]
        );
    }

    #[test]
    fn parse_exposed_ports_handles_none() {
        assert!(parse_exposed_ports("null").is_empty());
        assert!(parse_exposed_ports("").is_empty());
    }

    #[test]
    fn parse_image_ids_reads_array() {
        let json = r#"[{"ID":"sha256:eaa","ContainerName":"nginx-nginx-1","Repository":"nginx","Tag":"latest"}]"#;

        assert_eq!(
            parse_image_ids(json),
            vec![("nginx-nginx-1".to_owned(), "sha256:eaa".to_owned())]
        );
    }

    #[test]
    fn parse_ps_services_reads_ndjson() {
        let json = "{\"Name\":\"runa-app-1\",\"Service\":\"app\"}\n{\"Name\":\"runa-db-1\",\"Service\":\"db\"}\n";

        assert_eq!(
            parse_ps_services(json),
            vec![
                ("runa-app-1".to_owned(), "app".to_owned()),
                ("runa-db-1".to_owned(), "db".to_owned()),
            ]
        );
    }

    #[test]
    fn join_service_images_maps_service_to_id() {
        let images = vec![
            ("runa-app-1".to_owned(), "sha256:app".to_owned()),
            ("runa-db-1".to_owned(), "sha256:db".to_owned()),
        ];
        let services = vec![
            ("runa-app-1".to_owned(), "app".to_owned()),
            ("runa-db-1".to_owned(), "db".to_owned()),
        ];

        let mut joined = join_service_images(&images, &services);
        joined.sort();

        assert_eq!(
            joined,
            vec![
                ("app".to_owned(), "sha256:app".to_owned()),
                ("db".to_owned(), "sha256:db".to_owned()),
            ]
        );
    }
}
