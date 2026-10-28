//! Fields: `GET /{field}` (Self Lookup) and `GET /{ip}/{field}` (Explicit
//! Lookup) return one value plus a newline as plain text, whatever the
//! `Accept` header; a Field with no value is a 404 with an empty body.
//! Also CORS on JSON and plain-text responses.

mod common;

use axum::http::StatusCode;
use common::{FakeDatabases, TestApp, google};
use iplookup::databases::{IpData, LocationData};

const BROWSER: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

/// Every Field but `hostname` (no reverse DNS yet), with 8.8.8.8's values.
const GOOGLE_FIELDS: [(&str, &str); 8] = [
    ("ip", "8.8.8.8"),
    ("city", "Mountain View"),
    ("region", "California"),
    ("country", "US"),
    ("loc", "37.4056,-122.0775"),
    ("org", "AS15169 Google LLC"),
    ("postal", "94043"),
    ("timezone", "America/Los_Angeles"),
];

const FIELDS: [&str; 9] = [
    "ip", "hostname", "city", "region", "country", "loc", "org", "postal", "timezone",
];

fn app() -> TestApp {
    TestApp::new(
        FakeDatabases::new()
            .with("8.8.8.8", google())
            .with("2001:4860:4860::8888", google()),
    )
}

fn assert_text(res: &common::TestResponse, expected: &str, context: &str) {
    assert_eq!(res.status, StatusCode::OK, "{context}");
    assert_eq!(
        res.header("content-type"),
        Some("text/plain; charset=utf-8"),
        "{context}"
    );
    assert_eq!(res.text(), format!("{expected}\n"), "{context}");
}

fn assert_empty_404(res: &common::TestResponse, context: &str) {
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{context}");
    assert!(res.body.is_empty(), "{context}: {:?}", res.text());
}

#[tokio::test]
async fn ip_is_the_callers_address_as_plain_text() {
    let app = app();
    let res = app.get("/ip").from("8.8.8.8:4000").send().await;
    assert_text(&res, "8.8.8.8", "/ip");
    let res = app
        .get("/ip")
        .from("[2001:4860:4860::8888]:4000")
        .send()
        .await;
    assert_text(&res, "2001:4860:4860::8888", "/ip over IPv6");
}

#[tokio::test]
async fn every_field_works_for_self_and_explicit_lookups() {
    let app = app();
    for (field, value) in GOOGLE_FIELDS {
        let path = format!("/{field}");
        let res = app.get(&path).from("8.8.8.8:4000").send().await;
        assert_text(&res, value, &path);

        let path = format!("/8.8.8.8/{field}");
        let res = app.get(&path).from("1.1.1.1:4000").send().await;
        assert_text(&res, value, &path);
    }
    // An Explicit Lookup's `ip` echoes the address as typed.
    let res = app.get("/2001:4860:4860:0::8888/ip").send().await;
    assert_text(&res, "2001:4860:4860:0::8888", "IPv6 /ip");
    let res = app.get("/::ffff:8.8.8.8/org").send().await;
    assert_text(&res, "AS15169 Google LLC", "mapped /org");
}

#[tokio::test]
async fn fields_are_plain_text_whatever_the_accept_header() {
    let app = app();
    for accept in [BROWSER, "text/html", "application/json", "*/*"] {
        for path in ["/city", "/8.8.8.8/city"] {
            let res = app
                .get(path)
                .from("8.8.8.8:4000")
                .header("accept", accept)
                .send()
                .await;
            assert_text(&res, "Mountain View", &format!("{path} {accept}"));
        }
    }
}

#[tokio::test]
async fn a_field_without_a_value_is_an_empty_404() {
    let sparse = IpData {
        location: Some(LocationData {
            country: Some("AU".into()),
            ..Default::default()
        }),
        network: None,
    };
    let app = TestApp::new(FakeDatabases::new().with("1.1.1.1", sparse));
    for field in [
        "hostname", "city", "region", "loc", "org", "postal", "timezone",
    ] {
        for path in [format!("/{field}"), format!("/1.1.1.1/{field}")] {
            let res = app
                .get(&path)
                .from("1.1.1.1:4000")
                .header("accept", BROWSER)
                .send()
                .await;
            assert_empty_404(&res, &path);
        }
    }
    let res = app.get("/1.1.1.1/country").send().await;
    assert_text(&res, "AU", "/country");
}

#[tokio::test]
async fn a_bogons_fields_are_its_address_and_nothing_else() {
    let fake = FakeDatabases::new().with("10.0.0.1", google());
    let lookups = fake.lookups();
    let app = TestApp::new(fake);
    for field in FIELDS {
        let explicit = format!("/10.0.0.1/{field}");
        let own = format!("/{field}");
        for res in [
            app.get(&explicit).send().await,
            app.get(&own).from("10.0.0.1:4000").send().await,
        ] {
            if field == "ip" {
                assert_text(&res, "10.0.0.1", field);
            } else {
                assert_empty_404(&res, field);
            }
        }
    }
    // Echoed as typed for an Explicit Lookup.
    let res = app.get("/2001:DB8::1/ip").send().await;
    assert_text(&res, "2001:DB8::1", "/2001:DB8::1/ip");
    assert!(lookups.lock().unwrap().is_empty());
}

#[tokio::test]
async fn extra_keys_are_not_fields() {
    let app = app();
    for extra in [
        "asn",
        "country_name",
        "continent",
        "accuracy_radius",
        "readme",
        "bogon",
        "IP",
    ] {
        for path in [format!("/{extra}"), format!("/8.8.8.8/{extra}")] {
            let res = app.get(&path).from("8.8.8.8:4000").send().await;
            assert_eq!(res.status, StatusCode::NOT_FOUND, "{path}");
            assert_eq!(res.json()["error"]["title"], "Wrong ip", "{path}");
        }
    }
    // A Field name is not an address, nor a Field of a Field.
    for path in ["/ip/ip", "/city/json", "/8.8.8.8/ip/ip"] {
        let res = app.get(path).send().await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(res.json()["error"]["title"], "Wrong ip", "{path}");
    }
}

#[tokio::test]
async fn json_and_plain_text_responses_allow_any_origin() {
    let app = app();
    for path in [
        "/",
        "/json",
        "/8.8.8.8",
        "/8.8.8.8/json",
        "/10.0.0.1",
        "/city",
        "/8.8.8.8/city",
        "/hostname",
        "/hello",
        "/asn",
    ] {
        let res = app.get(path).from("8.8.8.8:4000").send().await;
        assert_eq!(
            res.header("access-control-allow-origin"),
            Some("*"),
            "{path}"
        );
    }
}
