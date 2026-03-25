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
            Commands::Down { product, fresh } => cmd_down(&product, fresh).await,
            Commands::Ls => cmd_ls(),
            Commands::Logs {
                product,
                service,
                follow,
            } => cmd_logs(&product, service.as_deref(), follow).await,
            Commands::Plan { product } => cmd_plan(&product),
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

async fn cmd_down(product: &str, fresh: bool) -> Result<(), Box<dyn std::error::Error>> {
    run_core::orchestrator::down(product, fresh).await?;

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
    let dirs = ProjectDirs::new(product);
    let env_file = dirs.env_file();

    if !secrets::env_file_exists(&env_file) {
        return Err(run_core::Error::ProjectNotInitialized(product.to_owned()).into());
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
