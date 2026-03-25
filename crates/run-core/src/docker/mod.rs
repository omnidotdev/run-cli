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
