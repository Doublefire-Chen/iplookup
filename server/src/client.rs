//! Working out the client's address.

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;
use ipnet::IpNet;

/// The client's address: the socket peer, unless the peer is a trusted
/// proxy, in which case its `X-Real-IP` header (when valid). IPv4-mapped
/// IPv6 addresses (`::ffff:a.b.c.d`) come back as plain IPv4.
pub fn client_ip(peer: SocketAddr, headers: &HeaderMap, trusted_proxies: &[IpNet]) -> IpAddr {
    let peer = peer.ip().to_canonical();
    if !trusted_proxies.iter().any(|net| net.contains(&peer)) {
        return peer;
    }
    let real_ip = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok());
    match real_ip {
        Some(ip) => ip.to_canonical(),
        None => {
            tracing::debug!("trusted proxy {peer} sent no valid X-Real-IP; using the peer");
            peer
        }
    }
}
