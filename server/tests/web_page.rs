//! Web pages: the page-or-data decision, Record injection into the
//! frontend's `index.html`, its assets, and HTTPS for pages (ADR 0002).

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{FakeDatabases, FakeReverseDns, TestApp, frontend, frontend_config, google};
use serde_json::json;

const BROWSER: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
const PROXY: &str = "trusted_proxies = [\"127.0.0.1\"]\n";

fn app(config: &str) -> (TestApp, tempfile::TempDir) {
    let web = frontend();
    let config = format!("{config}{}", frontend_config(web.path()));
    let fake = FakeDatabases::new().with("8.8.8.8", google());
    let dns = FakeReverseDns::new().name("8.8.8.8", "dns.google", Duration::from_secs(60));
    (TestApp::with_reverse_dns(fake, dns, &config), web)
}

#[tokio::test]
async fn a_browser_gets_the_home_page_with_its_record() {
    let (app, _web) = app("");
    let res = app
        .get("/")
        .from("8.8.8.8:4000")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.header("cache-control"), Some("no-store"));
    assert_eq!(res.header("vary"), Some("Accept"));
    assert_eq!(res.header("access-control-allow-origin"), None);
    let data = res.page_data();
    assert_eq!(data["record"]["ip"], "8.8.8.8");
    assert_eq!(data["record"]["hostname"], "dns.google");
    assert_eq!(data["record"]["org"], "AS15169 Google LLC");
    // The rest of index.html is untouched.
    let html = res.text();
    assert!(html.contains(r#"<script type="module" src="/assets/index-abc123.js">"#));
    assert!(html.ends_with("</head><body><div id=\"root\"></div></body></html>\n"));
}

#[tokio::test]
async fn the_page_shows_a_bogon_record() {
    let (app, _web) = app("");
    let res = app
        .get("/")
        .from("192.168.1.2:4000")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(
        res.page_data(),
        json!({"record": {"ip": "192.168.1.2", "bogon": true}})
    );
}

#[tokio::test]
async fn curl_still_gets_json() {
    let (app, _web) = app("");
    for accept in [None, Some("*/*"), Some("application/json")] {
        let mut req = app.get("/").from("8.8.8.8:4000");
        if let Some(accept) = accept {
            req = req.header("accept", accept);
        }
        let res = req.send().await;
        assert_eq!(res.status, StatusCode::OK, "{accept:?}");
        assert_eq!(res.json()["ip"], "8.8.8.8", "{accept:?}");
        assert_eq!(res.header("vary"), Some("Accept"));
    }
}

#[tokio::test]
async fn json_and_field_paths_never_return_the_page() {
    let (app, _web) = app("");
    let res = app
        .get("/json")
        .from("8.8.8.8:4000")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(res.json()["ip"], "8.8.8.8");
    let res = app
        .get("/city")
        .from("8.8.8.8:4000")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(
        res.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(res.text(), "Mountain View\n");
    let res = app
        .get("/8.8.8.8/json")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(res.json()["ip"], "8.8.8.8");
    let res = app
        .get("/8.8.8.8/org")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(res.text(), "AS15169 Google LLC\n");
}

#[tokio::test]
async fn assets_are_served_and_cached_for_good() {
    let (app, _web) = app("");
    let res = app.get("/assets/index-abc123.js").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.text(), "console.log(\"iplookup\");\n");
    assert_eq!(
        res.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert!(
        res.header("content-type").unwrap().contains("javascript"),
        "{:?}",
        res.header("content-type")
    );

    let res = app.get("/assets/missing.js").send().await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.header("cache-control"), None);
    let res = app.get("/assets/../index.html").send().await;
    assert_ne!(res.status, StatusCode::OK);
}

#[tokio::test]
async fn a_missing_frontend_is_a_server_error_not_a_crash() {
    let web = tempfile::tempdir().unwrap();
    let app = TestApp::with_config(FakeDatabases::new(), &frontend_config(web.path()));
    let res = app.get("/").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        res.text(),
        "The web pages are not installed on this server.\n"
    );
    // Data is still served.
    let res = app.get("/").send().await;
    assert_eq!(res.status, StatusCode::OK);
}

#[tokio::test]
async fn a_page_request_over_http_is_redirected_to_https() {
    let (app, _web) = app(PROXY);
    for (uri, location) in [
        ("/", "https://ip.example.com/"),
        ("/8.8.8.8?x=1", "https://ip.example.com/8.8.8.8?x=1"),
        ("/city", "https://ip.example.com/city"),
    ] {
        let res = app
            .get(uri)
            .from("127.0.0.1:4000")
            .header("host", "ip.example.com")
            .header("x-forwarded-proto", "http")
            .header("x-real-ip", "8.8.8.8")
            .header("accept", BROWSER)
            .send()
            .await;
        assert_eq!(res.status, StatusCode::MOVED_PERMANENTLY, "{uri}");
        assert_eq!(res.header("location"), Some(location), "{uri}");
        assert_eq!(res.header("strict-transport-security"), None);
    }
}

#[tokio::test]
async fn the_redirect_drops_a_port_in_host() {
    let (app, _web) = app(PROXY);
    let res = app
        .get("/")
        .from("127.0.0.1:4000")
        .header("host", "ip.example.com:80")
        .header("x-forwarded-proto", "http")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(res.header("location"), Some("https://ip.example.com/"));
}

#[tokio::test]
async fn non_html_requests_over_http_are_answered() {
    let (app, _web) = app(PROXY);
    for (uri, accept) in [
        ("/", "*/*"),
        ("/json", "*/*"),
        ("/city", "*/*"),
        ("/", "application/json"),
    ] {
        let res = app
            .get(uri)
            .from("127.0.0.1:4000")
            .header("host", "ip.example.com")
            .header("x-forwarded-proto", "http")
            .header("x-real-ip", "8.8.8.8")
            .header("accept", accept)
            .send()
            .await;
        assert_eq!(res.status, StatusCode::OK, "{uri} {accept}");
        assert_eq!(res.header("strict-transport-security"), None);
    }
}

#[tokio::test]
async fn https_responses_carry_hsts() {
    let (app, _web) = app(PROXY);
    for accept in [BROWSER, "*/*"] {
        let res = app
            .get("/")
            .from("127.0.0.1:4000")
            .header("host", "ip.example.com")
            .header("x-forwarded-proto", "https")
            .header("x-real-ip", "8.8.8.8")
            .header("accept", accept)
            .send()
            .await;
        assert_eq!(res.status, StatusCode::OK);
        assert_eq!(
            res.header("strict-transport-security"),
            Some("max-age=31536000")
        );
    }
    // Errors too.
    let res = app
        .get("/hello")
        .from("127.0.0.1:4000")
        .header("x-forwarded-proto", "https")
        .send()
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(
        res.header("strict-transport-security"),
        Some("max-age=31536000")
    );
}

#[tokio::test]
async fn x_forwarded_proto_from_an_untrusted_peer_is_ignored() {
    let (app, _web) = app(PROXY);
    for proto in ["http", "https"] {
        let res = app
            .get("/")
            .from("203.0.113.9:4000")
            .header("host", "ip.example.com")
            .header("x-forwarded-proto", proto)
            .header("accept", BROWSER)
            .send()
            .await;
        assert_eq!(res.status, StatusCode::OK, "{proto}");
        assert_eq!(res.header("strict-transport-security"), None, "{proto}");
        assert_eq!(res.page_data()["record"]["ip"], "203.0.113.9");
    }
}

#[tokio::test]
async fn without_x_forwarded_proto_nothing_is_redirected() {
    let (app, _web) = app(PROXY);
    let res = app
        .get("/")
        .from("127.0.0.1:4000")
        .header("host", "ip.example.com")
        .header("x-real-ip", "8.8.8.8")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.header("strict-transport-security"), None);
    assert_eq!(res.page_data()["record"]["ip"], "8.8.8.8");
}

#[tokio::test]
async fn a_browser_gets_the_record_page_of_an_address() {
    let (app, _web) = app("[hosts]\nmain = \"ip.example.com\"\n");
    let res = app.get("/8.8.8.8").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.header("cache-control"), Some("no-store"));
    assert_eq!(res.header("vary"), Some("Accept"));
    assert_eq!(res.header("access-control-allow-origin"), None);
    let data = res.page_data();
    assert_eq!(data["record"]["ip"], "8.8.8.8");
    assert_eq!(data["record"]["hostname"], "dns.google");
    assert_eq!(data["record"]["org"], "AS15169 Google LLC");
    assert_eq!(data["hosts"], json!({"main": "ip.example.com"}));
    assert_eq!(data.get("error"), None);

    // The same URL is still JSON for curl.
    let res = app.get("/8.8.8.8").send().await;
    assert_eq!(res.json()["ip"], "8.8.8.8");
    assert_eq!(res.header("vary"), Some("Accept"));
}

#[tokio::test]
async fn the_record_page_echoes_the_address_and_shows_bogons() {
    let (app, _web) = app("");
    let res = app
        .get("/::ffff:8.8.8.8")
        .header("accept", BROWSER)
        .send()
        .await;
    let data = res.page_data();
    assert_eq!(data["record"]["ip"], "::ffff:8.8.8.8");
    assert_eq!(data["record"]["org"], "AS15169 Google LLC");

    let res = app.get("/10.0.0.1").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(
        res.page_data(),
        json!({"record": {"ip": "10.0.0.1", "bogon": true}})
    );
}

#[tokio::test]
async fn every_page_carries_the_main_host() {
    let (app, _web) = app("[hosts]\nmain = \"ip.example.com\"\n");
    for uri in ["/", "/8.8.8.8", "/hello"] {
        let res = app.get(uri).header("accept", BROWSER).send().await;
        assert_eq!(res.page_data()["hosts"]["main"], "ip.example.com", "{uri}");
    }
}

#[tokio::test]
async fn pages_carry_the_v4_and_v6_hosts_when_configured() {
    let (both, _web) = app(
        "[hosts]\nmain = \"ip.example.com\"\nv4 = \"v4.ip.example.com\"\nv6 = \"v6.ip.example.com\"\n",
    );
    let res = both
        .get("/")
        .from("8.8.8.8:4000")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(
        res.page_data()["hosts"],
        json!({"main": "ip.example.com", "v4": "v4.ip.example.com", "v6": "v6.ip.example.com"})
    );

    // Unset, they are left out (the page then shows a single card).
    let (plain, _web) = app("");
    let res = plain.get("/").header("accept", BROWSER).send().await;
    assert_eq!(res.page_data().get("hosts"), None);
}

#[tokio::test]
async fn the_v4_and_v6_hosts_answer_cross_origin_self_lookups() {
    // The home page on ip.example.com fetches https://v4.…/json and
    // https://v6.…/json: Self Lookups on another Host, from another Origin.
    let (app, _web) = app(&format!(
        "{PROXY}[hosts]\nmain = \"ip.example.com\"\nv4 = \"v4.ip.example.com\"\nv6 = \"v6.ip.example.com\"\n[rate_limit]\nburst = 1\n"
    ));
    for (host, client) in [
        ("v4.ip.example.com", "8.8.8.8"),
        ("v6.ip.example.com", "2001:4860:4860::8888"),
    ] {
        for _ in 0..3 {
            let res = app
                .get("/json")
                .from("127.0.0.1:4000")
                .header("host", host)
                .header("x-forwarded-proto", "https")
                .header("x-real-ip", client)
                .header("origin", "https://ip.example.com")
                .header("accept", "*/*")
                .send()
                .await;
            assert_eq!(res.status, StatusCode::OK, "{host}");
            assert_eq!(res.header("access-control-allow-origin"), Some("*"));
            assert_eq!(res.json()["ip"], client, "{host}");
        }
    }
}

#[tokio::test]
async fn a_browser_gets_a_404_page_for_invalid_input_and_unknown_paths() {
    let (app, _web) = app("");
    for uri in [
        "/hello",
        "/8.8.8.08",
        "/google.com",
        "/8.8.8.8/hello",
        "/hello/json",
        "/a/b/c",
    ] {
        let res = app.get(uri).header("accept", BROWSER).send().await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(res.header("vary"), Some("Accept"), "{uri}");
        assert_eq!(
            res.page_data(),
            json!({"error": {
                "status": 404,
                "title": "Wrong ip",
                "message": "Please provide a valid IP address",
            }}),
            "{uri}"
        );

        // Anything else still gets ipinfo's JSON error.
        let res = app.get(uri).send().await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(res.json()["error"]["title"], "Wrong ip", "{uri}");
    }
}

#[tokio::test(start_paused = true)]
async fn record_pages_count_against_the_rate_limit() {
    let (app, _web) = app("[rate_limit]\nburst = 2\n");
    let res = app.get("/8.8.8.8").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::OK);
    let res = app.get("/8.8.8.8").send().await;
    assert_eq!(res.status, StatusCode::OK);

    // A browser gets a page saying so, with the same status and Retry-After.
    let res = app.get("/1.1.1.1").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.header("retry-after"), Some("1"));
    assert_eq!(res.header("vary"), Some("Accept"));
    assert_eq!(res.header("cache-control"), Some("no-store"));
    let data = res.page_data();
    assert_eq!(data["error"]["status"], 429);
    assert_eq!(data["error"]["title"], "Rate limit exceeded");
    assert_eq!(
        data["error"]["message"],
        "Too many lookups of other addresses. Please retry after 1 second."
    );
    assert_eq!(data.get("record"), None);

    // curl still gets JSON.
    let res = app.get("/1.1.1.1").send().await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.json()["error"]["title"], "Rate limit exceeded");

    // The home page (a Self Lookup) is never limited.
    let res = app.get("/").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::OK);

    tokio::time::advance(Duration::from_secs(1)).await;
    let res = app.get("/1.1.1.1").header("accept", BROWSER).send().await;
    assert_eq!(res.status, StatusCode::OK);
}
