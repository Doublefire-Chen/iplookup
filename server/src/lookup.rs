//! Lookups: from an address to its Record (ADR 0001).

use std::net::IpAddr;

use crate::bogon::is_bogon;
use crate::databases::DatabaseReader;
use crate::record::Record;
use crate::reverse_dns::ReverseDns;

/// The address in `text`, or `None` for anything that isn't exactly an
/// IPv4 or IPv6 address (hostnames, leading-zero octets, zone IDs …).
pub fn parse_ip(text: &str) -> Option<IpAddr> {
    text.parse().ok()
}

/// The Record of `ip`, with `ip` echoed as `as_typed` (for a Self Lookup,
/// the client address as text). `::ffff:a.b.c.d` is looked up as
/// `a.b.c.d`, and a Bogon never reaches the Data Sources or reverse DNS.
/// `hostname` is the address's PTR name, when it has one that answers in
/// time.
pub async fn lookup(
    databases: &dyn DatabaseReader,
    reverse_dns: &ReverseDns,
    as_typed: &str,
    ip: IpAddr,
) -> Record {
    let ip = ip.to_canonical();
    if is_bogon(ip) {
        return Record::bogon(as_typed.to_owned());
    }
    let mut record = Record::new(as_typed.to_owned(), databases.lookup(ip));
    record.hostname = reverse_dns.hostname(ip).await;
    record
}
