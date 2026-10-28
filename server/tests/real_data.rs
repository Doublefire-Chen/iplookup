//! The real Data Sources, read from a local copy in `server/testdata/`
//! (gitignored; never committed). Only stable facts are asserted. Without
//! the files these tests pass after printing why they skipped.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::TestApp;
use iplookup::config::Config;
use iplookup::databases::{LOCATION_FILE, MmdbDatabases, NETWORK_FILE};

fn testdata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

/// The app over the real databases, or `None` (with a message) when the
/// local copy is absent.
fn real_app() -> Option<TestApp> {
    let dir = testdata();
    if !dir.join(LOCATION_FILE).is_file() || !dir.join(NETWORK_FILE).is_file() {
        eprintln!(
            "SKIPPED: real-data test needs {LOCATION_FILE} and {NETWORK_FILE} in {}",
            dir.display()
        );
        return None;
    }
    let databases = MmdbDatabases::open(&dir).expect("opening the real databases");
    let config = Config::parse("", &dir).unwrap();
    Some(TestApp::with_reader(&config, Arc::new(databases)))
}

#[tokio::test]
async fn google_dns_over_ipv4() {
    let Some(app) = real_app() else { return };
    let res = app.get("/json").from("8.8.8.8:4000").send().await.json();
    assert_eq!(res["ip"], "8.8.8.8");
    assert_eq!(res["country"], "US");
    assert!(
        res["org"].as_str().unwrap().starts_with("AS15169 "),
        "{res}"
    );
    assert_eq!(res["asn"]["asn"], "AS15169");
}

#[tokio::test]
async fn cloudflare_dns() {
    let Some(app) = real_app() else { return };
    let res = app.get("/json").from("1.1.1.1:4000").send().await.json();
    assert!(
        res["org"].as_str().unwrap().starts_with("AS13335 "),
        "{res}"
    );
}

#[tokio::test]
async fn google_dns_over_ipv6() {
    let Some(app) = real_app() else { return };
    let res = app
        .get("/json")
        .from("[2001:4860:4860::8888]:4000")
        .send()
        .await
        .json();
    assert_eq!(res["ip"], "2001:4860:4860::8888");
    assert!(
        res["org"].as_str().unwrap().starts_with("AS15169 "),
        "{res}"
    );
}

#[tokio::test]
async fn explicit_lookups() {
    let Some(app) = real_app() else { return };
    for (path, ip, asn) in [
        ("/8.8.8.8", "8.8.8.8", "AS15169"),
        ("/1.1.1.1/json", "1.1.1.1", "AS13335"),
        ("/2001:4860:4860::8888", "2001:4860:4860::8888", "AS15169"),
        ("/::ffff:8.8.8.8/json", "::ffff:8.8.8.8", "AS15169"),
    ] {
        let res = app.get(path).send().await.json();
        assert_eq!(res["ip"], ip, "{path}");
        assert_eq!(res["asn"]["asn"], asn, "{path}: {res}");
    }
}

#[tokio::test]
async fn version_reports_both_real_data_sources() {
    let Some(app) = real_app() else { return };
    let res = app.get("/version").send().await;
    assert_eq!(res.status, axum::http::StatusCode::OK);
    let data = &res.json()["data"];
    assert_eq!(
        data["location"]["source"], "MaxMind GeoLite2 City",
        "{data}"
    );
    assert_eq!(data["network"]["source"], "IPinfo Lite", "{data}");
    for source in ["location", "network"] {
        let build = data[source]["build"].as_str().unwrap();
        assert!(humantime::parse_rfc3339(build).is_ok(), "{build}");
    }
}
