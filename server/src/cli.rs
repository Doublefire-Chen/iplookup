//! Command line: `iplookup [--config FILE] serve`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};

use crate::app::{self, AppState};
use crate::config::Config;
use crate::databases::{DatabaseReader, MmdbDatabases};
use crate::logging;
use crate::reverse_dns::HickoryPtr;

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
    /// Run the HTTP server.
    Serve,
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let config = Config::load(&cli.config)?;
    logging::init(config.log_level);
    match cli.command {
        Command::Serve => serve(config).await,
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let databases = MmdbDatabases::open(&config.data_dir)?;
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
    let router = app::router(AppState::new(&config, Arc::new(databases), Arc::new(ptr)));
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
