// SPDX-License-Identifier: Apache-2.0
//! `Float` through the CLI (spec #590, ticket #593): `oskac run` on the
//! host's native backend, `oskac run --interpret`, and a program built
//! with `oskac build` print the same bytes, and those bytes are the golden
//! file `tests/golden/float_arith/output.txt`. The golden file holds the
//! bits of each result, so it is the same on Linux, macOS and Windows
//! (ADR 0014, ADR 0027). To regenerate it after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test float_arith` and review the
//! diff.

mod common;

use std::path::Path;
use std::process::Command;

use common::portable::{output_of, Scratch};

fn golden_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/float_arith")
}

fn stdout_of(command: &mut Command) -> String {
    let output = output_of(command);
    assert!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

#[test]
fn run_interpret_and_build_print_the_golden_bytes() {
    let scratch = Scratch::new("oska-float", "arith");
    let program = golden_dir().join("program.osk");
    let oskac = env!("CARGO_BIN_EXE_oskac");
    let interpreted = stdout_of(
        Command::new(oskac)
            .arg("run")
            .arg("--interpret")
            .arg(&program),
    );
    let default = stdout_of(Command::new(oskac).arg("run").arg(&program));
    let exe = scratch
        .path()
        .join(if cfg!(windows) { "float.exe" } else { "float" });
    let built = output_of(
        Command::new(oskac)
            .arg("build")
            .arg(&program)
            .arg("-o")
            .arg(&exe),
    );
    assert!(
        built.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let native = stdout_of(&mut Command::new(&exe));

    let golden = golden_dir().join("output.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &interpreted).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("the golden file should exist");
    assert_eq!(
        interpreted, expected,
        "the interpreter differs from the golden file"
    );
    assert_eq!(
        default, expected,
        "`oskac run` differs from the golden file"
    );
    assert_eq!(
        native, expected,
        "the built program differs from the golden file"
    );
}
