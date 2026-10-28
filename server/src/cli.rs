//! Command line: `iplookup [--config FILE] serve` and
//! `iplookup [--config FILE] update [--data-dir DIR]`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};

use crate::app::{self, AppState};
use crate::config::Config;
use crate::databases::{DataSource, DatabaseReader, MmdbDatabases};
use crate::logging;
use crate::reverse_dns::HickoryPtr;
use crate::updater::Updater;

#[derive(Debug, Parser)]
#[command(
    name = "iplookup",
    version,
    about = "A self-hosted, ipinfo.io-compatible IP lookup service"
)]
pub struct Cli {
    /// Path to the TOML config file.
    #[arg(
        short,
        long,
        global = true,
        env = "IPLOOKUP_CONFIG",
        default_value = "iplookup.toml"
    )]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the HTTP server (downloading missing Data Sources first, and
    /// keeping them up to date in the background).
    Serve,
    /// Check both Data Sources for new data once, now.
    Update {
        /// Update this directory instead of the config's `[data] dir`.
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let mut config = Config::load(&cli.config)?;
    logging::init(config.log_level);
    match cli.command {
        Command::Serve => serve(config).await,
        Command::Update { data_dir } => {
            if let Some(dir) = data_dir {
                config.data_dir = dir;
            }
            update(config).await
        }
    }
}

/// One check of each Data Source, reported on stdout, one line each.
async fn update(config: Config) -> anyhow::Result<()> {
    let updater = Updater::new(&config)?;
    let mut failed = 0;
    for source in DataSource::ALL {
        let path = config.data_dir.join(source.file_name());
        match updater.check(source, None).await {
            Ok(outcome) => println!("{source}: {outcome} ({})", path.display()),
            Err(e) => {
                failed += 1;
                println!("{source}: failed, current file kept: {e:#}");
            }
        }
    }
    anyhow::ensure!(failed == 0, "{failed} of 2 Data Sources failed to update");
    Ok(())
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let updater = Updater::new(&config)?;
    updater.download_missing().await?;
    let databases = Arc::new(MmdbDatabases::open(&config.data_dir)?);
    for source in DataSource::ALL {
        if let Some(at) = updater.last_checked(source) {
            databases.set_checked(source, at);
        }
    }
    let info = databases.info();
    for source in [&info.location, &info.network] {
        tracing::info!(
            "loaded {} from {} (built {})",
            source.source,
            source.path.display(),
            source.build
        );
    }
    let ptr = HickoryPtr::new(config.dns_resolver, config.dns_timeout)?;
    match config.dns_resolver {
        Some(addr) => tracing::info!("reverse DNS via {addr}"),
        None => tracing::info!("reverse DNS via the system resolver"),
    }
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))?;
    let addr = listener.local_addr()?;
    let router = app::router(AppState::new(&config, databases.clone(), Arc::new(ptr)));
    tokio::spawn(updater.run(databases));
    // Printed (not just logged) so scripts and tests can find the port.
    println!("iplookup {} listening on http://{addr}", crate::VERSION);
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
