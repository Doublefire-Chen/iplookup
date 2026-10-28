//! Builds the HTTP router. Used by `serve` and, in-process, by the tests.
//!
//! The router reads the socket peer from [`ConnectInfo<SocketAddr>`], so it
//! must be served with `into_make_service_with_connect_info::<SocketAddr>()`
//! (tests insert the extension into each request instead).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ipnet::IpNet;

use crate::client::client_ip;
use crate::config::Config;
use crate::databases::DatabaseReader;
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

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(self_lookup))
        .route("/json", get(self_lookup))
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

fn json_response(body: String) -> Response {
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
