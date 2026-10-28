//! Server configuration, read from a TOML file.
//!
//! ```toml
//! listen = "127.0.0.1:18310"
//! # Peers whose X-Real-IP header is believed (IPs or CIDRs).
//! trusted_proxies = ["127.0.0.1", "::1"]
//! log_level = "info"
//!
//! [data]
//! dir = "data"
//!
//! [dns]
//! # Asked for reverse DNS (port 53 unless given); the system resolver
//! # when unset.
//! resolver = "127.0.0.1:53"
//! timeout = "500ms"
//! ```
//!
//! Every key is optional. Relative paths are resolved against the directory
//! containing the config file, so the server behaves the same whatever its
//! working directory is (e.g. under systemd).

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use ipnet::IpNet;
use serde::Deserialize;
use tracing::Level;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:18310";
/// How long a Lookup waits for reverse DNS before leaving `hostname` out.
pub const DEFAULT_DNS_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct Config {
    pub listen: SocketAddr,
    /// Peers whose `X-Real-IP` header names the client. Empty by default:
    /// the socket peer is the client.
    pub trusted_proxies: Vec<IpNet>,
    /// Level for iplookup's own log events (overridden by `RUST_LOG`).
    pub log_level: Level,
    /// Holds the Data Sources: `GeoLite2-City.mmdb` and `ipinfo_lite.mmdb`.
    pub data_dir: PathBuf,
    /// The resolver asked for PTR names; `None` means the system resolver.
    pub dns_resolver: Option<SocketAddr>,
    /// The hard limit on a reverse-DNS answer.
    pub dns_timeout: Duration,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    listen: Option<SocketAddr>,
    trusted_proxies: Option<Vec<String>>,
    log_level: Option<String>,
    data: Option<RawData>,
    dns: Option<RawDns>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawData {
    dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDns {
    resolver: Option<String>,
    timeout: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let base = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Self::parse(&text, base).with_context(|| format!("parsing config file {}", path.display()))
    }

    pub fn parse(text: &str, base: &Path) -> anyhow::Result<Self> {
        let raw: RawConfig = toml::from_str(text)?;
        let trusted_proxies = raw
            .trusted_proxies
            .unwrap_or_default()
            .iter()
            .map(|s| parse_net(s))
            .collect::<anyhow::Result<_>>()?;
        let log_level = match raw.log_level {
            Some(s) => s.parse().map_err(|_| {
                anyhow::anyhow!(
                    "log_level must be one of error, warn, info, debug or trace, not {s:?}"
                )
            })?,
            None => Level::INFO,
        };
        let data_dir = raw.data.and_then(|d| d.dir).unwrap_or("data".into());
        let dns = raw.dns.unwrap_or_default();
        let dns_resolver = dns.resolver.as_deref().map(parse_resolver).transpose()?;
        let dns_timeout = match dns.timeout.as_deref() {
            Some(s) => parse_timeout(s)?,
            None => DEFAULT_DNS_TIMEOUT,
        };
        Ok(Self {
            listen: raw
                .listen
                .unwrap_or(DEFAULT_LISTEN.parse().expect("valid default address")),
            trusted_proxies,
            log_level,
            data_dir: base.join(data_dir),
            dns_resolver,
            dns_timeout,
        })
    }
}

/// Parses an IP address (as a single-address network) or a CIDR.
fn parse_net(s: &str) -> anyhow::Result<IpNet> {
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Ok(IpNet::from(ip));
    }
    s.parse::<IpNet>().map_err(|_| {
        anyhow::anyhow!("trusted_proxies: {s:?} is not an IP address or CIDR (e.g. 10.0.0.0/8)")
    })
}

/// Parses `ip:port`, `[ipv6]:port` or a bare IP address (port 53).
fn parse_resolver(s: &str) -> anyhow::Result<SocketAddr> {
    if let Ok(addr) = s.parse::<SocketAddr>() {
        return Ok(addr);
    }
    s.parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, 53))
        .map_err(|_| {
            anyhow::anyhow!(
                "dns.resolver: {s:?} is not an IP address, optionally with a port (e.g. 127.0.0.1:53)"
            )
        })
}

/// Parses a positive duration such as `500ms` or `2s`.
fn parse_timeout(s: &str) -> anyhow::Result<Duration> {
    match humantime::parse_duration(s) {
        Ok(d) if !d.is_zero() => Ok(d),
        _ => anyhow::bail!("dns.timeout: {s:?} is not a positive duration (e.g. \"500ms\")"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_uses_defaults() {
        let c = Config::parse("", Path::new("/srv/iplookup")).unwrap();
        assert_eq!(c.listen.to_string(), DEFAULT_LISTEN);
        assert!(c.trusted_proxies.is_empty());
        assert_eq!(c.log_level, Level::INFO);
        assert_eq!(c.data_dir, Path::new("/srv/iplookup/data"));
        assert_eq!(c.dns_resolver, None);
        assert_eq!(c.dns_timeout, Duration::from_millis(500));
    }

    #[test]
    fn every_key_is_read() {
        let c = Config::parse(
            "listen = \"[::1]:9000\"\ntrusted_proxies = [\"127.0.0.1\", \"10.0.0.0/8\", \"::1\"]\nlog_level = \"debug\"\n[data]\ndir = \"/var/lib/iplookup\"\n[dns]\nresolver = \"127.0.0.1:5353\"\ntimeout = \"2s\"\n",
            Path::new("/etc/iplookup"),
        )
        .unwrap();
        assert_eq!(c.listen.to_string(), "[::1]:9000");
        let nets: Vec<String> = c.trusted_proxies.iter().map(|n| n.to_string()).collect();
        assert_eq!(nets, ["127.0.0.1/32", "10.0.0.0/8", "::1/128"]);
        assert_eq!(c.log_level, Level::DEBUG);
        assert_eq!(c.data_dir, Path::new("/var/lib/iplookup"));
        assert_eq!(c.dns_resolver, Some("127.0.0.1:5353".parse().unwrap()));
        assert_eq!(c.dns_timeout, Duration::from_secs(2));
    }

    #[test]
    fn dns_resolver_port_defaults_to_53() {
        for (text, addr) in [
            ("10.0.0.1", "10.0.0.1:53"),
            ("::1", "[::1]:53"),
            ("[::1]:5353", "[::1]:5353"),
        ] {
            let c =
                Config::parse(&format!("[dns]\nresolver = \"{text}\""), Path::new("/")).unwrap();
            assert_eq!(c.dns_resolver, Some(addr.parse().unwrap()), "{text}");
        }
    }

    #[test]
    fn relative_data_dir_resolves_against_config_dir() {
        let c = Config::parse("[data]\ndir = \"db\"", Path::new("/etc/iplookup")).unwrap();
        assert_eq!(c.data_dir, Path::new("/etc/iplookup/db"));
    }

    #[test]
    fn bad_values_and_unknown_keys_are_rejected() {
        for bad in [
            "lsiten = \"x\"",
            "trusted_proxies = [\"proxy.local\"]",
            "log_level = \"loud\"",
            "[data]\ndri = \"x\"",
            "[dns]\nresolver = \"dns.google\"",
            "[dns]\ntimeout = \"soon\"",
            "[dns]\ntimeout = \"0ms\"",
            "[dns]\ntimeout = 500",
            "[dns]\nresovler = \"127.0.0.1\"",
        ] {
            assert!(Config::parse(bad, Path::new("/")).is_err(), "{bad}");
        }
    }

    #[test]
    fn deploy_sample_parses() {
        let c = Config::parse(
            include_str!("../../deploy/iplookup.toml"),
            Path::new("/etc/iplookup"),
        )
        .unwrap();
        assert!(c.listen.ip().is_loopback());
    }
}
