use std::path::Path;

use tokio::process::Command;

use crate::config::{Hook, HookType};
use crate::orchestrator::secrets;

/// Execute post-start lifecycle hooks in order.
///
/// Currently only `HookType::Sql` is supported. Each hook runs a SQL command
/// inside a running container via `docker compose exec`.
///
/// # Errors
///
/// Returns `Error::HookFailed` if any hook's command exits with a non-zero status.
pub async fn run_hooks(
    hooks: &[Hook],
    project_dir: &Path,
    env_file: &Path,
    env_vars: &[(String, String)],
) -> crate::Result<()> {
    for hook in hooks {
        match hook.hook_type {
            HookType::Sql => run_sql_hook(hook, project_dir, env_file, env_vars).await?,
        }
    }

    Ok(())
}

/// Execute a single SQL hook against a container's psql.
async fn run_sql_hook(
    hook: &Hook,
    project_dir: &Path,
    env_file: &Path,
    env_vars: &[(String, String)],
) -> crate::Result<()> {
    let Some(sql_template) = &hook.sql else {
        return Ok(());
    };

    let database = hook.database.as_deref().unwrap_or("postgres");
    let sql = resolve_template(sql_template, env_vars);

    let status = Command::new("docker")
        .args(["compose", "-f", "compose.gen.yaml", "--env-file"])
        .arg(env_file)
        .args([
            "exec",
            "-T",
            &hook.target,
            "psql",
            "-U",
            "postgres",
            "-d",
            database,
            "-c",
            &sql,
        ])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| crate::Error::HookFailed {
            name: hook.name.clone(),
            message: e.to_string(),
        })?;

    if !status.success() {
        return Err(crate::Error::HookFailed {
            name: hook.name.clone(),
            message: format!("psql exited with {status}"),
        });
    }

    Ok(())
}

/// Replace template variables in SQL with values from env vars.
///
/// Substitutes `$AUTH_CLIENT_ID`, `$AUTH_CLIENT_SECRET_HASH`, `$REDIRECT_URI`,
/// and `$BASE_URL` with computed values.
fn resolve_template(sql: &str, env_vars: &[(String, String)]) -> String {
    let lookup = |key: &str| -> String {
        env_vars
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };

    let auth_client_id = lookup("AUTH_CLIENT_ID");
    let auth_client_secret = lookup("AUTH_CLIENT_SECRET");
    let base_url = lookup("BASE_URL");

    let auth_client_secret_hash = secrets::hash_secret(&auth_client_secret);
    let redirect_uri = format!("{base_url}/api/auth/oauth2/callback/omni");

    sql.replace("$AUTH_CLIENT_ID", &auth_client_id)
        .replace("$AUTH_CLIENT_SECRET_HASH", &auth_client_secret_hash)
        .replace("$REDIRECT_URI", &redirect_uri)
        .replace("$BASE_URL", &base_url)
}
