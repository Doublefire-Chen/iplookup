//! Explicit Lookup: `GET /{ip}` and `GET /{ip}/json` return the Record of
//! the address in the path, with ipinfo.io's edge cases (ADR 0001).

mod common;

use axum::http::StatusCode;
use common::{FakeDatabases, TestApp, google};
use serde_json::json;

const WRONG_IP: &str = r#"{
  "status": 404,
  "error": {
    "title": "Wrong ip",
    "message": "Please provide a valid IP address"
  }
}
"#;

fn app() -> TestApp {
    TestApp::new(
        FakeDatabases::new()
            .with("8.8.8.8", google())
            .with("2001:4860:4860::8888", google()),
    )
}

#[tokio::test]
async fn ipv4_and_ipv6_records_with_and_without_json() {
    let app = app();
    for ip in ["8.8.8.8", "2001:4860:4860::8888"] {
        for path in [format!("/{ip}"), format!("/{ip}/json")] {
            let res = app.get(&path).header("accept", "*/*").send().await;
            assert_eq!(res.status, StatusCode::OK, "{path}");
            let record = res.json();
            assert_eq!(record["ip"], ip, "{path}");
            assert_eq!(record["org"], "AS15169 Google LLC", "{path}");
            assert_eq!(record["country"], "US", "{path}");
            assert!(res.text().ends_with("}\n"));
        }
    }
    // Without an Accept header too, and the caller's own address is irrelevant.
    let res = app.get("/8.8.8.8").from("1.1.1.1:4000").send().await;
    assert_eq!(res.json()["ip"], "8.8.8.8");
}

#[tokio::test]
async fn ip_is_echoed_exactly_as_typed() {
    let app = app();
    for typed in [
        "2001:4860:4860:0:0::8888",
        "2001:4860:4860:0000::8888",
        "2001:4860:4860::8888",
    ] {
        let res = app.get(&format!("/{typed}/json")).send().await.json();
        assert_eq!(res["ip"], typed);
        assert_eq!(res["country"], "US", "{typed}");
    }
    // Upper case stays upper case (this one is a Bogon, which echoes too).
    let res = app.get("/2001:DB8:0:0::1").send().await.json();
    assert_eq!(res["ip"], "2001:DB8:0:0::1");
}

#[tokio::test]
async fn a_mapped_ipv4_address_is_looked_up_as_ipv4_but_echoed_as_typed() {
    let app = app();
    for path in ["/::ffff:8.8.8.8", "/::ffff:8.8.8.8/json", "/::FFFF:808:808"] {
        let res = app.get(path).send().await;
        assert_eq!(res.status, StatusCode::OK, "{path}");
        let record = res.json();
        assert_eq!(
            record["ip"],
            path.trim_start_matches('/').trim_end_matches("/json")
        );
        assert_eq!(record["org"], "AS15169 Google LLC", "{path}");
    }
}

#[tokio::test]
async fn bogons_get_only_ip_and_bogon_and_never_reach_the_databases() {
    let fake = FakeDatabases::new();
    let lookups = fake.lookups();
    let app = TestApp::new(fake);
    let bogons = [
        "10.0.0.1",        // private
        "172.16.5.4",      // private
        "192.168.1.1",     // private
        "127.0.0.1",       // loopback
        "169.254.10.20",   // link-local
        "192.0.2.1",       // documentation
        "198.51.100.1",    // documentation
        "203.0.113.9",     // documentation
        "100.64.0.1",      // CGNAT
        "224.0.0.1",       // multicast
        "0.0.0.0",         // reserved
        "240.0.0.1",       // reserved
        "255.255.255.255", // broadcast
        "::1",             // loopback
        "::",              // unspecified
        "fe80::1",         // link-local
        "fd00::1",         // unique local
        "2001:db8::1",     // documentation
        "ff02::1",         // multicast
        "100::1",          // discard-only
        "::ffff:10.0.0.1", // mapped private
        "2002:a00:1::1",   // 6to4 of a private address
    ];
    for ip in bogons {
        for path in [format!("/{ip}"), format!("/{ip}/json")] {
            let res = app.get(&path).send().await;
            assert_eq!(res.status, StatusCode::OK, "{path}");
            assert_eq!(res.json(), json!({"ip": ip, "bogon": true}), "{path}");
            assert_eq!(res.keys(), ["ip", "bogon"]);
            assert!(res.text().ends_with("}\n"));
        }
    }
    assert!(
        lookups.lock().unwrap().is_empty(),
        "{:?}",
        lookups.lock().unwrap()
    );
}

#[tokio::test]
async fn invalid_input_gets_the_wrong_ip_404() {
    let fake = FakeDatabases::new();
    let lookups = fake.lookups();
    let app = TestApp::new(fake);
    let invalid = [
        "/8.8.8.08",
        "/999.1.1.1",
        "/hello",
        "/google.com",
        "/8.8.8",
        "/8.8.8.8.8",
        "/0x8.8.8.8",
        "/2001:db8::g",
        "/2001:db8:::1",
        "/fe80::1%25eth0",
        "/::ffff:8.8.8.08",
        "/%20",
        "/8.8.8.8/hello",
        "/8.8.8.8/json/extra",
        "/hello/json",
        "/a/b/c",
    ];
    for path in invalid {
        for accept in ["*/*", "application/json"] {
            let res = app.get(path).header("accept", accept).send().await;
            assert_eq!(res.status, StatusCode::NOT_FOUND, "{path}");
            assert_eq!(
                res.header("content-type"),
                Some("application/json; charset=utf-8")
            );
            assert_eq!(res.text(), WRONG_IP, "{path}");
        }
    }
    assert!(lookups.lock().unwrap().is_empty());
}

#[tokio::test]
async fn reserved_names_are_never_treated_as_addresses() {
    let fake = FakeDatabases::new();
    let lookups = fake.lookups();
    let app = TestApp::new(fake);
    let reserved = [
        "json",
        "version",
        "ip",
        "hostname",
        "city",
        "region",
        "country",
        "loc",
        "org",
        "postal",
        "timezone",
        "assets",
        "assets/index.js",
        "favicon.ico",
        "index.html",
    ];
    for name in reserved {
        let res = app.get(&format!("/{name}")).send().await;
        assert_ne!(res.text().trim_end(), name, "/{name}");
        assert!(
            !res.text().contains(&format!(r#""ip": "{name}""#)),
            "/{name}"
        );
        assert!(!res.text().contains("bogon"), "/{name}: {}", res.text());
    }
    // Only Self Lookups (`/json` and the Fields) have looked anything up.
    let peer: std::net::IpAddr = "9.9.9.9".parse().unwrap();
    let lookups = lookups.lock().unwrap();
    assert_eq!(lookups.len(), 10, "{lookups:?}");
    assert!(lookups.iter().all(|ip| *ip == peer), "{lookups:?}");
}
