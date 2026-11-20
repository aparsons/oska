//! `oskac self info` names the Linux build of the running `oskac`, the
//! glibc build (`gnu`) or the musl build (`musl`), from the target it was
//! compiled for (spec #461, ticket #464, ADR 0022). On macOS and Windows
//! there is one build, and `linux_build` is `null`.

mod common;

use common::portable::{output_of, set_home, Scratch};
use common::Json;
use std::process::{Command, Output};

/// The build this test binary, and so the `oskac` beside it, was compiled
/// as: the same target compiles both.
fn expected_build() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_env = "musl")) {
        Some("musl")
    } else if cfg!(all(target_os = "linux", target_env = "gnu")) {
        Some("gnu")
    } else {
        None
    }
}

fn self_info(args: &[&str]) -> Output {
    let scratch = Scratch::new("oskac-self-build", "info");
    let home = scratch.path().join(".oska");
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command.args(args).env("OSKA_HOME", &home);
    set_home(&mut command, scratch.path());
    let output = output_of(&mut command);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    output
}

#[test]
fn self_info_reports_the_linux_build_of_the_running_oskac() {
    let output = self_info(&["self", "info", "--format=json"]);
    let info = Json::parse(String::from_utf8(output.stdout).expect("UTF-8").trim());
    let want = match expected_build() {
        Some(build) => Json::Str(build.to_string()),
        None => Json::Null,
    };
    assert_eq!(info.get("linux_build"), Some(&want));
    // The platform stays the one name for both builds.
    if let Some(Json::Str(platform)) = info.get("platform") {
        assert!(
            !platform.ends_with("-gnu") && !platform.ends_with("-musl"),
            "{platform}"
        );
    } else {
        panic!("platform should be a string");
    }

    let human = self_info(&["self", "info"]);
    let first = String::from_utf8(human.stdout).expect("UTF-8");
    let first = first.lines().next().expect("a first line").to_string();
    match expected_build() {
        Some(build) => assert!(first.ends_with(&format!(", {build} build)")), "{first}"),
        None => assert!(!first.contains("build"), "{first}"),
    }
}
