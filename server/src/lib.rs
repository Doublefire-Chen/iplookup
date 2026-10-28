//! iplookup: a self-hosted, ipinfo.io-compatible IP lookup service.
//!
//! The binary (`main.rs`) is a thin wrapper around [`cli::run`]. Everything
//! else lives here so integration tests can build the whole app in-process
//! (see `tests/common`).

pub mod app;
pub mod bogon;
pub mod cli;
pub mod client;
pub mod config;
pub mod databases;
pub mod logging;
pub mod lookup;
pub mod record;

/// The server version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where every Record's `readme` key points: the project README on GitHub,
/// the same for every deployment (ADR 0001).
pub const README_URL: &str = "https://github.com/Doublefire-Chen/iplookup/blob/main/README.md";
