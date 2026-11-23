// SPDX-License-Identifier: Apache-2.0
//! Scrubbed files: no fixture of the harness or the driver, and no
//! committed result, holds a real home path. Recorded transcripts and
//! results are scrubbed before they are committed. No test calls a model.

/// Every file beneath `dir`.
fn files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("the directory should be readable") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// No fixture (the harness's, and the driver's recorded transcripts), and
/// no committed result, holds a real home path: recorded transcripts and
/// results are scrubbed before they are committed.
#[test]
fn the_fixtures_and_the_committed_results_hold_no_home_path() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    files_under(&root.join("tests/fixtures"), &mut files);
    files_under(&root.join("../../eval/results"), &mut files);
    files_under(&root.join("../../scripts/eval-batch/fixtures"), &mut files);
    assert!(files.len() > 20, "the walk should find the fixtures");
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for home in ["/home/pi", "/Users/", "C:\\Users", "C:/Users"] {
            assert!(
                !text.contains(home),
                "{} holds a home path ({home})",
                path.display()
            );
        }
    }
}
