//! Self Lookup: `GET /` and `GET /json` return the caller's Record as JSON.

mod common;

use axum::http::StatusCode;
use common::{FakeDatabases, TestApp};
use iplookup::databases::{Coordinates, IpData, LocationData, NetworkData};
use serde_json::json;

const README: &str = "https://github.com/Doublefire-Chen/iplookup/blob/main/README.md";

fn google() -> IpData {
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

#[tokio::test]
async fn full_record_has_ipinfo_keys_then_the_extras() {
    let app = TestApp::new(FakeDatabases::new().with("8.8.8.8", google()));
    let res = app.get("/").from("8.8.8.8:4000").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(
        res.json(),
        json!({
            "ip": "8.8.8.8",
            "city": "Mountain View",
            "region": "California",
            "country": "US",
            "loc": "37.4056,-122.0775",
            "org": "AS15169 Google LLC",
            "postal": "94043",
            "timezone": "America/Los_Angeles",
            "readme": README,
            "country_name": "United States",
            "continent": "North America",
            "accuracy_radius": 1000,
            "asn": {"asn": "AS15169", "name": "Google LLC", "domain": "google.com"},
        })
    );
    assert_eq!(
        res.keys(),
        [
            "ip",
            "city",
            "region",
            "country",
            "loc",
            "org",
            "postal",
            "timezone",
            "readme",
            "country_name",
            "continent",
            "accuracy_radius",
            "asn",
        ]
    );
    assert!(res.text().ends_with("}\n"), "{:?}", res.text());
}

#[tokio::test]
async fn loc_always_has_four_decimals() {
    let mut data = google();
    data.location.as_mut().unwrap().coordinates = Some(Coordinates {
        latitude: 37.751,
        longitude: -97.8,
    });
    let app = TestApp::new(FakeDatabases::new().with("8.8.8.8", data));
    let res = app.get("/json").from("8.8.8.8:4000").send().await;
    assert_eq!(res.json()["loc"], "37.7510,-97.8000");
}

#[tokio::test]
async fn missing_values_are_left_out() {
    // No city, region, postal or coordinates, and no Network Data at all.
    let data = IpData {
        location: Some(LocationData {
            country: Some("AU".into()),
            country_name: Some("Australia".into()),
            ..Default::default()
        }),
        network: None,
    };
    let app = TestApp::new(FakeDatabases::new().with("1.1.1.1", data));
    let res = app.get("/").from("1.1.1.1:4000").send().await;
    assert_eq!(
        res.json(),
        json!({"ip": "1.1.1.1", "country": "AU", "readme": README, "country_name": "Australia"})
    );
    assert!(!res.text().contains("null"));
    assert!(!res.text().contains("\"\""));
}

#[tokio::test]
async fn an_unknown_address_gets_only_ip_and_readme() {
    let app = TestApp::new(FakeDatabases::new());
    let res = app.get("/json").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.json(), json!({"ip": "203.0.113.7", "readme": README}));
    assert!(res.text().ends_with('\n'));
}

#[tokio::test]
async fn org_without_a_name_is_just_the_asn_and_no_asn_means_no_org() {
    let network = |asn: Option<&str>, name: Option<&str>| IpData {
        location: None,
        network: Some(NetworkData {
            asn: asn.map(Into::into),
            name: name.map(Into::into),
            domain: None,
        }),
    };
    let app = TestApp::new(
        FakeDatabases::new()
            .with("192.0.2.1", network(Some("AS64500"), None))
            .with("192.0.2.2", network(None, Some("Nameless Networks"))),
    );
    let res = app.get("/").from("192.0.2.1:1").send().await.json();
    assert_eq!(res["org"], "AS64500");
    assert_eq!(res["asn"], json!({"asn": "AS64500"}));
    let res = app.get("/").from("192.0.2.2:1").send().await.json();
    assert!(res.get("org").is_none(), "{res}");
    assert_eq!(res["asn"], json!({"name": "Nameless Networks"}));
}

#[tokio::test]
async fn curl_gets_json_from_root_and_json_is_json_even_for_browsers() {
    let app = TestApp::new(FakeDatabases::new().with("8.8.8.8", google()));
    let res = app
        .get("/")
        .from("8.8.8.8:4000")
        .header("accept", "*/*")
        .send()
        .await;
    assert_eq!(res.json()["org"], "AS15169 Google LLC");
    let res = app
        .get("/json")
        .from("8.8.8.8:4000")
        .header(
            "accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.json()["org"], "AS15169 Google LLC");
}

#[tokio::test]
async fn ipv6_peers_get_their_ipv6_record() {
    let app = TestApp::new(FakeDatabases::new().with("2001:4860:4860::8888", google()));
    let res = app
        .get("/json")
        .from("[2001:4860:4860::8888]:4000")
        .send()
        .await;
    assert_eq!(res.json()["ip"], "2001:4860:4860::8888");
    assert_eq!(res.json()["country"], "US");
}

#[tokio::test]
async fn a_mapped_ipv4_peer_is_reported_and_looked_up_as_plain_ipv4() {
    let app = TestApp::new(FakeDatabases::new().with("8.8.8.8", google()));
    let res = app.get("/json").from("[::ffff:8.8.8.8]:4000").send().await;
    assert_eq!(res.json()["ip"], "8.8.8.8");
    assert_eq!(res.json()["country"], "US");
}

#[tokio::test]
async fn x_real_ip_is_ignored_from_untrusted_peers() {
    // The default config trusts no proxy.
    let app = TestApp::new(FakeDatabases::new().with("8.8.8.8", google()));
    let res = app
        .get("/json")
        .from("127.0.0.1:4000")
        .header("x-real-ip", "8.8.8.8")
        .send()
        .await;
    assert_eq!(res.json(), json!({"ip": "127.0.0.1", "readme": README}));

    // Trusting one proxy doesn't make other peers trusted.
    let app = TestApp::with_config(
        FakeDatabases::new().with("8.8.8.8", google()),
        "trusted_proxies = [\"10.0.0.0/8\"]",
    );
    let res = app
        .get("/json")
        .from("192.0.2.9:4000")
        .header("x-real-ip", "8.8.8.8")
        .send()
        .await;
    assert_eq!(res.json()["ip"], "192.0.2.9");
}

#[tokio::test]
async fn x_real_ip_is_used_from_trusted_proxies() {
    let app = TestApp::with_config(
        FakeDatabases::new()
            .with("8.8.8.8", google())
            .with("2001:4860:4860::8888", google()),
        "trusted_proxies = [\"127.0.0.1\", \"::1\", \"10.0.0.0/8\"]",
    );
    for peer in ["127.0.0.1:4000", "[::1]:4000", "10.1.2.3:4000"] {
        let res = app
            .get("/json")
            .from(peer)
            .header("x-real-ip", "8.8.8.8")
            .send()
            .await;
        assert_eq!(res.json()["ip"], "8.8.8.8", "via {peer}");
        assert_eq!(res.json()["country"], "US", "via {peer}");
    }
    // A trusted proxy reached over a mapped address is still trusted, and
    // a mapped X-Real-IP is reported as plain IPv4.
    let res = app
        .get("/")
        .from("[::ffff:127.0.0.1]:4000")
        .header("x-real-ip", "::ffff:8.8.8.8")
        .send()
        .await;
    assert_eq!(res.json()["ip"], "8.8.8.8");
    let res = app
        .get("/")
        .from("127.0.0.1:4000")
        .header("x-real-ip", "2001:4860:4860::8888")
        .send()
        .await;
    assert_eq!(res.json()["ip"], "2001:4860:4860::8888");
}

#[tokio::test]
async fn a_trusted_proxy_without_a_valid_x_real_ip_is_the_client() {
    let app = TestApp::with_config(FakeDatabases::new(), "trusted_proxies = [\"127.0.0.1\"]");
    let res = app.get("/json").from("127.0.0.1:4000").send().await;
    assert_eq!(res.json()["ip"], "127.0.0.1");
    let res = app
        .get("/json")
        .from("127.0.0.1:4000")
        .header("x-real-ip", "not an ip")
        .send()
        .await;
    assert_eq!(res.json()["ip"], "127.0.0.1");
}
