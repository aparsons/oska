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
