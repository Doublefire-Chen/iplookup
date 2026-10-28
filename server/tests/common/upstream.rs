//! A `wiremock` stand-in for both Data Sources' download servers, shaped
//! like the real ones: MaxMind's API (Basic auth) and IPinfo's URL (token)
//! both redirect to a storage URL that serves the file.
//!
//! ```ignore
//! let upstream = Upstream::start().await;
//! upstream.publish_location(&mmdb::location("Old", 1), 1).await; // 1 download expected
//! let config = upstream.config(dir.path(), "", "");
//! ```

use std::path::Path;

use sha2::{Digest, Sha256};
use wiremock::matchers::{basic_auth, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const ACCOUNT_ID: &str = "424242";
pub const LICENSE_KEY: &str = "test-licence-key-SECRET";
pub const TOKEN: &str = "test-ipinfo-token-SECRET";

const MAXMIND_PATH: &str = "/geoip/databases/GeoLite2-City/download";
const MAXMIND_STORAGE: &str = "/storage/geolite2-city/GeoLite2-City_20261027.tar.gz";
const IPINFO_PATH: &str = "/data/ipinfo_lite.mmdb";
const IPINFO_STORAGE: &str = "/storage/ipinfo/ipinfo_lite.mmdb";
pub const LAST_MODIFIED: &str = "Wed, 28 Oct 2026 00:00:00 GMT";

pub struct Upstream {
    pub server: MockServer,
}

impl Upstream {
    pub async fn start() -> Self {
        Self {
            server: MockServer::start().await,
        }
    }

    /// A config whose credentials and base URLs point here, with
    /// `data_dir` as the data directory, plus extra TOML lines for the top
    /// level and the `[data]` table.
    pub fn config(&self, data_dir: &Path, top_level: &str, data_table: &str) -> String {
        let uri = self.server.uri();
        format!(
            "{top_level}\n[data]\ndir = \"{}\"\n{data_table}\n[maxmind]\naccount_id = {ACCOUNT_ID}\nlicense_key = \"{LICENSE_KEY}\"\nbase_url = \"{uri}\"\n[ipinfo]\ntoken = \"{TOKEN}\"\nbase_url = \"{uri}\"\n",
            data_dir.display()
        )
    }

    /// MaxMind publishes `mmdb` (in a `.tar.gz` with its SHA-256); the
    /// archive must be downloaded exactly `downloads` times before the next
    /// [`Upstream::reset`].
    pub async fn publish_location(&self, mmdb: &[u8], downloads: u64) {
        let archive = archive(mmdb);
        let sha = hex::encode(Sha256::digest(&archive));
        self.publish_archive(archive, &sha, downloads).await;
    }

    /// MaxMind publishes `archive` with `sha256` as its checksum (which
    /// may be wrong).
    pub async fn publish_archive(&self, archive: Vec<u8>, sha256: &str, downloads: u64) {
        let maxmind = |suffix: &str| {
            Mock::given(method("GET"))
                .and(path(MAXMIND_PATH))
                .and(query_param("suffix", suffix))
                .and(basic_auth(ACCOUNT_ID, LICENSE_KEY))
        };
        maxmind("tar.gz.sha256")
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(format!("{sha256}  GeoLite2-City_20261027.tar.gz\n")),
            )
            .mount(&self.server)
            .await;
        maxmind("tar.gz")
            .respond_with(self.redirect(MAXMIND_STORAGE))
            .mount(&self.server)
            .await;
        Mock::given(method("GET"))
            .and(path(MAXMIND_STORAGE))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .expect(downloads)
            .named("GeoLite2 City archive download")
            .mount(&self.server)
            .await;
    }

    /// IPinfo publishes `mmdb` with `etag`; a request carrying that ETag
    /// gets 304. The file must be downloaded exactly `downloads` times
    /// before the next [`Upstream::reset`].
    pub async fn publish_network(&self, mmdb: &[u8], etag: &str, downloads: u64) {
        Mock::given(method("GET"))
            .and(path(IPINFO_PATH))
            .and(query_param("token", TOKEN))
            .respond_with(self.redirect(IPINFO_STORAGE))
            .mount(&self.server)
            .await;
        Mock::given(method("GET"))
            .and(path(IPINFO_STORAGE))
            .and(header("if-none-match", etag))
            .respond_with(ResponseTemplate::new(304))
            .with_priority(1)
            .mount(&self.server)
            .await;
        Mock::given(method("GET"))
            .and(path(IPINFO_STORAGE))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", etag)
                    .insert_header("last-modified", LAST_MODIFIED)
                    .set_body_bytes(mmdb.to_vec()),
            )
            .expect(downloads)
            .named("IPinfo Lite download")
            .mount(&self.server)
            .await;
    }

    /// Every request gets `status` (e.g. 429).
    pub async fn fail_with(&self, status: u16) {
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&self.server)
            .await;
    }

    /// Checks the expected download counts, then forgets everything
    /// published.
    pub async fn reset(&self) {
        self.server.verify().await;
        self.server.reset().await;
    }

    fn redirect(&self, to: &str) -> ResponseTemplate {
        ResponseTemplate::new(302).insert_header("location", format!("{}{to}", self.server.uri()))
    }
}

/// A `.tar.gz` laid out like MaxMind's, holding `mmdb`.
pub fn archive(mmdb: &[u8]) -> Vec<u8> {
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    for (name, data) in [
        ("GeoLite2-City_20261027/LICENSE.txt", &b"test licence\n"[..]),
        ("GeoLite2-City_20261027/GeoLite2-City.mmdb", mmdb),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, data).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}
