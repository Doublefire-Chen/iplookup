//! Shared test harness: the real router built in-process with a
//! [`FakeDatabases`] reader, driven over HTTP with
//! `tower::ServiceExt::oneshot` (no network port).
//!
//! ```ignore
//! mod common;
//! use common::{TestApp, FakeDatabases};
//!
//! let app = TestApp::new(FakeDatabases::new().with("8.8.8.8", data), &[]);
//! let res = app.get("/json").from("8.8.8.8:4000").send().await;
//! assert_eq!(res.status, StatusCode::OK);
//! ```
//!
//! Tests only look at requests and responses, never at internal state.

#![allow(dead_code)] // each test binary uses a different subset

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use iplookup::app::{self, AppState};
use iplookup::config::Config;
use iplookup::databases::{
    Coordinates, DataSourceInfo, DataSourcesInfo, DatabaseReader, IpData, LocationData, NetworkData,
};
use serde_json::Value;
use tower::ServiceExt;

/// The peer of every request that doesn't set one.
pub const DEFAULT_PEER: &str = "203.0.113.7:50000";

/// A [`DatabaseReader`] that knows only the addresses it is given, and
/// logs every address it is asked about (see [`FakeDatabases::lookups`]).
#[derive(Default)]
pub struct FakeDatabases {
    data: HashMap<IpAddr, IpData>,
    lookups: Arc<Mutex<Vec<IpAddr>>>,
}

impl FakeDatabases {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, ip: &str, data: IpData) -> Self {
        self.data.insert(ip.parse().expect("valid test IP"), data);
        self
    }

    /// A handle on the log of looked-up addresses; take it before handing
    /// the fake to the app.
    pub fn lookups(&self) -> Arc<Mutex<Vec<IpAddr>>> {
        self.lookups.clone()
    }
}

impl DatabaseReader for FakeDatabases {
    fn lookup(&self, ip: IpAddr) -> IpData {
        self.lookups.lock().unwrap().push(ip);
        self.data.get(&ip).cloned().unwrap_or_default()
    }

    fn info(&self) -> DataSourcesInfo {
        let info = |source: &str| DataSourceInfo {
            source: source.to_owned(),
            path: format!("/fake/{source}").into(),
            build: 1_767_225_600,
        };
        DataSourcesInfo {
            location: info("GeoLite2-City"),
            network: info("ipinfo"),
        }
    }
}

/// Full Location and Network Data, as for Google's 8.8.8.8.
pub fn google() -> IpData {
    IpData {
        location: Some(LocationData {
            city: Some("Mountain View".into()),
            region: Some("California".into()),
            country: Some("US".into()),
            country_name: Some("United States".into()),
            continent: Some("North America".into()),
            postal: Some("94043".into()),
            coordinates: Some(Coordinates {
                latitude: 37.4056,
                longitude: -122.0775,
            }),
            accuracy_radius: Some(1000),
            timezone: Some("America/Los_Angeles".into()),
        }),
        network: Some(NetworkData {
            asn: Some("AS15169".into()),
            name: Some("Google LLC".into()),
            domain: Some("google.com".into()),
        }),
    }
}

pub struct TestApp {
    pub router: Router,
}

impl TestApp {
    /// The app with `databases`, configured from `config_toml`.
    pub fn with_config(databases: impl DatabaseReader, config_toml: &str) -> Self {
        let config = Config::parse(config_toml, Path::new("/")).expect("valid test config");
        Self::with_reader(&config, Arc::new(databases))
    }

    pub fn with_reader(config: &Config, databases: Arc<dyn DatabaseReader>) -> Self {
        Self {
            router: app::router(AppState::new(config, databases)),
        }
    }

    /// The app with `databases` and the default config.
    pub fn new(databases: impl DatabaseReader) -> Self {
        Self::with_config(databases, "")
    }

    pub fn get(&self, uri: &str) -> RequestBuilder<'_> {
        RequestBuilder {
            app: self,
            request: Request::get(uri),
            peer: DEFAULT_PEER.parse().unwrap(),
        }
    }
}

pub struct RequestBuilder<'a> {
    app: &'a TestApp,
    request: axum::http::request::Builder,
    peer: SocketAddr,
}

impl RequestBuilder<'_> {
    /// Sets the socket peer address.
    pub fn from(mut self, peer: &str) -> Self {
        self.peer = peer.parse().expect("valid peer address");
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.request = self.request.header(name, value);
        self
    }

    pub async fn send(self) -> TestResponse {
        let mut request = self.request.body(Body::empty()).unwrap();
        request.extensions_mut().insert(ConnectInfo(self.peer));
        let response = self.app.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        TestResponse {
            status,
            headers,
            body,
        }
    }
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl TestResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|v| v.to_str().unwrap())
    }

    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap()
    }

    /// The body as JSON, after checking it is a JSON response.
    pub fn json(&self) -> Value {
        assert_eq!(
            self.header("content-type"),
            Some("application/json; charset=utf-8")
        );
        serde_json::from_slice(&self.body).expect("JSON body")
    }

    /// The JSON body's top-level keys, in order (read from the
    /// pretty-printed text, where they are the lines indented by two).
    pub fn keys(&self) -> Vec<String> {
        assert!(
            self.json().is_object(),
            "not a JSON object: {}",
            self.text()
        );
        self.text()
            .lines()
            .filter_map(|line| line.strip_prefix("  \""))
            .map(|rest| rest.split('"').next().unwrap().to_owned())
            .collect()
    }
}
