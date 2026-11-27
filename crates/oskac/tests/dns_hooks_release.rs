// SPDX-License-Identifier: Apache-2.0
//! The DNS test hooks (`OSKA_DNS_HOSTS`, `OSKA_DNS_RESOLV_CONF`,
//! `OSKA_DNS_PORT`; ADR 0026) are for debug builds of Oska only (#738).
//!
//! Cargo runs these tests with a debug `oskac`, so the release path is
//! reached the way a release `oskac` reaches it: the host that runs the
//! program, and the compiler that builds it, are told "no hooks"
//! (`RealHost::with_dns_test_hooks(false)`, `oska_native::build_hooked(..,
//! false)`). Neither is something a program or its environment can set. A
//! test with the hooks on is the control: the same program, the same
//! variables, and the lookup is redirected.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use std::path::Path;
use std::process::Command;

use differential::host_target;

/// Connects by name to its own loopback listener: exit code 0 when the
/// name resolved and the connection was made, 1 when it did not.
const PROGRAM: &str = r#"fn main(net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("printer.invalid", listener.port()).connect()?
    client.close()
    Ok(())
}
"#;

/// A hosts file that names `printer.invalid`, a name no resolver knows.
const HOSTS: &str = "127.0.0.1 printer.invalid\n";

fn set_hooks_in_process_environment() {
    // Every test of this file sets the same values, so the order in which
    // they run does not matter.
    std::env::set_var("OSKA_DNS_HOSTS", HOSTS);
    std::env::set_var("OSKA_DNS_RESOLV_CONF", "");
    std::env::set_var("OSKA_DNS_PORT", "9");
}

fn interpreted_exit_code(hooks: bool) -> i32 {
    set_hooks_in_process_environment();
    let mut host = oska_run::RealHost::new().with_dns_test_hooks(hooks);
    let argv = vec!["main.osk".to_string()];
    oska_run::run(PROGRAM, &mut host, &argv).exit_code
}

fn native_exit_code(hooks: bool, dir: &Path) -> i32 {
    let report = oska_check::check_module(PROGRAM);
    let program = report.program.expect("the program checks");
    let bytes = oska_native::build_hooked(
        &program,
        host_target(),
        &oska_native::Unplaced,
        oska_check::limits::Limits::NONE,
        hooks,
    )
    .expect("the program builds");
    let exe = dir.join(format!("program{}", host_target().suffix()));
    std::fs::write(&exe, bytes).expect("the executable is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .expect("the executable is made executable");
    }
    let mut command = Command::new(&exe);
    command
        .current_dir(dir)
        .env("OSKA_DNS_HOSTS", HOSTS)
        .env("OSKA_DNS_RESOLV_CONF", "")
        .env("OSKA_DNS_PORT", "9");
    if let Ok(root) = std::env::var("SYSTEMROOT") {
        command.env("SYSTEMROOT", root);
    }
    command
        .output()
        .expect("the executable runs")
        .status
        .code()
        .expect("the executable exits")
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("oska-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

#[test]
fn the_interpreter_honors_the_hooks_only_when_its_host_says_so() {
    assert_eq!(
        interpreted_exit_code(true),
        0,
        "hooks on: the hosts file answers"
    );
    assert_ne!(
        interpreted_exit_code(false),
        0,
        "hooks off: the variables are ignored and the name is not found"
    );
}

#[test]
fn a_native_executable_honors_the_hooks_only_when_built_with_them() {
    let dir = scratch("dns-hooks-native");
    assert_eq!(
        native_exit_code(true, &dir),
        0,
        "hooks on: the hosts file answers"
    );
    assert_ne!(
        native_exit_code(false, &dir),
        0,
        "hooks off: the variables are ignored and the name is not found"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_host_and_a_build_with_no_say_ignore_the_hooks() {
    // The default is the release behaviour: the hooks are opt-in.
    let dir = scratch("dns-hooks-default");
    assert_ne!(
        {
            set_hooks_in_process_environment();
            let mut host = oska_run::RealHost::new();
            let argv = vec!["main.osk".to_string()];
            oska_run::run(PROGRAM, &mut host, &argv).exit_code
        },
        0
    );
    let report = oska_check::check_module(PROGRAM);
    let program = report.program.expect("the program checks");
    let plain = oska_native::build(&program, host_target(), &oska_native::Unplaced)
        .expect("the program builds");
    let off = oska_native::build_hooked(
        &program,
        host_target(),
        &oska_native::Unplaced,
        oska_check::limits::Limits::NONE,
        false,
    )
    .expect("the program builds");
    assert_eq!(plain, off, "`build` is the build without the hooks");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `oskac` this test was built with is a debug or a release build,
/// and runs the program as that build should: the wiring from
/// `cfg!(debug_assertions)` to both backends. (Under `cargo test
/// --release` this is the release proof.)
#[test]
fn oskac_honors_the_hooks_exactly_when_it_is_a_debug_build() {
    let dir = scratch("dns-hooks-oskac");
    std::fs::write(dir.join("main.osk"), PROGRAM).expect("the program is written");
    let exe = dir.join(format!("hooked{}", host_target().suffix()));
    let oskac = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
        command
            .current_dir(&dir)
            .args(["--no-cache"])
            .env("OSKA_DNS_HOSTS", HOSTS)
            .env("OSKA_DNS_RESOLV_CONF", "")
            .env("OSKA_DNS_PORT", "9");
        if let Ok(root) = std::env::var("SYSTEMROOT") {
            command.env("SYSTEMROOT", root);
        }
        command
    };
    let want = if cfg!(debug_assertions) {
        Some(0)
    } else {
        Some(1)
    };
    let interpreted = oskac()
        .args(["run", "--interpret", "main.osk"])
        .output()
        .expect("oskac runs");
    assert_eq!(
        interpreted.status.code(),
        want,
        "interpreter: {interpreted:?}"
    );
    // `oskac run` (native) builds with the flag too.
    let ran = oskac()
        .args(["run", "main.osk"])
        .output()
        .expect("oskac runs");
    assert_eq!(ran.status.code(), want, "native run: {ran:?}");
    // So does `oskac test`, on both backends.
    std::fs::write(
        dir.join("hook_test.osk"),
        r#"#[integration]
test "connects by name" {
    let listener = t.net.listen_only().restrict_to_loopback(0)
    match t.net.restrict_to("printer.invalid", listener.port()).connect() {
        Ok(client) => client.close()
        Err(error) => t.fail("no connect")
    }
}
"#,
    )
    .expect("the test file is written");
    let want_test = if cfg!(debug_assertions) {
        Some(0)
    } else {
        Some(1)
    };
    for backend in [&["--interpret"][..], &[][..]] {
        let tested = oskac()
            .args(["test", "--integration"])
            .args(backend)
            .arg("hook_test.osk")
            .output()
            .expect("oskac tests");
        assert_eq!(
            tested.status.code(),
            want_test,
            "test {backend:?}: {tested:?}"
        );
    }
    let built = oskac()
        .args(["build", "main.osk", "-o"])
        .arg(&exe)
        .output()
        .expect("oskac builds");
    assert!(built.status.success(), "{built:?}");
    let mut program = Command::new(&exe);
    program
        .current_dir(&dir)
        .env("OSKA_DNS_HOSTS", HOSTS)
        .env("OSKA_DNS_RESOLV_CONF", "")
        .env("OSKA_DNS_PORT", "9");
    if let Ok(root) = std::env::var("SYSTEMROOT") {
        program.env("SYSTEMROOT", root);
    }
    let native = program.output().expect("the executable runs");
    assert_eq!(native.status.code(), want, "native: {native:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
