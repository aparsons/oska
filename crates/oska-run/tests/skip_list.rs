//! The native differential skip list (`common/skips.rs`) names only tests
//! that exist, each with a reason.

mod common;

use std::path::Path;

#[test]
fn skip_list_names_real_tests() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for (case, reason) in common::skips::SKIPS {
        assert!(!reason.trim().is_empty(), "`{case}` needs a reason");
        let (file, path) = case
            .split_once("::")
            .unwrap_or_else(|| panic!("`{case}` should be `<test file>::<test>`"));
        // `oskac`'s corpus test checks its own labels.
        if file == "native_corpus" {
            continue;
        }
        let name = path.rsplit("::").next().unwrap_or(path);
        let source = std::fs::read_to_string(dir.join(format!("{file}.rs")))
            .unwrap_or_else(|e| panic!("`{case}`: no test file `{file}.rs`: {e}"));
        assert!(
            source.contains(&format!("fn {name}()")),
            "`{case}`: `{file}.rs` has no test `{name}`"
        );
    }
}

#[test]
fn skip_list_has_no_duplicates() {
    let mut cases: Vec<&str> = common::skips::SKIPS.iter().map(|(case, _)| *case).collect();
    let all = cases.len();
    cases.sort_unstable();
    cases.dedup();
    assert_eq!(cases.len(), all, "a test is in the skip list twice");
}
