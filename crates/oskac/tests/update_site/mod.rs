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

use crate::common::portable::{json_path, output_of, set_home, Scratch};
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

// ---- The `self update` scene (#259, #260) --------------------------------

pub const NOW: &str = "1800000000";

/// The digest the fixture's signed `SHA256SUMS` lists for `oskac-fixture`
/// in release `version` of the site `site`.
pub fn signed_digest(site: &str, version: &str) -> String {
    let sums = std::fs::read_to_string(
        here("tests/fixtures/update/site")
            .join(site)
            .join(format!("releases/v{version}/SHA256SUMS")),
    )
    .expect("the fixture SHA256SUMS");
    sums.lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().next())
        .expect("a digest")
        .to_string()
}

/// What the site serves as release `version`'s binary.
pub fn served_binary(site: &str, version: &str) -> Vec<u8> {
    std::fs::read(
        here("tests/fixtures/update/site")
            .join(site)
            .join(format!("releases/v{version}/oskac-fixture")),
    )
    .expect("the fixture binary")
}

/// One `oskac self update` scene: a scratch Oska home with an installed
/// `oskac` (a script) in `bin`, and a receipt for it unless told otherwise.
pub struct Scene {
    pub home: Scratch,
    pub bin: PathBuf,
    pub exe: PathBuf,
    pub version: &'static str,
}

pub const INSTALLED: &[u8] = b"#!/bin/sh\necho \"oskac installed\"\n";

impl Scene {
    pub fn new(name: &str, version: &'static str) -> Scene {
        let home = Scratch::new("self-update", name);
        let bin = home.path().join(".oska").join("bin");
        std::fs::create_dir_all(&bin).expect("the bin directory");
        let exe = bin.join("oskac-fixture");
        std::fs::write(&exe, INSTALLED).expect("the installed oskac");
        Scene {
            home,
            bin,
            exe,
            version,
        }
    }

    pub fn receipt_path(&self) -> PathBuf {
        self.home.path().join(".oska").join("receipt.json")
    }

    /// An installer-style receipt for the installed `oskac`, listing the
    /// version file as the installers do.
    pub fn receipt(&self, serial: u64) -> &Scene {
        let home = self.home.path().join(".oska");
        let version_file = home.join("version");
        std::fs::write(&version_file, format!("{}\n", self.version)).expect("the version file");
        let text = format!(
            "{{\n  \"schema\": 1,\n  \"version\": \"{}\",\n  \"tag\": \"v{}\",\n  \"platform\": \"linux-x86_64\",\n  \"sha256\": \"{}\",\n  \"installer\": \"sh\",\n  \"index_serial\": {serial},\n  \"files\": [\n    {{\"path\": {}, \"kind\": \"binary\", \"sha256\": \"{}\"}},\n    {{\"path\": {}, \"kind\": \"record\", \"sha256\": \"{}\"}},\n    {{\"path\": {}, \"kind\": \"record\"}}\n  ]\n}}\n",
            self.version,
            self.version,
            "ab".repeat(32),
            json_path(&self.exe),
            "ab".repeat(32),
            json_path(&version_file),
            "cd".repeat(32),
            json_path(&self.receipt_path()),
        );
        std::fs::write(self.receipt_path(), text).expect("the receipt");
        self
    }

    pub fn oskac(&self, port: u16, args: &[&str]) -> (i32, String, String) {
        let key = std::fs::read_to_string(here("tests/fixtures/update/test_key.pub"))
            .expect("the test key");
        let root = std::fs::read_to_string(here("tests/tls/pki/root.hex")).expect("the test root");
        let mut command = Command::new(test_oskac());
        set_home(&mut command, self.home.path())
            .env("OSKA_HOME", self.home.path().join(".oska"))
            .env("OSKAC_TEST_SITE_PORT", port.to_string())
            .env("OSKAC_TEST_SITE_ROOT", root.trim())
            .env("OSKAC_TEST_SITE_KEY", key.trim())
            .env("OSKAC_TEST_NOW", NOW)
            .env("OSKAC_TEST_VERSION", self.version)
            .env("OSKAC_TEST_EXE", &self.exe)
            .env("OSKAC_TEST_ASSETS", "oskac-fixture")
            .arg("self")
            .args(args);
        let out = output_of(&mut command);
        (
            out.status.code().expect("an exit code"),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// Run against the site `site`, which must see `requests` requests.
    pub fn against(
        &self,
        scenario: &str,
        site: &str,
        requests: usize,
        args: &[&str],
    ) -> (i32, String, String) {
        let server = Server::start(scenario, "chain_good.hex", site, requests);
        let answer = self.oskac(server.port, args);
        assert_eq!(server.served(), requests, "the number of requests");
        answer
    }

    /// Everything beside the live binary, by name.
    pub fn bin_files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.bin)
            .expect("the bin directory")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    pub fn receipt_text(&self) -> String {
        std::fs::read_to_string(self.receipt_path()).expect("the receipt")
    }

    /// The live binary and the receipt as bytes, to compare after a failure.
    pub fn snapshot(&self) -> (Vec<u8>, String) {
        (
            std::fs::read(&self.exe).unwrap_or_default(),
            std::fs::read_to_string(self.receipt_path()).unwrap_or_default(),
        )
    }

    /// The live binary is as installed, the receipt is byte for byte as it
    /// was, and no staging file is left.
    pub fn assert_untouched(&self, before: &(Vec<u8>, String)) {
        assert_eq!(
            &self.snapshot(),
            before,
            "the live binary or the receipt changed"
        );
        assert_eq!(
            self.bin_files(),
            ["oskac-fixture"],
            "a file was left behind"
        );
        let home = self.home.path().join(".oska");
        let mut names: Vec<String> = std::fs::read_dir(&home)
            .expect("the Oska home")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        let expected: &[&str] = if before.1.is_empty() {
            &["bin"]
        } else {
            &["bin", "receipt.json", "version"]
        };
        assert_eq!(names, expected, "a file was left in the Oska home");
    }
}

pub fn error_of(out: &str) -> (String, String) {
    let v = json(out);
    assert_eq!(v.get("ok"), Some(&Json::Bool(false)), "{out}");
    (
        str_of(&v, "error").to_string(),
        str_of(&v, "message").to_string(),
    )
}

pub fn receipt_json(scene: &Scene) -> Json {
    Json::parse(&scene.receipt_text())
}

pub fn field<'a>(receipt: &'a Json, key: &str) -> &'a str {
    str_of(receipt, key)
}
