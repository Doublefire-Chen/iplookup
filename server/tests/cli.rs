//! The `iplookup` binary: starting from a config file.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use iplookup::databases::{LOCATION_FILE, NETWORK_FILE};

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
fn missing_databases_stop_startup() {
    let stderr = serve_fails("listen = \"127.0.0.1:0\"\n", |_| {});
    assert!(stderr.contains("Location Data"), "{stderr}");
    assert!(stderr.contains(LOCATION_FILE), "{stderr}");
}

#[test]
fn an_unreadable_database_stops_startup() {
    let stderr = serve_fails("listen = \"127.0.0.1:0\"\n", |data| {
        std::fs::write(data.join(LOCATION_FILE), b"not an mmdb file").unwrap();
    });
    assert!(stderr.contains("opening Location Data"), "{stderr}");
    assert!(stderr.contains(LOCATION_FILE), "{stderr}");
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
    let mut child = iplookup(&config)
        .arg("serve")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let next_line = |want: &str| loop {
        let line = rx
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("no stdout line containing {want:?}"));
        if line.contains(want) {
            return line;
        }
    };

    let line = next_line("listening on http://");
    let addr = line.rsplit("http://").next().unwrap().to_owned();
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(b"GET /json HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let log = next_line("GET /json");

    child.kill().unwrap();
    child.wait().unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(response.contains("\"ip\": \"127.0.0.1\""), "{response}");
    assert!(response.ends_with("}\n"), "{response:?}");
    assert!(log.contains("DEBUG") && log.contains(" 200 "), "{log}");
}
