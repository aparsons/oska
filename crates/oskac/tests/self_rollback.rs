// SPDX-License-Identifier: Apache-2.0
//! `oskac self update --rollback` and `--keep-previous` (#260 of spec #230),
//! and the removal of the kept copy by `oskac self uninstall`: driven as a
//! user drives them, against the fixture signed release site of
//! `update_site/mod.rs` (see `self_update.rs`). The tests that run a fixture
//! release (shell scripts) run on Linux and macOS; the uninstall tests run
//! wherever `oskac` does. Attack tests: a receipt is advisory, so one that
//! names some other file, or a link, as the kept copy must get nothing
//! outside the install read, used or deleted.

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

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod site {
    use crate::common::portable::json_path;
    use crate::common::Json;
    use crate::update_site::{
        error_of, field, json, receipt_json, served_binary, signed_digest, str_of, Scene,
    };

    // ---- Rollback and --keep-previous (#260 of spec #230) ---------------------

    /// What the kept copy of 0.6.0 is called beside the live binary.
    const KEPT_NAME: &str = "oskac-fixture-0.6.0";

    /// An install of 0.6.0 whose binary is the published one (the bytes the
    /// signed `SHA256SUMS` of 0.6.0 lists), updated to 0.7.0 with
    /// `--keep-previous`: now at 0.7.0, with a kept copy of 0.6.0.
    fn kept_scene(name: &str) -> Scene {
        let scene = Scene::new(name, "0.6.0");
        std::fs::write(&scene.exe, served_binary("update", "0.6.0")).expect("the published binary");
        scene.receipt(3);
        // The index, then the signed SHA256SUMS of 0.6.0 to identify the binary
        // that is replaced, then those of 0.7.0 and the binary.
        let (code, out, _) = scene.against(
            "site",
            "update",
            7,
            &["update", "--keep-previous", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        Scene {
            version: "0.7.0",
            ..scene
        }
    }

    fn kept_receipt(scene: &Scene) -> Json {
        receipt_json(scene)
            .get("kept_previous")
            .cloned()
            .expect("a kept_previous in the receipt")
    }

    #[test]
    fn keep_previous_keeps_the_replaced_binary_and_records_it() {
        let scene = Scene::new("keep", "0.6.0");
        std::fs::write(&scene.exe, served_binary("update", "0.6.0")).expect("the published binary");
        scene.receipt(3);
        let (code, out, _) = scene.against(
            "site",
            "update",
            7,
            &["update", "--keep-previous", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        let v = json(&out);
        assert_eq!(str_of(&v, "action"), "updated");
        let kept = v.get("kept_previous").expect("kept_previous in the output");
        assert_eq!(str_of(kept, "version"), "0.6.0");
        // The live binary is the new one; beside it, the old one, byte for byte.
        assert_eq!(
            std::fs::read(&scene.exe).expect("live"),
            served_binary("update", "0.7.0")
        );
        assert_eq!(scene.bin_files(), ["oskac-fixture", KEPT_NAME]);
        let path = scene.bin.join(KEPT_NAME);
        assert_eq!(
            std::fs::read(&path).expect("the kept copy"),
            served_binary("update", "0.6.0")
        );
        assert_eq!(str_of(kept, "path"), path.to_str().expect("text"));
        // The receipt records version, path and the digest from the signed
        // SHA256SUMS, and lists the copy so that an uninstall removes it.
        let digest = signed_digest("update", "0.6.0");
        let recorded = kept_receipt(&scene);
        assert_eq!(str_of(&recorded, "version"), "0.6.0");
        assert_eq!(str_of(&recorded, "path"), path.to_str().expect("text"));
        assert_eq!(str_of(&recorded, "sha256"), digest);
        assert_eq!(str_of(kept, "sha256"), digest);
        let text = scene.receipt_text();
        assert!(
            text.contains(&format!(
                "{{\"path\": {}, \"kind\": \"binary\", \"sha256\": \"{digest}\"}}",
                json_path(&path)
            )),
            "{text}"
        );
        assert_eq!(field(&receipt_json(&scene), "previous_version"), "0.6.0");
    }

    #[test]
    fn a_binary_that_is_not_the_published_one_is_not_kept() {
        let scene = Scene::new("keep-modified", "0.6.0");
        scene.receipt(3);
        let (code, out, _) = scene.against(
            "site",
            "update",
            7,
            &["update", "--keep-previous", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        let v = json(&out);
        assert_eq!(str_of(&v, "action"), "updated");
        assert_eq!(v.get("kept_previous"), Some(&Json::Null));
        assert_eq!(scene.bin_files(), ["oskac-fixture"]);
        let warnings = format!("{:?}", v.get("warnings"));
        assert!(warnings.contains("was not kept"), "{warnings}");
        assert!(receipt_json(&scene).get("kept_previous").is_none());
    }

    #[test]
    fn rollback_from_a_kept_copy_uses_it_after_the_checker_re_hashes_it() {
        let scene = kept_scene("rollback-kept");
        // Only the index is fetched: nothing is downloaded.
        let (code, out, _) = scene.against(
            "site",
            "update",
            2,
            &["update", "--rollback", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        let v = json(&out);
        assert_eq!(str_of(&v, "action"), "downgraded");
        assert_eq!(str_of(&v, "source"), "kept");
        assert_eq!(str_of(&v, "from"), "0.7.0");
        assert_eq!(str_of(&v, "to"), "0.6.0");
        assert_eq!(
            std::fs::read(&scene.exe).expect("live"),
            served_binary("update", "0.6.0")
        );
        // The copy is spent, and the receipt no longer records it.
        assert_eq!(scene.bin_files(), ["oskac-fixture"]);
        let receipt = receipt_json(&scene);
        assert_eq!(field(&receipt, "version"), "0.6.0");
        assert_eq!(field(&receipt, "previous_version"), "0.7.0");
        assert!(receipt.get("kept_previous").is_none());
        assert!(!scene.receipt_text().contains(KEPT_NAME));
    }

    #[test]
    fn rollback_with_no_kept_copy_downloads_the_previous_release() {
        let scene = Scene::new("rollback-download", "0.6.0");
        scene.receipt(3);
        let (code, out, _) = scene.against("site", "update", 5, &["update", "--format=json"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(scene.bin_files(), ["oskac-fixture"]);
        let scene = Scene {
            version: "0.7.0",
            ..scene
        };
        let (code, out, _) = scene.against(
            "site",
            "update",
            5,
            &["update", "--rollback", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        let v = json(&out);
        assert_eq!(str_of(&v, "action"), "downgraded");
        assert_eq!(str_of(&v, "source"), "downloaded");
        assert_eq!(str_of(&v, "to"), "0.6.0");
        assert_eq!(
            std::fs::read(&scene.exe).expect("live"),
            served_binary("update", "0.6.0")
        );
        assert_eq!(scene.bin_files(), ["oskac-fixture"]);
        let receipt = receipt_json(&scene);
        assert_eq!(field(&receipt, "version"), "0.6.0");
        assert_eq!(field(&receipt, "previous_version"), "0.7.0");
    }

    #[test]
    fn a_tampered_kept_copy_is_ignored_and_the_release_is_downloaded() {
        let scene = kept_scene("rollback-tampered");
        let path = scene.bin.join(KEPT_NAME);
        std::fs::write(&path, b"#!/bin/sh\necho \"oskac 0.6.0\"\necho tampered\n").expect("tamper");
        let (code, out, _) = scene.against(
            "site",
            "update",
            5,
            &["update", "--rollback", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        let v = json(&out);
        assert_eq!(str_of(&v, "action"), "downgraded");
        assert_eq!(str_of(&v, "source"), "downloaded");
        let warnings = format!("{:?}", v.get("warnings"));
        assert!(
            warnings.contains("does not match the SHA-256 recorded"),
            "{warnings}"
        );
        // What is live is the release's own binary, not the tampered copy, and
        // the tampered copy is discarded.
        assert_eq!(
            std::fs::read(&scene.exe).expect("live"),
            served_binary("update", "0.6.0")
        );
        assert_eq!(scene.bin_files(), ["oskac-fixture"]);
        assert!(receipt_json(&scene).get("kept_previous").is_none());
    }

    #[test]
    fn a_missing_kept_copy_falls_back_to_a_download_too() {
        let scene = kept_scene("rollback-missing");
        std::fs::remove_file(scene.bin.join(KEPT_NAME)).expect("remove the copy");
        let (code, out, _) = scene.against(
            "site",
            "update",
            5,
            &["update", "--rollback", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        assert_eq!(str_of(&json(&out), "source"), "downloaded");
    }

    #[test]
    fn the_next_update_discards_a_kept_copy_unless_it_keeps_again() {
        let scene = kept_scene("next-discards");
        let (code, out, _) = scene.against(
            "site",
            "update",
            5,
            &[
                "update",
                "--version",
                "0.5.0",
                "--allow-downgrade",
                "--format=json",
            ],
        );
        assert_eq!(code, 0, "{out}");
        assert_eq!(scene.bin_files(), ["oskac-fixture"]);
        assert!(receipt_json(&scene).get("kept_previous").is_none());
        assert!(!scene.receipt_text().contains(KEPT_NAME));

        // With --keep-previous it keeps the copy of what it replaces, and only
        // that: the older one goes.
        let scene = kept_scene("next-keeps-again");
        let (code, out, _) = scene.against(
            "site",
            "update",
            7,
            &[
                "update",
                "--version",
                "0.5.0",
                "--allow-downgrade",
                "--keep-previous",
                "--format=json",
            ],
        );
        assert_eq!(code, 0, "{out}");
        assert_eq!(scene.bin_files(), ["oskac-fixture", "oskac-fixture-0.7.0"]);
        assert_eq!(str_of(&kept_receipt(&scene), "version"), "0.7.0");
        assert_eq!(
            str_of(&kept_receipt(&scene), "sha256"),
            signed_digest("update", "0.7.0")
        );
    }

    #[test]
    fn rollback_without_a_previous_version_is_refused_before_any_network_use() {
        let scene = Scene::new("rollback-none", "0.6.0");
        scene.receipt(3);
        let before = scene.snapshot();
        // Nothing listens on port 1: a connection would be an `internal` or
        // `network_error`, not this code.
        let (code, out, _) = scene.oskac(1, &["update", "--rollback", "--format=json"]);
        assert_eq!(code, 1, "{out}");
        assert_eq!(error_of(&out).0, "no_previous_version");
        scene.assert_untouched(&before);
    }

    #[test]
    fn rollback_and_keep_previous_are_flags_of_self_update_alone() {
        let scene = Scene::new("rollback-usage", "0.6.0");
        scene.receipt(1);
        let before = scene.snapshot();
        for args in [
            &["check", "--rollback", "--format=json"][..],
            &["check", "--keep-previous", "--format=json"],
            &["info", "--keep-previous", "--format=json"],
            &[
                "update",
                "--rollback",
                "--version",
                "0.5.0",
                "--format=json",
            ],
            &[
                "update",
                "--rollback",
                "--version",
                "0.5.0",
                "--allow-downgrade",
                "--format=json",
            ],
        ] {
            let (code, out, _) = scene.oskac(1, args);
            assert_eq!(code, 2, "{args:?}: {out}");
            assert_eq!(error_of(&out).0, "usage", "{args:?}: {out}");
        }
        scene.assert_untouched(&before);
    }

    /// A scene at 0.7.0 whose receipt says the previous version is 0.6.0
    /// and that a copy of it is kept at `path`, with the digest the signed
    /// `SHA256SUMS` lists for 0.6.0.
    fn scene_with_kept_at(name: &str, path: impl Fn(&Scene) -> std::path::PathBuf) -> Scene {
        let scene = Scene::new(name, "0.7.0");
        scene.receipt(3);
        let text = scene.receipt_text().replace(
            "\"index_serial\": 3,",
            &format!(
                "\"index_serial\": 3,\n  \"previous_version\": \"0.6.0\",\n  \"kept_previous\": {{\"version\": \"0.6.0\", \"path\": {}, \"sha256\": \"{}\"}},",
                json_path(&path(&scene)),
                signed_digest("update", "0.6.0")
            ),
        );
        std::fs::write(scene.receipt_path(), text).expect("the receipt");
        scene
    }

    /// A file outside the install holding exactly what the signed
    /// `SHA256SUMS` lists for 0.6.0: a kept copy a tampered receipt could
    /// name.
    fn outside_file(scene: &Scene) -> std::path::PathBuf {
        let path = scene.home.path().join("outside");
        std::fs::write(&path, served_binary("update", "0.6.0")).expect("outside");
        path
    }

    #[test]
    fn a_receipt_naming_a_file_outside_the_install_as_the_kept_copy_is_not_used() {
        let scene = scene_with_kept_at("attack-outside", |s| s.home.path().join("outside"));
        let outside = outside_file(&scene);
        let (code, out, _) = scene.against(
            "site",
            "update",
            5,
            &["update", "--rollback", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        assert_eq!(str_of(&json(&out), "source"), "downloaded");
        assert_eq!(
            std::fs::read(&outside).expect("outside"),
            served_binary("update", "0.6.0")
        );
    }

    #[test]
    fn a_kept_copy_that_is_a_link_is_neither_used_nor_followed_to_delete() {
        let scene = scene_with_kept_at("attack-link", |s| s.bin.join(KEPT_NAME));
        let outside = outside_file(&scene);
        let link = scene.bin.join(KEPT_NAME);
        std::os::unix::fs::symlink(&outside, &link).expect("a link");
        let (code, out, _) = scene.against(
            "site",
            "update",
            5,
            &["update", "--rollback", "--format=json"],
        );
        assert_eq!(code, 0, "{out}");
        assert_eq!(str_of(&json(&out), "source"), "downloaded");
        assert_eq!(
            std::fs::read(&outside).expect("what the link pointed at is still there"),
            served_binary("update", "0.6.0")
        );
    }

    #[test]
    fn an_update_deletes_nothing_a_receipt_names_but_the_kept_copy() {
        // A file beside the binary, and one outside, both named as the kept
        // copy by a receipt that is not what `--keep-previous` writes.
        for (name, inside) in [("attack-discard-in", true), ("attack-discard-out", false)] {
            let scene = scene_with_kept_at(name, |s| {
                if inside {
                    s.bin.join("other-tool")
                } else {
                    s.home.path().join("outside")
                }
            });
            let target = if inside {
                scene.bin.join("other-tool")
            } else {
                scene.home.path().join("outside")
            };
            std::fs::write(&target, b"mine").expect("a file");
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
            assert_eq!(std::fs::read(&target).expect("still there"), b"mine");
            assert!(receipt_json(&scene).get("kept_previous").is_none());
        }
    }
}

// ---- oskac self uninstall ---------------------------------------------------

use common::portable::{json_path, output_of, set_home, Scratch};
use common::Json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The SHA-256 of the three bytes `abc`.
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// A copy of `oskac` installed in `home`, as the installer would put it,
/// and `extra` fields and `entries` (extra `files` entries) in its receipt.
/// Returns the executable, canonical, as `oskac` knows itself.
fn install(home: &Path, extra: &str, entries: &str) -> PathBuf {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    let exe = bin.join(format!("oskac{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_oskac"), &exe).expect("copy oskac");
    let exe = exe.canonicalize().expect("canonical");
    std::fs::write(home.join("version"), format!("{VERSION}\n")).expect("version file");
    let receipt = format!(
        "{{\"schema\":1,\"version\":\"{VERSION}\",\"installer\":\"sh\"{extra},\"files\":[\
         {{\"path\":{},\"kind\":\"binary\",\"sha256\":\"aa\"}},\
         {{\"path\":{},\"kind\":\"record\",\"sha256\":\"bb\"}}{entries},\
         {{\"path\":{},\"kind\":\"record\"}}]}}",
        json_path(&exe),
        json_path(&home.join("version")),
        json_path(&home.join("receipt.json")),
    );
    std::fs::write(home.join("receipt.json"), receipt).expect("receipt");
    exe
}

fn uninstall(exe: &Path, home: &Path) -> (i32, Json) {
    let mut command = Command::new(exe);
    command
        .args(["self", "uninstall", "--format=json"])
        .env("OSKA_HOME", home);
    set_home(&mut command, home.parent().expect("parent"));
    let output: Output = output_of(&mut command);
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    (
        output.status.code().expect("a code"),
        Json::parse(text.trim()),
    )
}

/// The receipt fields and entry that `--keep-previous` writes for a copy at
/// `kept`.
fn kept_receipt(kept: &Path) -> (String, String) {
    (
        format!(
            ",\"previous_version\":\"0.0.1\",\"kept_previous\":{{\"version\":\"0.0.1\",\"path\":{},\"sha256\":\"{ABC}\"}}",
            json_path(kept)
        ),
        format!(
            ",{{\"path\":{},\"kind\":\"binary\",\"sha256\":\"{ABC}\"}}",
            json_path(kept)
        ),
    )
}

fn listed(done: &Json, key: &str, name: &str) -> bool {
    format!("{:?}", done.get(key)).contains(name)
}

/// What `--keep-previous` leaves: removed while it still has the SHA-256
/// recorded from the signed `SHA256SUMS`, kept and reported once it changed.
#[test]
fn uninstall_removes_a_kept_copy_and_keeps_a_changed_one() {
    for (name, content, removed) in [("kept", "abc", true), ("kept-changed", "abd", false)] {
        let scratch = Scratch::new("oskac-rollback", name);
        let home = scratch.path().join(".oska");
        let canonical_home = {
            std::fs::create_dir_all(&home).expect("home");
            home.canonicalize().expect("canonical")
        };
        let suffix = std::env::consts::EXE_SUFFIX;
        let kept = canonical_home
            .join("bin")
            .join(format!("oskac-0.0.1{suffix}"));
        let (extra, entry) = kept_receipt(&kept);
        let exe = install(&canonical_home, &extra, &entry);
        std::fs::write(&kept, content).expect("the kept copy");
        let (code, done) = uninstall(&exe, &canonical_home);
        assert_eq!(code, 0, "{done:?}");
        assert_eq!(listed(&done, "removed", "oskac-0.0.1"), removed, "{done:?}");
        assert_eq!(listed(&done, "kept", "oskac-0.0.1"), !removed, "{done:?}");
        assert_eq!(kept.exists(), !removed);
    }
}

/// A receipt is advisory: naming another file of `bin` as the kept copy,
/// with the SHA-256 of what it holds, does not make uninstall remove it.
#[test]
fn uninstall_does_not_remove_another_file_a_receipt_calls_the_kept_copy() {
    let scratch = Scratch::new("oskac-rollback", "attack-other");
    let home = scratch.path().join(".oska");
    std::fs::create_dir_all(&home).expect("home");
    let home = home.canonicalize().expect("canonical");
    let other = home.join("bin").join("other-tool");
    let (extra, entry) = kept_receipt(&other);
    let exe = install(&home, &extra, &entry);
    std::fs::write(&other, "abc").expect("another file");
    let (code, done) = uninstall(&exe, &home);
    assert_eq!(code, 0, "{done:?}");
    assert!(listed(&done, "kept", "other-tool"), "{done:?}");
    assert_eq!(std::fs::read(&other).expect("still there"), b"abc");
}

/// A receipt naming a file outside the Oska home is refused whole, and
/// nothing is read or removed.
#[test]
fn uninstall_refuses_a_kept_copy_outside_the_install_and_touches_nothing() {
    let scratch = Scratch::new("oskac-rollback", "attack-outside");
    let home = scratch.path().join(".oska");
    std::fs::create_dir_all(&home).expect("home");
    let home = home.canonicalize().expect("canonical");
    let outside = scratch
        .path()
        .canonicalize()
        .expect("canonical")
        .join("outside");
    std::fs::write(&outside, "abc").expect("outside");
    let (extra, entry) = kept_receipt(&outside);
    let exe = install(&home, &extra, &entry);
    let (code, done) = uninstall(&exe, &home);
    assert_ne!(code, 0, "{done:?}");
    assert_eq!(std::fs::read(&outside).expect("still there"), b"abc");
    assert!(home.join("receipt.json").exists());
}

/// The kept copy at its own name but a link: it is not followed, and what
/// it points at is neither read nor removed.
#[cfg(unix)]
#[test]
fn uninstall_does_not_follow_a_link_at_the_kept_copys_name() {
    let scratch = Scratch::new("oskac-rollback", "attack-link");
    let home = scratch.path().join(".oska");
    std::fs::create_dir_all(&home).expect("home");
    let home = home.canonicalize().expect("canonical");
    let kept = home.join("bin").join("oskac-0.0.1");
    let outside = scratch
        .path()
        .canonicalize()
        .expect("canonical")
        .join("outside");
    std::fs::write(&outside, "abc").expect("outside");
    let (extra, entry) = kept_receipt(&kept);
    let exe = install(&home, &extra, &entry);
    std::os::unix::fs::symlink(&outside, &kept).expect("a link");
    let (_, done) = uninstall(&exe, &home);
    assert_eq!(
        std::fs::read(&outside).expect("still there"),
        b"abc",
        "{done:?}"
    );
    assert!(!listed(&done, "removed", "outside"), "{done:?}");
}
