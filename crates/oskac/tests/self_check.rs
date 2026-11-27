// SPDX-License-Identifier: Apache-2.0
//! `oskac self check` (#258 of spec #230), driven as a user or an agent
//! drives it: the CLI is run against a fixture signed release index served
//! over TLS on loopback, and what it prints, returns and leaves on disk is
//! asserted, never how the update checker is built.
//!
//! The server is the loopback TLS test server of the TLS tests
//! (`tls/server.osk`, scenario `site`), with its test CA, serving a
//! fixture directory of `fixtures/update/site/<case>` (made by
//! `fixtures/update/generate.sh`, signed by a throwaway test key). The
//! `oskac` under test is built with the test-only feature `test-site`
//! into a target directory of its own, which points the update checker at
//! the loopback server, its test CA and the test key, and lets the test fix
//! the clock, the version and the "running" executable (a fixture file).
//! A plain `oskac` has none of that: `scripts/test-site-guard` keeps the
//! feature out of every release build.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;
mod update_site;

use std::path::PathBuf;
use std::process::Command;

use common::portable::{json_path, output_of, set_home, Scratch};
use common::Json;
use update_site::{here, json, str_of, test_oskac, Server};

/// One `oskac self check` run: a scratch home with a "running" executable
/// and, optionally, a receipt.
struct Run {
    home: Scratch,
    exe: PathBuf,
    port: u16,
    chain_root: String,
    version: &'static str,
    now: &'static str,
}

/// The digest of the fixture executable `exe/genuine`, from the fixture's
/// signed `SHA256SUMS`.
fn genuine_digest() -> String {
    let sums = std::fs::read_to_string(here(
        "tests/fixtures/update/site/ok/releases/v0.6.0/SHA256SUMS",
    ))
    .expect("the fixture SHA256SUMS");
    sums.lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().next())
        .expect("a digest")
        .to_string()
}

const NOW: &str = "1800000000";

impl Run {
    fn new(name: &str, port: u16, exe: &str) -> Run {
        let home = Scratch::new("self-check", name);
        let target = home.path().join("oskac-fixture");
        std::fs::copy(here("tests/fixtures/update/exe").join(exe), &target).expect("the fixture");
        Run {
            exe: target,
            home,
            port,
            chain_root: std::fs::read_to_string(here("tests/tls/pki/root.hex"))
                .expect("the test root")
                .trim()
                .to_string(),
            version: "0.6.0",
            now: NOW,
        }
    }

    fn receipt_path(&self) -> PathBuf {
        self.home.path().join(".oska").join("receipt.json")
    }

    /// An installer-style receipt that records the fixture executable.
    fn receipt(&self, serial: u64) -> &Run {
        let dir = self.home.path().join(".oska");
        std::fs::create_dir_all(&dir).expect("the Oska home");
        let text = format!(
            "{{\n  \"schema\": 1,\n  \"version\": \"{}\",\n  \"sha256\": \"{}\",\n  \"index_serial\": {serial},\n  \"files\": [\n    {{\"path\": {}, \"kind\": \"binary\", \"sha256\": \"{}\"}}\n  ]\n}}\n",
            self.version,
            genuine_digest(),
            json_path(&self.exe),
            genuine_digest()
        );
        std::fs::write(self.receipt_path(), text).expect("the receipt");
        self
    }

    fn oskac(&self, args: &[&str]) -> (i32, String, String) {
        let key = std::fs::read_to_string(here("tests/fixtures/update/test_key.pub"))
            .expect("the test key");
        let mut command = Command::new(test_oskac());
        set_home(&mut command, self.home.path())
            .env("OSKA_HOME", self.home.path().join(".oska"))
            .env("OSKAC_TEST_SITE_PORT", self.port.to_string())
            .env("OSKAC_TEST_SITE_ROOT", &self.chain_root)
            .env("OSKAC_TEST_SITE_KEY", key.trim())
            .env("OSKAC_TEST_NOW", self.now)
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

    fn serial_in_receipt(&self) -> String {
        let text = std::fs::read_to_string(self.receipt_path()).expect("the receipt");
        Json::parse(&text)
            .get("index_serial")
            .map(|v| format!("{v:?}"))
            .unwrap_or_default()
    }
}

fn identity(value: &Json) -> &Json {
    value.get("identity").expect("an identity")
}

/// A check against the fixture site `site` that expects `requests`
/// requests, with a receipt of serial `serial` when it is given.
fn check(
    name: &str,
    site: &str,
    exe: &str,
    serial: Option<u64>,
    requests: usize,
    args: &[&str],
    tweak: impl FnOnce(&mut Run),
) -> (Run, (i32, String, String)) {
    let server = Server::start("site", "chain_good.hex", site, requests);
    let mut run = Run::new(name, server.port, exe);
    if let Some(serial) = serial {
        run.receipt(serial);
    }
    tweak(&mut run);
    let answer = run.oskac(args);
    assert_eq!(server.served(), requests, "the number of requests");
    (run, answer)
}

#[test]
fn an_update_exists_and_the_binary_is_genuine() {
    let (run, (code, out, _)) = check(
        "update",
        "ok",
        "genuine",
        Some(3),
        4,
        &["check", "--format=json"],
        |_| {},
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(v.get("ok"), Some(&Json::Bool(true)));
    assert_eq!(str_of(&v, "action"), "checked");
    assert_eq!(str_of(&v, "installed"), "0.6.0");
    assert_eq!(str_of(&v, "latest"), "0.7.0");
    assert_eq!(str_of(&v, "tag"), "v0.7.0");
    assert_eq!(str_of(&v, "comparison"), "behind");
    assert_eq!(v.get("update_available"), Some(&Json::Bool(true)));
    assert_eq!(v.get("index_serial"), Some(&Json::Num(5)));
    assert_eq!(str_of(&v, "index_expires"), "2099-01-01T00:00:00Z");
    assert_eq!(v.get("serial_remembered"), Some(&Json::Bool(true)));
    assert_eq!(str_of(&v, "site"), "https://oskalang.dev");
    assert_eq!(v.get("warnings"), Some(&Json::Arr(vec![])));
    let who = identity(&v);
    assert_eq!(str_of(who, "state"), "genuine");
    assert_eq!(str_of(who, "sha256"), genuine_digest());
    assert_eq!(str_of(who, "expected_sha256"), genuine_digest());
    assert_eq!(who.get("matches_receipt"), Some(&Json::Bool(true)));
    // The receipt's serial is raised, and nothing else about it changes.
    assert_eq!(run.serial_in_receipt(), "Num(5)");
    let text = std::fs::read_to_string(run.receipt_path()).expect("the receipt");
    assert!(text.contains("\"index_serial\": 5,\n  \"files\""), "{text}");
}

#[test]
fn a_current_install_says_so_and_exits_zero() {
    let (_, (code, out, _)) = check("current", "ok", "genuine", None, 4, &["check"], |r| {
        r.version = "0.7.0";
    });
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("installed: 0.7.0"), "{out}");
    assert!(out.contains("up to date"), "{out}");
    assert!(out.contains("this oskac is genuine"), "{out}");
    assert!(!out.contains("to update, run"), "{out}");
}

#[test]
fn a_build_ahead_of_the_latest_release_is_not_offered_a_downgrade() {
    let (_, (code, out, _)) = check(
        "ahead",
        "ok",
        "genuine",
        None,
        // The index and its signature, then a SHA256SUMS that is not there.
        3,
        &["check", "--format=json"],
        |r| r.version = "0.9.0",
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(str_of(&v, "comparison"), "ahead");
    assert_eq!(v.get("update_available"), Some(&Json::Bool(false)));
    // No release 0.9.0 is published: a source build, said plainly.
    assert_eq!(str_of(identity(&v), "state"), "unknown_release");
}

#[test]
fn a_changed_binary_is_reported_modified_and_not_as_an_error() {
    let (_, (code, out, _)) = check(
        "modified",
        "ok",
        "modified",
        Some(1),
        4,
        &["check", "--format=json"],
        |_| {},
    );
    assert_eq!(code, 0, "{out}");
    let who_value = json(&out);
    let who = identity(&who_value);
    assert_eq!(str_of(who, "state"), "modified");
    assert_eq!(str_of(who, "expected_sha256"), genuine_digest());
    assert_ne!(str_of(who, "sha256"), genuine_digest());
    // The receipt recorded the genuine digest, and the file is another.
    assert_eq!(who.get("matches_receipt"), Some(&Json::Bool(false)));
}

#[test]
fn no_identify_skips_the_two_extra_downloads() {
    let (_, (code, out, _)) = check(
        "skipped",
        "ok",
        "modified",
        None,
        2,
        &["check", "--format=json", "--no-identify"],
        |_| {},
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    let who = identity(&v);
    assert_eq!(str_of(who, "state"), "skipped");
    assert_eq!(who.get("sha256"), Some(&Json::Null));
    assert_eq!(v.get("serial_remembered"), Some(&Json::Bool(false)));
}

#[test]
fn the_next_step_is_oskac_self_update_on_a_managed_install_and_the_installer_otherwise() {
    let (_, (_, managed, _)) = check("managed", "ok", "genuine", Some(1), 4, &["check"], |_| {});
    assert!(
        managed
            .trim_end()
            .ends_with("to update, run: oskac self update"),
        "{managed}"
    );
    let (_, (_, other, _)) = check("unmanaged", "ok", "genuine", None, 4, &["check"], |_| {});
    assert!(other.contains("serial remembered: no"), "{other}");
    let installer = if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    };
    assert!(
        other
            .trim_end()
            .lines()
            .last()
            .is_some_and(|l| l.starts_with("to update, run: ") && l.contains(installer)),
        "{other}"
    );
}

#[test]
fn fewer_than_thirty_days_left_is_a_warning() {
    let (_, (code, out, _)) = check(
        "soon",
        "soon",
        "genuine",
        Some(1),
        2,
        &["check", "--format=json", "--no-identify"],
        |r| r.now = "1893456000",
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    match v.get("warnings") {
        Some(Json::Arr(items)) => {
            assert_eq!(items.len(), 1, "{out}");
            assert!(
                matches!(&items[0], Json::Str(s) if s.contains("expires in 9 days")),
                "{out}"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// A refused check: exit 1, the stable code in JSON, nothing on standard
/// output in text mode, and the receipt as it was.
fn refused(name: &str, site: &str, serial: u64, requests: usize, code: &str, wording: &str) {
    let (run, (status, out, _)) = check(
        name,
        site,
        "genuine",
        Some(serial),
        requests,
        &["check", "--format=json"],
        |_| {},
    );
    assert_eq!(status, 1, "{out}");
    let v = json(&out);
    assert_eq!(v.get("ok"), Some(&Json::Bool(false)), "{out}");
    assert_eq!(str_of(&v, "error"), code, "{out}");
    assert!(str_of(&v, "message").contains(wording), "{out}");
    assert_eq!(run.serial_in_receipt(), format!("Num({serial})"));
}

#[test]
fn an_expired_index_is_stale() {
    refused(
        "stale",
        "expired",
        1,
        2,
        "stale_index",
        "check the system clock",
    );
}

#[test]
fn a_serial_below_the_remembered_one_is_a_rollback() {
    refused(
        "rollback",
        "ok",
        9,
        2,
        "rollback_refused",
        "serial 5, below 9",
    );
}

#[test]
fn an_index_signed_in_another_namespace_is_a_bad_index() {
    refused(
        "namespace",
        "wrong_namespace",
        1,
        2,
        "bad_index",
        "oska-index",
    );
}

#[test]
fn an_index_changed_after_signing_is_a_bad_index() {
    refused(
        "tampered",
        "tampered_index",
        1,
        2,
        "bad_index",
        "not signed",
    );
}

#[test]
fn tampered_checksums_are_refused_before_any_line_is_read() {
    refused("sums", "bad_checksums", 1, 4, "bad_checksums", "SHA256SUMS");
}

#[test]
fn a_failed_check_in_text_mode_prints_nothing_on_stdout() {
    let (_, (code, out, err)) = check("text", "expired", "genuine", None, 2, &["check"], |_| {});
    assert_eq!(code, 1);
    assert_eq!(out, "");
    assert!(err.starts_with("oskac self check: "), "{err}");
}

/// A TLS failure is `tls_error`, never "no update".
fn tls_failure(name: &str, chain: &str) {
    let server = Server::start("site", chain, "ok", 1);
    let run = Run::new(name, server.port, "genuine");
    let (code, out, _) = run.oskac(&["check", "--format=json"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(str_of(&json(&out), "error"), "tls_error", "{out}");
}

#[test]
fn a_certificate_from_an_unknown_ca_is_a_tls_error() {
    tls_failure("unknown-ca", "chain_unknown.hex");
}

#[test]
fn a_certificate_for_the_wrong_host_is_a_tls_error() {
    tls_failure("wrong-host", "chain_wrong_host.hex");
}

#[test]
fn an_expired_certificate_is_a_tls_error_that_mentions_the_clock() {
    let server = Server::start("site", "chain_expired.hex", "ok", 1);
    let run = Run::new("expired-cert", server.port, "genuine");
    let (code, out, _) = run.oskac(&["check", "--format=json"]);
    assert_eq!(code, 1, "{out}");
    let v = json(&out);
    assert_eq!(str_of(&v, "error"), "tls_error", "{out}");
    assert!(str_of(&v, "message").contains("clock"), "{out}");
}

#[test]
fn a_closed_port_is_a_network_error() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("an address")
        .port();
    let run = Run::new("closed", port, "genuine");
    let (code, out, _) = run.oskac(&["check", "--format=json"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(str_of(&json(&out), "error"), "network_error", "{out}");
}

#[test]
fn a_redirect_is_refused_and_never_followed() {
    let server = Server::start("site_redirect", "chain_good.hex", "ok", 1);
    let run = Run::new("redirect", server.port, "genuine");
    let (code, out, _) = run.oskac(&["check", "--format=json"]);
    assert_eq!(code, 1, "{out}");
    let v = json(&out);
    assert_eq!(str_of(&v, "error"), "network_error", "{out}");
    assert!(
        str_of(&v, "message").contains("redirects are refused"),
        "{out}"
    );
    assert_eq!(server.served(), 1);
}

#[test]
fn a_response_far_past_the_cap_is_refused() {
    let server = Server::start("site_large", "chain_good.hex", "ok", 1);
    let run = Run::new("large", server.port, "genuine");
    let (code, out, _) = run.oskac(&["check", "--format=json"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(str_of(&json(&out), "error"), "network_error", "{out}");
}

#[test]
fn an_unknown_flag_is_a_usage_error_in_json_and_the_flag_belongs_to_self_check_alone() {
    let run = Run::new("usage", 1, "genuine");
    let (code, out, _) = run.oskac(&["check", "--bogus", "--format=json"]);
    assert_eq!(code, 2, "{out}");
    assert_eq!(str_of(&json(&out), "error"), "usage");
    let (code, _, err) = run.oskac(&["info", "--no-identify"]);
    assert_eq!(code, 2);
    assert!(err.contains("oskac self check"), "{err}");
    // The same misuse under --format=json is a JSON error, on any command.
    let (code, out, _) = run.oskac(&["info", "--no-identify", "--format=json"]);
    assert_eq!(code, 2, "{out}");
    assert_eq!(str_of(&json(&out), "error"), "usage");
    let mut command = Command::new(test_oskac());
    command.args(["check", "--no-identify", "--format=json"]);
    let out = output_of(&mut command);
    assert_eq!(out.status.code(), Some(2));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(str_of(&json(&text), "error"), "usage", "{text}");
    // An unknown flag to `self check`, and a second subcommand, likewise.
    let (code, out, _) = run.oskac(&["check", "--no-identify", "--bogus=1", "--format=json"]);
    assert_eq!(code, 2, "{out}");
    assert_eq!(str_of(&json(&out), "error"), "usage");
    let (code, out, _) = run.oskac(&["check", "extra", "--format=json"]);
    assert_eq!(code, 2, "{out}");
    assert_eq!(str_of(&json(&out), "error"), "usage");
}
