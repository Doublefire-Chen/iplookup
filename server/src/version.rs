//! `/version`: the backend version and each Data Source's state, doubling
//! as the health check.
//!
//! ```json
//! {"version": "0.1.0", "data": {
//!   "location": {"source": "GeoLite2-City", "build": "…Z", "checked": "…Z"},
//!   "network": {"source": "ipinfo_lite", "build": "…Z", "checked": "…Z"}}}
//! ```
//!
//! Times are RFC 3339 in UTC. As in a Record, missing values leave their
//! key out: `checked` until the updater has checked upstream, and a whole
//! Data Source while it isn't loaded (which also makes the status 503).

use std::time::{Duration, UNIX_EPOCH};

use axum::http::StatusCode;
use serde::Serialize;

use crate::databases::{DataSourceInfo, DataSourcesInfo};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VersionInfo {
    /// The server's (crate) version.
    pub version: &'static str,
    pub data: DataState,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DataState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<SourceState>,
}

/// One loaded Data Source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceState {
    /// The database type from the file's metadata.
    pub source: String,
    /// When the loaded file was built.
    pub build: String,
    /// When upstream was last checked for new data (successfully).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<String>,
}

impl VersionInfo {
    pub fn new(info: &DataSourcesInfo) -> Self {
        let state = |s: &DataSourceInfo| SourceState {
            source: s.source.clone(),
            build: rfc3339(s.build),
            checked: s.checked.map(rfc3339),
        };
        Self {
            version: crate::VERSION,
            data: DataState {
                location: info.location.as_ref().map(state),
                network: info.network.as_ref().map(state),
            },
        }
    }

    /// 200 when both Data Sources are loaded, else 503.
    pub fn status(&self) -> StatusCode {
        if self.data.location.is_some() && self.data.network.is_some() {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        }
    }

    /// Pretty-printed JSON ending in a newline, like a Record.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("version info always serialises");
        json.push('\n');
        json
    }
}

/// Seconds since the Unix epoch as RFC 3339 UTC, e.g.
/// `2026-10-28T00:00:00Z`.
fn rfc3339(secs: u64) -> String {
    humantime::format_rfc3339_seconds(UNIX_EPOCH + Duration::from_secs(secs)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(checked: Option<u64>) -> DataSourceInfo {
        DataSourceInfo {
            source: "GeoLite2-City".into(),
            path: "/data/GeoLite2-City.mmdb".into(),
            build: 1_793_145_600,
            checked,
        }
    }

    #[test]
    fn times_are_rfc3339_utc_and_unknown_ones_are_left_out() {
        let info = DataSourcesInfo {
            location: Some(source(Some(1_793_160_000))),
            network: Some(source(None)),
        };
        let v = VersionInfo::new(&info);
        assert_eq!(v.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&v.to_json()).unwrap();
        assert_eq!(json["data"]["location"]["build"], "2026-10-28T00:00:00Z");
        assert_eq!(json["data"]["location"]["checked"], "2026-10-28T04:00:00Z");
        assert!(json["data"]["network"].get("checked").is_none());
    }

    #[test]
    fn a_missing_source_is_unhealthy() {
        let info = DataSourcesInfo {
            location: Some(source(None)),
            network: None,
        };
        let v = VersionInfo::new(&info);
        assert_eq!(v.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!v.to_json().contains("network"));
    }
}
