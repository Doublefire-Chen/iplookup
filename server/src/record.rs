//! The Record: the answer to a Lookup, in ipinfo.io's JSON shape (ADR 0001).
//!
//! ipinfo's flat keys and formats come first, then the extras. Missing
//! values leave their key out; nothing is ever `null` or `""`.

use serde::Serialize;

use crate::databases::IpData;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    /// The address as requested.
    pub ip: String,
    /// Set only on a Bogon's Record, which carries nothing else.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bogon: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// `"lat,lon"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loc: Option<String>,
    /// `"AS<n> <name>"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Always set, except on a Bogon's Record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme: Option<&'static str>,
    // Extras, after ipinfo's keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continent: Option<String>,
    /// The Accuracy Radius, in kilometres.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy_radius: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<Asn>,
}

/// The `asn` extra, shaped like ipinfo's paid-tier `asn` object.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Asn {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

/// One of ipinfo's nine Fields, served on its own as plain text. The
/// extras (`country_name`, `asn` …) are JSON-only and not Fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Ip,
    Hostname,
    City,
    Region,
    Country,
    Loc,
    Org,
    Postal,
    Timezone,
}

impl Field {
    /// The Field named `name` in a path, such as `city` in `/city`.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "ip" => Self::Ip,
            "hostname" => Self::Hostname,
            "city" => Self::City,
            "region" => Self::Region,
            "country" => Self::Country,
            "loc" => Self::Loc,
            "org" => Self::Org,
            "postal" => Self::Postal,
            "timezone" => Self::Timezone,
            _ => return None,
        })
    }
}

impl Record {
    /// The value of `field`, or `None` when the Record has none.
    pub fn field(&self, field: Field) -> Option<&str> {
        match field {
            Field::Ip => Some(&self.ip),
            Field::Hostname => self.hostname.as_deref(),
            Field::City => self.city.as_deref(),
            Field::Region => self.region.as_deref(),
            Field::Country => self.country.as_deref(),
            Field::Loc => self.loc.as_deref(),
            Field::Org => self.org.as_deref(),
            Field::Postal => self.postal.as_deref(),
            Field::Timezone => self.timezone.as_deref(),
        }
    }

    /// A Record for `ip` (as requested) from what the Data Sources know.
    pub fn new(ip: String, data: IpData) -> Self {
        let location = data.location.unwrap_or_default();
        let org = data.network.as_ref().and_then(|n| {
            let asn = n.asn.as_deref()?;
            Some(match &n.name {
                Some(name) => format!("{asn} {name}"),
                None => asn.to_owned(),
            })
        });
        Self {
            ip,
            bogon: false,
            hostname: None,
            city: location.city,
            region: location.region,
            country: location.country,
            loc: location
                .coordinates
                .map(|c| format!("{:.4},{:.4}", c.latitude, c.longitude)),
            org,
            postal: location.postal,
            timezone: location.timezone,
            readme: Some(crate::README_URL),
            country_name: location.country_name,
            continent: location.continent,
            accuracy_radius: location.accuracy_radius,
            asn: data.network.map(|n| Asn {
                asn: n.asn,
                name: n.name,
                domain: n.domain,
            }),
        }
    }

    /// A Bogon's Record: `{"ip": …, "bogon": true}` and nothing else.
    pub fn bogon(ip: String) -> Self {
        Self {
            ip,
            bogon: true,
            hostname: None,
            city: None,
            region: None,
            country: None,
            loc: None,
            org: None,
            postal: None,
            timezone: None,
            readme: None,
            country_name: None,
            continent: None,
            accuracy_radius: None,
            asn: None,
        }
    }

    /// Pretty-printed JSON ending in a newline, like ipinfo.io.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("a Record always serialises");
        json.push('\n');
        json
    }
}
