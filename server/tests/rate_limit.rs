//! Rate limiting: Explicit Lookups spend a token from the client's bucket
//! (an IPv4 address or an IPv6 /64), which holds `rate_limit.burst` and
//! refills at `rate_limit.per_second`. An empty bucket means a 429 with
//! `Retry-After`. Self Lookups and allow-listed clients are never limited.
//!
//! Every test runs on tokio's paused clock: time only moves when a test
//! advances it.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{FakeDatabases, TestApp, TestResponse, google};

const RATE_LIMITED: &str = r#"{
  "status": 429,
  "error": {
    "title": "Rate limit exceeded",
    "message": "Too many lookups of other addresses. Please retry after 1 second."
  }
}
"#;

fn app(config: &str) -> TestApp {
    TestApp::with_config(FakeDatabases::new().with("8.8.8.8", google()), config)
}

/// Makes `n` Explicit Lookups from `peer`, asserting each succeeds.
async fn spend(app: &TestApp, peer: &str, n: usize) {
    for i in 0..n {
        let res = app.get("/8.8.8.8").from(peer).send().await;
        assert_eq!(res.status, StatusCode::OK, "lookup {i} from {peer}");
    }
}

async fn explicit(app: &TestApp, peer: &str) -> TestResponse {
    app.get("/8.8.8.8").from(peer).send().await
}

#[tokio::test(start_paused = true)]
async fn after_the_default_burst_of_60_comes_a_429() {
    let app = app("");
    spend(&app, "9.9.9.9:1", 60).await;
    let res = explicit(&app, "9.9.9.9:1").await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.header("retry-after"), Some("1"));
    assert_eq!(res.header("access-control-allow-origin"), Some("*"));
    res.json();
    assert_eq!(res.text(), RATE_LIMITED);
}

#[tokio::test(start_paused = true)]
async fn every_explicit_route_spends_a_token() {
    let app = app("[rate_limit]\nburst = 3");
    for uri in ["/8.8.8.8", "/8.8.8.8/json", "/8.8.8.8/city"] {
        assert_eq!(app.get(uri).send().await.status, StatusCode::OK, "{uri}");
    }
    for uri in ["/8.8.8.8", "/8.8.8.8/json", "/8.8.8.8/city", "/10.0.0.1"] {
        let res = app.get(uri).send().await;
        assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS, "{uri}");
    }
}

#[tokio::test(start_paused = true)]
async fn retry_after_says_when_the_next_token_comes() {
    let app = app("[rate_limit]\nburst = 2\nper_second = 0.25");
    spend(&app, "9.9.9.9:1", 2).await;
    let res = explicit(&app, "9.9.9.9:1").await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.header("retry-after"), Some("4"));
    assert!(
        res.text().contains("retry after 4 seconds"),
        "{}",
        res.text()
    );

    tokio::time::advance(Duration::from_secs(3)).await;
    let res = explicit(&app, "9.9.9.9:1").await;
    assert_eq!(res.header("retry-after"), Some("1"));

    tokio::time::advance(Duration::from_secs(1)).await;
    spend(&app, "9.9.9.9:1", 1).await;
    assert_eq!(
        explicit(&app, "9.9.9.9:1").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test(start_paused = true)]
async fn tokens_refill_at_per_second_up_to_burst() {
    let app = app("[rate_limit]\nburst = 10\nper_second = 2");
    spend(&app, "9.9.9.9:1", 10).await;
    assert_eq!(
        explicit(&app, "9.9.9.9:1").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );

    // Three seconds bring back six tokens.
    tokio::time::advance(Duration::from_secs(3)).await;
    spend(&app, "9.9.9.9:1", 6).await;
    assert_eq!(
        explicit(&app, "9.9.9.9:1").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );

    // A long wait refills the bucket only up to `burst`.
    tokio::time::advance(Duration::from_secs(3600)).await;
    spend(&app, "9.9.9.9:1", 10).await;
    assert_eq!(
        explicit(&app, "9.9.9.9:1").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test(start_paused = true)]
async fn each_ipv4_address_has_its_own_bucket() {
    let app = app("[rate_limit]\nburst = 2");
    spend(&app, "198.51.100.1:1", 2).await;
    let res = explicit(&app, "198.51.100.1:2").await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    spend(&app, "198.51.100.2:1", 2).await;
    // An IPv4-mapped peer is the same client.
    let res = explicit(&app, "[::ffff:198.51.100.2]:1").await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test(start_paused = true)]
async fn ipv6_clients_share_a_bucket_per_64() {
    let app = app("[rate_limit]\nburst = 2");
    spend(&app, "[2001:db8:1:2::1]:1", 1).await;
    spend(&app, "[2001:db8:1:2:ffff:ffff:ffff:ffff]:1", 1).await;
    let res = explicit(&app, "[2001:db8:1:2:abcd::9]:1").await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    // The neighbouring /64 is someone else.
    spend(&app, "[2001:db8:1:3::1]:1", 2).await;
}

#[tokio::test(start_paused = true)]
async fn self_lookups_are_never_limited() {
    let app = app("[rate_limit]\nburst = 1");
    spend(&app, "8.8.8.8:1", 1).await;
    assert_eq!(
        explicit(&app, "8.8.8.8:1").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    for _ in 0..100 {
        for uri in ["/", "/json", "/city", "/ip"] {
            let res = app.get(uri).from("8.8.8.8:1").send().await;
            assert_eq!(res.status, StatusCode::OK, "{uri}");
        }
    }
}

#[tokio::test(start_paused = true)]
async fn wrong_ip_spends_no_token() {
    let app = app("[rate_limit]\nburst = 1");
    for uri in ["/example.com", "/8.8.8.8/nope", "/a/b/c"] {
        let res = app.get(uri).send().await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{uri}");
    }
    spend(&app, common::DEFAULT_PEER, 1).await;
}

#[tokio::test(start_paused = true)]
async fn allow_listed_clients_are_never_limited() {
    let app = app("[rate_limit]\nburst = 1\nallow = [\"203.0.113.7\", \"2001:db8::/32\"]");
    spend(&app, "203.0.113.7:1", 100).await;
    spend(&app, "[2001:db8:5::1]:1", 100).await;
    spend(&app, "[::ffff:203.0.113.7]:1", 100).await;
    // Their neighbours are not.
    spend(&app, "203.0.113.8:1", 1).await;
    assert_eq!(
        explicit(&app, "203.0.113.8:1").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test(start_paused = true)]
async fn the_client_behind_a_trusted_proxy_is_limited_not_the_proxy() {
    let app = app(
        "trusted_proxies = [\"127.0.0.1\"]\n[rate_limit]\nburst = 1\nallow = [\"203.0.113.7\"]",
    );
    let via_proxy = |real_ip: &'static str| {
        app.get("/8.8.8.8")
            .from("127.0.0.1:1")
            .header("x-real-ip", real_ip)
    };
    assert_eq!(
        via_proxy("198.51.100.1").send().await.status,
        StatusCode::OK
    );
    let res = via_proxy("198.51.100.1").send().await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    // Another client through the same proxy has its own bucket.
    assert_eq!(
        via_proxy("198.51.100.2").send().await.status,
        StatusCode::OK
    );
    // The allow-list applies to the real client too.
    for _ in 0..10 {
        assert_eq!(via_proxy("203.0.113.7").send().await.status, StatusCode::OK);
    }
    // An untrusted peer's X-Real-IP is ignored: the peer is the client.
    spend(&app, "198.51.100.9:1", 1).await;
    let res = app
        .get("/8.8.8.8")
        .from("198.51.100.9:1")
        .header("x-real-ip", "203.0.113.7")
        .send()
        .await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
}
