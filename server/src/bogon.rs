//! Bogons: addresses that are not routable on the public internet. Their
//! Record is just `{"ip": …, "bogon": true}`, with no data lookup.
//!
//! The ranges follow ipinfo.io's bogon list: private, loopback, link-local,
//! documentation, CGNAT, multicast and other reserved IPv4 and IPv6
//! ranges, plus 6to4 and Teredo addresses that embed an IPv4 Bogon.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::{Ipv4Net, Ipv6Net};

const IPV4: &[&str] = &[
    "0.0.0.0/8",          // "this" network
    "10.0.0.0/8",         // private
    "100.64.0.0/10",      // CGNAT
    "127.0.0.0/8",        // loopback
    "169.254.0.0/16",     // link-local
    "172.16.0.0/12",      // private
    "192.0.0.0/24",       // IETF protocol assignments
    "192.0.2.0/24",       // documentation (TEST-NET-1)
    "192.88.99.0/24",     // deprecated 6to4 relay anycast
    "192.168.0.0/16",     // private
    "198.18.0.0/15",      // benchmarking
    "198.51.100.0/24",    // documentation (TEST-NET-2)
    "203.0.113.0/24",     // documentation (TEST-NET-3)
    "224.0.0.0/4",        // multicast
    "240.0.0.0/4",        // reserved
    "255.255.255.255/32", // limited broadcast
];

const IPV6: &[&str] = &[
    "::/128",         // unspecified
    "::1/128",        // loopback
    "::ffff:0:0/96",  // IPv4-mapped (only reached if not canonicalised)
    "::/96",          // IPv4-compatible (deprecated)
    "64:ff9b:1::/48", // local-use IPv4/IPv6 translation
    "100::/64",       // discard-only
    "2001:10::/28",   // ORCHID (deprecated)
    "2001:20::/28",   // ORCHIDv2
    "2001:db8::/32",  // documentation
    "3fff::/20",      // documentation
    "5f00::/16",      // segment routing SIDs
    "fc00::/7",       // unique local
    "fe80::/10",      // link-local
    "fec0::/10",      // site-local (deprecated)
    "ff00::/8",       // multicast
];

/// Whether `ip` is a Bogon. An IPv4-mapped IPv6 address is judged by its
/// IPv4 address.
pub fn is_bogon(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => is_bogon_v4(v4),
        IpAddr::V6(v6) => is_bogon_v6(v6),
    }
}

fn is_bogon_v4(ip: Ipv4Addr) -> bool {
    IPV4.iter()
        .any(|net| net.parse::<Ipv4Net>().unwrap().contains(&ip))
}

fn is_bogon_v6(ip: Ipv6Addr) -> bool {
    if IPV6
        .iter()
        .any(|net| net.parse::<Ipv6Net>().unwrap().contains(&ip))
    {
        return true;
    }
    let s = ip.segments();
    let embedded = match s {
        // 6to4: 2002:AABB:CCDD::/48 carries A.B.C.D.
        [0x2002, hi, lo, ..] => Some(v4(hi, lo)),
        // Teredo: 2001:0::/32 carries the client's address, inverted, last.
        [0x2001, 0, .., hi, lo] => Some(v4(!hi, !lo)),
        _ => None,
    };
    embedded.is_some_and(is_bogon_v4)
}

fn v4(hi: u16, lo: u16) -> Ipv4Addr {
    Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_range_parses() {
        for net in IPV4 {
            net.parse::<Ipv4Net>().unwrap();
        }
        for net in IPV6 {
            net.parse::<Ipv6Net>().unwrap();
        }
    }

    #[test]
    fn bogons_and_public_addresses() {
        let bogons = [
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.7",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:10.0.0.1",
            "::1.2.3.4",
            "100::1",
            "2001:db8::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "2002:a00:1::1",                        // 6to4 of 10.0.0.1
            "2001:0:4136:e378:8000:63bf:f5ff:fffe", // Teredo of 10.0.0.1
        ];
        for ip in bogons {
            assert!(is_bogon(ip.parse().unwrap()), "{ip} should be a Bogon");
        }
        let public = [
            "1.1.1.1",
            "8.8.8.8",
            "100.63.255.255",
            "100.128.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "223.255.255.255",
            "::ffff:8.8.8.8",
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
            "2002:808:808::1",                      // 6to4 of 8.8.8.8
            "2001:0:4136:e378:8000:63bf:f7f7:f7f7", // Teredo of 8.8.8.8
        ];
        for ip in public {
            assert!(!is_bogon(ip.parse().unwrap()), "{ip} should be public");
        }
    }
}
