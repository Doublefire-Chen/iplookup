//! Web pages: the page-or-data decision, the built frontend
//! (`frontend.dir`) and Record injection.
//!
//! A page is the frontend's `index.html` with a [`PageData`] block
//! embedded before `</head>`:
//!
//! ```html
//! <script id="iplookup-data" type="application/json">{"record":{…}}</script>
//! ```
//!
//! so the React app renders the Record on first paint without fetching it.

use std::path::{Path, PathBuf};

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use tower::ServiceExt;
use tower_http::services::ServeDir;

use crate::config::Hosts;
use crate::record::Record;
use crate::version::VersionInfo;

/// The `id` of the embedded data block, read by `web/src/pageData.ts`.
pub const DATA_ID: &str = "iplookup-data";

/// Whether the client wants a web page rather than data: its `Accept`
/// header mentions `text/html` (every browser's does, curl's `*/*`
/// doesn't).
pub fn wants_page(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| v.to_ascii_lowercase().contains("text/html"))
}

/// What a page is given on first paint. Every item is optional and left
/// out when unset, so later pages can add their own (such as the IPv4 and
/// IPv6 hosts) without changing existing ones.
#[derive(Debug, Default, Serialize)]
pub struct PageData {
    /// The Record the page shows: the visitor's own on the home page, the
    /// looked-up address's on a Record page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<Record>,
    /// Why the page shows no Record (an unknown path, or the rate limit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<PageError>,
    /// The backend version and Data Source state (the `/version` page).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionInfo>,
    /// The configured hostnames (filled in by the app on every page).
    #[serde(skip_serializing_if = "PageHosts::is_empty")]
    pub hosts: PageHosts,
}

impl PageData {
    pub fn record(record: Record) -> Self {
        Self {
            record: Some(record),
            ..Self::default()
        }
    }

    pub fn version(version: VersionInfo) -> Self {
        Self {
            version: Some(version),
            ..Self::default()
        }
    }

    pub fn error(status: StatusCode, title: &str, message: &str) -> Self {
        Self {
            error: Some(PageError {
                status: status.as_u16(),
                title: title.to_owned(),
                message: message.to_owned(),
            }),
            ..Self::default()
        }
    }
}

/// The page's HTTP status with the same title and message the JSON error
/// would carry.
#[derive(Debug, Serialize)]
pub struct PageError {
    pub status: u16,
    pub title: String,
    pub message: String,
}

/// `hosts.*` from the config, for the commands a page shows.
#[derive(Debug, Default, Clone, Serialize)]
pub struct PageHosts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    /// The IPv4-only and IPv6-only hostnames, for the home page's
    /// cross-host Self Lookups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v4: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v6: Option<String>,
}

impl PageHosts {
    fn is_empty(&self) -> bool {
        self.main.is_none() && self.v4.is_none() && self.v6.is_none()
    }
}

/// The built web pages in `frontend.dir`.
pub struct Frontend {
    dir: PathBuf,
    hosts: PageHosts,
}

impl Frontend {
    pub fn new(dir: &Path, hosts: &Hosts) -> Self {
        Self {
            dir: dir.to_owned(),
            hosts: PageHosts {
                main: hosts.main.clone(),
                v4: hosts.v4.clone(),
                v6: hosts.v6.clone(),
            },
        }
    }

    /// `index.html` with `data` (plus the configured hosts) embedded, sent
    /// with `status`. It is read on every request, so a new build is picked
    /// up without a restart; the response is never cached (it holds a
    /// Record).
    pub async fn page(&self, status: StatusCode, mut data: PageData) -> Response {
        data.hosts = self.hosts.clone();
        let path = self.dir.join("index.html");
        let html = match tokio::fs::read_to_string(&path).await {
            Ok(html) => html,
            Err(e) => {
                tracing::error!(
                    "can't read the web page {} (frontend.dir): {e}",
                    path.display()
                );
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; charset=utf-8"),
                    )],
                    "The web pages are not installed on this server.\n",
                )
                    .into_response();
            }
        };
        (
            status,
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            inject(&html, &data),
        )
            .into_response()
    }

    /// A file under `/assets/`. Vite gives these content-hashed names, so
    /// they never change and may be cached for good.
    pub async fn asset(&self, request: Request) -> Response {
        let files = ServeDir::new(&self.dir);
        let mut response = match files.oneshot(request).await {
            Ok(response) => response.into_response(),
            Err(never) => match never {},
        };
        if response.status().is_success() {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
        }
        response
    }
}

/// `html` with `data` embedded as a JSON block just before `</head>` (or at
/// the start, if there is none). The JSON is escaped so that nothing in it,
/// such as a hostile PTR name, can close the script tag.
pub fn inject(html: &str, data: &PageData) -> String {
    let block = format!(
        r#"<script id="{DATA_ID}" type="application/json">{}</script>"#,
        script_safe_json(data)
    );
    let at = html.find("</head>").unwrap_or(0);
    let mut out = String::with_capacity(html.len() + block.len());
    out.push_str(&html[..at]);
    out.push_str(&block);
    out.push_str(&html[at..]);
    out
}

/// `value` as JSON that is safe inside `<script>`: `<`, `>` and `&` (and
/// the JavaScript line separators) are written as `\uXXXX` escapes. Those
/// characters can only occur inside JSON strings, where the escapes mean
/// the same thing, so the JSON parses to the same value.
fn script_safe_json(value: &impl Serialize) -> String {
    let json = serde_json::to_string(value).expect("page data always serialises");
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(hostname: &str) -> Record {
        let mut record = Record::new("8.8.8.8".into(), Default::default());
        record.hostname = Some(hostname.into());
        record
    }

    #[test]
    fn data_goes_before_the_end_of_head() {
        let html = inject(
            "<html><head><title>x</title></head><body></body></html>",
            &PageData::default(),
        );
        assert_eq!(
            html,
            r#"<html><head><title>x</title><script id="iplookup-data" type="application/json">{}</script></head><body></body></html>"#
        );
    }

    #[test]
    fn a_hostile_hostname_cannot_close_the_script_tag() {
        let evil = "</script><script>alert(1)</script><!--&\u{2028}";
        let html = inject("<head></head>", &PageData::record(record(evil)));
        assert_eq!(html.matches("</script>").count(), 1, "{html}");
        assert!(!html.contains("<!--"), "{html}");
        let json = html
            .strip_prefix(r#"<head><script id="iplookup-data" type="application/json">"#)
            .and_then(|s| s.strip_suffix("</script></head>"))
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["record"]["hostname"], evil);
    }

    #[test]
    fn accept_decides_page_or_data() {
        let accept = |v: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::ACCEPT, HeaderValue::from_str(v).unwrap());
            wants_page(&h)
        };
        assert!(accept(
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
        ));
        assert!(accept("Text/HTML"));
        assert!(!accept("*/*"));
        assert!(!accept("application/json"));
        assert!(!wants_page(&HeaderMap::new()));
    }
}
