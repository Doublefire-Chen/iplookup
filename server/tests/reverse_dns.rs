//! Reverse DNS: `hostname` is the address's PTR name, when it answers within
//! `dns.timeout`; answers are cached for their TTL (at most an hour).
//!
//! Time-sensitive tests run on tokio's paused clock, so they are instant.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{FakeDatabases, FakeReverseDns, TestApp, google};
use iplookup::reverse_dns::{HickoryPtr, PtrResolver};
use tokio::time::Instant;

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(60 * 60);

fn app(reverse_dns: FakeReverseDns) -> TestApp {
    app_with_config(reverse_dns, "")
}

fn app_with_config(reverse_dns: FakeReverseDns, config: &str) -> TestApp {
    let databases = FakeDatabases::new().with("8.8.8.8", google());
    TestApp::with_reverse_dns(databases, reverse_dns, config)
}

fn google_dns() -> FakeReverseDns {
    FakeReverseDns::new().name("8.8.8.8", "dns.google", HOUR)
}

#[tokio::test]
async fn self_lookup_has_hostname_right_after_ip() {
    let app = app(google_dns());
    let res = app.get("/json").from("8.8.8.8:4000").send().await;
    assert_eq!(res.json()["hostname"], "dns.google");
    assert_eq!(res.keys()[..3], ["ip", "hostname", "city"]);
}

#[tokio::test]
async fn explicit_lookup_has_hostname() {
    let app = app(google_dns());
    for uri in ["/8.8.8.8", "/8.8.8.8/json", "/::ffff:8.8.8.8"] {
        let res = app.get(uri).send().await;
        assert_eq!(res.status, StatusCode::OK, "{uri}");
        assert_eq!(res.json()["hostname"], "dns.google", "{uri}");
    }
}

#[tokio::test]
async fn hostname_field_returns_the_ptr_name() {
    let app = app(google_dns());
    let res = app.get("/8.8.8.8/hostname").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.text(), "dns.google\n");
    let res = app.get("/hostname").from("8.8.8.8:4000").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.text(), "dns.google\n");
}

#[tokio::test]
async fn no_ptr_leaves_hostname_out() {
    let app = app(FakeReverseDns::new().no_ptr("8.8.8.8", HOUR));
    let res = app.get("/8.8.8.8").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(
        !res.keys().contains(&"hostname".to_owned()),
        "{}",
        res.text()
    );
    let res = app.get("/8.8.8.8/hostname").send().await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.text(), "");
}

#[tokio::test(start_paused = true)]
async fn slow_answer_is_given_up_after_the_default_timeout() {
    let app = app(FakeReverseDns::new().slow("8.8.8.8", MINUTE, "dns.google", HOUR));
    let start = Instant::now();
    let res = app.get("/8.8.8.8").send().await;
    let elapsed = start.elapsed();
    assert_eq!(res.status, StatusCode::OK);
    let record = res.json();
    assert_eq!(record.get("hostname"), None);
    assert_eq!(record["city"], "Mountain View");
    assert!(
        (Duration::from_millis(500)..Duration::from_millis(550)).contains(&elapsed),
        "{elapsed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn timeout_is_configurable() {
    let reverse_dns =
        FakeReverseDns::new().slow("8.8.8.8", Duration::from_millis(300), "dns.google", HOUR);
    let app = app_with_config(reverse_dns, "[dns]\ntimeout = \"100ms\"");
    let start = Instant::now();
    let res = app.get("/8.8.8.8").send().await;
    let elapsed = start.elapsed();
    assert_eq!(res.json().get("hostname"), None);
    assert!(
        (Duration::from_millis(100)..Duration::from_millis(150)).contains(&elapsed),
        "{elapsed:?}"
    );

    // An answer within a longer timeout is used.
    let reverse_dns =
        FakeReverseDns::new().slow("8.8.8.8", Duration::from_millis(300), "dns.google", HOUR);
    let app = app_with_config(reverse_dns, "[dns]\ntimeout = \"1s\"");
    let res = app.get("/8.8.8.8").send().await;
    assert_eq!(res.json()["hostname"], "dns.google");
}

#[tokio::test(start_paused = true)]
async fn repeated_lookups_within_the_ttl_hit_the_cache() {
    let reverse_dns = FakeReverseDns::new().name("8.8.8.8", "dns.google", 10 * MINUTE);
    let queries = reverse_dns.queries();
    let app = app(reverse_dns);
    for uri in [
        "/8.8.8.8",
        "/8.8.8.8/json",
        "/8.8.8.8/hostname",
        "/::ffff:8.8.8.8",
    ] {
        let res = app.get(uri).send().await;
        assert_eq!(res.status, StatusCode::OK, "{uri}");
    }
    let res = app.get("/json").from("8.8.8.8:4000").send().await;
    assert_eq!(res.json()["hostname"], "dns.google");
    assert_eq!(queries.lock().unwrap().len(), 1);

    tokio::time::advance(9 * MINUTE).await;
    app.get("/8.8.8.8").send().await;
    assert_eq!(queries.lock().unwrap().len(), 1, "still within the TTL");

    tokio::time::advance(2 * MINUTE).await;
    let res = app.get("/8.8.8.8").send().await;
    assert_eq!(res.json()["hostname"], "dns.google");
    assert_eq!(queries.lock().unwrap().len(), 2, "TTL expired");
}

#[tokio::test(start_paused = true)]
async fn no_ptr_and_timeouts_are_cached_too() {
    let reverse_dns = FakeReverseDns::new().no_ptr("8.8.8.8", 10 * MINUTE).slow(
        "8.8.4.4",
        MINUTE,
        "dns.google",
        HOUR,
    );
    let queries = reverse_dns.queries();
    let app = app(reverse_dns);
    for _ in 0..3 {
        app.get("/8.8.8.8").send().await;
        let start = Instant::now();
        let res = app.get("/8.8.4.4").send().await;
        assert_eq!(res.json().get("hostname"), None);
        assert!(start.elapsed() < Duration::from_millis(550));
    }
    assert_eq!(
        queries.lock().unwrap().len(),
        2,
        "{:?}",
        queries.lock().unwrap()
    );
}

#[tokio::test(start_paused = true)]
async fn ttl_is_capped_at_an_hour() {
    let reverse_dns = FakeReverseDns::new().name("8.8.8.8", "dns.google", 24 * HOUR);
    let queries = reverse_dns.queries();
    let app = app(reverse_dns);
    app.get("/8.8.8.8").send().await;
    tokio::time::advance(HOUR - MINUTE).await;
    app.get("/8.8.8.8").send().await;
    assert_eq!(queries.lock().unwrap().len(), 1);
    tokio::time::advance(2 * MINUTE).await;
    app.get("/8.8.8.8").send().await;
    assert_eq!(queries.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn bogons_never_reach_the_resolver() {
    let reverse_dns = FakeReverseDns::new()
        .name("10.0.0.1", "router.lan", HOUR)
        .name("192.168.1.2", "laptop.lan", HOUR);
    let queries = reverse_dns.queries();
    let app = app(reverse_dns);
    for uri in [
        "/10.0.0.1",
        "/10.0.0.1/json",
        "/10.0.0.1/hostname",
        "/::ffff:10.0.0.1",
        "/127.0.0.1",
        "/::1",
        "/fe80::1",
    ] {
        let res = app.get(uri).send().await;
        assert!(
            res.text().contains("\"bogon\": true") || res.text().is_empty(),
            "{uri}"
        );
    }
    let res = app.get("/json").from("192.168.1.2:4000").send().await;
    assert_eq!(res.json()["bogon"], true);
    let res = app.get("/hostname").from("192.168.1.2:4000").send().await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert!(
        queries.lock().unwrap().is_empty(),
        "{:?}",
        queries.lock().unwrap()
    );
}

/// The real resolver, against the system's DNS. Needs network, so it only
/// runs on request: `cargo test --test reverse_dns -- --ignored`.
#[tokio::test]
#[ignore = "needs network DNS"]
async fn real_resolver_finds_google_dns() {
    let ptr = HickoryPtr::new(None, Duration::from_secs(5)).expect("system DNS config");
    let answer = ptr.ptr("8.8.8.8".parse().unwrap()).await;
    assert_eq!(answer.name.as_deref(), Some("dns.google"));
    assert!(answer.ttl > Duration::ZERO);
    let answer = ptr.ptr("192.0.2.1".parse().unwrap()).await;
    assert_eq!(answer.name, None);
}
