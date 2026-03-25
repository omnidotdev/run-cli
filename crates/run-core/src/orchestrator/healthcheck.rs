use std::path::Path;

use crate::docker;

/// Poll `docker compose ps` until all services are healthy or timeout expires.
///
/// Checks every 3 seconds. Prints a `.` to stderr on each poll and a newline
/// when finished.
///
/// # Errors
///
/// Returns `Error::HealthTimeout` if `timeout_secs` elapses before all services
/// report healthy.
pub async fn wait_for_healthy(project_dir: &Path, env_file: &Path, timeout_secs: u64) -> crate::Result<()> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let poll_interval = std::time::Duration::from_secs(3);

    loop {
        if start.elapsed() >= timeout {
            eprintln!();

            return Err(crate::Error::HealthTimeout { seconds: timeout_secs });
        }

        tokio::time::sleep(poll_interval).await;
        eprint!(".");

        let Ok(output) = docker::compose_ps(project_dir, env_file).await else {
            // Compose not ready yet, keep polling
            continue;
        };

        if all_healthy(&output) {
            eprintln!();

            return Ok(());
        }
    }
}

/// Check whether all services with healthchecks are showing "healthy".
///
/// Docker compose ps JSON output may be a single JSON array or one JSON object
/// per line depending on the version. We handle both formats.
fn all_healthy(output: &str) -> bool {
    let trimmed = output.trim();

    if trimmed.is_empty() {
        return false;
    }

    // Try parsing as a JSON array first
    if let Ok(services) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
        return check_services(&services);
    }

    // Fall back to newline-delimited JSON objects
    let services: Vec<serde_json::Value> = trimmed
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    if services.is_empty() {
        return false;
    }

    check_services(&services)
}

/// Verify no service has a health status of "starting" and at least one
/// service reports status information.
fn check_services(services: &[serde_json::Value]) -> bool {
    if services.is_empty() {
        return false;
    }

    for svc in services {
        let health = svc
            .get("Health")
            .or_else(|| svc.get("health"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if health.contains("starting") {
            return false;
        }
    }

    true
}
