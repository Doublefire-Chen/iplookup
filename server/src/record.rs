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
    pub readme: &'static str,
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

impl Record {
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
            readme: crate::README_URL,
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

    /// Pretty-printed JSON ending in a newline, like ipinfo.io.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("a Record always serialises");
        json.push('\n');
        json
    }
}
