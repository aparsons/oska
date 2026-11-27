// SPDX-License-Identifier: Apache-2.0
//! The update checker `oskac` embeds (spec #230, ticket #257) is the
//! executable `oskac build --target <platform>` makes of the same source,
//! byte for byte, for the platform this `oskac` was built for. CI builds
//! and runs this on every runner, so every supported platform is covered.

use std::path::Path;
use std::process::Command;

mod common;
use common::portable::Scratch;

/// The embedded executable, as `build.rs` wrote it.
const EMBEDDED: &[u8] = include_bytes!(env!("OSKA_UPDATE_CHECKER_BIN"));
const PLATFORM: &str = env!("OSKA_UPDATE_CHECKER_PLATFORM");

fn build(dir: &Scratch, target: &str) -> Vec<u8> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("update/checker.osk");
    std::fs::copy(&source, dir.join("checker.osk")).expect("should copy the source");
    let out = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["build", "checker.osk", "-o", "built", "--target", target])
        .current_dir(dir.path())
        .output()
        .expect("oskac should run");
    assert!(out.status.success(), "oskac build failed: {out:?}");
    std::fs::read(dir.join("built")).expect("should read the executable")
}

#[test]
fn embedded_checker_equals_oskac_build_of_the_same_source() {
    let built = build(&Scratch::new("update-checker", "same"), PLATFORM);
    assert!(!EMBEDDED.is_empty(), "the embedded checker is empty");
    assert!(
        built == EMBEDDED,
        "the embedded update checker for {PLATFORM} differs from `oskac build --target {PLATFORM}`"
    );
}
