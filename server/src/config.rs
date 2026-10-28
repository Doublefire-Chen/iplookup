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
//! # How often the Data Sources are checked for new data.
//! update_interval = "12h"
//!
//! [maxmind]
//! # GeoLite2 City (Location Data). Without these it is never downloaded.
//! account_id = 123456
//! license_key = "…"
//!
//! [ipinfo]
//! # IPinfo Lite (Network Data). Without it, it is never downloaded.
//! token = "…"
//!
//! [dns]
//! # Asked for reverse DNS (port 53 unless given); the system resolver
//! # when unset.
//! resolver = "127.0.0.1:53"
//! timeout = "500ms"
//!
//! [rate_limit]
//! # Explicit Lookups per client (IPv4 address or IPv6 /64): a token
//! # bucket holding `burst` tokens, refilled at `per_second`.
//! burst = 60
//! per_second = 1
//! # Clients never limited (IPs or CIDRs).
//! allow = ["192.0.2.0/24"]
//!
//! [frontend]
//! # The built web pages (`pnpm build` output: index.html and assets/).
//! dir = "web"
//! ```
//!
//! Every key is optional. Relative paths are resolved against the directory
//! containing the config file, so the server behaves the same whatever its
//! working directory is (e.g. under systemd).
//!
//! Credentials live only in this file (keep it mode 0600). They are
//! wrapped in [`Secret`], whose `Debug` output hides them, so they never
//! reach a log.

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
/// How often the Data Sources are checked for new data.
pub const DEFAULT_UPDATE_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
/// Where GeoLite2 City is downloaded from.
pub const DEFAULT_MAXMIND_URL: &str = "https://download.maxmind.com";
/// Where IPinfo Lite is downloaded from.
pub const DEFAULT_IPINFO_URL: &str = "https://ipinfo.io";
/// How many Explicit Lookups a client may make in a row.
pub const DEFAULT_RATE_LIMIT_BURST: u32 = 60;
/// How many Explicit Lookups per second a client gets back.
pub const DEFAULT_RATE_LIMIT_PER_SECOND: f64 = 1.0;

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
    /// How often the updater checks each Data Source for new data.
    pub update_interval: Duration,
    pub maxmind: MaxMind,
    pub ipinfo: Ipinfo,
    pub rate_limit: RateLimitConfig,
    /// The built web pages: `index.html` (into which Records are injected)
    /// and the `assets/` it loads. Read on every page request, so a new
    /// build shows up without a restart.
    pub frontend_dir: PathBuf,
}

/// The token bucket every client gets for Explicit Lookups.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Tokens in a full bucket (at least 1).
    pub burst: u32,
    /// Tokens added per second (positive, may be fractional).
    pub per_second: f64,
    /// Clients that are never limited.
    pub allow: Vec<IpNet>,
}

/// Where and how GeoLite2 City (Location Data) is downloaded.
#[derive(Debug, Clone)]
pub struct MaxMind {
    /// `None` when not configured: the file is then never downloaded.
    pub credentials: Option<MaxMindCredentials>,
    /// Scheme and host of MaxMind's download API (overridden in tests).
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct MaxMindCredentials {
    pub account_id: String,
    pub license_key: Secret,
}

/// Where and how IPinfo Lite (Network Data) is downloaded.
#[derive(Debug, Clone)]
pub struct Ipinfo {
    /// `None` when not configured: the file is then never downloaded.
    pub token: Option<Secret>,
    /// Scheme and host of IPinfo's download URL (overridden in tests).
    pub base_url: String,
}

/// A credential. Its `Debug` output is `"***"`, so it can't be logged by
/// accident; [`Secret::expose`] is the only way to read it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("\"***\"")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    listen: Option<SocketAddr>,
    trusted_proxies: Option<Vec<String>>,
    log_level: Option<String>,
    data: Option<RawData>,
    dns: Option<RawDns>,
    maxmind: Option<RawMaxMind>,
    ipinfo: Option<RawIpinfo>,
    rate_limit: Option<RawRateLimit>,
    frontend: Option<RawFrontend>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFrontend {
    dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRateLimit {
    burst: Option<u32>,
    per_second: Option<f64>,
    allow: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawData {
    dir: Option<PathBuf>,
    update_interval: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMaxMind {
    account_id: Option<AccountId>,
    license_key: Option<String>,
    base_url: Option<String>,
}

/// MaxMind Account IDs are numbers; either `123456` or `"123456"` works.
#[derive(Deserialize)]
#[serde(untagged)]
enum AccountId {
    Number(u64),
    Text(String),
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIpinfo {
    token: Option<String>,
    base_url: Option<String>,
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
            .map(|s| parse_net("trusted_proxies", s))
            .collect::<anyhow::Result<_>>()?;
        let log_level = match raw.log_level {
            Some(s) => s.parse().map_err(|_| {
                anyhow::anyhow!(
                    "log_level must be one of error, warn, info, debug or trace, not {s:?}"
                )
            })?,
            None => Level::INFO,
        };
        let data = raw.data.unwrap_or_default();
        let data_dir = data.dir.unwrap_or("data".into());
        let update_interval = match data.update_interval.as_deref() {
            Some(s) => parse_duration("data.update_interval", s, "12h")?,
            None => DEFAULT_UPDATE_INTERVAL,
        };
        let dns = raw.dns.unwrap_or_default();
        let dns_resolver = dns.resolver.as_deref().map(parse_resolver).transpose()?;
        let dns_timeout = match dns.timeout.as_deref() {
            Some(s) => parse_duration("dns.timeout", s, "500ms")?,
            None => DEFAULT_DNS_TIMEOUT,
        };
        let maxmind = raw.maxmind.unwrap_or_default();
        let credentials = match (maxmind.account_id, maxmind.license_key) {
            (None, None) => None,
            (Some(id), Some(key)) => Some(MaxMindCredentials {
                account_id: match id {
                    AccountId::Number(n) => n.to_string(),
                    AccountId::Text(s) => s,
                },
                license_key: Secret::new(key),
            }),
            _ => anyhow::bail!("maxmind: set both account_id and license_key, or neither"),
        };
        let maxmind = MaxMind {
            credentials,
            base_url: parse_base_url("maxmind.base_url", maxmind.base_url, DEFAULT_MAXMIND_URL)?,
        };
        let ipinfo = raw.ipinfo.unwrap_or_default();
        let ipinfo = Ipinfo {
            token: ipinfo.token.map(Secret::new),
            base_url: parse_base_url("ipinfo.base_url", ipinfo.base_url, DEFAULT_IPINFO_URL)?,
        };
        let rate_limit = raw.rate_limit.unwrap_or_default();
        let rate_limit = RateLimitConfig {
            burst: match rate_limit.burst {
                Some(0) => anyhow::bail!("rate_limit.burst must be at least 1"),
                Some(n) => n,
                None => DEFAULT_RATE_LIMIT_BURST,
            },
            per_second: match rate_limit.per_second {
                Some(r) if !(r.is_finite() && r > 0.0) => {
                    anyhow::bail!("rate_limit.per_second must be a positive number, not {r}")
                }
                Some(r) => r,
                None => DEFAULT_RATE_LIMIT_PER_SECOND,
            },
            allow: rate_limit
                .allow
                .unwrap_or_default()
                .iter()
                .map(|s| parse_net("rate_limit.allow", s))
                .collect::<anyhow::Result<_>>()?,
        };
        let frontend_dir = raw.frontend.unwrap_or_default().dir.unwrap_or("web".into());
        Ok(Self {
            listen: raw
                .listen
                .unwrap_or(DEFAULT_LISTEN.parse().expect("valid default address")),
            trusted_proxies,
            log_level,
            data_dir: base.join(data_dir),
            dns_resolver,
            dns_timeout,
            update_interval,
            maxmind,
            ipinfo,
            rate_limit,
            frontend_dir: base.join(frontend_dir),
        })
    }
}

/// Parses an IP address (as a single-address network) or a CIDR.
fn parse_net(key: &str, s: &str) -> anyhow::Result<IpNet> {
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Ok(IpNet::from(ip));
    }
    s.parse::<IpNet>()
        .map_err(|_| anyhow::anyhow!("{key}: {s:?} is not an IP address or CIDR (e.g. 10.0.0.0/8)"))
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

/// Parses a positive duration such as `500ms`, `2s` or `12h`.
fn parse_duration(key: &str, s: &str, example: &str) -> anyhow::Result<Duration> {
    match humantime::parse_duration(s) {
        Ok(d) if !d.is_zero() => Ok(d),
        _ => anyhow::bail!("{key}: {s:?} is not a positive duration (e.g. \"{example}\")"),
    }
}

/// An `http(s)://host[:port]` URL without a trailing slash.
fn parse_base_url(key: &str, value: Option<String>, default: &str) -> anyhow::Result<String> {
    let Some(url) = value else {
        return Ok(default.to_owned());
    };
    anyhow::ensure!(
        url.starts_with("https://") || url.starts_with("http://"),
        "{key}: {url:?} is not an http(s) URL"
    );
    Ok(url.trim_end_matches('/').to_owned())
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
        assert_eq!(c.update_interval, Duration::from_secs(12 * 60 * 60));
        assert!(c.maxmind.credentials.is_none());
        assert_eq!(c.maxmind.base_url, DEFAULT_MAXMIND_URL);
        assert!(c.ipinfo.token.is_none());
        assert_eq!(c.ipinfo.base_url, DEFAULT_IPINFO_URL);
        assert_eq!(c.rate_limit.burst, 60);
        assert_eq!(c.rate_limit.per_second, 1.0);
        assert!(c.rate_limit.allow.is_empty());
        assert_eq!(c.frontend_dir, Path::new("/srv/iplookup/web"));
    }

    #[test]
    fn every_key_is_read() {
        let c = Config::parse(
            "listen = \"[::1]:9000\"\ntrusted_proxies = [\"127.0.0.1\", \"10.0.0.0/8\", \"::1\"]\nlog_level = \"debug\"\n[data]\ndir = \"/var/lib/iplookup\"\nupdate_interval = \"6h\"\n[dns]\nresolver = \"127.0.0.1:5353\"\ntimeout = \"2s\"\n[maxmind]\naccount_id = 123456\nlicense_key = \"mm-key\"\nbase_url = \"http://127.0.0.1:1/\"\n[ipinfo]\ntoken = \"ii-token\"\nbase_url = \"http://127.0.0.1:2\"\n[rate_limit]\nburst = 5\nper_second = 0.5\nallow = [\"203.0.113.7\", \"2001:db8::/32\"]\n[frontend]\ndir = \"/opt/iplookup/web\"\n",
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
        assert_eq!(c.update_interval, Duration::from_secs(6 * 60 * 60));
        let mm = c.maxmind.credentials.as_ref().unwrap();
        assert_eq!(mm.account_id, "123456");
        assert_eq!(mm.license_key.expose(), "mm-key");
        assert_eq!(c.maxmind.base_url, "http://127.0.0.1:1");
        assert_eq!(c.ipinfo.token.as_ref().unwrap().expose(), "ii-token");
        assert_eq!(c.ipinfo.base_url, "http://127.0.0.1:2");
        assert_eq!(c.rate_limit.burst, 5);
        assert_eq!(c.rate_limit.per_second, 0.5);
        let allow: Vec<String> = c.rate_limit.allow.iter().map(|n| n.to_string()).collect();
        assert_eq!(allow, ["203.0.113.7/32", "2001:db8::/32"]);
        assert_eq!(c.frontend_dir, Path::new("/opt/iplookup/web"));
    }

    #[test]
    fn rate_limit_per_second_may_be_an_integer() {
        let c = Config::parse("[rate_limit]\nper_second = 2", Path::new("/")).unwrap();
        assert_eq!(c.rate_limit.per_second, 2.0);
    }

    #[test]
    fn account_id_may_be_text() {
        let c = Config::parse(
            "[maxmind]\naccount_id = \"42\"\nlicense_key = \"k\"",
            Path::new("/"),
        )
        .unwrap();
        assert_eq!(c.maxmind.credentials.unwrap().account_id, "42");
    }

    #[test]
    fn credentials_never_show_in_debug_output() {
        let c = Config::parse(
            "[maxmind]\naccount_id = 1\nlicense_key = \"mm-secret\"\n[ipinfo]\ntoken = \"ii-secret\"",
            Path::new("/"),
        )
        .unwrap();
        let debug = format!("{c:?}");
        assert!(
            !debug.contains("mm-secret") && !debug.contains("ii-secret"),
            "{debug}"
        );
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
            "[data]\nupdate_interval = \"daily\"",
            "[data]\nupdate_interval = \"0s\"",
            "[maxmind]\nlicense_key = \"k\"",
            "[maxmind]\naccount_id = 1",
            "[maxmind]\nbase_url = \"download.maxmind.com\"",
            "[ipinfo]\ntokne = \"t\"",
            "[rate_limit]\nburst = 0",
            "[rate_limit]\nburst = -1",
            "[rate_limit]\nper_second = 0",
            "[rate_limit]\nper_second = -1.5",
            "[rate_limit]\nper_second = \"1/s\"",
            "[rate_limit]\nallow = [\"office\"]",
            "[rate_limit]\nalow = []",
            "[frontend]\ndri = \"web\"",
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
