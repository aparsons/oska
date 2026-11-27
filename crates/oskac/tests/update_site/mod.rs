// SPDX-License-Identifier: Apache-2.0
//! Shared by the `self check` and `self update` tests (specs #230, tickets
//! #258 and #259): the loopback TLS test server serving a fixture release
//! site, and the `oskac` built with the test-only feature `test-site`.
//! Each test file declares `mod differential;` and `mod update_site;`.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

use crate::common::Json;
use crate::differential::{build_native, Case};

const SERVER: &str = include_str!("../tls/server.osk");

pub fn here(parts: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(parts)
}

/// The test server, built natively once.
fn server_exe() -> &'static Path {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| build_native(&Case::file("self_check_server", SERVER)).0)
}

/// `oskac` built with `--features test-site`, once, into its own target
/// directory under this test run's.
pub fn test_oskac() -> &'static Path {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| {
        let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("test-site");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let status = Command::new(cargo)
            .args(["build", "--quiet", "-p", "oskac", "--features", "test-site"])
            .arg("--target-dir")
            .arg(&target)
            .arg("--manifest-path")
            .arg(here("Cargo.toml"))
            .status()
            .expect("cargo should run");
        assert!(status.success(), "cannot build oskac with test-site");
        target
            .join("debug")
            .join(format!("oskac{}", std::env::consts::EXE_SUFFIX))
    })
}

/// A running test server, killed when it is dropped.
pub struct Server {
    child: Child,
    pub port: u16,
}

impl Server {
    /// Serve `connections` connections of `scenario` from the fixture
    /// site `site`, with the certificate chain `chain`.
    pub fn start(scenario: &str, chain: &str, site: &str, connections: usize) -> Server {
        // Build `oskac` before the server listens: the first test to need it
        // waits for a build that can take over a minute on a slow runner,
        // and a server waiting for its client gives up after the runtime's
        // 60 seconds of silence on `accept` and closes its port.
        test_oskac();
        let pki = here("tests/tls/pki");
        let mut child = Command::new(server_exe())
            .arg(pki.join(chain))
            .arg(pki.join("leaf.key.hex"))
            .arg(scenario)
            .arg(connections.to_string())
            .arg(here("tests/fixtures/update/site").join(site))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the test server should start");
        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("a piped stdout"))
            .read_line(&mut line)
            .expect("the server prints its port");
        let port = line.trim().parse().expect("a port");
        Server { child, port }
    }

    /// The number of connections the server served, once it has served
    /// the number it was started for (so a client that made fewer or more
    /// requests than expected shows).
    pub fn served(mut self) -> usize {
        let mut text = String::new();
        self.child
            .stderr
            .take()
            .expect("a piped stderr")
            .read_to_string(&mut text)
            .expect("the server's stderr");
        let _ = self.child.wait();
        text.lines().filter(|l| l.contains("alert")).count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn json(text: &str) -> Json {
    Json::parse(text.trim())
}

pub fn str_of<'a>(value: &'a Json, key: &str) -> &'a str {
    match value.get(key) {
        Some(Json::Str(s)) => s,
        other => panic!("no string {key}: {other:?}"),
    }
}
