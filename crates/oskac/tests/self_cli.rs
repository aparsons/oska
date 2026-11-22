// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `oskac self info` and `oskac self uninstall` (spec
//! #191, ticket #203), in text and JSON, including refusals. Each test
//! makes an Oska home in a temporary directory and runs a copy of `oskac`
//! installed in it, as the installer would have put it there.

mod common;

use common::portable::{json_escape, json_path, output_of, set_home, shown, Scratch};
use common::Json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A scratch directory for the test named `name`.
fn new_scratch(name: &str) -> Scratch {
    Scratch::new("oskac-self", name)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("UTF-8")
}

/// Install a copy of `oskac` into `home`, with a receipt listing it.
/// `extra` is added to the receipt's top level and `version` is what the
/// receipt records. Returns the installed executable.
fn install(home: &Path, version: &str, extra: &str) -> PathBuf {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    let exe = bin.join(if cfg!(windows) { "oskac.exe" } else { "oskac" });
    std::fs::copy(env!("CARGO_BIN_EXE_oskac"), &exe).expect("copy oskac");
    std::fs::write(home.join("version"), format!("{version}\n")).expect("version file");
    let receipt = format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"installer\":\"sh\",\"installed_at\":\"2026-11-12T09:30:00Z\"{extra},\"files\":[\
         {{\"path\":{},\"kind\":\"binary\",\"sha256\":\"aa\"}},\
         {{\"path\":{},\"kind\":\"record\",\"sha256\":\"bb\"}},\
         {{\"path\":{},\"kind\":\"record\"}}]}}",
        json_path(&exe),
        json_path(&home.join("version")),
        json_path(&home.join("receipt.json")),
    );
    std::fs::write(home.join("receipt.json"), receipt).expect("receipt");
    exe
}

fn run(exe: &Path, home: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(exe);
    command.args(args).env("OSKA_HOME", home);
    set_home(&mut command, home.parent().expect("parent"));
    output_of(&mut command)
}

fn oskac(home: &Path, args: &[&str]) -> Output {
    run(Path::new(env!("CARGO_BIN_EXE_oskac")), home, args)
}

fn json_of(output: &Output) -> Json {
    Json::parse(text(&output.stdout).trim())
}

#[test]
fn info_without_a_receipt_reports_null_and_no_match() {
    let scratch = new_scratch("info-none");
    let home = scratch.path().join(".oska");
    let output = oskac(&home, &["self", "info", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stderr), "");
    let info = json_of(&output);
    assert_eq!(info.get("version"), Some(&Json::Str(VERSION.to_string())));
    assert_eq!(info.get("receipt"), Some(&Json::Null));
    assert_eq!(info.get("receipt_matches"), Some(&Json::Bool(false)));
    assert_eq!(info.get("oska_home"), Some(&Json::Str(shown(&home))));
    let platform = match info.get("platform") {
        Some(Json::Str(p)) => p.clone(),
        other => panic!("platform should be a string, got {other:?}"),
    };
    assert!(platform.contains('-'), "platform {platform}");
    assert!(matches!(info.get("executable"), Some(Json::Str(_))));
}

#[test]
fn info_of_a_managed_install_reports_the_receipt_and_a_match() {
    let scratch = new_scratch("info-managed");
    let home = scratch.path().join(".oska");
    let exe = install(
        &home,
        VERSION,
        ",\"added_by_a_newer_installer\":{\"x\":[1,2]}",
    );
    let output = run(&exe, &home, &["self", "info", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
    let info = json_of(&output);
    assert_eq!(info.get("receipt_matches"), Some(&Json::Bool(true)));
    let receipt = info.get("receipt").expect("receipt");
    assert_eq!(
        receipt.get("version"),
        Some(&Json::Str(VERSION.to_string()))
    );
    // A field this oskac does not know is passed through.
    assert!(receipt.get("added_by_a_newer_installer").is_some());
    let exe = exe.canonicalize().expect("canonical exe");
    assert_eq!(info.get("executable"), Some(&Json::Str(shown(&exe))));

    let human = run(&exe, &home, &["self", "info"]);
    let human = text(&human.stdout);
    assert!(human.starts_with(&format!("oskac {VERSION} (")), "{human}");
    assert!(human.contains("managed install: yes"), "{human}");
    assert!(human.contains("by the sh installer"), "{human}");
}

#[test]
fn info_does_not_match_another_version_or_another_copy() {
    let scratch = new_scratch("info-mismatch");
    let home = scratch.path().join(".oska");
    let exe = install(&home, "0.0.1", "");
    // The receipt's version is not this oskac's.
    let output = run(&exe, &home, &["self", "info", "--format=json"]);
    assert_eq!(
        json_of(&output).get("receipt_matches"),
        Some(&Json::Bool(false))
    );
    // Right version, but run from a path the receipt does not list.
    install(&home, VERSION, "");
    let output = oskac(&home, &["self", "info", "--format=json"]);
    let info = json_of(&output);
    assert_eq!(info.get("receipt_matches"), Some(&Json::Bool(false)));
    assert!(matches!(info.get("receipt"), Some(Json::Obj(_))));
    let human = text(&oskac(&home, &["self", "info"]).stdout);
    assert!(human.contains("managed install: no"), "{human}");
}

#[test]
fn info_reports_an_unreadable_receipt() {
    let scratch = new_scratch("info-bad");
    let home = scratch.path().join(".oska");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("receipt.json"), "{not json").expect("receipt");
    let info = json_of(&oskac(&home, &["self", "info", "--format=json"]));
    assert_eq!(info.get("receipt"), Some(&Json::Null));
    assert!(matches!(info.get("receipt_error"), Some(Json::Str(_))));
    assert_eq!(info.get("receipt_matches"), Some(&Json::Bool(false)));
}

#[test]
fn uninstall_removes_what_the_receipt_lists_and_nothing_else() {
    let scratch = new_scratch("uninstall");
    let home = scratch.path().join(".oska");
    let exe = install(&home, VERSION, "");
    let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
    assert_eq!(text(&output.stderr), "");
    assert_eq!(output.status.code(), Some(0));
    let done = json_of(&output);
    assert_eq!(done.get("ok"), Some(&Json::Bool(true)));
    assert_eq!(done.get("action"), Some(&Json::Str("uninstalled".into())));
    let Some(Json::Arr(removed)) = done.get("removed") else {
        panic!("removed should be an array")
    };
    let Some(Json::Arr(pending)) = done.get("pending") else {
        panic!("pending should be an array")
    };
    if cfg!(windows) {
        // Windows cannot delete the running oskac.exe: it is moved aside
        // and reported as pending, with where it went.
        assert_eq!(removed.len(), 4, "{removed:?}");
        assert_eq!(pending.len(), 1, "{pending:?}");
        let exe = exe.canonicalize().unwrap_or(exe);
        assert_eq!(pending[0].get("path"), Some(&Json::Str(shown(&exe))));
        let Some(Json::Str(moved_to)) = pending[0].get("moved_to") else {
            panic!("moved_to should be a string: {pending:?}")
        };
        assert!(Path::new(moved_to).exists(), "{moved_to}");
        let _ = std::fs::remove_file(moved_to);
    } else {
        assert_eq!(removed.len(), 5, "{removed:?}");
        assert!(pending.is_empty(), "{pending:?}");
    }
    assert_eq!(done.get("kept"), Some(&Json::Arr(vec![])));
    assert_eq!(done.get("path_hint"), Some(&Json::Null));
    assert!(!home.exists(), "the empty Oska home should be removed");
}

#[test]
fn uninstall_keeps_a_directory_with_something_else_in_it() {
    let scratch = new_scratch("uninstall-keep");
    let home = scratch.path().join(".oska");
    let exe = install(&home, VERSION, "");
    std::fs::write(home.join("notes.txt"), "mine").expect("notes");
    std::fs::create_dir_all(scratch.path().join(".config/oska")).expect("config");
    let output = run(&exe, &home, &["self", "uninstall"]);
    assert_eq!(output.status.code(), Some(0));
    let human = text(&output.stdout);
    assert!(human.starts_with("Uninstalled oskac from "), "{human}");
    assert!(human.contains("Kept:"), "{human}");
    assert!(human.contains("(not empty)"), "{human}");
    assert!(human.contains("settings and credentials"), "{human}");
    assert!(home.join("notes.txt").exists());
    assert!(!home.join("receipt.json").exists());
    assert!(scratch.path().join(".config/oska").exists());
}

#[test]
fn uninstall_without_a_receipt_is_not_installed() {
    let scratch = new_scratch("uninstall-none");
    let home = scratch.path().join(".oska");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("keep.txt"), "x").expect("file");
    let output = oskac(&home, &["self", "uninstall", "--format=json"]);
    assert_eq!(output.status.code(), Some(1));
    let refusal = json_of(&output);
    assert_eq!(refusal.get("ok"), Some(&Json::Bool(false)));
    assert_eq!(
        refusal.get("error"),
        Some(&Json::Str("not_installed".into()))
    );
    assert!(home.join("keep.txt").exists());

    let output = oskac(&home, &["self", "uninstall"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(text(&output.stdout), "");
    assert!(text(&output.stderr).starts_with("oskac self uninstall: there is no install receipt"));
}

/// Rewrite the receipt of an installed home with one more entry.
fn add_entry(home: &Path, entry: &str) {
    let path = home.join("receipt.json");
    let receipt = std::fs::read_to_string(&path).expect("receipt");
    let at = receipt.rfind("]}").expect("end of files");
    std::fs::write(
        &path,
        format!("{},{}{}", &receipt[..at], entry, &receipt[at..]),
    )
    .expect("receipt");
}

#[test]
fn uninstall_refuses_unsafe_entries_and_removes_nothing() {
    let scratch = new_scratch("uninstall-unsafe");
    let outside = scratch.path().join("outside.txt");
    std::fs::write(&outside, "precious").expect("outside");
    let cases = [
        (
            format!(
                "{{\"path\":{},\"kind\":\"record\",\"sha256\":\"cc\"}}",
                json_path(&outside)
            ),
            "outside the Oska home",
        ),
        (
            "{\"path\":\"relative.txt\",\"kind\":\"record\",\"sha256\":\"cc\"}".to_string(),
            "not an absolute path",
        ),
        (
            "{\"path\":\"HOME/bin/../../outside.txt\",\"kind\":\"record\",\"sha256\":\"cc\"}"
                .to_string(),
            "climbs out",
        ),
        (
            "{\"path\":\"HOME/deep/er.txt\",\"kind\":\"record\",\"sha256\":\"cc\"}".to_string(),
            "not directly in",
        ),
        (
            "{\"path\":\"HOME/x\",\"kind\":\"directory\",\"sha256\":\"cc\"}".to_string(),
            "does not remove",
        ),
        (
            "{\"path\":\"HOME/.config/oska/credentials\",\"kind\":\"record\",\"sha256\":\"cc\"}"
                .to_string(),
            ".config/oska",
        ),
    ];
    for (n, (entry, why)) in cases.iter().enumerate() {
        let home = scratch.path().join(format!("h{n}")).join(".oska");
        let exe = install(&home, VERSION, "");
        // JSON-escaped, with the platform's separators.
        let sep = json_escape(std::path::MAIN_SEPARATOR_STR);
        let home_text = json_escape(&home.to_string_lossy());
        add_entry(&home, &entry.replace('/', &sep).replace("HOME", &home_text));
        let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{why}: {}",
            text(&output.stdout)
        );
        let refusal = json_of(&output);
        assert_eq!(
            refusal.get("error"),
            Some(&Json::Str("unsafe_path".into())),
            "{why}"
        );
        let Some(Json::Str(message)) = refusal.get("message") else {
            panic!("message")
        };
        assert!(message.contains(why), "{why}: {message}");
        assert!(exe.exists(), "{why}: nothing may be removed");
        assert!(outside.exists());
    }
}

#[test]
fn uninstall_refuses_an_unreadable_receipt() {
    let scratch = new_scratch("uninstall-garbled");
    let home = scratch.path().join(".oska");
    let exe = install(&home, VERSION, "");
    std::fs::write(home.join("receipt.json"), "{\"schema\":1,\"files\":").expect("receipt");
    let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        json_of(&output).get("error"),
        Some(&Json::Str("unsafe_path".into()))
    );
    assert!(exe.exists());
}

#[cfg(unix)]
#[test]
fn uninstall_refuses_a_symlinked_entry_or_home() {
    use std::os::unix::fs::symlink;
    let scratch = new_scratch("uninstall-symlink");
    let target = scratch.path().join("target.txt");
    std::fs::write(&target, "precious").expect("target");

    // A listed file that is a symlink.
    let home = scratch.path().join("a/.oska");
    let exe = install(&home, VERSION, "");
    symlink(&target, home.join("link")).expect("symlink");
    add_entry(
        &home,
        &format!(
            "{{\"path\":\"{}\",\"kind\":\"record\",\"sha256\":\"cc\"}}",
            home.join("link").display()
        ),
    );
    let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
    assert_eq!(
        json_of(&output).get("error"),
        Some(&Json::Str("unsafe_path".into()))
    );
    assert!(exe.exists() && target.exists());

    // An Oska home reached through a symlink above it.
    let real = scratch.path().join("b-real");
    let exe = install(&real.join(".oska"), VERSION, "");
    symlink(&real, scratch.path().join("b-link")).expect("symlink");
    let home = scratch.path().join("b-link/.oska");
    let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
    assert_eq!(
        json_of(&output).get("error"),
        Some(&Json::Str("unsafe_path".into()))
    );
    assert!(exe.exists());
}

#[test]
fn uninstall_reports_the_path_hint_when_bin_is_on_the_path() {
    let scratch = new_scratch("uninstall-path");
    let home = scratch.path().join(".oska");
    let exe = install(&home, VERSION, "");
    let mut command = Command::new(&exe);
    command
        .args(["self", "uninstall", "--format=json"])
        .env("OSKA_HOME", &home)
        .env("PATH", home.join("bin"));
    set_home(&mut command, scratch.path());
    let output = output_of(&mut command);
    let done = json_of(&output);
    let Some(Json::Str(hint)) = done.get("path_hint") else {
        panic!("path_hint should be a string: {done:?}")
    };
    assert!(hint.contains("is on your PATH"), "{hint}");
}

#[test]
fn unknown_self_commands_and_flags_are_json_usage_errors_with_format_json() {
    let scratch = new_scratch("usage-json");
    for args in [
        &["self", "--format=json"][..],
        &["self", "update", "--format=json"],
        &["self", "info", "--bogus", "--format=json"],
        &[
            "self",
            "uninstall",
            "--target",
            "linux-x86_64",
            "--format=json",
        ],
    ] {
        let output = oskac(scratch.path(), args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let error = json_of(&output);
        assert_eq!(error.get("ok"), Some(&Json::Bool(false)), "{args:?}");
        assert_eq!(
            error.get("error"),
            Some(&Json::Str("usage".into())),
            "{args:?}"
        );
        let Some(Json::Str(message)) = error.get("message") else {
            panic!("message should be a string: {error:?}")
        };
        assert!(message.contains("oskac self info"), "{args:?}: {message}");
    }
}

#[test]
fn unknown_self_commands_are_usage_errors() {
    let scratch = new_scratch("usage");
    for args in [&["self"][..], &["self", "update"], &["self", "info", "x"]] {
        let output = oskac(scratch.path(), args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(text(&output.stderr).contains("oskac self info"), "{args:?}");
    }
}

// ---- The kinds install.sh writes outside the Oska home ---------------------------------
//
// The outcome for every kind, on every platform, is checked by the shared
// receipt fixtures (`receipt_fixtures.rs`). These tests add what oskac
// says about them. On Windows oskac refuses these kinds, as install.ps1
// never writes them.

#[cfg(unix)]
mod install_sh_kinds {
    use super::*;
    use crate::common::portable::{json_str, literal_path};

    /// The hook script and settings `--hook` installs: the published files.
    const HOOK_SCRIPT: &str = include_str!("../../../site/agent-setup/oska-check.sh");
    const HOOK_SETTINGS: &str =
        include_str!("../../../site/agent-setup/claude-settings-post-edit.json");
    /// The marker `install.sh --add-to-path` ends its profile line with.
    const PROFILE_MARK: &str = "# added by the Oska installer";

    /// A project with the hook files `--hook` writes, with the given contents.
    /// Returns the project, the script and the settings.
    fn hooked_project(root: &Path, script: &str, settings: &str) -> (PathBuf, PathBuf, PathBuf) {
        let project = root.join("project");
        let hooks = project.join(".claude").join("hooks");
        std::fs::create_dir_all(&hooks).expect("hooks dir");
        let script_path = hooks.join("oska-check.sh");
        let settings_path = project.join(".claude").join("settings.json");
        std::fs::write(&script_path, script).expect("hook script");
        std::fs::write(&settings_path, settings).expect("hook settings");
        (project, script_path, settings_path)
    }

    /// A receipt entry for a hook file, as `install.sh --hook` records it.
    fn hook_entry(kind: &str, path: &Path, sha256: Option<&str>, project: &Path) -> String {
        let sum = sha256.map_or_else(String::new, |s| format!(",\"sha256\":{}", json_str(s)));
        format!(
            "{{\"path\":{},\"kind\":{}{sum},\"project\":{}}}",
            json_path(path),
            json_str(kind),
            json_path(project)
        )
    }

    fn strings(value: Option<&Json>) -> Vec<String> {
        match value {
            Some(Json::Arr(items)) => items
                .iter()
                .map(|item| match item {
                    Json::Str(s) => s.clone(),
                    other => panic!("expected a string, got {other:?}"),
                })
                .collect(),
            other => panic!("expected an array, got {other:?}"),
        }
    }

    #[test]
    fn uninstall_removes_the_hook_and_leaves_path_edits_to_the_installer() {
        let scratch = new_scratch("uninstall-kinds");
        let home = scratch.path().join(".oska");
        let exe = install(&home, VERSION, "");
        let bin = home.join("bin");
        let (project, script, settings) =
            hooked_project(scratch.path(), HOOK_SCRIPT, HOOK_SETTINGS);
        let profile = scratch.path().join(".bashrc");
        let line = format!("export PATH=\"{}:$PATH\" {PROFILE_MARK}", bin.display());
        let profile_text = format!("# mine\n{line}\n");
        std::fs::write(&profile, &profile_text).expect("profile");
        add_entry(
            &home,
            &format!(
                "{{\"path\":{},\"kind\":\"profile_line\",\"line\":{}}}",
                json_path(&profile),
                json_str(&line)
            ),
        );
        add_entry(
            &home,
            &hook_entry("hook_script", &script, Some("dd"), &project),
        );
        add_entry(
            &home,
            &hook_entry("hook_settings", &settings, Some("ee"), &project),
        );

        let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
        assert_eq!(text(&output.stderr), "");
        assert_eq!(output.status.code(), Some(0));
        let done = json_of(&output);
        assert_eq!(done.get("ok"), Some(&Json::Bool(true)));
        let removed = strings(done.get("removed"));
        assert!(removed.contains(&shown(&script)), "{removed:?}");
        assert!(removed.contains(&shown(&settings)), "{removed:?}");
        assert!(!script.exists() && !settings.exists());
        // The PATH edits are the installer's to undo: reported, not made.
        assert_eq!(
            std::fs::read_to_string(&profile).expect("profile"),
            profile_text
        );
        let Some(Json::Str(hint)) = done.get("path_hint") else {
            panic!("path_hint should be a string: {done:?}")
        };
        assert!(hint.contains(&shown(&profile)), "{hint}");
        assert!(hint.contains(&line), "{hint}");
        assert!(hint.contains("--uninstall"), "{hint}");
    }

    #[test]
    fn uninstall_keeps_a_changed_hook_and_settings_it_did_not_create() {
        let scratch = new_scratch("uninstall-hook-keep");
        let home = scratch.path().join(".oska");
        let exe = install(&home, VERSION, "");
        let (project, script, settings) = hooked_project(
            scratch.path(),
            "#!/bin/sh\necho mine\n",
            "{\"mine\":true}\n",
        );
        add_entry(
            &home,
            &hook_entry("hook_script", &script, Some("dd"), &project),
        );
        add_entry(
            &home,
            &hook_entry("hook_settings", &settings, None, &project),
        );
        let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
        assert_eq!(output.status.code(), Some(0));
        let kept = strings(json_of(&output).get("kept"));
        assert!(kept.contains(&shown(&script)), "{kept:?}");
        assert!(kept.contains(&shown(&settings)), "{kept:?}");
        assert!(script.exists() && settings.exists());

        let home = scratch.path().join("b").join(".oska");
        let exe = install(&home, VERSION, "");
        add_entry(
            &home,
            &hook_entry("hook_script", &script, Some("dd"), &project),
        );
        add_entry(
            &home,
            &hook_entry("hook_settings", &settings, None, &project),
        );
        let human = text(&run(&exe, &home, &["self", "uninstall"]).stdout);
        assert!(human.contains("changed since it was installed"), "{human}");
        assert!(
            human.contains("remove its oskac hook entries by hand"),
            "{human}"
        );
    }

    #[test]
    fn uninstall_refuses_a_tampered_receipt_and_removes_nothing() {
        let scratch = new_scratch("uninstall-tampered");
        let (project, script, settings) =
            hooked_project(scratch.path(), HOOK_SCRIPT, HOOK_SETTINGS);
        let notes = scratch.path().join("notes.txt");
        std::fs::write(&notes, "notes\n").expect("notes");
        let profile = scratch.path().join(".profile");
        std::fs::write(&profile, "# mine\n").expect("profile");
        let marked = format!("export PATH=\"/x:$PATH\" {PROFILE_MARK}");
        let cases = [
            (
                hook_entry(
                    "hook_script",
                    &project.join(".claude").join("hooks").join("other.sh"),
                    Some("dd"),
                    &project,
                ),
                "not where --hook writes it",
            ),
            (
                hook_entry("hook_script", &notes, Some("dd"), &project),
                "not where --hook writes it",
            ),
            (
                hook_entry("hook_settings", &settings, Some("dd"), Path::new("project")),
                "not an absolute path",
            ),
            // `literal_path` keeps the `..`: `join("..")` drops it on Windows.
            (
                hook_entry(
                    "hook_settings",
                    &literal_path(&project, &["x", "..", ".claude", "settings.json"]),
                    Some("dd"),
                    &literal_path(&project, &["x", ".."]),
                ),
                "climbs out",
            ),
            (
                hook_entry(
                    "hook_settings",
                    &settings,
                    Some("dd"),
                    &literal_path(&project, &["x", ".."]),
                ),
                "climbs out",
            ),
            (
                format!(
                    "{{\"path\":{},\"kind\":\"profile_line\",\"line\":{}}}",
                    json_path(&notes),
                    json_str(&marked)
                ),
                "not a shell profile",
            ),
            (
                format!(
                    "{{\"path\":{},\"kind\":\"profile_line\",\"line\":\"rm -rf ~\"}}",
                    json_path(&profile)
                ),
                "not the marked line",
            ),
            (
                format!(
                    "{{\"path\":{},\"kind\":\"user_path_entry\"}}",
                    json_path(scratch.path())
                ),
                // install.ps1 writes it, never install.sh.
                "never writes",
            ),
        ];
        for (n, (entry, why)) in cases.iter().enumerate() {
            // In the scratch directory, which is the user's home, beside the
            // profile.
            let home = scratch.path().join(format!(".oska{n}"));
            let exe = install(&home, VERSION, "");
            // A good entry first, so that a removal before the refusal shows.
            add_entry(
                &home,
                &hook_entry("hook_script", &script, Some("dd"), &project),
            );
            add_entry(&home, entry);
            let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{why}: the receipt entry {entry} gave {}",
                text(&output.stdout)
            );
            let refusal = json_of(&output);
            assert_eq!(
                refusal.get("error"),
                Some(&Json::Str("unsafe_path".into())),
                "{why}"
            );
            let Some(Json::Str(message)) = refusal.get("message") else {
                panic!("message")
            };
            assert!(message.contains(why), "{why}: {message}");
            assert!(
                exe.exists() && script.exists() && settings.exists(),
                "{why}"
            );
            assert_eq!(
                std::fs::read_to_string(&profile).expect("profile"),
                "# mine\n"
            );
        }
    }

    #[test]
    fn uninstall_by_another_oskac_keeps_the_binary_and_record_but_removes_the_hook() {
        let scratch = new_scratch("uninstall-unmanaged");
        for (n, version) in [VERSION, "0.0.1"].into_iter().enumerate() {
            let root = scratch.path().join(format!("r{n}"));
            let home = root.join(".oska");
            let exe = install(&home, version, "");
            let (project, script, settings) = hooked_project(&root, HOOK_SCRIPT, HOOK_SETTINGS);
            add_entry(
                &home,
                &hook_entry("hook_script", &script, Some("dd"), &project),
            );
            add_entry(
                &home,
                &hook_entry("hook_settings", &settings, Some("ee"), &project),
            );
            // The right version run from a copy the receipt does not list, and
            // the listed copy of another version.
            let output = if version == VERSION {
                oskac(&home, &["self", "uninstall", "--format=json"])
            } else {
                run(&exe, &home, &["self", "uninstall", "--format=json"])
            };
            assert_eq!(output.status.code(), Some(0), "{version}");
            let done = json_of(&output);
            assert_eq!(done.get("ok"), Some(&Json::Bool(true)), "{version}");
            let kept = strings(done.get("kept"));
            assert!(kept.contains(&shown(&exe)), "{version}: {kept:?}");
            assert!(
                kept.contains(&shown(&home.join("receipt.json"))),
                "{version}: {kept:?}"
            );
            assert!(exe.exists() && home.join("receipt.json").exists());
            assert!(!script.exists() && !settings.exists(), "{version}");
        }
    }
}

#[test]
fn info_says_it_checks_no_digest_and_reads_any_spelling_of_schema_1() {
    let scratch = new_scratch("info-schema");
    let home = scratch.path().join(".oska");
    let exe = install(&home, VERSION, "");
    let path = home.join("receipt.json");
    let receipt = std::fs::read_to_string(&path).expect("receipt");
    std::fs::write(&path, receipt.replace("\"schema\":1,", "\"schema\":1.0,")).expect("receipt");
    // The receipt is printed back with its 1.0, which this test's JSON
    // reader does not take, so the fields are looked for in the text.
    let info = text(&run(&exe, &home, &["self", "info", "--format=json"]).stdout);
    assert!(info.contains("\"digest_checked\":false"), "{info}");
    assert!(info.contains("\"receipt_matches\":true"), "{info}");
    assert!(info.contains("\"schema\":1.0"), "{info}");
}
