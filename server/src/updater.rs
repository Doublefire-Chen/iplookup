//! The updater: keeps both Data Sources up to date.
//!
//! - GeoLite2 City comes from MaxMind's download API (HTTP Basic auth with
//!   the Account ID and licence key) as a `.tar.gz`. MaxMind's published
//!   SHA-256 is fetched first: if it matches the installed archive's, nothing
//!   is downloaded; otherwise the archive must match it before unpacking.
//! - IPinfo Lite comes from its mmdb URL with the token, downloaded only
//!   when changed (`If-None-Match` / `If-Modified-Since`).
//!
//! A new file is written to a temporary path in the data directory, opened
//! and test-looked-up, atomically renamed over the old one, then swapped in
//! memory ([`MmdbDatabases::replace`]). Only one copy of each file is kept,
//! as the GeoLite licence requires. Any failure (network error, bad
//! checksum, corrupt file, 429) leaves the current file and data alone.
//!
//! What was installed (checksum or `ETag` / `Last-Modified`) and when
//! upstream was last checked are kept next to each file in
//! `<file>.state` (JSON), so a restart doesn't download again.
//!
//! Credentials come only from the config file and never appear in errors
//! or logs: request errors are stripped of their URL (IPinfo's carries the
//! token).

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use reqwest::{RequestBuilder, Response, StatusCode, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::config::{Config, Ipinfo, MaxMind};
use crate::databases::{DataSource, Mmdb, MmdbDatabases, open_mmdb};

/// The first retry after a failed check comes this long after it; each
/// further failure doubles the wait, up to the update interval.
pub const FIRST_RETRY: Duration = Duration::from_secs(5 * 60);
/// A download taking longer than this has failed.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// What one check of a Data Source did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Upstream has nothing new; the file was left alone.
    Unchanged,
    /// A new file was downloaded, verified and installed.
    Downloaded {
        /// The new file's build time, in seconds since the Unix epoch.
        build: u64,
    },
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Unchanged => f.write_str("unchanged"),
            Outcome::Downloaded { build } => write!(
                f,
                "downloaded new data built {}",
                humantime::format_rfc3339_seconds(UNIX_EPOCH + Duration::from_secs(*build))
            ),
        }
    }
}

pub struct Updater {
    client: reqwest::Client,
    data_dir: PathBuf,
    interval: Duration,
    maxmind: MaxMind,
    ipinfo: Ipinfo,
    /// What is known about each installed file (from its state file).
    state: Mutex<HashMap<DataSource, FileState>>,
}

/// What was installed, and when upstream was last checked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct FileState {
    /// MaxMind: the SHA-256 of the archive the file came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    /// IPinfo: the download's `ETag`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
    /// IPinfo: the download's `Last-Modified`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_modified: Option<String>,
    /// The last successful check, in seconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checked: Option<u64>,
}

/// The result of asking upstream.
enum Fetched {
    Unchanged,
    /// A new file, still at its temporary path, and what to remember.
    New(NamedTempFile, FileState),
}

impl Updater {
    /// An updater for `config.data_dir`, using the credentials, base URLs
    /// and interval from `config`.
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("iplookup/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .context("building the HTTP client")?;
        let state = DataSource::ALL
            .into_iter()
            .map(|source| (source, read_state(&config.data_dir, source)))
            .collect();
        Ok(Self {
            client,
            data_dir: config.data_dir.clone(),
            interval: config.update_interval,
            maxmind: config.maxmind.clone(),
            ipinfo: config.ipinfo.clone(),
            state: Mutex::new(state),
        })
    }

    /// Whether `source` has credentials, so it can be downloaded at all.
    pub fn can_update(&self, source: DataSource) -> bool {
        match source {
            DataSource::Location => self.maxmind.credentials.is_some(),
            DataSource::Network => self.ipinfo.token.is_some(),
        }
    }

    /// When upstream was last checked successfully for `source`, if ever.
    pub fn last_checked(&self, source: DataSource) -> Option<SystemTime> {
        let checked = self.state.lock().unwrap()[&source].checked?;
        Some(UNIX_EPOCH + Duration::from_secs(checked))
    }

    /// Downloads every Data Source whose file is missing (before the app
    /// can serve). Fails on the first one that can't be downloaded.
    pub async fn download_missing(&self) -> anyhow::Result<()> {
        remove_leftovers(&self.data_dir);
        for source in DataSource::ALL {
            let path = self.data_dir.join(source.file_name());
            if path.is_file() {
                continue;
            }
            tracing::info!("{source}: {} is missing; downloading it", path.display());
            let outcome = self
                .check(source, None)
                .await
                .with_context(|| format!("downloading {source} to {}", path.display()))?;
            tracing::info!("{source}: {outcome}");
        }
        Ok(())
    }

    /// Checks `source` upstream once and installs new data if there is any.
    /// With `live`, new data is also swapped into it and the check time
    /// recorded there. A missing file is always downloaded.
    pub async fn check(
        &self,
        source: DataSource,
        live: Option<&MmdbDatabases>,
    ) -> anyhow::Result<Outcome> {
        std::fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("creating data directory {}", self.data_dir.display()))?;
        let path = self.data_dir.join(source.file_name());
        let known = if path.is_file() {
            self.state.lock().unwrap()[&source].clone()
        } else {
            FileState::default()
        };
        let fetched = match source {
            DataSource::Location => self.fetch_maxmind(&known).await?,
            DataSource::Network => self.fetch_ipinfo(&known).await?,
        };
        let (mut state, outcome, reader) = match fetched {
            Fetched::Unchanged => (known, Outcome::Unchanged, None),
            Fetched::New(file, state) => {
                let target = path.clone();
                let reader = tokio::task::spawn_blocking(move || install(file, &target, source))
                    .await
                    .context("installing the new file")??;
                let build = reader.metadata().build_epoch;
                (state, Outcome::Downloaded { build }, Some(reader))
            }
        };
        let now = SystemTime::now();
        state.checked = now.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs());
        if let Err(e) = write_state(&self.data_dir, source, &state) {
            tracing::warn!("{source}: {e:#}");
        }
        self.state.lock().unwrap().insert(source, state);
        if let Some(live) = live {
            if let Some(reader) = reader {
                live.replace(source, reader);
            }
            live.set_checked(source, now);
        }
        Ok(outcome)
    }

    /// Checks each Data Source with credentials every update interval
    /// (counted from its last check, so a restart doesn't check early) and
    /// swaps new data into `databases`. A failed check keeps the current
    /// data, logs a warning and is retried after a back-off.
    pub async fn run(self, databases: Arc<MmdbDatabases>) {
        struct Schedule {
            source: DataSource,
            next: tokio::time::Instant,
            failures: u32,
        }
        let now = tokio::time::Instant::now();
        let mut schedules: Vec<Schedule> = Vec::new();
        for source in DataSource::ALL {
            if !self.can_update(source) {
                tracing::warn!(
                    "{source} is never updated: {}",
                    self.missing_credentials(source)
                );
                continue;
            }
            let since = self
                .last_checked(source)
                .and_then(|t| t.elapsed().ok())
                .unwrap_or(self.interval);
            schedules.push(Schedule {
                source,
                next: now + self.interval.saturating_sub(since),
                failures: 0,
            });
        }
        loop {
            let Some(due) = schedules.iter_mut().min_by_key(|s| s.next) else {
                return;
            };
            tokio::time::sleep_until(due.next).await;
            let source = due.source;
            match self.check(source, Some(&databases)).await {
                Ok(outcome) => {
                    tracing::info!("{source}: {outcome}");
                    due.failures = 0;
                    due.next = tokio::time::Instant::now() + self.interval;
                }
                Err(e) => {
                    due.failures += 1;
                    let delay = backoff(due.failures, self.interval);
                    tracing::warn!(
                        "{source}: update failed, keeping the current data; retrying in {}: {e:#}",
                        humantime::format_duration(delay)
                    );
                    due.next = tokio::time::Instant::now() + delay;
                }
            }
        }
    }

    fn missing_credentials(&self, source: DataSource) -> &'static str {
        match source {
            DataSource::Location => {
                "no credentials: set [maxmind] account_id and license_key in the config file"
            }
            DataSource::Network => "no credentials: set [ipinfo] token in the config file",
        }
    }

    async fn fetch_maxmind(&self, known: &FileState) -> anyhow::Result<Fetched> {
        let source = DataSource::Location;
        let Some(credentials) = &self.maxmind.credentials else {
            anyhow::bail!(self.missing_credentials(source));
        };
        let url = format!(
            "{}/geoip/databases/GeoLite2-City/download",
            self.maxmind.base_url
        );
        let get = |suffix: &str| {
            self.client
                .get(&url)
                .query(&[("suffix", suffix)])
                .basic_auth(
                    &credentials.account_id,
                    Some(credentials.license_key.expose()),
                )
        };
        let body = send(get("tar.gz.sha256"), "MaxMind (checksum)")
            .await?
            .text()
            .await
            .map_err(|e| request_error(e, "MaxMind (checksum)"))?;
        let published = parse_sha256(&body)?;
        if known.sha256.as_deref() == Some(published.as_str()) {
            return Ok(Fetched::Unchanged);
        }
        let archive = send(get("tar.gz"), "MaxMind")
            .await?
            .bytes()
            .await
            .map_err(|e| request_error(e, "MaxMind"))?;
        let actual = hex::encode(Sha256::digest(&archive));
        anyhow::ensure!(
            actual == published,
            "bad checksum: the downloaded archive's SHA-256 is {actual}, MaxMind published {published}"
        );
        let dir = self.data_dir.clone();
        let file = tokio::task::spawn_blocking(move || extract_mmdb(&archive, &dir))
            .await
            .context("unpacking the archive")??;
        Ok(Fetched::New(
            file,
            FileState {
                sha256: Some(published),
                ..FileState::default()
            },
        ))
    }

    async fn fetch_ipinfo(&self, known: &FileState) -> anyhow::Result<Fetched> {
        let source = DataSource::Network;
        let Some(token) = &self.ipinfo.token else {
            anyhow::bail!(self.missing_credentials(source));
        };
        let mut request = self
            .client
            .get(format!("{}/data/ipinfo_lite.mmdb", self.ipinfo.base_url))
            .query(&[("token", token.expose())]);
        if let Some(etag) = &known.etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = &known.last_modified {
            request = request.header(header::IF_MODIFIED_SINCE, last_modified);
        }
        let mut response = send(request, "IPinfo").await?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(Fetched::Unchanged);
        }
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|v: &header::HeaderValue| v.to_str().ok())
                .map(str::to_owned)
        };
        let state = FileState {
            etag: header(header::ETAG),
            last_modified: header(header::LAST_MODIFIED),
            ..FileState::default()
        };
        let mut file = temp_file(&self.data_dir, source)?;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| request_error(e, "IPinfo"))?
        {
            file.write_all(&chunk).context("writing the download")?;
        }
        Ok(Fetched::New(file, state))
    }
}

/// The wait before retry number `failures` (from 1): [`FIRST_RETRY`],
/// doubling each time, never more than `interval`.
fn backoff(failures: u32, interval: Duration) -> Duration {
    FIRST_RETRY
        .saturating_mul(2u32.saturating_pow(failures.saturating_sub(1)))
        .min(interval)
}

/// Sends `request` and fails on any status but success or 304.
async fn send(request: RequestBuilder, what: &str) -> anyhow::Result<Response> {
    let response = request.send().await.map_err(|e| request_error(e, what))?;
    let status = response.status();
    if status.is_success() || status == StatusCode::NOT_MODIFIED {
        return Ok(response);
    }
    let hint = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => " (check the credentials)",
        StatusCode::TOO_MANY_REQUESTS => " (rate-limited; backing off)",
        _ => "",
    };
    anyhow::bail!("{what} answered HTTP {status}{hint}")
}

/// A request error without its URL, which may carry a credential.
fn request_error(e: reqwest::Error, what: &str) -> anyhow::Error {
    anyhow::Error::new(e.without_url()).context(format!("{what}: request failed"))
}

/// The checksum in MaxMind's `<sha256>  <file name>` answer.
fn parse_sha256(body: &str) -> anyhow::Result<String> {
    let sha = body.split_whitespace().next().unwrap_or("");
    anyhow::ensure!(
        sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "MaxMind's checksum answer is not a SHA-256"
    );
    Ok(sha.to_ascii_lowercase())
}

/// Copies `GeoLite2-City.mmdb` out of the `.tar.gz` into a temporary file
/// (nothing else in the archive is written anywhere).
fn extract_mmdb(archive: &[u8], dir: &Path) -> anyhow::Result<NamedTempFile> {
    let source = DataSource::Location;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries().context("reading the archive")? {
        let mut entry = entry.context("reading the archive")?;
        let wanted = entry.header().entry_type().is_file()
            && entry
                .path()
                .ok()
                .and_then(|p| p.file_name().map(|n| n == source.file_name()))
                .unwrap_or(false);
        if wanted {
            let mut file = temp_file(dir, source)?;
            std::io::copy(&mut entry, file.as_file_mut()).context("unpacking the archive")?;
            return Ok(file);
        }
    }
    anyhow::bail!("the archive has no {}", source.file_name())
}

/// Checks the new file, then renames it over the current one.
fn install(file: NamedTempFile, path: &Path, source: DataSource) -> anyhow::Result<Mmdb> {
    file.as_file().sync_all().context("writing the new file")?;
    let reader = open_mmdb(file.path(), source).context("the new file is unusable")?;
    file.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("moving the new file to {}", path.display()))?;
    Ok(reader)
}

/// A temporary file in `dir`, deleted unless installed. Named
/// `.<file>.<random>.tmp`, so leftovers can be recognised.
fn temp_file(dir: &Path, source: DataSource) -> anyhow::Result<NamedTempFile> {
    tempfile::Builder::new()
        .prefix(&format!(".{}.", source.file_name()))
        .suffix(".tmp")
        .tempfile_in(dir)
        .with_context(|| format!("creating a temporary file in {}", dir.display()))
}

/// Deletes temporary files left by a crash, so no stale copy lingers.
fn remove_leftovers(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let leftover = name.ends_with(".tmp")
            && DataSource::ALL
                .iter()
                .any(|s| name.starts_with(&format!(".{}.", s.file_name())));
        if leftover && std::fs::remove_file(entry.path()).is_ok() {
            tracing::info!("removed leftover {}", entry.path().display());
        }
    }
}

fn state_path(dir: &Path, source: DataSource) -> PathBuf {
    dir.join(format!("{}.state", source.file_name()))
}

fn read_state(dir: &Path, source: DataSource) -> FileState {
    let path = state_path(dir, source);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return FileState::default();
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        tracing::warn!("ignoring {}: {e}", path.display());
        FileState::default()
    })
}

fn write_state(dir: &Path, source: DataSource, state: &FileState) -> anyhow::Result<()> {
    let path = state_path(dir, source);
    let mut file = tempfile::Builder::new()
        .prefix(&format!(".{}.state.", source.file_name()))
        .suffix(".tmp")
        .tempfile_in(dir)?;
    serde_json::to_writer_pretty(&mut file, state)?;
    file.persist(&path)
        .map_err(|e| e.error)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_up_to_the_interval() {
        let hour = Duration::from_secs(3600);
        let minutes = |n: u64| Duration::from_secs(n * 60);
        assert_eq!(backoff(1, hour), minutes(5));
        assert_eq!(backoff(2, hour), minutes(10));
        assert_eq!(backoff(4, hour), minutes(40));
        assert_eq!(backoff(5, hour), hour);
        assert_eq!(backoff(100, hour), hour);
    }

    #[test]
    fn maxmind_checksum_answers() {
        let sha = "a".repeat(64);
        assert_eq!(
            parse_sha256(&format!("{sha}  GeoLite2-City_20261027.tar.gz\n")).unwrap(),
            sha
        );
        assert_eq!(parse_sha256(&"AB".repeat(32)).unwrap(), "ab".repeat(32));
        assert!(parse_sha256("").is_err());
        assert!(parse_sha256("<html>").is_err());
    }
}
