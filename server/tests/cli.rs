//! The `iplookup` binary: starting from a config file, downloading the
//! Data Sources (from a `wiremock` stand-in) and `iplookup update`.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use common::mmdb;
use common::upstream::{LICENSE_KEY, TOKEN, Upstream};
use iplookup::config::Config;
use iplookup::databases::{DataSource, LOCATION_FILE, NETWORK_FILE};
use iplookup::updater::Updater;

const V1: u64 = 1_793_059_200; // 2026-10-27

fn iplookup(config: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_iplookup"));
    cmd.arg("--config").arg(config).env_remove("RUST_LOG");
    cmd
}

/// Runs `iplookup serve`, expecting it to fail; returns its stderr.
fn serve_fails(config_toml: &str, setup: impl FnOnce(&Path)) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("data")).unwrap();
    setup(&dir.path().join("data"));
    let config = dir.path().join("iplookup.toml");
    std::fs::write(&config, config_toml).unwrap();
    let out = iplookup(&config)
        .arg("serve")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success(), "serve started");
    String::from_utf8(out.stderr).unwrap()
}

#[test]
fn a_missing_config_file_is_a_clear_error() {
    let out = iplookup(Path::new("/nonexistent/iplookup.toml"))
        .arg("serve")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("reading config file /nonexistent/iplookup.toml"),
        "{stderr}"
    );
}

#[test]
fn a_bad_config_is_a_clear_error() {
    let stderr = serve_fails("log_level = \"loud\"\n", |_| {});
    assert!(stderr.contains("parsing config file"), "{stderr}");
    assert!(stderr.contains("log_level"), "{stderr}");
}

#[test]
fn missing_databases_without_credentials_stop_startup() {
    let stderr = serve_fails("listen = \"127.0.0.1:0\"\n", |_| {});
    assert!(
        stderr.contains("downloading Location Data (GeoLite2 City)"),
        "{stderr}"
    );
    assert!(stderr.contains(LOCATION_FILE), "{stderr}");
    assert!(
        stderr.contains("set [maxmind] account_id and license_key"),
        "{stderr}"
    );
}

#[test]
fn an_unreadable_database_stops_startup() {
    let stderr = serve_fails("listen = \"127.0.0.1:0\"\n", |data| {
        std::fs::write(data.join(LOCATION_FILE), b"not an mmdb file").unwrap();
        std::fs::write(data.join(NETWORK_FILE), mmdb::network("AS1", "Net", V1)).unwrap();
    });
    assert!(stderr.contains("opening Location Data"), "{stderr}");
    assert!(stderr.contains(LOCATION_FILE), "{stderr}");
}

/// A running `iplookup serve`, killed on drop.
struct Server {
    child: Child,
    lines: mpsc::Receiver<String>,
    /// Every stdout line read so far.
    seen: Vec<String>,
    addr: String,
}

impl Server {
    /// Starts `iplookup serve` and waits until it listens.
    fn start(config: &Path) -> Self {
        let mut child = iplookup(config)
            .arg("serve")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let (tx, lines) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut server = Self {
            child,
            lines,
            seen: Vec::new(),
            addr: String::new(),
        };
        let line = server.next_line("listening on http://");
        server.addr = line.rsplit("http://").next().unwrap().to_owned();
        server
    }

    /// The next stdout line containing `want`.
    fn next_line(&mut self, want: &str) -> String {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(20))
                .unwrap_or_else(|_| {
                    panic!("no stdout line containing {want:?} in {:#?}", self.seen)
                });
            self.seen.push(line.clone());
            if line.contains(want) {
                return line;
            }
        }
    }

    /// The raw HTTP response to `GET path`.
    fn get(&self, path: &str) -> String {
        let mut stream = TcpStream::connect(&self.addr).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_no_credentials(output: &str) {
    for secret in [LICENSE_KEY, TOKEN] {
        assert!(!output.contains(secret), "credential in output: {output}");
    }
}

/// Starts the real server on the real databases (skipped without them),
/// asks for a Self Lookup over TCP, and checks the debug request log line.
#[test]
fn serve_answers_a_self_lookup_and_logs_it_at_debug() {
    let testdata = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
    if !testdata.join(LOCATION_FILE).is_file() || !testdata.join(NETWORK_FILE).is_file() {
        eprintln!(
            "SKIPPED: needs {LOCATION_FILE} and {NETWORK_FILE} in {}",
            testdata.display()
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("iplookup.toml");
    std::fs::write(
        &config,
        format!(
            "listen = \"127.0.0.1:0\"\nlog_level = \"debug\"\n[data]\ndir = \"{}\"\n",
            testdata.display()
        ),
    )
    .unwrap();
    let mut server = Server::start(&config);
    let response = server.get("/json");
    let log = server.next_line("GET /json");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(response.contains("\"ip\": \"127.0.0.1\""), "{response}");
    assert!(response.ends_with("}\n"), "{response:?}");
    assert!(log.contains("DEBUG") && log.contains(" 200 "), "{log}");
}

/// Publishes version 1 of both Data Sources, each downloadable once.
async fn publish_v1(upstream: &Upstream) {
    upstream
        .publish_location(&mmdb::location("Oldtown", V1), 1)
        .await;
    upstream
        .publish_network(&mmdb::network("AS1", "Old Net", V1), "\"v1\"", 1)
        .await;
}

/// Writes a config for `upstream` with data in `<dir>/data`, listening on
/// any free port, plus extra lines for the `[data]` table.
fn write_config(upstream: &Upstream, dir: &Path, data_table: &str) -> PathBuf {
    let config = dir.join("iplookup.toml");
    let toml = upstream.config(&dir.join("data"), "listen = \"127.0.0.1:0\"", data_table);
    std::fs::write(&config, toml).unwrap();
    config
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_downloads_missing_data_then_serves_lookups() {
    let upstream = Upstream::start().await;
    publish_v1(&upstream).await;
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&upstream, dir.path(), "");

    let server = Server::start(&config);
    let response = server.get("/8.8.8.8");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(response.contains("\"city\": \"Oldtown\""), "{response}");
    assert!(response.contains("\"org\": \"AS1 Old Net\""), "{response}");
    assert!(
        server
            .seen
            .iter()
            .any(|l| l.contains("is missing; downloading")),
        "{:#?}",
        server.seen
    );
    assert_no_credentials(&server.seen.join("\n"));
    drop(server);
    upstream.reset().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_exits_when_the_first_download_fails() {
    let upstream = Upstream::start().await;
    upstream.fail_with(500).await;
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&upstream, dir.path(), "");

    let out = iplookup(&config)
        .arg("serve")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("error: downloading Location Data (GeoLite2 City) to "),
        "{stderr}"
    );
    assert!(
        stderr.contains("HTTP 500 Internal Server Error"),
        "{stderr}"
    );
    assert_no_credentials(&stderr);
    assert_no_credentials(&String::from_utf8(out.stdout).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_logs_a_warning_and_keeps_serving_when_an_update_fails() {
    let upstream = Upstream::start().await;
    publish_v1(&upstream).await;
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_config(&upstream, dir.path(), "update_interval = \"1s\"");
    let config = Config::load(&config_path).unwrap();
    let updater = Updater::new(&config).unwrap();
    for source in DataSource::ALL {
        updater.check(source, None).await.unwrap();
    }
    upstream.reset().await;
    upstream.fail_with(429).await;

    let mut server = Server::start(&config_path);
    let warning = server.next_line("update failed");
    assert!(warning.contains("WARN"), "{warning}");
    assert!(warning.contains("keeping the current data"), "{warning}");
    assert!(warning.contains("429"), "{warning}");
    let response = server.get("/8.8.8.8");
    assert!(response.contains("\"city\": \"Oldtown\""), "{response}");
    assert_no_credentials(&server.seen.join("\n"));
}

/// Runs `iplookup update` with `args`.
fn update(config: &Path, args: &[&str]) -> Output {
    iplookup(config)
        .arg("update")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn update_checks_once_and_reports_each_source() {
    let upstream = Upstream::start().await;
    publish_v1(&upstream).await;
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&upstream, dir.path(), "");
    let other = dir.path().join("other");
    let other_arg = other.to_str().unwrap();

    let out = update(&config, &["--data-dir", other_arg]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "Location Data (GeoLite2 City): downloaded new data built 2026-10-27T00:00:00Z ({})",
            other.join(LOCATION_FILE).display()
        )),
        "{stdout}"
    );
    assert!(
        stdout
            .contains("Network Data (IPinfo Lite): downloaded new data built 2026-10-27T00:00:00Z"),
        "{stdout}"
    );
    assert!(other.join(LOCATION_FILE).is_file() && other.join(NETWORK_FILE).is_file());
    assert!(!dir.path().join("data").exists());

    // Nothing new upstream: nothing downloaded (each file only once).
    let out = update(&config, &["--data-dir", other_arg]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains("Location Data (GeoLite2 City): unchanged"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Network Data (IPinfo Lite): unchanged"),
        "{stdout}"
    );
    assert_no_credentials(&stdout);
    upstream.reset().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_reports_failures_and_exits_non_zero() {
    let upstream = Upstream::start().await;
    upstream.fail_with(429).await;
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&upstream, dir.path(), "");

    let out = update(&config, &[]);
    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    for source in [
        "Location Data (GeoLite2 City)",
        "Network Data (IPinfo Lite)",
    ] {
        assert!(
            stdout.contains(&format!("{source}: failed, current file kept: ")),
            "{stdout}"
        );
    }
    assert!(stdout.contains("HTTP 429 Too Many Requests"), "{stdout}");
    assert!(stderr.contains("2 of 2 Data Sources failed"), "{stderr}");
    assert_no_credentials(&stdout);
    assert_no_credentials(&stderr);
}
