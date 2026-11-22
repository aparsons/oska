// SPDX-License-Identifier: Apache-2.0
//! Golden files: compare what a test saw with a file under `tests/golden/`,
//! or rewrite the file when `UPDATE_GOLDEN` is set.

use std::path::Path;

/// Assert that `actual` is the content of the golden file at `golden`.
/// With `UPDATE_GOLDEN` set, write `actual` there first (and its
/// directory), so an intended change is one rerun and a reviewed diff.
pub fn assert_golden_file(golden: &Path, actual: &str) {
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("the golden directory");
        std::fs::write(golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(golden)
        .unwrap_or_else(|e| panic!("golden file {} should exist: {e}", golden.display()));
    assert!(
        actual == expected,
        "output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}

/// `text` with the number of each `"duration_ms":` replaced by `<ms>`, so
/// a golden file of an `oskac test --format=json` report stays the same
/// from run to run. It panics when a `"duration_ms":` is not followed by
/// a non-negative integer, so masking never hides a malformed duration.
pub fn mask_durations(text: &str) -> String {
    const KEY: &str = "\"duration_ms\":";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(KEY) {
        let (before, after) = rest.split_at(at + KEY.len());
        out.push_str(before);
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        assert!(
            digits > 0,
            "`duration_ms` should be a non-negative integer in\n{text}"
        );
        out.push_str("<ms>");
        rest = &after[digits..];
    }
    out.push_str(rest);
    out
}

/// Run `oskac args` in `dir`, then assert that the run, as a transcript
/// of the command line, the exit code, stdout, stderr and the text of
/// `file` in `dir` afterwards, is the golden file at `golden`. Returns
/// the run's output.
pub fn assert_run_and_file(
    golden: &Path,
    dir: &Path,
    args: &[&str],
    file: &str,
) -> std::process::Output {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run");
    let text = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).expect("output should be UTF-8");
    let actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}--- file {file}\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        text(&output.stdout),
        text(&output.stderr),
        std::fs::read_to_string(dir.join(file)).expect("should read the file"),
    );
    assert_golden_file(golden, &actual);
    output
}
