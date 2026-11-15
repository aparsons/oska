//! End-to-end tests of `oskac self info` and `oskac self uninstall` (spec
//! #191, ticket #203), in text and JSON, including refusals. Each test
//! makes an Oska home in a temporary directory and runs a copy of `oskac`
//! installed in it, as the installer would have put it there.

mod common;

use common::Json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A scratch directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("oskac-self-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir.canonicalize().expect("canonical scratch dir"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
    let esc = |p: &Path| p.to_string_lossy().replace('\\', "\\\\");
    let receipt = format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"installer\":\"sh\",\"installed_at\":\"2026-11-12T09:30:00Z\"{extra},\"files\":[\
         {{\"path\":\"{}\",\"kind\":\"binary\",\"sha256\":\"aa\"}},\
         {{\"path\":\"{}\",\"kind\":\"record\",\"sha256\":\"bb\"}},\
         {{\"path\":\"{}\",\"kind\":\"record\"}}]}}",
        esc(&exe),
        esc(&home.join("version")),
        esc(&home.join("receipt.json")),
    );
    std::fs::write(home.join("receipt.json"), receipt).expect("receipt");
    exe
}

/// Run `command`, trying again while the kernel says the executable is
/// busy: another test's `fork` can hold the copy's write handle open for an
/// instant after the copy is made.
fn output_of(command: &mut Command) -> Output {
    for _ in 0..50 {
        match command.output() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => return result.expect("oskac should run"),
        }
    }
    command.output().expect("oskac should run")
}

fn run(exe: &Path, home: &Path, args: &[&str]) -> Output {
    output_of(
        Command::new(exe)
            .args(args)
            .env("OSKA_HOME", home)
            .env("HOME", home.parent().expect("parent")),
    )
}

fn oskac(home: &Path, args: &[&str]) -> Output {
    run(Path::new(env!("CARGO_BIN_EXE_oskac")), home, args)
}

fn json_of(output: &Output) -> Json {
    Json::parse(text(&output.stdout).trim())
}

#[test]
fn info_without_a_receipt_reports_null_and_no_match() {
    let scratch = Scratch::new("info-none");
    let home = scratch.0.join(".oska");
    let output = oskac(&home, &["self", "info", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stderr), "");
    let info = json_of(&output);
    assert_eq!(info.get("version"), Some(&Json::Str(VERSION.to_string())));
    assert_eq!(info.get("receipt"), Some(&Json::Null));
    assert_eq!(info.get("receipt_matches"), Some(&Json::Bool(false)));
    assert_eq!(
        info.get("oska_home"),
        Some(&Json::Str(home.to_string_lossy().into_owned()))
    );
    let platform = match info.get("platform") {
        Some(Json::Str(p)) => p.clone(),
        other => panic!("platform should be a string, got {other:?}"),
    };
    assert!(platform.contains('-'), "platform {platform}");
    assert!(matches!(info.get("executable"), Some(Json::Str(_))));
}

#[test]
fn info_of_a_managed_install_reports_the_receipt_and_a_match() {
    let scratch = Scratch::new("info-managed");
    let home = scratch.0.join(".oska");
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
    assert_eq!(
        info.get("executable"),
        Some(&Json::Str(exe.to_string_lossy().into_owned()))
    );

    let human = run(&exe, &home, &["self", "info"]);
    let human = text(&human.stdout);
    assert!(human.starts_with(&format!("oskac {VERSION} (")), "{human}");
    assert!(human.contains("managed install: yes"), "{human}");
    assert!(human.contains("by the sh installer"), "{human}");
}

#[test]
fn info_does_not_match_another_version_or_another_copy() {
    let scratch = Scratch::new("info-mismatch");
    let home = scratch.0.join(".oska");
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
    let scratch = Scratch::new("info-bad");
    let home = scratch.0.join(".oska");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("receipt.json"), "{not json").expect("receipt");
    let info = json_of(&oskac(&home, &["self", "info", "--format=json"]));
    assert_eq!(info.get("receipt"), Some(&Json::Null));
    assert!(matches!(info.get("receipt_error"), Some(Json::Str(_))));
    assert_eq!(info.get("receipt_matches"), Some(&Json::Bool(false)));
}

#[test]
fn uninstall_removes_what_the_receipt_lists_and_nothing_else() {
    let scratch = Scratch::new("uninstall");
    let home = scratch.0.join(".oska");
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
    assert_eq!(removed.len(), 5, "{removed:?}");
    assert_eq!(done.get("kept"), Some(&Json::Arr(vec![])));
    assert_eq!(done.get("path_hint"), Some(&Json::Null));
    assert!(!home.exists(), "the empty Oska home should be removed");
}

#[test]
fn uninstall_keeps_a_directory_with_something_else_in_it() {
    let scratch = Scratch::new("uninstall-keep");
    let home = scratch.0.join(".oska");
    let exe = install(&home, VERSION, "");
    std::fs::write(home.join("notes.txt"), "mine").expect("notes");
    std::fs::create_dir_all(scratch.0.join(".config/oska")).expect("config");
    let output = run(&exe, &home, &["self", "uninstall"]);
    assert_eq!(output.status.code(), Some(0));
    let human = text(&output.stdout);
    assert!(human.starts_with("Uninstalled oskac from "), "{human}");
    assert!(human.contains("Kept:"), "{human}");
    assert!(human.contains("(not empty)"), "{human}");
    assert!(human.contains("settings and credentials"), "{human}");
    assert!(home.join("notes.txt").exists());
    assert!(!home.join("receipt.json").exists());
    assert!(scratch.0.join(".config/oska").exists());
}

#[test]
fn uninstall_without_a_receipt_is_not_installed() {
    let scratch = Scratch::new("uninstall-none");
    let home = scratch.0.join(".oska");
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

#[test]
fn uninstall_by_a_copy_the_receipt_does_not_record_is_refused() {
    let scratch = Scratch::new("uninstall-unmanaged");
    let home = scratch.0.join(".oska");
    let exe = install(&home, VERSION, "");
    // This test binary's oskac is not the one in the receipt.
    let output = oskac(&home, &["self", "uninstall", "--format=json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        json_of(&output).get("error"),
        Some(&Json::Str("not_managed".into()))
    );
    assert!(exe.exists(), "nothing should be removed");
    assert!(home.join("receipt.json").exists());
}

#[test]
fn uninstall_of_another_version_is_refused() {
    let scratch = Scratch::new("uninstall-version");
    let home = scratch.0.join(".oska");
    let exe = install(&home, "0.0.1", "");
    let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        json_of(&output).get("error"),
        Some(&Json::Str("not_managed".into()))
    );
    assert!(exe.exists());
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
    let scratch = Scratch::new("uninstall-unsafe");
    let outside = scratch.0.join("outside.txt");
    std::fs::write(&outside, "precious").expect("outside");
    let cases = [
        (
            format!(
                "{{\"path\":\"{}\",\"kind\":\"record\",\"sha256\":\"cc\"}}",
                outside.to_string_lossy().replace('\\', "\\\\")
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
        let home = scratch.0.join(format!("h{n}")).join(".oska");
        let exe = install(&home, VERSION, "");
        // JSON-escaped, with the platform's separators.
        let sep = std::path::MAIN_SEPARATOR_STR.replace('\\', "\\\\");
        let home_text = home.to_string_lossy().replace('\\', "\\\\");
        add_entry(&home, &entry.replace('/', &sep).replace("HOME", &home_text));
        let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
        assert_eq!(output.status.code(), Some(1), "{why}");
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
    let scratch = Scratch::new("uninstall-garbled");
    let home = scratch.0.join(".oska");
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
    let scratch = Scratch::new("uninstall-symlink");
    let target = scratch.0.join("target.txt");
    std::fs::write(&target, "precious").expect("target");

    // A listed file that is a symlink.
    let home = scratch.0.join("a/.oska");
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
    let real = scratch.0.join("b-real");
    let exe = install(&real.join(".oska"), VERSION, "");
    symlink(&real, scratch.0.join("b-link")).expect("symlink");
    let home = scratch.0.join("b-link/.oska");
    let output = run(&exe, &home, &["self", "uninstall", "--format=json"]);
    assert_eq!(
        json_of(&output).get("error"),
        Some(&Json::Str("unsafe_path".into()))
    );
    assert!(exe.exists());
}

#[test]
fn uninstall_reports_the_path_hint_when_bin_is_on_the_path() {
    let scratch = Scratch::new("uninstall-path");
    let home = scratch.0.join(".oska");
    let exe = install(&home, VERSION, "");
    let output = output_of(
        Command::new(&exe)
            .args(["self", "uninstall", "--format=json"])
            .env("OSKA_HOME", &home)
            .env("PATH", home.join("bin")),
    );
    let done = json_of(&output);
    let Some(Json::Str(hint)) = done.get("path_hint") else {
        panic!("path_hint should be a string: {done:?}")
    };
    assert!(hint.contains("is on your PATH"), "{hint}");
}

#[test]
fn unknown_self_commands_are_usage_errors() {
    let scratch = Scratch::new("usage");
    for args in [&["self"][..], &["self", "update"], &["self", "info", "x"]] {
        let output = oskac(&scratch.0, args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(text(&output.stderr).contains("oskac self info"), "{args:?}");
    }
}
