//! The database reader: given an IP address, its Location Data and Network
//! Data, plus each Data Source's build info.
//!
//! [`DatabaseReader`] is the seam. [`MmdbDatabases`] reads the real files
//! (MaxMind GeoLite2 City and IPinfo Lite); tests use a fake.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use maxminddb::{Reader, geoip2};
use serde::Deserialize;

/// The Location Data file in the data directory.
pub const LOCATION_FILE: &str = "GeoLite2-City.mmdb";
/// The Network Data file in the data directory.
pub const NETWORK_FILE: &str = "ipinfo_lite.mmdb";

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
}

/// The real Data Sources, read from mmdb files.
pub struct MmdbDatabases {
    location: Reader<Vec<u8>>,
    network: Reader<Vec<u8>>,
    location_path: PathBuf,
    network_path: PathBuf,
}

impl MmdbDatabases {
    /// Opens [`LOCATION_FILE`] and [`NETWORK_FILE`] in `data_dir`.
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let location_path = data_dir.join(LOCATION_FILE);
        let network_path = data_dir.join(NETWORK_FILE);
        let location = open_mmdb(&location_path, "Location Data (GeoLite2 City)", |t| {
            t.contains("City")
        })?;
        let network = open_mmdb(&network_path, "Network Data (IPinfo Lite)", |t| {
            t.starts_with("ipinfo")
        })?;
        Ok(Self {
            location,
            network,
            location_path,
            network_path,
        })
    }

    fn location(&self, ip: IpAddr) -> Result<Option<LocationData>, maxminddb::MaxMindDbError> {
        let Some(city) = self.location.lookup(ip)?.decode::<geoip2::City>()? else {
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

    fn network(&self, ip: IpAddr) -> Result<Option<NetworkData>, maxminddb::MaxMindDbError> {
        let Some(lite) = self.network.lookup(ip)?.decode::<IpinfoLite>()? else {
            return Ok(None);
        };
        let data = NetworkData {
            asn: owned(lite.asn),
            name: owned(lite.as_name),
            domain: owned(lite.as_domain),
        };
        Ok((data != NetworkData::default()).then_some(data))
    }
}

impl DatabaseReader for MmdbDatabases {
    fn lookup(&self, ip: IpAddr) -> IpData {
        IpData {
            location: self.location(ip).unwrap_or_else(|e| {
                tracing::warn!("Location Data lookup of {ip} failed: {e}");
                None
            }),
            network: self.network(ip).unwrap_or_else(|e| {
                tracing::warn!("Network Data lookup of {ip} failed: {e}");
                None
            }),
        }
    }

    fn info(&self) -> DataSourcesInfo {
        let info = |reader: &Reader<Vec<u8>>, path: &Path| DataSourceInfo {
            source: reader.metadata().database_type.clone(),
            path: path.to_owned(),
            build: reader.metadata().build_epoch,
        };
        DataSourcesInfo {
            location: info(&self.location, &self.location_path),
            network: info(&self.network, &self.network_path),
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

fn open_mmdb(
    path: &Path,
    what: &str,
    expected_type: impl Fn(&str) -> bool,
) -> anyhow::Result<Reader<Vec<u8>>> {
    let reader = Reader::open_readfile(path)
        .with_context(|| format!("opening {what} database {}", path.display()))?;
    let db_type = &reader.metadata().database_type;
    anyhow::ensure!(
        expected_type(db_type),
        "{} is a {db_type:?} database, not {what}",
        path.display()
    );
    Ok(reader)
}

fn owned(s: Option<&str>) -> Option<String> {
    s.filter(|s| !s.is_empty()).map(str::to_owned)
}
