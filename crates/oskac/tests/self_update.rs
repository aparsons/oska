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

use std::process::Command;

use common::portable::json_path;
use common::Json;
use update_site::{
    error_of, field, here, json, receipt_json, served_binary, signed_digest, str_of, Scene,
};

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
    let (_, out, _) = scene.oskac(1, &["update", "--format=json"]);
    let message = error_of(&out).1;
    assert!(
        message.contains("receipt records version 0.5.0"),
        "{message}"
    );
    assert!(
        message.contains("not the one the installer recorded"),
        "{message}"
    );
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
