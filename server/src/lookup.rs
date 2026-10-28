//! Explicit Lookups: from the address as typed to its Record (ADR 0001).

use std::net::IpAddr;

use crate::bogon::is_bogon;
use crate::databases::DatabaseReader;
use crate::record::Record;

/// The address in `text`, or `None` for anything that isn't exactly an
/// IPv4 or IPv6 address (hostnames, leading-zero octets, zone IDs …).
pub fn parse_ip(text: &str) -> Option<IpAddr> {
    text.parse().ok()
}

/// The Record of `ip`, with `ip` echoed as `as_typed`. `::ffff:a.b.c.d` is
/// looked up as `a.b.c.d`, and a Bogon never reaches the Data Sources.
pub fn explicit_lookup(databases: &dyn DatabaseReader, as_typed: &str, ip: IpAddr) -> Record {
    let ip = ip.to_canonical();
    if is_bogon(ip) {
        return Record::bogon(as_typed.to_owned());
    }
    Record::new(as_typed.to_owned(), databases.lookup(ip))
}
