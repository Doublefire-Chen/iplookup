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
//! Reverse DNS goes to a [`FakeReverseDns`], which by default knows no PTR
//! names (no test ever does real DNS).
//!
//! Tests only look at requests and responses, never at internal state.

#![allow(dead_code)] // each test binary uses a different subset

pub mod mmdb;
pub mod upstream;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use iplookup::app::{self, AppState};
use iplookup::config::Config;
use iplookup::databases::{
    Coordinates, DataSource, DataSourceInfo, DataSourcesInfo, DatabaseReader, IpData, LocationData,
    NetworkData,
};
use iplookup::reverse_dns::{BoxFuture, PtrAnswer, PtrResolver};
use serde_json::Value;
use tower::ServiceExt;

/// The peer of every request that doesn't set one: a public address the
/// fakes know nothing about (Quad9's DNS).
pub const DEFAULT_PEER: &str = "9.9.9.9:50000";

/// Build time of both fake Data Sources: 2026-01-01T00:00:00Z.
pub const FAKE_BUILD: u64 = 1_767_225_600;

/// A [`DatabaseReader`] that knows only the addresses it is given, and
/// logs every address it is asked about (see [`FakeDatabases::lookups`]).
/// Both Data Sources are loaded (built [`FAKE_BUILD`], never checked)
/// unless [`FakeDatabases::unloaded`] says otherwise.
#[derive(Default)]
pub struct FakeDatabases {
    data: HashMap<IpAddr, IpData>,
    lookups: Arc<Mutex<Vec<IpAddr>>>,
    unloaded: Vec<DataSource>,
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

    /// `source` is reported as not loaded.
    pub fn unloaded(mut self, source: DataSource) -> Self {
        self.unloaded.push(source);
        self
    }
}

impl DatabaseReader for FakeDatabases {
    fn lookup(&self, ip: IpAddr) -> IpData {
        self.lookups.lock().unwrap().push(ip);
        self.data.get(&ip).cloned().unwrap_or_default()
    }

    fn info(&self) -> DataSourcesInfo {
        let info = |source: DataSource, name: &str| {
            (!self.unloaded.contains(&source)).then(|| DataSourceInfo {
                source: name.to_owned(),
                path: format!("/fake/{name}").into(),
                build: FAKE_BUILD,
                checked: None,
            })
        };
        DataSourcesInfo {
            location: info(DataSource::Location, "GeoLite2-City"),
            network: info(DataSource::Network, "ipinfo_lite"),
        }
    }
}

/// A [`PtrResolver`] that answers from a table, and logs every address it
/// is asked about (see [`FakeReverseDns::queries`]). Unknown addresses
/// have no PTR.
#[derive(Default)]
pub struct FakeReverseDns {
    answers: HashMap<IpAddr, (Duration, PtrAnswer)>,
    queries: Arc<Mutex<Vec<IpAddr>>>,
}

impl FakeReverseDns {
    pub fn new() -> Self {
        Self::default()
    }

    /// `ip` has the PTR name `name`, cacheable for `ttl`.
    pub fn name(self, ip: &str, name: &str, ttl: Duration) -> Self {
        self.slow(ip, Duration::ZERO, name, ttl)
    }

    /// `ip` has no PTR name, and says so for `ttl`.
    pub fn no_ptr(mut self, ip: &str, ttl: Duration) -> Self {
        let answer = PtrAnswer { name: None, ttl };
        self.answers
            .insert(ip.parse().expect("valid test IP"), (Duration::ZERO, answer));
        self
    }

    /// `ip` has the PTR name `name`, but the answer takes `delay`.
    pub fn slow(mut self, ip: &str, delay: Duration, name: &str, ttl: Duration) -> Self {
        let answer = PtrAnswer {
            name: Some(name.to_owned()),
            ttl,
        };
        self.answers
            .insert(ip.parse().expect("valid test IP"), (delay, answer));
        self
    }

    /// A handle on the log of queried addresses; take it before handing
    /// the fake to the app.
    pub fn queries(&self) -> Arc<Mutex<Vec<IpAddr>>> {
        self.queries.clone()
    }
}

impl PtrResolver for FakeReverseDns {
    fn ptr(&self, ip: IpAddr) -> BoxFuture<'_, PtrAnswer> {
        self.queries.lock().unwrap().push(ip);
        let (delay, answer) = self.answers.get(&ip).cloned().unwrap_or((
            Duration::ZERO,
            PtrAnswer {
                name: None,
                ttl: Duration::from_secs(60),
            },
        ));
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            answer
        })
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

    /// The app with `databases` and `reverse_dns`, configured from
    /// `config_toml`.
    pub fn with_reverse_dns(
        databases: impl DatabaseReader,
        reverse_dns: FakeReverseDns,
        config_toml: &str,
    ) -> Self {
        let config = Config::parse(config_toml, Path::new("/")).expect("valid test config");
        Self {
            router: app::router(AppState::new(
                &config,
                Arc::new(databases),
                Arc::new(reverse_dns),
            )),
        }
    }

    /// The app with `databases` and a reverse DNS that knows no names.
    pub fn with_reader(config: &Config, databases: Arc<dyn DatabaseReader>) -> Self {
        Self {
            router: app::router(AppState::new(
                config,
                databases,
                Arc::new(FakeReverseDns::new()),
            )),
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

/// The `index.html` of [`frontend`]: just enough of Vite's output to find
/// the injected data block in.
pub const INDEX_HTML: &str = "<!doctype html>\n<html><head><title>iplookup</title><script type=\"module\" src=\"/assets/index-abc123.js\"></script></head><body><div id=\"root\"></div></body></html>\n";

/// A built frontend in a temporary directory: [`INDEX_HTML`] and
/// `assets/index-abc123.js`. Keep the directory alive while the app runs.
pub fn frontend() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), INDEX_HTML).unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("assets/index-abc123.js"),
        "console.log(\"iplookup\");\n",
    )
    .unwrap();
    dir
}

/// The config line pointing `frontend.dir` at `dir`.
pub fn frontend_config(dir: &Path) -> String {
    format!("[frontend]\ndir = {:?}\n", dir.display().to_string())
}

impl TestResponse {
    /// The JSON block injected into a page, after checking it is one.
    pub fn page_data(&self) -> Value {
        assert_eq!(
            self.header("content-type"),
            Some("text/html; charset=utf-8")
        );
        let start = r#"<script id="iplookup-data" type="application/json">"#;
        let html = self.text();
        let json = &html[html.find(start).expect("a data block") + start.len()..];
        let json = &json[..json.find("</script>").unwrap()];
        serde_json::from_str(json).expect("JSON data block")
    }
}
