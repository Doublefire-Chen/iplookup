//! Logging: `tracing` to stdout.
//!
//! `RUST_LOG`, when set, decides everything. Otherwise the config's
//! `log_level` applies to iplookup's own events and other crates log only
//! warnings. `info` covers startup; `debug` adds one line per request.

use std::io::IsTerminal;

use tracing::Level;
use tracing_subscriber::EnvFilter;

pub fn init(level: Level) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("warn,iplookup={level}")));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stdout)
        // No colour escapes in the systemd journal or other non-terminals.
        .with_ansi(std::io::stdout().is_terminal())
        .init();
}
