//! The Windows runtime in native code (#177, ADR 0020): what `std.rt`
//! does on Windows that no other platform's executable shows, checked on
//! `windows-latest`. Elsewhere each program is only built, for
//! `windows-x86_64`. The differential suite (`native_*.rs`) checks the
//! rest against the interpreter there.

mod common;

use common::portable::Scratch;
use std::path::Path;
use std::process::{Command, Output};

const PLATFORM: &str = "windows-x86_64";

/// A scratch directory holding `main.osk` with `src`.
fn workspace(name: &str, src: &str) -> Scratch {
    let dir = Scratch::new("windows_runtime", name);
    std::fs::write(dir.path().join("main.osk"), src).expect("should write main.osk");
    dir
}

/// `oskac` with `args`, run in `dir`.
fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// A program that prints, then overflows the native stack: `dbg` of a
/// chain five million links deep recurses natively once per link.
const OVERFLOW: &str = "enum Chain {\n    Link(Int, Chain),\n    End,\n}\n\n\
     fn chain() -> Chain {\n    var c = Chain.End\n    var i = 0\n    \
     while i < 5000000 {\n        c = Chain.Link(i, c)\n        i = i + 1\n    }\n    c\n}\n\n\
     fn main(io: Stdout) -> Result<(), IoError> {\n    \
     io.println(\"printed before the fault\")?\n    let c = dbg(chain())\n    Ok(())\n}\n";

/// A stack overflow reaches `std.rt`'s vectored exception handler: what
/// stdout held, then a report naming `STATUS_STACK_OVERFLOW`, and the
/// process ends with that code, as Windows ends a faulted process.
#[test]
fn a_stack_overflow_shows_the_output_then_the_exception_report() {
    let dir = workspace("overflow", OVERFLOW);
    let built = oskac(
        dir.path(),
        &["build", "main.osk", "--target", PLATFORM, "-o", "main.exe"],
    );
    assert!(built.status.success(), "{built:?}");
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return;
    }
    let output = Command::new(dir.path().join("main.exe"))
        .current_dir(dir.path())
        .output()
        .expect("the executable should start");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "printed before the fault\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: the native program faulted with exception 0xC00000FD\n"
    );
    // `STATUS_STACK_OVERFLOW` as the exit code's 32 bits.
    assert_eq!(output.status.code(), Some(0xC000_00FD_u32 as i32));
}

/// On Windows a variable's name ignores case on both backends, as
/// `std::env::var` does there; elsewhere it does not.
#[test]
fn environment_names_ignore_case_on_windows_as_in_the_interpreter() {
    let src = "fn main(io: Stdout, env: Env) {\n    match env.get(\"oska_case_test\") {\n        \
               Some(v) => { let _ = io.println(\"set: {v}\") }\n        \
               None => { let _ = io.println(\"not set\") }\n    }\n}\n";
    let dir = workspace("env_case", src);
    let built = oskac(
        dir.path(),
        &["build", "main.osk", "--target", PLATFORM, "-o", "main.exe"],
    );
    assert!(built.status.success(), "{built:?}");
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return;
    }
    let native = Command::new(dir.path().join("main.exe"))
        .env("OSKA_CASE_TEST", "yes")
        .current_dir(dir.path())
        .output()
        .expect("the executable should start");
    let interpreted = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["run", "--interpret", "main.osk"])
        .env("OSKA_CASE_TEST", "yes")
        .current_dir(dir.path())
        .output()
        .expect("the interpreter should start");
    assert_eq!(String::from_utf8_lossy(&native.stdout), "set: yes\n");
    assert_eq!(native.stdout, interpreted.stdout);
}
