//! Builds the HTTP router. Used by `serve` and, in-process, by the tests.
//!
//! The router reads the socket peer from [`ConnectInfo<SocketAddr>`], so it
//! must be served with `into_make_service_with_connect_info::<SocketAddr>()`
//! (tests insert the extension into each request instead).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ipnet::IpNet;

use crate::client::client_ip;
use crate::config::Config;
use crate::databases::DatabaseReader;
use crate::lookup::{explicit_lookup, parse_ip};
use crate::record::Record;

#[derive(Clone)]
pub struct AppState {
    pub trusted_proxies: Arc<[IpNet]>,
    pub databases: Arc<dyn DatabaseReader>,
}

impl AppState {
    pub fn new(config: &Config, databases: Arc<dyn DatabaseReader>) -> Self {
        Self {
            trusted_proxies: config.trusted_proxies.clone().into(),
            databases,
        }
    }
}

/// Routes are matched in order: reserved names (static routes such as
/// `/json`, which axum always prefers over `/{ip}`), then the path parsed as
/// an IP address, then the "Wrong ip" 404.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(self_lookup))
        .route("/json", get(self_lookup))
        .route("/{ip}", get(explicit))
        .route("/{ip}/json", get(explicit))
        .fallback(wrong_ip)
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}

/// A Self Lookup: the Record of the address the request came from, as JSON.
async fn self_lookup(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let ip = client_ip(peer, &headers, &state.trusted_proxies);
    let record = Record::new(ip.to_string(), state.databases.lookup(ip));
    json_response(record.to_json())
}

/// An Explicit Lookup: the Record of the address in the path, as JSON.
async fn explicit(State(state): State<AppState>, Path(as_typed): Path<String>) -> Response {
    match parse_ip(&as_typed) {
        Some(ip) => json_response(explicit_lookup(&*state.databases, &as_typed, ip).to_json()),
        None => wrong_ip().await,
    }
}

/// ipinfo.io's 404 for anything that isn't a route or an IP address.
async fn wrong_ip() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "Wrong ip",
        "Please provide a valid IP address",
    )
}

/// An ipinfo-style error: `{"status": …, "error": {"title", "message"}}`.
pub fn error_response(status: StatusCode, title: &str, message: &str) -> Response {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        status: u16,
        error: Error<'a>,
    }
    #[derive(serde::Serialize)]
    struct Error<'a> {
        title: &'a str,
        message: &'a str,
    }
    let body = Body {
        status: status.as_u16(),
        error: Error { title, message },
    };
    let mut body = serde_json::to_string_pretty(&body).expect("JSON always serialises");
    body.push('\n');
    (status, json_response(body)).into_response()
}

pub fn json_response(body: String) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        body,
    )
        .into_response()
}

/// Logs one line per request at debug level.
async fn log_request(request: Request, next: Next) -> Response {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return next.run(request).await;
    }
    let start = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.to_string())
        .unwrap_or_else(|| "-".to_owned());
    let response = next.run(request).await;
    tracing::debug!(
        "{peer} {method} {uri} {} {:.1?}",
        response.status().as_u16(),
        start.elapsed()
    );
    response
}
