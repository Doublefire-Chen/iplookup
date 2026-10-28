//! `/version`: the backend version and each Data Source's state, as JSON
//! or a page (by `Accept`), doubling as the health check (200 only when
//! both Data Sources are loaded, else 503). How `build` and `checked`
//! follow the updater is tested in tests/updater.rs.

mod common;

use axum::http::StatusCode;
use common::{FakeDatabases, TestApp, frontend, frontend_config, google};
use iplookup::databases::DataSource;
use serde_json::json;

const BROWSER: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

fn expected_version() -> serde_json::Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "data": {
            "location": {"source": "MaxMind GeoLite2 City", "build": "2026-01-01T00:00:00Z"},
            "network": {"source": "IPinfo Lite", "build": "2026-01-01T00:00:00Z"},
        }
    })
}

#[tokio::test]
async fn curl_gets_the_version_and_both_data_sources() {
    let app = TestApp::new(FakeDatabases::new());
    let res = app.get("/version").header("accept", "*/*").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.header("access-control-allow-origin"), Some("*"));
    assert_eq!(res.header("vary"), Some("Accept"));
    assert_eq!(res.json(), expected_version());
    assert_eq!(res.keys(), ["version", "data"]);
    assert!(res.text().ends_with("}\n"));
}

#[tokio::test]
async fn a_missing_data_source_makes_it_unhealthy() {
    for source in DataSource::ALL {
        let app = TestApp::new(FakeDatabases::new().unloaded(source));
        let res = app.get("/version").send().await;
        assert_eq!(res.status, StatusCode::SERVICE_UNAVAILABLE, "{source}");
        let mut expected = expected_version();
        let key = match source {
            DataSource::Location => "location",
            DataSource::Network => "network",
        };
        expected["data"].as_object_mut().unwrap().remove(key);
        assert_eq!(res.json(), expected, "{source}");
    }
    let app = TestApp::new(
        FakeDatabases::new()
            .unloaded(DataSource::Location)
            .unloaded(DataSource::Network),
    );
    let res = app.get("/version").send().await;
    assert_eq!(res.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.json()["data"], json!({}));
}

#[tokio::test]
async fn a_browser_gets_the_version_page() {
    let web = frontend();
    let app = TestApp::with_config(FakeDatabases::new(), &frontend_config(web.path()));
    let res = app.get("/version").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.header("vary"), Some("Accept"));
    assert_eq!(res.header("access-control-allow-origin"), None);
    assert_eq!(res.page_data(), json!({"version": expected_version()}));

    let app = TestApp::with_config(
        FakeDatabases::new().unloaded(DataSource::Network),
        &frontend_config(web.path()),
    );
    let res = app.get("/version").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(res.page_data()["version"]["data"].get("network").is_none());
}

#[tokio::test]
async fn version_is_not_an_ip_and_never_rate_limited() {
    let fake = FakeDatabases::new().with("8.8.8.8", google());
    let lookups = fake.lookups();
    let app = TestApp::with_config(fake, "[rate_limit]\nburst = 1\nper_second = 0.001\n");
    assert_eq!(app.get("/8.8.8.8").send().await.status, StatusCode::OK);
    assert_eq!(
        app.get("/8.8.8.8").send().await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    lookups.lock().unwrap().clear();
    for _ in 0..5 {
        let res = app.get("/version").send().await;
        assert_eq!(res.status, StatusCode::OK);
        assert_eq!(res.json()["version"], env!("CARGO_PKG_VERSION"));
    }
    assert!(lookups.lock().unwrap().is_empty());
}
