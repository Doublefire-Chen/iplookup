//! Builds the HTTP router. Used by `serve` and, in-process, by the tests.
//!
//! The router reads the socket peer from [`ConnectInfo<SocketAddr>`], so it
//! must be served with `into_make_service_with_connect_info::<SocketAddr>()`
//! (tests insert the extension into each request instead).

use std::net::{IpAddr, SocketAddr};
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
use crate::lookup::{lookup, parse_ip};
use crate::rate_limit::RateLimiter;
use crate::record::{Field, Record};
use crate::reverse_dns::{PtrResolver, ReverseDns};

#[derive(Clone)]
pub struct AppState {
    pub trusted_proxies: Arc<[IpNet]>,
    pub databases: Arc<dyn DatabaseReader>,
    pub reverse_dns: Arc<ReverseDns>,
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    /// `ptr` answers reverse-DNS queries; the app adds `dns.timeout` and
    /// the cache.
    pub fn new(
        config: &Config,
        databases: Arc<dyn DatabaseReader>,
        ptr: Arc<dyn PtrResolver>,
    ) -> Self {
        Self {
            trusted_proxies: config.trusted_proxies.clone().into(),
            databases,
            reverse_dns: Arc::new(ReverseDns::new(ptr, config.dns_timeout)),
            rate_limiter: Arc::new(RateLimiter::new(&config.rate_limit)),
        }
    }

    /// Spends one of the client's Explicit Lookup tokens; `Some` is the
    /// 429 to send instead when the bucket is empty. Every Explicit Lookup
    /// (API or page) calls this before looking anything up; Self Lookups
    /// never do.
    pub fn rate_limited(&self, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
        let client = client_ip(peer, headers, &self.trusted_proxies);
        self.rate_limiter.check(client).err().map(|wait| {
            let secs = wait.as_secs();
            let unit = if secs == 1 { "second" } else { "seconds" };
            let mut res = error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded",
                &format!("Too many lookups of other addresses. Please retry after {secs} {unit}."),
            );
            res.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(secs));
            res
        })
    }

    /// The Record of `ip`, echoed as `as_typed`.
    async fn lookup(&self, as_typed: &str, ip: IpAddr) -> Record {
        lookup(&*self.databases, &self.reverse_dns, as_typed, ip).await
    }
}

/// Routes are matched in order: reserved names (static routes such as
/// `/json`, which axum always prefers over `/{ip}`, and the Field names,
/// checked first in [`one_segment`]), then the path parsed as an IP
/// address, then the "Wrong ip" 404.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(self_lookup))
        .route("/json", get(self_lookup))
        .route("/{ip}", get(one_segment))
        .route("/{ip}/{field}", get(two_segments))
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
    json_response(self_record(&state, peer, &headers).await.to_json())
}

/// `/{field}` (a Self Lookup Field) or `/{ip}` (an Explicit Lookup,
/// rate-limited).
async fn one_segment(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(segment): Path<String>,
) -> Response {
    if let Some(field) = Field::parse(&segment) {
        return field_response(self_record(&state, peer, &headers).await.field(field));
    }
    let Some(ip) = parse_ip(&segment) else {
        return wrong_ip().await;
    };
    if let Some(limited) = state.rate_limited(peer, &headers) {
        return limited;
    }
    json_response(state.lookup(&segment, ip).await.to_json())
}

/// `/{ip}/json` or `/{ip}/{field}`: an Explicit Lookup as JSON, or one of
/// its Fields as plain text (rate-limited).
async fn two_segments(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((as_typed, suffix)): Path<(String, String)>,
) -> Response {
    let Some(ip) = parse_ip(&as_typed) else {
        return wrong_ip().await;
    };
    let field = match suffix.as_str() {
        "json" => None,
        _ => match Field::parse(&suffix) {
            Some(field) => Some(field),
            None => return wrong_ip().await,
        },
    };
    if let Some(limited) = state.rate_limited(peer, &headers) {
        return limited;
    }
    let record = state.lookup(&as_typed, ip).await;
    match field {
        None => json_response(record.to_json()),
        Some(field) => field_response(record.field(field)),
    }
}

/// The Record of the client's address (a Bogon's Record for a Bogon).
async fn self_record(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> Record {
    let ip = client_ip(peer, headers, &state.trusted_proxies);
    state.lookup(&ip.to_string(), ip).await
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

/// A JSON response (CORS-enabled, like ipinfo.io).
pub fn json_response(body: String) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            ),
            ANY_ORIGIN,
        ],
        body,
    )
        .into_response()
}

/// A Field as plain text plus a newline, whatever the `Accept` header; a
/// Field with no value is a 404 with an empty body (ADR 0001).
fn field_response(value: Option<&str>) -> Response {
    match value {
        Some(value) => (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/plain; charset=utf-8"),
                ),
                ANY_ORIGIN,
            ],
            format!("{value}\n"),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, [ANY_ORIGIN]).into_response(),
    }
}

/// `Access-Control-Allow-Origin: *`, on every JSON and plain-text response.
const ANY_ORIGIN: (header::HeaderName, HeaderValue) = (
    header::ACCESS_CONTROL_ALLOW_ORIGIN,
    HeaderValue::from_static("*"),
);

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
