// SPDX-License-Identifier: Apache-2.0
//! `oskac self update` (#259 of spec #230), driven as a user or an agent
//! drives it: the CLI is run against a fixture signed release site served
//! over TLS on loopback (see `self_check.rs` and `update_site/mod.rs`), and
//! what it prints, returns and leaves on disk is asserted.
//!
//! The fixture "binaries" are shell scripts that print `oskac X.Y.Z`, which
//! `oskac` runs to read the new binary's version, so these tests run where
//! a script can: Linux and macOS. The Windows replacement (the running file
//! renamed aside) is covered by unit tests of `self_update.rs`.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;
mod update_site;

use std::path::PathBuf;
use std::process::Command;

use common::portable::{json_path, output_of, set_home, Scratch};
use common::Json;
use update_site::{here, json, str_of, test_oskac, Server};

const NOW: &str = "1800000000";

/// The digest the fixture's signed `SHA256SUMS` lists for `oskac-fixture`
/// in release `version` of the site `site`.
fn signed_digest(site: &str, version: &str) -> String {
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
fn served_binary(site: &str, version: &str) -> Vec<u8> {
    std::fs::read(
        here("tests/fixtures/update/site")
            .join(site)
            .join(format!("releases/v{version}/oskac-fixture")),
    )
    .expect("the fixture binary")
}

/// One `oskac self update` scene: a scratch Oska home with an installed
/// `oskac` (a script) in `bin`, and a receipt for it unless told otherwise.
struct Scene {
    home: Scratch,
    bin: PathBuf,
    exe: PathBuf,
    version: &'static str,
}

const INSTALLED: &[u8] = b"#!/bin/sh\necho \"oskac installed\"\n";

impl Scene {
    fn new(name: &str, version: &'static str) -> Scene {
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

    fn receipt_path(&self) -> PathBuf {
        self.home.path().join(".oska").join("receipt.json")
    }

    /// An installer-style receipt for the installed `oskac`, listing the
    /// version file as the installers do.
    fn receipt(&self, serial: u64) -> &Scene {
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

    fn oskac(&self, port: u16, args: &[&str]) -> (i32, String, String) {
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
    fn against(
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
    fn bin_files(&self) -> Vec<String> {
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

    fn receipt_text(&self) -> String {
        std::fs::read_to_string(self.receipt_path()).expect("the receipt")
    }

    /// The live binary and the receipt as bytes, to compare after a failure.
    fn snapshot(&self) -> (Vec<u8>, String) {
        (
            std::fs::read(&self.exe).unwrap_or_default(),
            std::fs::read_to_string(self.receipt_path()).unwrap_or_default(),
        )
    }

    /// The live binary is as installed, the receipt is byte for byte as it
    /// was, and no staging file is left.
    fn assert_untouched(&self, before: &(Vec<u8>, String)) {
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

fn error_of(out: &str) -> (String, String) {
    let v = json(out);
    assert_eq!(v.get("ok"), Some(&Json::Bool(false)), "{out}");
    (
        str_of(&v, "error").to_string(),
        str_of(&v, "message").to_string(),
    )
}

fn receipt_json(scene: &Scene) -> Json {
    Json::parse(&scene.receipt_text())
}

fn field<'a>(receipt: &'a Json, key: &str) -> &'a str {
    str_of(receipt, key)
}

#[test]
fn a_managed_install_updates_to_the_latest_release() {
    let scene = Scene::new("latest", "0.6.0");
    scene.receipt(3);
    let (code, out, _) = scene.against("site", "update", 5, &["update", "--format=json"]);
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(v.get("ok"), Some(&Json::Bool(true)));
    assert_eq!(str_of(&v, "action"), "updated");
    assert_eq!(str_of(&v, "from"), "0.6.0");
    assert_eq!(str_of(&v, "to"), "0.7.0");
    assert_eq!(v.get("index_serial"), Some(&Json::Num(5)));
    assert_eq!(str_of(&v, "site"), "https://oskalang.dev");
    assert_eq!(v.get("warnings"), Some(&Json::Arr(vec![])));
    // The live binary is the release's, runs, and nothing else is beside it.
    assert_eq!(
        std::fs::read(&scene.exe).expect("live"),
        served_binary("update", "0.7.0")
    );
    let ran = Command::new(&scene.exe)
        .output()
        .expect("the new binary runs");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "oskac 0.7.0\n");
    assert_eq!(scene.bin_files(), ["oskac-fixture"]);
    // The receipt: the new version, digest and serial, the old version kept
    // as `previous_version`, and the records the installers read agree.
    let digest = signed_digest("update", "0.7.0");
    let receipt = receipt_json(&scene);
    assert_eq!(field(&receipt, "version"), "0.7.0");
    assert_eq!(field(&receipt, "tag"), "v0.7.0");
    assert_eq!(field(&receipt, "previous_version"), "0.6.0");
    assert_eq!(field(&receipt, "sha256"), digest);
    assert_eq!(receipt.get("index_serial"), Some(&Json::Num(5)));
    let text = scene.receipt_text();
    assert!(
        text.contains(&format!("\"kind\": \"binary\", \"sha256\": \"{digest}\"")),
        "{text}"
    );
    assert!(text.contains("\"installer\": \"sh\""), "{text}");
    assert_eq!(
        std::fs::read_to_string(scene.home.path().join(".oska/version")).expect("version"),
        "0.7.0\n"
    );
}

#[test]
fn running_it_again_says_current_and_changes_nothing() {
    let scene = Scene::new("again", "0.6.0");
    scene.receipt(3);
    let (code, _, _) = scene.against("site", "update", 5, &["update"]);
    assert_eq!(code, 0);
    let scene = Scene {
        version: "0.7.0",
        ..scene
    };
    let before = scene.snapshot();
    let (code, out, _) = scene.against("site", "update", 2, &["update", "--format=json"]);
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(str_of(&v, "action"), "current");
    assert_eq!(str_of(&v, "from"), "0.7.0");
    assert_eq!(str_of(&v, "to"), "0.7.0");
    assert_eq!(scene.snapshot(), before);
    assert_eq!(scene.bin_files(), ["oskac-fixture"]);
}

#[test]
fn a_current_install_raises_the_remembered_serial_and_nothing_else() {
    let scene = Scene::new("current-serial", "0.7.0");
    scene.receipt(2);
    let (code, out, _) = scene.against("site", "update", 2, &["update", "--format=json"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(str_of(&json(&out), "action"), "current");
    let receipt = receipt_json(&scene);
    assert_eq!(receipt.get("index_serial"), Some(&Json::Num(5)));
    assert_eq!(field(&receipt, "version"), "0.7.0");
}

#[test]
fn version_pins_a_newer_release_that_is_not_the_latest() {
    let scene = Scene::new("pin", "0.5.0");
    scene.receipt(1);
    let (code, out, _) = scene.against(
        "site",
        "update",
        5,
        &["update", "--version", "0.6.0", "--format=json"],
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(str_of(&v, "action"), "updated");
    assert_eq!(str_of(&v, "to"), "0.6.0");
    assert_eq!(
        std::fs::read(&scene.exe).expect("live"),
        served_binary("update", "0.6.0")
    );
    assert_eq!(field(&receipt_json(&scene), "version"), "0.6.0");
}

#[test]
fn text_output_says_what_changed() {
    let scene = Scene::new("text", "0.6.0");
    scene.receipt(1);
    let (code, out, err) = scene.against("site", "update", 5, &["update"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("updated oskac from 0.6.0 to 0.7.0\n"),
        "{out}"
    );
    assert!(out.contains("signature verified"), "{out}");
}

#[test]
fn an_older_release_is_refused_without_allow_downgrade() {
    // The latest release is older than the installed one.
    let scene = Scene::new("ahead", "0.9.0");
    scene.receipt(1);
    let before = scene.snapshot();
    let (code, out, _) = scene.against("site", "update", 2, &["update", "--format=json"]);
    assert_eq!(code, 1, "{out}");
    let (error, message) = error_of(&out);
    assert_eq!(error, "downgrade_refused");
    assert!(
        message.contains("--version 0.7.0 --allow-downgrade"),
        "{message}"
    );
    scene.assert_untouched(&before);
    // A named older release too.
    let scene = Scene::new("named-older", "0.7.0");
    scene.receipt(1);
    let before = scene.snapshot();
    let (code, out, err) = scene.against("site", "update", 2, &["update", "--version", "0.6.0"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "");
    assert!(err.starts_with("oskac self update: "), "{err}");
    assert!(err.contains("--allow-downgrade"), "{err}");
    scene.assert_untouched(&before);
}

#[test]
fn allow_downgrade_goes_back_and_is_reported_as_downgraded() {
    let scene = Scene::new("downgrade", "0.7.0");
    scene.receipt(1);
    let (code, out, _) = scene.against(
        "site",
        "update",
        5,
        &[
            "update",
            "--version",
            "0.6.0",
            "--allow-downgrade",
            "--format=json",
        ],
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(str_of(&v, "action"), "downgraded");
    assert_eq!(str_of(&v, "from"), "0.7.0");
    assert_eq!(str_of(&v, "to"), "0.6.0");
    assert_eq!(
        std::fs::read(&scene.exe).expect("live"),
        served_binary("update", "0.6.0")
    );
    let receipt = receipt_json(&scene);
    assert_eq!(field(&receipt, "version"), "0.6.0");
    assert_eq!(field(&receipt, "previous_version"), "0.7.0");
}

#[test]
fn allow_downgrade_lowers_the_version_guard_and_nothing_else() {
    // A serial below the receipt's is still refused.
    let scene = Scene::new("downgrade-rollback", "0.7.0");
    scene.receipt(9);
    let before = scene.snapshot();
    let (code, out, _) = scene.against(
        "site",
        "update",
        2,
        &[
            "update",
            "--version",
            "0.6.0",
            "--allow-downgrade",
            "--format=json",
        ],
    );
    assert_eq!(code, 1, "{out}");
    assert_eq!(error_of(&out).0, "rollback_refused");
    scene.assert_untouched(&before);
}

#[test]
fn allow_downgrade_needs_version_and_belongs_to_self_update() {
    let scene = Scene::new("usage", "0.6.0");
    scene.receipt(1);
    let before = scene.snapshot();
    for args in [
        &["update", "--allow-downgrade", "--format=json"][..],
        &["update", "--bogus", "--format=json"],
        &["update", "--version", "--format=json"],
        &["check", "--version", "0.6.0", "--format=json"],
        &["check", "--allow-downgrade", "--format=json"],
        &["update", "--no-identify", "--format=json"],
        &["update", "extra", "--format=json"],
    ] {
        let (code, out, _) = scene.oskac(1, args);
        assert_eq!(code, 2, "{args:?}: {out}");
        assert_eq!(error_of(&out).0, "usage", "{args:?}: {out}");
    }
    scene.assert_untouched(&before);
}

/// An unmanaged install is refused with guidance, before any network use
/// (a listener that is never connected to proves it) and with no change.
fn unmanaged(scene: &Scene, why: &str) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    listener.set_nonblocking(true).expect("non-blocking");
    let port = listener.local_addr().expect("an address").port();
    let before = scene.snapshot();
    let (code, out, _) = scene.oskac(port, &["update", "--format=json"]);
    assert_eq!(code, 1, "{why}: {out}");
    let (error, message) = error_of(&out);
    assert_eq!(error, "unmanaged_install", "{why}: {out}");
    assert!(message.contains("Re-run the installer"), "{why}: {message}");
    assert!(message.contains("rebuild from source"), "{why}: {message}");
    assert!(
        matches!(listener.accept(), Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "{why}: it connected"
    );
    assert_eq!(scene.snapshot(), before, "{why}");
    assert_eq!(scene.bin_files(), ["oskac-fixture"], "{why}");
    // A text refusal says the same on standard error.
    let (code, out, err) = scene.oskac(port, &["update"]);
    assert_eq!((code, out.as_str()), (1, ""), "{why}");
    assert!(err.contains("not a managed install"), "{why}: {err}");
}

#[test]
fn an_install_with_no_receipt_is_refused_but_still_checks() {
    let scene = Scene::new("no-receipt", "0.6.0");
    unmanaged(&scene, "no receipt");
    // `self check` works on it, as on every install.
    let (code, out, _) = scene.against(
        "site",
        "ok",
        2,
        &["check", "--no-identify", "--format=json"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        json(&out).get("serial_remembered"),
        Some(&Json::Bool(false))
    );
}

#[test]
fn a_receipt_for_another_path_is_refused() {
    let scene = Scene::new("other-path", "0.6.0");
    scene.receipt(1);
    // The receipt now records a binary somewhere else: this is a copy.
    let text = scene.receipt_text().replace(
        &json_path(&scene.exe),
        &json_path(&scene.home.path().join("elsewhere").join("oskac")),
    );
    std::fs::write(scene.receipt_path(), text).expect("the receipt");
    unmanaged(&scene, "another path");
    let (code, out, _) = scene.against(
        "site",
        "ok",
        2,
        &["check", "--no-identify", "--format=json"],
    );
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_receipt_for_another_version_is_refused() {
    let scene = Scene::new("other-version", "0.6.0");
    scene.receipt(1);
    let text = scene
        .receipt_text()
        .replace("\"version\": \"0.6.0\"", "\"version\": \"0.5.0\"");
    std::fs::write(scene.receipt_path(), text).expect("the receipt");
    unmanaged(&scene, "another version");
}

/// A failed update leaves the live binary and the receipt byte for byte as
/// they were, and no staging file.
fn fails(
    name: &str,
    scenario: &str,
    site: &str,
    requests: usize,
    args: &[&str],
    code_is: &str,
) -> String {
    let scene = Scene::new(name, "0.6.0");
    scene.receipt(3);
    let before = scene.snapshot();
    let (code, out, _) = scene.against(scenario, site, requests, args);
    assert_eq!(code, 1, "{out}");
    let (error, message) = error_of(&out);
    assert_eq!(error, code_is, "{out}");
    scene.assert_untouched(&before);
    message
}

#[test]
fn a_bad_index_signature_changes_nothing() {
    fails(
        "bad-index",
        "site",
        "tampered_index",
        2,
        &["update", "--format=json"],
        "bad_index",
    );
}

#[test]
fn a_tampered_sha256sums_changes_nothing() {
    fails(
        "bad-sums",
        "site",
        "update_bad_sums",
        4,
        &["update", "--format=json"],
        "bad_checksums",
    );
}

#[test]
fn a_binary_whose_digest_differs_changes_nothing() {
    let message = fails(
        "bad-digest",
        "site",
        "update_bad_digest",
        5,
        &["update", "--format=json"],
        "checksum_mismatch",
    );
    assert!(message.contains("SHA-256"), "{message}");
}

#[test]
fn a_binary_that_reports_another_version_changes_nothing() {
    let message = fails(
        "wrong-version",
        "site",
        "update_wrong_version",
        5,
        &["update", "--format=json"],
        "version_mismatch",
    );
    assert!(message.contains("oskac 0.9.9"), "{message}");
}

#[test]
fn a_connection_cut_mid_download_changes_nothing() {
    fails(
        "cut",
        "site_cut",
        "update",
        5,
        &["update", "--format=json"],
        "network_error",
    );
}

#[test]
fn a_release_the_site_does_not_have_changes_nothing() {
    let message = fails(
        "no-release",
        "site",
        "update",
        3,
        &["update", "--version", "0.8.0", "--format=json"],
        "network_error",
    );
    assert!(message.contains("no release 0.8.0"), "{message}");
}

#[test]
fn a_failed_replacement_leaves_the_live_install_and_no_staging_file() {
    let scene = Scene::new("replace-fails", "0.6.0");
    // The path the receipt records is a directory: a file cannot be moved
    // over it, though everything before the move succeeds.
    std::fs::remove_file(&scene.exe).expect("remove");
    std::fs::create_dir(&scene.exe).expect("a directory in its place");
    scene.receipt(3);
    let before = scene.receipt_text();
    let (code, out, _) = scene.against("site", "update", 5, &["update", "--format=json"]);
    assert_eq!(code, 1, "{out}");
    let (error, message) = error_of(&out);
    assert_eq!(error, "replace_failed", "{out}");
    assert!(message.contains("as it was"), "{message}");
    assert!(scene.exe.is_dir());
    assert_eq!(scene.receipt_text(), before);
    assert_eq!(scene.bin_files(), ["oskac-fixture"]);
    assert_eq!(
        std::fs::read_to_string(scene.home.path().join(".oska/version")).expect("version"),
        "0.6.0\n"
    );
    let names: Vec<String> = std::fs::read_dir(scene.home.path().join(".oska"))
        .expect("the Oska home")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        !names.iter().any(|n| n.ends_with(".tmp")),
        "a temporary file was left: {names:?}"
    );
}

/// The checker may write one file: the staging file beside the live binary.
/// This is its source's own signature (the program's authority is visible
/// there, and the checker enforces the narrowing), and the live binary is
/// written by `oskac` alone.
#[test]
fn the_update_checker_may_write_only_one_named_file() {
    let source = std::fs::read_to_string(here("update/checker.osk")).expect("the checker");
    let writes: Vec<&str> = source.lines().filter(|l| l.contains("write")).collect();
    // The one grant: a write capability narrowed to the staging name, used
    // to write the verified bytes; nowhere else in the program.
    let uses: Vec<&&str> = writes
        .iter()
        .filter(|l| l.contains("restrict_to") || l.contains("write_bytes") || l.contains(".write("))
        .collect();
    assert_eq!(uses.len(), 1, "{uses:?}");
    assert!(
        uses[0].contains(".restrict_to(\".\").restrict_to_file(Path.from(staging)).write_bytes("),
        "{}",
        uses[0]
    );
    // `oskac` never hands it the live path as the file to write: the
    // staging name is its own.
    let rust = std::fs::read_to_string(here("src/self_update.rs")).expect("self_update.rs");
    assert!(rust.contains("staging: staging.name()"));
}
