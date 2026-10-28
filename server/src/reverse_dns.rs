//! Reverse DNS: given an IP address, its PTR name or nothing, within a
//! hard timeout.
//!
//! [`PtrResolver`] is the seam: one PTR query, answered with a name (or
//! none) and how long the answer may be cached. [`HickoryPtr`] asks a real
//! resolver; tests use a fake. [`ReverseDns`] wraps either with the timeout
//! and an in-memory cache, so every answer (including "no PTR" and
//! timeouts) is remembered for its TTL, capped at an hour. The name is shown
//! as returned: there is no forward-confirmation.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use hickory_resolver::TokioResolver;
use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ResolverConfig};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::net::{DnsError, NetError};
use hickory_resolver::proto::rr::{Name, RData};
use tokio::time::Instant;

/// The longest any answer is cached, whatever its TTL.
pub const MAX_TTL: Duration = Duration::from_secs(60 * 60);
/// How long an answer without a TTL (a timeout, a failed query, a negative
/// answer without SOA) is cached.
pub const NO_TTL: Duration = Duration::from_secs(5 * 60);
/// Past this many entries, expired ones are dropped (and, if that is not
/// enough, the whole cache) before adding another.
const MAX_ENTRIES: usize = 100_000;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The answer to one PTR query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtrAnswer {
    /// The PTR name without its trailing dot, or `None` when there is none
    /// (or the query failed).
    pub name: Option<String>,
    /// How long the answer may be cached.
    pub ttl: Duration,
}

pub trait PtrResolver: Send + Sync + 'static {
    /// Asks for the PTR record of `ip`. May take arbitrarily long:
    /// [`ReverseDns`] enforces the timeout.
    fn ptr(&self, ip: IpAddr) -> BoxFuture<'_, PtrAnswer>;
}

/// Reverse DNS with a hard timeout and a cache in front of a
/// [`PtrResolver`].
pub struct ReverseDns {
    resolver: Arc<dyn PtrResolver>,
    timeout: Duration,
    cache: Mutex<HashMap<IpAddr, Cached>>,
}

struct Cached {
    name: Option<String>,
    expires: Instant,
}

impl ReverseDns {
    pub fn new(resolver: Arc<dyn PtrResolver>, timeout: Duration) -> Self {
        Self {
            resolver,
            timeout,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The PTR name of `ip`, or `None` when it has none or the resolver
    /// takes longer than the timeout. Callers keep Bogons away from here.
    pub async fn hostname(&self, ip: IpAddr) -> Option<String> {
        let now = Instant::now();
        if let Some(cached) = self.cache.lock().unwrap().get(&ip)
            && cached.expires > now
        {
            return cached.name.clone();
        }
        let answer = match tokio::time::timeout(self.timeout, self.resolver.ptr(ip)).await {
            Ok(answer) => answer,
            Err(_) => {
                tracing::debug!("reverse DNS for {ip} timed out after {:?}", self.timeout);
                PtrAnswer {
                    name: None,
                    ttl: NO_TTL,
                }
            }
        };
        let mut cache = self.cache.lock().unwrap();
        let now = Instant::now();
        if cache.len() >= MAX_ENTRIES {
            cache.retain(|_, c| c.expires > now);
            if cache.len() >= MAX_ENTRIES {
                cache.clear();
            }
        }
        cache.insert(
            ip,
            Cached {
                name: answer.name.clone(),
                expires: now + answer.ttl.min(MAX_TTL),
            },
        );
        answer.name
    }
}

/// PTR queries through hickory-resolver, to the configured resolver or,
/// when none is set, the system's (`/etc/resolv.conf`).
pub struct HickoryPtr {
    resolver: TokioResolver,
}

impl HickoryPtr {
    /// `server` is the resolver to ask (UDP, falling back to TCP); `None`
    /// means the system resolver. `timeout` bounds each query.
    pub fn new(server: Option<SocketAddr>, timeout: Duration) -> anyhow::Result<Self> {
        let mut builder = match server {
            Some(addr) => {
                let connection = |mut c: ConnectionConfig| {
                    c.port = addr.port();
                    c
                };
                let server = NameServerConfig::new(
                    addr.ip(),
                    true,
                    vec![
                        connection(ConnectionConfig::udp()),
                        connection(ConnectionConfig::tcp()),
                    ],
                );
                TokioResolver::builder_with_config(
                    ResolverConfig::from_name_servers(vec![server]),
                    TokioRuntimeProvider::default(),
                )
            }
            None => TokioResolver::builder_tokio().context("reading the system DNS config")?,
        };
        let options = builder.options_mut();
        options.timeout = timeout;
        options.attempts = 0;
        Ok(Self {
            resolver: builder.build().context("building the DNS resolver")?,
        })
    }
}

impl PtrResolver for HickoryPtr {
    fn ptr(&self, ip: IpAddr) -> BoxFuture<'_, PtrAnswer> {
        Box::pin(async move {
            match self.resolver.reverse_lookup(Name::from(ip)).await {
                Ok(lookup) => {
                    let mut ptrs = lookup.answers().iter().filter_map(|r| match &r.data {
                        RData::PTR(ptr) => Some((ptr.0.to_utf8(), r.ttl)),
                        _ => None,
                    });
                    match ptrs.next() {
                        Some((name, ttl)) => PtrAnswer {
                            name: Some(name.trim_end_matches('.').to_owned()),
                            ttl: Duration::from_secs(ttl.into()),
                        },
                        None => PtrAnswer {
                            name: None,
                            ttl: NO_TTL,
                        },
                    }
                }
                Err(NetError::Dns(DnsError::NoRecordsFound(no_records))) => PtrAnswer {
                    name: None,
                    ttl: no_records
                        .negative_ttl
                        .map_or(NO_TTL, |ttl| Duration::from_secs(ttl.into())),
                },
                Err(e) => {
                    tracing::debug!("reverse DNS for {ip} failed: {e}");
                    PtrAnswer {
                        name: None,
                        ttl: NO_TTL,
                    }
                }
            }
        })
    }
}
