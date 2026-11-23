// SPDX-License-Identifier: Apache-2.0
//! `oskac self info` names the Linux build of the running `oskac`, the
//! glibc build (`gnu`) or the musl build (`musl`), from the target it was
//! compiled for (spec #461, ticket #464, ADR 0022). On macOS and Windows
//! there is one build, and `linux_build` is `null`.

mod common;

use common::portable::{output_of, set_home, Scratch};
use common::Json;
use std::process::{Command, Output};

/// The build the `oskac` binary is, read from the binary itself rather
/// than from how this test was compiled: the glibc build is linked
/// dynamically, so its ELF file has a `PT_INTERP` program header naming the
/// dynamic loader; the musl build is static and has none. `None` when it
/// is not an ELF file (macOS, Windows).
fn build_of_the_binary() -> Option<&'static str> {
    let elf = std::fs::read(env!("CARGO_BIN_EXE_oskac")).expect("read oskac");
    if !elf.starts_with(b"\x7fELF") {
        return None;
    }
    // 64-bit, little-endian: the only Linux processors with a release.
    assert_eq!((elf[4], elf[5]), (2, 1), "not a 64-bit little-endian ELF");
    let u16_at = |at: usize| usize::from(u16::from_le_bytes([elf[at], elf[at + 1]]));
    let phoff = u64::from_le_bytes(elf[0x20..0x28].try_into().expect("8 bytes"));
    let phoff = usize::try_from(phoff).expect("offset");
    let (entsize, count) = (u16_at(0x36), u16_at(0x38));
    const PT_INTERP: u32 = 3;
    let interp = (0..count).any(|i| {
        let at = phoff + i * entsize;
        u32::from_le_bytes(elf[at..at + 4].try_into().expect("4 bytes")) == PT_INTERP
    });
    Some(if interp { "gnu" } else { "musl" })
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
    let want = match build_of_the_binary() {
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
    match build_of_the_binary() {
        Some(build) => assert!(first.ends_with(&format!(", {build} build)")), "{first}"),
        None => assert!(!first.contains("build"), "{first}"),
    }
}
