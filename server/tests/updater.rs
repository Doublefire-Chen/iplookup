//! The real updater against a `wiremock` stand-in for MaxMind and IPinfo
//! (no real network). Tests look only at what the updater reports, the
//! files in the data directory, what was downloaded, and what Lookups see.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::upstream::{self, LICENSE_KEY, TOKEN, Upstream};
use common::{TestApp, mmdb};
use iplookup::config::Config;
use iplookup::databases::{DataSource, LOCATION_FILE, MmdbDatabases, NETWORK_FILE};
use iplookup::updater::{Outcome, Updater};

const V1: u64 = 1_793_059_200; // 2026-10-27
const V2: u64 = 1_793_145_600; // 2026-10-28

fn updater(upstream: &Upstream, dir: &Path) -> Updater {
    let config = Config::parse(&upstream.config(dir, "", ""), Path::new("/")).unwrap();
    Updater::new(&config).unwrap()
}

/// Publishes version 1 of both Data Sources and downloads it into `dir`.
async fn install_v1(upstream: &Upstream, dir: &Path) {
    upstream
        .publish_location(&mmdb::location("Oldtown", V1), 1)
        .await;
    upstream
        .publish_network(&mmdb::network("AS1", "Old Net", V1), "\"v1\"", 1)
        .await;
    let updater = updater(upstream, dir);
    for source in DataSource::ALL {
        let outcome = updater.check(source, None).await.unwrap();
        assert_eq!(outcome, Outcome::Downloaded { build: V1 }, "{source}");
    }
    upstream.reset().await;
}

/// Each data file's bytes and modification time.
fn snapshot(dir: &Path) -> Vec<(Vec<u8>, std::time::SystemTime)> {
    [LOCATION_FILE, NETWORK_FILE]
        .iter()
        .map(|f| {
            let path = dir.join(f);
            (
                std::fs::read(&path).unwrap(),
                std::fs::metadata(&path).unwrap().modified().unwrap(),
            )
        })
        .collect()
}

/// Only the two data files and their state files: no temporary copies.
fn assert_one_copy_each(dir: &Path) {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "GeoLite2-City.mmdb",
            "GeoLite2-City.mmdb.state",
            "ipinfo_lite.mmdb",
            "ipinfo_lite.mmdb.state"
        ]
    );
}

#[tokio::test]
async fn an_empty_directory_gets_both_files() {
    let upstream = Upstream::start().await;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data"); // doesn't exist yet
    install_v1(&upstream, &data).await;

    assert_one_copy_each(&data);
    assert_eq!(
        std::fs::read(data.join(LOCATION_FILE)).unwrap(),
        mmdb::location("Oldtown", V1)
    );
    let databases = MmdbDatabases::open(&data).unwrap();
    let app = TestApp::with_reader(&Config::parse("", &data).unwrap(), Arc::new(databases));
    let res = app.get("/8.8.8.8").send().await.json();
    assert_eq!(res["city"], "Oldtown");
    assert_eq!(res["org"], "AS1 Old Net");
}

#[tokio::test]
async fn unchanged_upstream_is_not_downloaded_again() {
    let upstream = Upstream::start().await;
    let dir = tempfile::tempdir().unwrap();
    install_v1(&upstream, dir.path()).await;
    let before = snapshot(dir.path());

    // Same SHA-256 and a 304 Not Modified: no download (expect 0).
    upstream
        .publish_location(&mmdb::location("Oldtown", V1), 0)
        .await;
    upstream
        .publish_network(&mmdb::network("AS1", "Old Net", V1), "\"v1\"", 0)
        .await;
    // A fresh updater, as after a restart: it remembers what is installed.
    let updater = updater(&upstream, dir.path());
    for source in DataSource::ALL {
        assert_eq!(
            updater.check(source, None).await.unwrap(),
            Outcome::Unchanged,
            "{source}"
        );
        assert!(updater.last_checked(source).is_some());
    }
    upstream.reset().await;
    assert_eq!(snapshot(dir.path()), before);
}

#[tokio::test]
async fn a_missing_file_is_downloaded_even_if_upstream_is_unchanged() {
    let upstream = Upstream::start().await;
    let dir = tempfile::tempdir().unwrap();
    install_v1(&upstream, dir.path()).await;
    std::fs::remove_file(dir.path().join(NETWORK_FILE)).unwrap();

    upstream
        .publish_network(&mmdb::network("AS1", "Old Net", V1), "\"v1\"", 1)
        .await;
    let outcome = updater(&upstream, dir.path())
        .check(DataSource::Network, None)
        .await
        .unwrap();
    assert_eq!(outcome, Outcome::Downloaded { build: V1 });
    upstream.reset().await;
    assert_one_copy_each(dir.path());
}

#[tokio::test]
async fn changed_upstream_replaces_the_file_and_new_lookups_see_it() {
    let upstream = Upstream::start().await;
    let dir = tempfile::tempdir().unwrap();
    install_v1(&upstream, dir.path()).await;
    let databases = Arc::new(MmdbDatabases::open(dir.path()).unwrap());
    let config = Config::parse("", dir.path()).unwrap();
    let app = TestApp::with_reader(&config, databases.clone());
    let res = app.get("/8.8.8.8").send().await.json();
    assert_eq!(
        (&res["city"], &res["org"]),
        (&"Oldtown".into(), &"AS1 Old Net".into())
    );

    upstream
        .publish_location(&mmdb::location("Newtown", V2), 1)
        .await;
    upstream
        .publish_network(&mmdb::network("AS2", "New Net", V2), "\"v2\"", 1)
        .await;
    let updater = updater(&upstream, dir.path());
    for source in DataSource::ALL {
        let outcome = updater.check(source, Some(&databases)).await.unwrap();
        assert_eq!(outcome, Outcome::Downloaded { build: V2 }, "{source}");
    }
    upstream.reset().await;

    let res = app.get("/8.8.8.8").send().await.json();
    assert_eq!(res["city"], "Newtown");
    assert_eq!(res["org"], "AS2 New Net");
    assert_one_copy_each(dir.path());
    assert_eq!(
        std::fs::read(dir.path().join(NETWORK_FILE)).unwrap(),
        mmdb::network("AS2", "New Net", V2)
    );
    let info = iplookup::databases::DatabaseReader::info(&*databases);
    assert_eq!((info.location.build, info.network.build), (V2, V2));
    assert!(info.location.checked.is_some() && info.network.checked.is_some());
}

/// Runs one failing check of `source` against a server that has v1
/// installed and loaded, and checks nothing changed: files, Lookups.
async fn assert_failure_keeps_current_data(
    source: DataSource,
    publish_bad: impl AsyncFnOnce(&Upstream),
    expected_error: &str,
) {
    let upstream = Upstream::start().await;
    let dir = tempfile::tempdir().unwrap();
    install_v1(&upstream, dir.path()).await;
    let before = snapshot(dir.path());
    let databases = Arc::new(MmdbDatabases::open(dir.path()).unwrap());
    let app = TestApp::with_reader(&Config::parse("", dir.path()).unwrap(), databases.clone());

    publish_bad(&upstream).await;
    let err = updater(&upstream, dir.path())
        .check(source, Some(&databases))
        .await
        .unwrap_err();
    let err = format!("{err:#}");
    assert!(err.contains(expected_error), "{source}: {err}");
    assert!(!err.contains(LICENSE_KEY) && !err.contains(TOKEN), "{err}");

    assert_eq!(snapshot(dir.path()), before);
    assert_one_copy_each(dir.path());
    let res = app.get("/8.8.8.8").send().await.json();
    assert_eq!(res["city"], "Oldtown");
    assert_eq!(res["org"], "AS1 Old Net");
}

#[tokio::test]
async fn a_bad_checksum_keeps_the_current_data() {
    assert_failure_keeps_current_data(
        DataSource::Location,
        async |up: &Upstream| {
            let archive = upstream::archive(&mmdb::location("Newtown", V2));
            up.publish_archive(archive, &"0".repeat(64), 1).await;
        },
        "bad checksum",
    )
    .await;
}

#[tokio::test]
async fn a_corrupt_geolite_file_keeps_the_current_data() {
    assert_failure_keeps_current_data(
        DataSource::Location,
        async |up: &Upstream| {
            up.publish_location(b"not an mmdb file", 1).await;
        },
        "the new file is unusable",
    )
    .await;
}

#[tokio::test]
async fn a_corrupt_ipinfo_file_keeps_the_current_data() {
    assert_failure_keeps_current_data(
        DataSource::Network,
        async |up: &Upstream| {
            up.publish_network(b"<html>Oops</html>", "\"v2\"", 1).await;
        },
        "the new file is unusable",
    )
    .await;
}

#[tokio::test]
async fn the_wrong_kind_of_database_keeps_the_current_data() {
    assert_failure_keeps_current_data(
        DataSource::Network,
        async |up: &Upstream| {
            up.publish_network(&mmdb::location("Newtown", V2), "\"v2\"", 1)
                .await;
        },
        "not Network Data",
    )
    .await;
}

#[tokio::test]
async fn rate_limiting_keeps_the_current_data() {
    for source in DataSource::ALL {
        assert_failure_keeps_current_data(
            source,
            async |up: &Upstream| up.fail_with(429).await,
            "HTTP 429 Too Many Requests",
        )
        .await;
    }
}

#[tokio::test]
async fn rejected_credentials_are_reported_without_showing_them() {
    for source in DataSource::ALL {
        assert_failure_keeps_current_data(
            source,
            async |up: &Upstream| up.fail_with(401).await,
            "check the credentials",
        )
        .await;
    }
}

#[tokio::test]
async fn an_unreachable_server_never_shows_the_token() {
    let dir = tempfile::tempdir().unwrap();
    // Nothing listens on port 1.
    let config = Config::parse(
        &format!(
            "[data]\ndir = \"{}\"\n[ipinfo]\ntoken = \"{TOKEN}\"\nbase_url = \"http://127.0.0.1:1\"\n",
            dir.path().display()
        ),
        Path::new("/"),
    )
    .unwrap();
    let err = Updater::new(&config)
        .unwrap()
        .check(DataSource::Network, None)
        .await
        .unwrap_err();
    let err = format!("{err:#}");
    assert!(err.contains("IPinfo: request failed"), "{err}");
    assert!(!err.contains(TOKEN), "{err}");
}

#[tokio::test]
async fn a_source_without_credentials_is_not_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::parse(
        &format!("[data]\ndir = \"{}\"\n", dir.path().display()),
        Path::new("/"),
    )
    .unwrap();
    let updater = Updater::new(&config).unwrap();
    let err = updater.check(DataSource::Location, None).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("set [maxmind] account_id and license_key"),
        "{err}"
    );
    let err = updater.check(DataSource::Network, None).await.unwrap_err();
    assert!(err.to_string().contains("set [ipinfo] token"), "{err}");
}
