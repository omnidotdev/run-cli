use clap::{Parser, Subcommand};
use run_core::catalog::Catalog;
use run_core::config::state::ProjectDirs;
use run_core::orchestrator::secrets;

/// Run -- self-hosting made simple
#[derive(Parser)]
#[command(name = "run", version, about, args_conflicts_with_subcommands = true)]
struct Cli {
    /// Product to start (shorthand for `run up <product>`)
    product: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a product stack
    Up {
        /// Product name (e.g. "runa")
        product: String,
    },
    /// Stop a product stack
    Down {
        /// Product name
        product: String,
        /// Remove all data (volumes, secrets, certs)
        #[arg(long)]
        fresh: bool,
        /// Skip the automatic pre-wipe backup (only meaningful with --fresh)
        #[arg(long)]
        no_backup: bool,
    },
    /// List available products
    Ls,
    /// Tail container logs
    Logs {
        /// Product name
        product: String,
        /// Filter to a specific service
        service: Option<String>,
        /// Follow log output
        #[arg(short, long)]
        follow: bool,
    },
    /// Show what would happen without making changes
    Plan {
        /// Product name
        product: String,
    },
    /// Back up a stack's data volumes to a local snapshot
    Backup {
        /// Product name or path
        product: String,
        /// Snapshot name (defaults to a timestamp)
        #[arg(long)]
        name: Option<String>,
    },
    /// Restore a stack's data volumes from a snapshot
    Restore {
        /// Product name or path
        product: String,
        /// Snapshot name (see `run backups`)
        snapshot: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// List a stack's backup snapshots
    Backups {
        /// Product name or path
        product: String,
    },
    /// Show a stack's services, or all stacks when no target is given
    Status {
        /// Product name or path (omit to list every stack)
        product: Option<String>,
    },
    /// Pull the latest images and recreate a stack, rolling back on failure
    Update {
        /// Product name or path
        product: String,
        /// Skip the automatic pre-update backup
        #[arg(long)]
        no_backup: bool,
    },
    /// Emit a fractal.yaml to deploy a stack to Fractal cloud
    Deploy {
        /// Product name or path
        product: String,
        /// Deploy target (currently only "fractal")
        #[arg(long, default_value = "fractal")]
        to: String,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // `run runa` is shorthand for `run up runa`
    let result = if let Some(product) = cli.product {
        cmd_up(&product).await
    } else if let Some(command) = cli.command {
        match command {
            Commands::Up { product } => cmd_up(&product).await,
            Commands::Down {
                product,
                fresh,
                no_backup,
            } => cmd_down(&product, fresh, no_backup).await,
            Commands::Ls => cmd_ls(),
            Commands::Logs {
                product,
                service,
                follow,
            } => cmd_logs(&product, service.as_deref(), follow).await,
            Commands::Plan { product } => cmd_plan(&product),
            Commands::Backup { product, name } => cmd_backup(&product, name.as_deref()).await,
            Commands::Restore { product, snapshot, yes } => cmd_restore(&product, &snapshot, yes).await,
            Commands::Backups { product } => cmd_backups(&product),
            Commands::Status { product } => cmd_status(product.as_deref()).await,
            Commands::Update { product, no_backup } => cmd_update(&product, no_backup).await,
            Commands::Deploy { product, to } => cmd_deploy(&product, &to),
        }
    } else {
        Cli::parse_from(["run", "--help"]);
        Ok(())
    };

    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

#[allow(clippy::future_not_send)] // Catalog is only used synchronously before any awaits
async fn cmd_up(product: &str) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = run_core::catalog::default_catalog();
    run_core::orchestrator::up(product, &catalog).await?;

    Ok(())
}

async fn cmd_down(product: &str, fresh: bool, no_backup: bool) -> Result<(), Box<dyn std::error::Error>> {
    let name = run_core::catalog::project_name(product)?;
    run_core::orchestrator::down(&name, fresh, no_backup).await?;

    Ok(())
}

#[allow(clippy::future_not_send)] // Catalog is only used synchronously before any awaits
async fn cmd_update(product: &str, no_backup: bool) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = run_core::catalog::default_catalog();
    run_core::orchestrator::update(product, &catalog, no_backup).await?;

    Ok(())
}

#[allow(clippy::unnecessary_wraps)] // Uniform return type for dispatch
fn cmd_ls() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = run_core::catalog::default_catalog();

    for product in catalog.list() {
        println!("{product}");
    }

    Ok(())
}

async fn cmd_logs(product: &str, service: Option<&str>, follow: bool) -> Result<(), Box<dyn std::error::Error>> {
    let name = run_core::catalog::project_name(product)?;
    let dirs = ProjectDirs::new(&name);
    let env_file = dirs.env_file();

    if !secrets::env_file_exists(&env_file) {
        return Err(run_core::Error::ProjectNotInitialized(name).into());
    }

    run_core::docker::compose_logs(dirs.root(), &env_file, service, follow).await?;

    Ok(())
}

fn cmd_plan(product: &str) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = run_core::catalog::default_catalog();
    let output = run_core::orchestrator::plan(product, &catalog)?;
    print!("{output}");

    Ok(())
}

async fn cmd_backup(product: &str, name: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let project = run_core::catalog::project_name(product)?;
    let dirs = ProjectDirs::new(&project);

    run_core::docker::check_docker().await?;
    let snapshot = run_core::orchestrator::backup::backup(&project, &dirs.backups_dir(), name).await?;
    println!("Backed up {project} to snapshot {snapshot}");

    Ok(())
}

async fn cmd_restore(product: &str, snapshot: &str, yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    let project = run_core::catalog::project_name(product)?;
    let dirs = ProjectDirs::new(&project);

    if !yes
        && !confirm(&format!(
            "Restore overwrites {project} data from `{snapshot}` and cannot be undone. Continue? [y/N] "
        ))
    {
        println!("Aborted.");
        return Ok(());
    }

    run_core::docker::check_docker().await?;
    run_core::orchestrator::backup::restore(&dirs.backups_dir(), snapshot).await?;
    println!("Restored {project} from {snapshot}");

    Ok(())
}

fn cmd_backups(product: &str) -> Result<(), Box<dyn std::error::Error>> {
    let project = run_core::catalog::project_name(product)?;
    let dirs = ProjectDirs::new(&project);
    let snapshots = run_core::orchestrator::backup::list_snapshots(&dirs.backups_dir())?;

    if snapshots.is_empty() {
        println!("No backups for {project}");
    } else {
        for snapshot in snapshots {
            println!("{snapshot}");
        }
    }

    Ok(())
}

async fn cmd_status(product: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(target) = product {
        let name = run_core::catalog::project_name(target)?;
        run_core::orchestrator::status(&name).await?;
    } else {
        let projects = run_core::orchestrator::list_projects()?;

        if projects.is_empty() {
            println!("No stacks yet. Try `run up uptime-kuma`.");
        } else {
            for (name, status) in projects {
                println!("{name}\t{status:?}");
            }
        }
    }

    Ok(())
}

fn cmd_deploy(product: &str, to: &str) -> Result<(), Box<dyn std::error::Error>> {
    if to != "fractal" {
        return Err(format!("unknown deploy target `{to}` (only `fractal` is supported)").into());
    }

    let catalog = run_core::catalog::default_catalog();
    let manifest = run_core::catalog::resolve(product, &catalog)?;
    let (yaml, warnings) = run_core::fractal::to_fractal_yaml(&manifest)?;

    print!("{yaml}");

    if !warnings.is_empty() {
        eprintln!("\nwarnings:");
        for warning in warnings {
            eprintln!("  - {warning}");
        }
    }

    eprintln!(
        "\nReview the manifest above, then import it at https://fractal.omni.dev (New Project -> Import -> Fractal)."
    );

    Ok(())
}

/// Prompt for a yes/no confirmation. Returns false when non-interactive, so a
/// destructive action never proceeds unattended without `--yes`.
fn confirm(prompt: &str) -> bool {
    use std::io::{IsTerminal, Write};

    if !std::io::stdin().is_terminal() {
        return false;
    }

    eprint!("{prompt}");
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }

    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
