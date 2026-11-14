//! `oskac build --target` (spec #153, ticket #157): the target platform
//! an executable is built for, named as release assets name `oskac`
//! builds, on every host.
//!
//! Each case writes its files into a fresh directory and runs the real
//! binary there. The cases with golden output compare the exit code,
//! stdout, stderr and the files the build left with
//! `tests/golden/build_target/<case>.txt`; regenerate them after an
//! intended change with
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test build_target` and review
//! the diff.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n";

/// A new, empty directory for `case`, holding `files`.
fn workspace(case: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("build_target")
        .join(case);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the case directory");
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    dir
}

/// Run `oskac args` in `dir`.
fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// The bytes of the file `name` in `dir`.
fn read(dir: &Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name} should exist: {e}"))
}

/// Every file under `dir`, by relative path.
fn files_in(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).expect("should list the directory") {
        let entry = entry.expect("a directory entry");
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            files_in(&entry.path(), &format!("{name}/"), out);
        } else {
            out.insert(name);
        }
    }
}

/// `text` with the native separator after each of `prefixes` turned into
/// `/`, in both the plain form
/// (`dist\name`) and the JSON-escaped form (`dist\\name`). oskac prints paths as the OS spells them, so on Windows
/// the goldens, which hold `/`, are compared after this.
fn slash_after(text: &str, prefixes: &[&str]) -> String {
    let mut out = text.to_string();
    for prefix in prefixes.iter().filter(|p| !p.is_empty()) {
        out = out.replace(&format!("{prefix}\\\\"), &format!("{prefix}/"));
        out = out.replace(&format!("{prefix}\\"), &format!("{prefix}/"));
    }
    out
}

#[test]
fn slash_after_handles_plain_and_json_separators() {
    assert_eq!(
        slash_after(r#"E0391: dist\hello and "dist\\hello""#, &["dist"]),
        r#"E0391: dist/hello and "dist/hello""#
    );
    assert_eq!(slash_after(r"a\b", &[""]), r"a\b");
}

/// Run `oskac args` in a fresh directory holding `files`, and compare
/// with the golden file `case`. The directory, for a closer look.
fn assert_golden(case: &str, files: &[(&str, &str)], args: &[&str]) -> PathBuf {
    let dir = workspace(case, files);
    let mut before = BTreeSet::new();
    files_in(&dir, "", &mut before);
    let output = oskac(&dir, args);
    let mut after = BTreeSet::new();
    files_in(&dir, "", &mut after);
    let written: Vec<&String> = after.difference(&before).collect();
    let prefixes: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();
    let actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}--- written\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        slash_after(&String::from_utf8_lossy(&output.stdout), &prefixes),
        slash_after(&String::from_utf8_lossy(&output.stderr), &prefixes),
        written.iter().map(|w| format!("{w}\n")).collect::<String>(),
    );
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/build_target")
        .join(format!("{case}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a golden directory");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("{} should exist: {e}\n{actual}", golden.display()));
    assert!(
        actual == expected,
        "{} differs; rerun with UPDATE_GOLDEN=1 if the change is intended\n--- actual\n{actual}\n--- expected\n{expected}",
        golden.display()
    );
    dir
}

/// A field of `bytes`, little-endian, at `at`.
fn field(bytes: &[u8], at: usize, size: usize) -> u64 {
    bytes[at..at + size]
        .iter()
        .rev()
        .fold(0, |acc, &b| acc << 8 | u64::from(b))
}

/// The ELF header's `e_machine`.
const EM_AARCH64: u64 = 183;
const EM_X86_64: u64 = 62;

/// The host's supported platform, named as release assets name it.
fn host_platform() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("linux-aarch64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x86_64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-aarch64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("macos-x86_64")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows-x86_64")
    } else {
        None
    }
}

// ---- Naming the target ---------------------------------------------------------

#[test]
fn linux_aarch64_names_an_aarch64_linux_executable() {
    let dir = assert_golden(
        "linux_aarch64",
        &[("hello.osk", HELLO)],
        &["build", "hello.osk", "--target", "linux-aarch64"],
    );
    let elf = read(&dir, "hello");
    assert_eq!(&elf[..4], b"\x7fELF");
    assert_eq!(field(&elf, 18, 2), EM_AARCH64);
}

/// Every host builds for x86_64 Linux, the Pi included (ticket #159).
#[test]
fn linux_x86_64_names_an_x86_64_linux_executable() {
    let dir = assert_golden(
        "linux_x86_64",
        &[("hello.osk", HELLO)],
        &["build", "hello.osk", "--target", "linux-x86_64"],
    );
    let elf = read(&dir, "hello");
    assert_eq!(&elf[..4], b"\x7fELF");
    assert_eq!(field(&elf, 18, 2), EM_X86_64);
}

#[test]
fn the_target_may_be_written_with_an_equals_sign() {
    let dir = workspace("equals", &[("hello.osk", HELLO)]);
    let spaced = oskac(
        &dir,
        &["build", "hello.osk", "--target", "linux-aarch64", "-o", "a"],
    );
    let joined = oskac(
        &dir,
        &["build", "hello.osk", "--target=linux-aarch64", "-o", "b"],
    );
    assert!(spaced.status.success() && joined.status.success());
    assert!(read(&dir, "a") == read(&dir, "b"));
}

/// Without `--target`, `oskac build` builds for the host's platform when
/// it has a backend, else for `linux-aarch64`, as before `--target`
/// existed: on aarch64 Linux, the same bytes as `--target linux-aarch64`.
#[test]
fn without_a_target_the_host_platform_is_built_for_when_it_has_a_backend() {
    let dir = workspace("default", &[("hello.osk", HELLO)]);
    let host_builds = host_platform().is_some_and(|host| {
        oskac(
            &dir,
            &["build", "hello.osk", "--target", host, "-o", "host"],
        )
        .status
        .success()
    });
    let expected = match host_platform() {
        Some(host) if host_builds => host,
        _ => "linux-aarch64",
    };
    let named = oskac(
        &dir,
        &["build", "hello.osk", "--target", expected, "-o", "named"],
    );
    assert!(named.status.success(), "{named:?}");
    let default = oskac(&dir, &["build", "hello.osk", "-o", "default"]);
    assert!(default.status.success(), "{default:?}");
    assert!(
        read(&dir, "default") == read(&dir, "named"),
        "the default build is not the `{expected}` one"
    );
    if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        assert_eq!(expected, "linux-aarch64");
    }
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(expected, "linux-x86_64");
    }
}

// ---- Every target --------------------------------------------------------------

#[test]
fn all_writes_one_executable_per_platform_with_a_backend_into_the_output_directory() {
    let dir = assert_golden(
        "all",
        &[("hello.osk", HELLO), ("dist/.keep", "")],
        &["build", "hello.osk", "--target", "all", "-o", "dist"],
    );
    for platform in ["linux-aarch64", "linux-x86_64"] {
        let alone = oskac(
            &dir,
            &["build", "hello.osk", "--target", platform, "-o", "alone"],
        );
        assert!(alone.status.success(), "{platform}");
        assert!(
            read(&dir, &format!("dist/hello-{platform}")) == read(&dir, "alone"),
            "{platform}"
        );
    }
}

#[test]
fn all_reports_every_output_in_json() {
    assert_golden(
        "all_json",
        &[("hello.osk", HELLO), ("dist/.keep", "")],
        &[
            "build",
            "hello.osk",
            "--target",
            "all",
            "-o",
            "dist",
            "--format=json",
        ],
    );
}

#[test]
fn all_without_an_output_directory_writes_into_the_current_one() {
    assert_golden(
        "all_here",
        &[("src/hello.osk", HELLO)],
        &["build", "src/hello.osk", "--target", "all"],
    );
}

#[test]
fn all_names_a_package_s_executables_after_its_directory() {
    assert_golden(
        "all_package",
        &[
            (
                "greeter/oska.toml",
                "[package]\nname = \"greeter\"\nversion = \"0.1.0\"\n",
            ),
            ("greeter/src/main.osk", HELLO),
            ("out/.keep", ""),
        ],
        &["build", "greeter", "--target", "all", "-o", "out"],
    );
}

#[test]
fn all_into_a_missing_directory_is_refused_and_writes_nothing() {
    assert_golden(
        "all_missing_directory",
        &[("hello.osk", HELLO)],
        &["build", "hello.osk", "--target", "all", "-o", "missing"],
    );
}

// ---- Refusals ------------------------------------------------------------------

/// A typo is a usage error: nothing is built, and the message lists the
/// names `--target` takes.
#[test]
fn an_unknown_platform_is_a_usage_error_listing_the_names() {
    assert_golden(
        "unknown",
        &[("hello.osk", HELLO)],
        &["build", "hello.osk", "--target", "linux-arm64"],
    );
    assert_golden(
        "unknown_json",
        &[("hello.osk", HELLO)],
        &[
            "build",
            "hello.osk",
            "--target",
            "linux-arm64",
            "--format=json",
        ],
    );
}

/// The platform is refused before the program is read: a usage error
/// says so whatever the path names.
#[test]
fn an_unknown_platform_is_refused_before_the_program_is_read() {
    let dir = workspace("unknown_before_read", &[]);
    let output = oskac(&dir, &["build", "missing.osk", "--target", "x"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E0393"), "{stderr}");
}

/// A supported platform with no backend yet is not a typo: it has a code
/// of its own, and the message names the release that adds it.
#[test]
fn a_supported_platform_with_no_backend_yet_is_refused_naming_its_release() {
    assert_golden(
        "macos",
        &[("hello.osk", HELLO)],
        &["build", "hello.osk", "--target", "macos-aarch64"],
    );
    assert_golden(
        "windows_json",
        &[("hello.osk", HELLO)],
        &[
            "build",
            "hello.osk",
            "--target",
            "windows-x86_64",
            "--format=json",
        ],
    );
}

#[test]
fn a_target_with_no_platform_is_a_usage_error() {
    let dir = workspace("no_platform", &[("hello.osk", HELLO)]);
    let output = oskac(&dir, &["build", "hello.osk", "--target"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("oskac - the Oska compiler"));
    assert!(!dir.join("hello").exists());
}

/// `oskac run` and `oskac test` always use the host's platform.
#[test]
fn only_build_takes_a_target() {
    let dir = workspace("run_target", &[("hello.osk", HELLO)]);
    for command in ["run", "test", "check"] {
        let output = oskac(&dir, &[command, "hello.osk", "--target", "linux-aarch64"]);
        assert_eq!(output.status.code(), Some(2), "{command}");
        assert!(output.stdout.is_empty(), "{command}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with("oskac - the Oska compiler"),
            "{command}"
        );
    }
}
