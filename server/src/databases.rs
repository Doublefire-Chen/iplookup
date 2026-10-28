//! The database reader: given an IP address, its Location Data and Network
//! Data, plus each Data Source's build info.
//!
//! [`DatabaseReader`] is the seam. [`MmdbDatabases`] reads the real files
//! (MaxMind GeoLite2 City and IPinfo Lite); tests use a fake. Each file is
//! held behind an atomic swap, so the updater can replace it while Lookups
//! go on: a Lookup in progress finishes on the data it started with.

use std::fmt;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use arc_swap::ArcSwap;
use maxminddb::{Reader, geoip2};
use serde::Deserialize;

/// The Location Data file in the data directory.
pub const LOCATION_FILE: &str = "GeoLite2-City.mmdb";
/// The Network Data file in the data directory.
pub const NETWORK_FILE: &str = "ipinfo_lite.mmdb";

/// One of the two Data Sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataSource {
    /// MaxMind GeoLite2 City.
    Location,
    /// IPinfo Lite.
    Network,
}

impl DataSource {
    pub const ALL: [DataSource; 2] = [DataSource::Location, DataSource::Network];

    /// The file's name in the data directory.
    pub fn file_name(self) -> &'static str {
        match self {
            DataSource::Location => LOCATION_FILE,
            DataSource::Network => NETWORK_FILE,
        }
    }

    /// Whether an mmdb `database_type` is this Data Source's.
    fn is_type(self, database_type: &str) -> bool {
        match self {
            DataSource::Location => database_type.contains("City"),
            DataSource::Network => database_type.starts_with("ipinfo"),
        }
    }
}

impl fmt::Display for DataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DataSource::Location => "Location Data (GeoLite2 City)",
            DataSource::Network => "Network Data (IPinfo Lite)",
        })
    }
}

pub trait DatabaseReader: Send + Sync + 'static {
    /// What the Data Sources know about `ip`. Each part is `None` when its
    /// Data Source has nothing for the address.
    fn lookup(&self, ip: IpAddr) -> IpData;

    /// Where each Data Source was loaded from and when it was built.
    fn info(&self) -> DataSourcesInfo;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IpData {
    pub location: Option<LocationData>,
    pub network: Option<NetworkData>,
}

/// Where an IP address is. Every item is optional.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocationData {
    pub city: Option<String>,
    /// The first-level subdivision's name, e.g. `California`.
    pub region: Option<String>,
    /// ISO 3166-1 alpha-2 code, e.g. `US`.
    pub country: Option<String>,
    /// English country name, e.g. `United States`.
    pub country_name: Option<String>,
    /// English continent name, e.g. `North America`.
    pub continent: Option<String>,
    pub postal: Option<String>,
    pub coordinates: Option<Coordinates>,
    /// The Accuracy Radius, in kilometres.
    pub accuracy_radius: Option<u16>,
    /// IANA time zone name, e.g. `America/Chicago`.
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

/// Which network an IP address belongs to. Every item is optional.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetworkData {
    /// The AS number with its prefix, e.g. `AS15169`.
    pub asn: Option<String>,
    /// The organisation's name, e.g. `Google LLC`.
    pub name: Option<String>,
    /// The organisation's domain, e.g. `google.com`.
    pub domain: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataSourcesInfo {
    pub location: DataSourceInfo,
    pub network: DataSourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataSourceInfo {
    /// The database type from the file's metadata, e.g. `GeoLite2-City`.
    pub source: String,
    pub path: PathBuf,
    /// Build time, in seconds since the Unix epoch.
    pub build: u64,
    /// When the updater last checked upstream for new data (successfully,
    /// whether or not there was any), in seconds since the Unix epoch;
    /// `None` if it hasn't since startup and has no record of it.
    pub checked: Option<u64>,
}

/// An opened, verified mmdb file of one Data Source.
pub type Mmdb = Reader<Vec<u8>>;

/// The real Data Sources, read from mmdb files and swappable at runtime.
pub struct MmdbDatabases {
    location: Slot,
    network: Slot,
}

struct Slot {
    reader: ArcSwap<Mmdb>,
    path: PathBuf,
    /// Seconds since the Unix epoch; 0 for never.
    checked: AtomicU64,
}

impl MmdbDatabases {
    /// Opens [`LOCATION_FILE`] and [`NETWORK_FILE`] in `data_dir`.
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let slot = |source: DataSource| -> anyhow::Result<Slot> {
            let path = data_dir.join(source.file_name());
            Ok(Slot {
                reader: ArcSwap::from_pointee(open_mmdb(&path, source)?),
                path,
                checked: AtomicU64::new(0),
            })
        };
        Ok(Self {
            location: slot(DataSource::Location)?,
            network: slot(DataSource::Network)?,
        })
    }

    /// Swaps in new data for `source`. Lookups already running finish on
    /// the old data, which is dropped after the last of them.
    pub fn replace(&self, source: DataSource, reader: Mmdb) {
        self.slot(source).reader.store(Arc::new(reader));
    }

    /// Records when upstream was last checked for new `source` data.
    pub fn set_checked(&self, source: DataSource, at: SystemTime) {
        let secs = at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.slot(source).checked.store(secs, Ordering::Relaxed);
    }

    fn slot(&self, source: DataSource) -> &Slot {
        match source {
            DataSource::Location => &self.location,
            DataSource::Network => &self.network,
        }
    }
}

fn location(reader: &Mmdb, ip: IpAddr) -> Result<Option<LocationData>, maxminddb::MaxMindDbError> {
    let Some(city) = reader.lookup(ip)?.decode::<geoip2::City>()? else {
        return Ok(None);
    };
    // Fall back to the country the network is registered in when
    // GeoLite has no located country (e.g. anycast addresses).
    let country = if city.country.iso_code.is_some() {
        &city.country
    } else {
        &city.registered_country
    };
    let loc = &city.location;
    let data = LocationData {
        city: owned(city.city.names.english),
        region: city
            .subdivisions
            .first()
            .and_then(|s| owned(s.names.english)),
        country: owned(country.iso_code),
        country_name: owned(country.names.english),
        continent: owned(city.continent.names.english),
        postal: owned(city.postal.code),
        coordinates: loc
            .latitude
            .zip(loc.longitude)
            .map(|(latitude, longitude)| Coordinates {
                latitude,
                longitude,
            }),
        accuracy_radius: loc.accuracy_radius,
        timezone: owned(loc.time_zone),
    };
    Ok((data != LocationData::default()).then_some(data))
}

fn network(reader: &Mmdb, ip: IpAddr) -> Result<Option<NetworkData>, maxminddb::MaxMindDbError> {
    let Some(lite) = reader.lookup(ip)?.decode::<IpinfoLite>()? else {
        return Ok(None);
    };
    let data = NetworkData {
        asn: owned(lite.asn),
        name: owned(lite.as_name),
        domain: owned(lite.as_domain),
    };
    Ok((data != NetworkData::default()).then_some(data))
}

impl DatabaseReader for MmdbDatabases {
    fn lookup(&self, ip: IpAddr) -> IpData {
        IpData {
            location: location(&self.location.reader.load(), ip).unwrap_or_else(|e| {
                tracing::warn!("Location Data lookup of {ip} failed: {e}");
                None
            }),
            network: network(&self.network.reader.load(), ip).unwrap_or_else(|e| {
                tracing::warn!("Network Data lookup of {ip} failed: {e}");
                None
            }),
        }
    }

    fn info(&self) -> DataSourcesInfo {
        let info = |slot: &Slot| {
            let reader = slot.reader.load();
            DataSourceInfo {
                source: reader.metadata().database_type.clone(),
                path: slot.path.clone(),
                build: reader.metadata().build_epoch,
                checked: Some(slot.checked.load(Ordering::Relaxed)).filter(|&t| t != 0),
            }
        };
        DataSourcesInfo {
            location: info(&self.location),
            network: info(&self.network),
        }
    }
}

/// An IPinfo Lite record. Its keys are flat; the country and continent
/// keys are ignored because Location Data comes from GeoLite2.
#[derive(Deserialize)]
struct IpinfoLite<'a> {
    asn: Option<&'a str>,
    as_name: Option<&'a str>,
    as_domain: Option<&'a str>,
}

/// An address every real Data Source knows; a new file must answer for it.
const PROBE_IP: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(8, 8, 8, 8));

/// Opens `path` as `source`'s database and checks it: its type must match
/// and a test Lookup must find data.
pub fn open_mmdb(path: &Path, source: DataSource) -> anyhow::Result<Mmdb> {
    let reader = Reader::open_readfile(path)
        .with_context(|| format!("opening {source} database {}", path.display()))?;
    let db_type = &reader.metadata().database_type;
    anyhow::ensure!(
        source.is_type(db_type),
        "{} is a {db_type:?} database, not {source}",
        path.display()
    );
    let found = match source {
        DataSource::Location => location(&reader, PROBE_IP).map(|d| d.is_some()),
        DataSource::Network => network(&reader, PROBE_IP).map(|d| d.is_some()),
    }
    .with_context(|| format!("test Lookup in {source} database {}", path.display()))?;
    anyhow::ensure!(
        found,
        "{source} database {} has no data for {PROBE_IP}",
        path.display()
    );
    Ok(reader)
}

fn owned(s: Option<&str>) -> Option<String> {
    s.filter(|s| !s.is_empty()).map(str::to_owned)
}
