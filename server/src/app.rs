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
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ipnet::IpNet;

use crate::client::{Scheme, client_ip, forwarded_scheme};
use crate::config::Config;
use crate::databases::DatabaseReader;
use crate::lookup::{lookup, parse_ip};
use crate::page::{Frontend, PageData, wants_page};
use crate::rate_limit::RateLimiter;
use crate::record::{Field, Record};
use crate::reverse_dns::{PtrResolver, ReverseDns};
use crate::version::VersionInfo;

#[derive(Clone)]
pub struct AppState {
    pub trusted_proxies: Arc<[IpNet]>,
    pub databases: Arc<dyn DatabaseReader>,
    pub reverse_dns: Arc<ReverseDns>,
    pub rate_limiter: Arc<RateLimiter>,
    pub frontend: Arc<Frontend>,
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
            frontend: Arc::new(Frontend::new(&config.frontend_dir, &config.hosts)),
        }
    }

    /// Spends one of the client's Explicit Lookup tokens; `Some` is the
    /// 429 to send instead when the bucket is empty: a page for a browser
    /// (`page`), else ipinfo-style JSON, both with `Retry-After`. Every
    /// Explicit Lookup (API or page) calls this before looking anything up;
    /// Self Lookups never do.
    pub async fn rate_limited(
        &self,
        peer: SocketAddr,
        headers: &HeaderMap,
        page: bool,
    ) -> Option<Response> {
        let client = client_ip(peer, headers, &self.trusted_proxies);
        let secs = self.rate_limiter.check(client).err()?.as_secs();
        let unit = if secs == 1 { "second" } else { "seconds" };
        let status = StatusCode::TOO_MANY_REQUESTS;
        let title = "Rate limit exceeded";
        let message =
            format!("Too many lookups of other addresses. Please retry after {secs} {unit}.");
        let mut res = if page {
            self.frontend
                .page(status, PageData::error(status, title, &message))
                .await
        } else {
            error_response(status, title, &message)
        };
        res.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(secs));
        Some(res)
    }

    /// The Record of `ip`, echoed as `as_typed`.
    async fn lookup(&self, as_typed: &str, ip: IpAddr) -> Record {
        lookup(&*self.databases, &self.reverse_dns, as_typed, ip).await
    }
}

/// Routes are matched in order: reserved names (static routes such as
/// `/json`, `/version` and `/assets/…`, which axum always prefers over
/// `/{ip}`, and the Field names, checked first in [`one_segment`]), then
/// the path parsed as an IP address, then the "Wrong ip" 404. Every response passes through
/// [`https_only_pages`].
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/json", get(self_lookup))
        .route("/version", get(version))
        .route("/assets/{*path}", get(asset))
        .route("/{ip}", get(one_segment))
        .route("/{ip}/{field}", get(two_segments))
        .fallback(wrong_ip)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            https_only_pages,
        ))
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}

/// `/`: a Self Lookup, as the home page (with the Record injected) for a
/// browser, as JSON for anything else.
async fn home(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let record = self_record(&state, peer, &headers).await;
    let mut response = if wants_page(&headers) {
        state
            .frontend
            .page(StatusCode::OK, PageData::record(record))
            .await
    } else {
        json_response(record.to_json())
    };
    vary_accept(&mut response);
    response
}

/// Marks a response whose form (page or data) followed `Accept`.
fn vary_accept(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
}

/// `/json`: a Self Lookup as JSON, whatever the `Accept` header.
async fn self_lookup(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    json_response(self_record(&state, peer, &headers).await.to_json())
}

/// `/version`: the backend version and each Data Source's state, as a
/// page for a browser, as JSON for anything else. Also the health check:
/// 200 only when both Data Sources are loaded, else 503. Never
/// rate-limited.
async fn version(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let info = VersionInfo::new(&state.databases.info());
    let status = info.status();
    let mut response = if wants_page(&headers) {
        state.frontend.page(status, PageData::version(info)).await
    } else {
        (status, json_response(info.to_json())).into_response()
    };
    vary_accept(&mut response);
    response
}

/// A built file of the web pages (never rate-limited).
async fn asset(State(state): State<AppState>, request: Request) -> Response {
    state.frontend.asset(request).await
}

/// `/{field}` (a Self Lookup Field) or `/{ip}` (an Explicit Lookup,
/// rate-limited): the Record page for a browser, JSON for anything else.
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
        return wrong_ip(State(state), headers).await;
    };
    let page = wants_page(&headers);
    let mut response = match state.rate_limited(peer, &headers, page).await {
        Some(limited) => limited,
        None => {
            let record = state.lookup(&segment, ip).await;
            if page {
                state
                    .frontend
                    .page(StatusCode::OK, PageData::record(record))
                    .await
            } else {
                json_response(record.to_json())
            }
        }
    };
    vary_accept(&mut response);
    response
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
        return wrong_ip(State(state), headers).await;
    };
    let field = match suffix.as_str() {
        "json" => None,
        _ => match Field::parse(&suffix) {
            Some(field) => Some(field),
            None => return wrong_ip(State(state), headers).await,
        },
    };
    if let Some(limited) = state.rate_limited(peer, &headers, false).await {
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

/// ipinfo.io's 404 for anything that isn't a route or an IP address; a
/// browser gets the 404 page instead.
async fn wrong_ip(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let status = StatusCode::NOT_FOUND;
    let (title, message) = ("Wrong ip", "Please provide a valid IP address");
    let mut response = if wants_page(&headers) {
        state
            .frontend
            .page(status, PageData::error(status, title, message))
            .await
    } else {
        error_response(status, title, message)
    };
    vary_accept(&mut response);
    response
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

/// `Strict-Transport-Security` on every HTTPS response.
const HSTS: HeaderValue = HeaderValue::from_static("max-age=31536000");

/// ADR 0002: a page request (`Accept: text/html`) that reached the proxy
/// over plain HTTP is sent to the same URL on HTTPS with a 301; every other
/// request is answered over whichever scheme it came on, and HTTPS answers
/// carry HSTS. The scheme is only known from a trusted proxy's
/// `X-Forwarded-Proto` (see [`forwarded_scheme`]): with no proxy, or one
/// that doesn't say, nothing is redirected and no HSTS is sent.
async fn https_only_pages(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| *addr);
    let scheme =
        peer.and_then(|peer| forwarded_scheme(peer, request.headers(), &state.trusted_proxies));
    match scheme {
        Some(Scheme::Http)
            if matches!(*request.method(), Method::GET | Method::HEAD)
                && wants_page(request.headers()) =>
        {
            match https_url(request.headers(), request.uri()) {
                Some(location) => (
                    StatusCode::MOVED_PERMANENTLY,
                    [(header::LOCATION, location)],
                )
                    .into_response(),
                None => next.run(request).await,
            }
        }
        Some(Scheme::Https) => {
            let mut response = next.run(request).await;
            response
                .headers_mut()
                .insert(header::STRICT_TRANSPORT_SECURITY, HSTS);
            response
        }
        _ => next.run(request).await,
    }
}

/// `https://` + the request's `Host` (without any port) + its path and
/// query; `None` when there is no valid `Host`.
fn https_url(headers: &HeaderMap, uri: &Uri) -> Option<HeaderValue> {
    let host = headers.get(header::HOST)?.to_str().ok()?;
    let authority: axum::http::uri::Authority = host.parse().ok()?;
    if host.contains('@') {
        return None;
    }
    let path = uri.path_and_query().map_or("/", |pq| pq.as_str());
    HeaderValue::try_from(format!("https://{}{path}", authority.host())).ok()
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
