//! The parity gate (spec #155, ticket #181; ADR 0014): every supported
//! platform is native, so a program the native backend cannot compile yet
//! (`E0390`) is an error for `oskac run` and `oskac test`, with the same
//! diagnostic and exit code as `oskac build`, and no run on the
//! interpreter. `--interpret` still runs it. The goldens in
//! `native_default.rs` show the exact output; this shows that the three
//! commands agree, for a file and a package.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::portable::Scratch;

/// Whether this host runs `oskac run` and `oskac test` natively: every
/// supported platform. Elsewhere the interpreter runs, and no gate
/// applies.
const NATIVE_HOST: bool = cfg!(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
));

/// `main` and a test that the native backend refuses (`E0390`): `dbg` of
/// a directory capability, whose interpreter text holds an OS handle.
const REFUSED: &str = "fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {\n    \
                       let d = dbg(fs.restrict_to(\".\"))\n    io.println(\"ran\")\n}\n\n\
                       test \"refused\" {\n    let d = dbg(t.fs.restrict_to(\".\"))\n    \
                       t.assert(true)\n}\n";

const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

fn oskac(dir: &Path, args: &[&str]) -> Output {
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).expect("a temporary directory");
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .env("TMPDIR", &tmp)
        .env("TMP", &tmp)
        .env("TEMP", &tmp)
        .output()
        .expect("oskac should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `run` and `test` on `target` in `dir` refuse as `build` does: exit
/// code 2, the same `E0390` diagnostic, nothing on stdout; and run with
/// `--interpret`.
fn assert_gated(dir: &Path, target: &str) {
    let build = oskac(dir, &["build", target, "-o", "out"]);
    assert_eq!(build.status.code(), Some(2), "{build:?}");
    let refusal = text(&build.stderr);
    assert!(refusal.contains("error[E0390]: "), "{refusal}");
    assert!(!refusal.contains("note:"), "{refusal}");
    assert!(!dir.join("out").exists(), "build wrote an executable");
    for command in ["run", "test"] {
        let output = oskac(dir, &[command, target]);
        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        assert_eq!(text(&output.stdout), "", "{command}: nothing runs");
        if command == "run" {
            assert_eq!(text(&output.stderr), refusal, "{command}: {output:?}");
        } else {
            // The test's own `dbg`, at its own place.
            let stderr = text(&output.stderr);
            assert!(
                stderr.lines().count() == 1
                    && stderr.contains(
                        ":7:13: error[E0390]: `oskac build` cannot compile `dbg` of a `Dir` yet"
                    ),
                "{command}: {stderr}"
            );
        }
    }
    let run = oskac(dir, &["run", target, "--interpret"]);
    assert_eq!(run.status.code(), Some(0), "{run:?}");
    assert_eq!(text(&run.stdout), "ran\n");
    let test = oskac(dir, &["test", target, "--interpret"]);
    assert_eq!(test.status.code(), Some(0), "{test:?}");
    assert!(text(&test.stdout).contains("1 passed"), "{test:?}");
}

#[test]
fn a_file_the_native_backend_refuses_is_an_error_for_run_test_and_build() {
    if !NATIVE_HOST {
        return;
    }
    let dir = Scratch::new("oskac-gate", "file");
    std::fs::write(dir.join("refused.osk"), REFUSED).expect("the program");
    assert_gated(&dir, "refused.osk");
}

#[test]
fn a_package_the_native_backend_refuses_is_an_error_for_run_test_and_build() {
    if !NATIVE_HOST {
        return;
    }
    let dir = Scratch::new("oskac-gate", "package");
    std::fs::create_dir_all(dir.join("app/src")).expect("the package");
    std::fs::write(dir.join("app/oska.toml"), MANIFEST).expect("the manifest");
    std::fs::write(dir.join("app/src/main.osk"), REFUSED).expect("the program");
    assert_gated(&dir, "app");
}

#[test]
fn json_reports_the_refusal_as_an_error() {
    if !NATIVE_HOST {
        return;
    }
    let dir = Scratch::new("oskac-gate", "json");
    std::fs::write(dir.join("refused.osk"), REFUSED).expect("the program");
    let run = oskac(&dir, &["run", "refused.osk", "--format=json"]);
    assert_eq!(run.status.code(), Some(2), "{run:?}");
    let stderr = text(&run.stderr);
    assert!(
        stderr.contains("\"code\":\"E0390\"") && stderr.contains("\"severity\":\"error\""),
        "{stderr}"
    );
    let test = oskac(&dir, &["test", "refused.osk", "--format=json"]);
    assert_eq!(test.status.code(), Some(2), "{test:?}");
    assert!(
        text(&test.stdout).contains("\"code\":\"E0390\""),
        "{test:?}"
    );
}
