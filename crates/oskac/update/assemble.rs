// SPDX-License-Identifier: Apache-2.0
// Shared by `build.rs` and the tests (`include!`): the source of the
// update checker for a build. With `test_site` false it is `checker.osk`
// as it is; with it true, the block between the `BEGIN pinned` and `END
// pinned` lines is replaced by `test_site.osk` (the `test-site` feature).

/// The checker's source for a build.
pub fn checker_source(checker: &str, test_site: &str, with_test_site: bool) -> String {
    if !with_test_site {
        return checker.to_string();
    }
    let begin = "// BEGIN pinned\n";
    let end = "// END pinned\n";
    let from = checker.find(begin).expect("a BEGIN pinned mark");
    let to = checker.find(end).expect("an END pinned mark");
    // The test block starts after its own header comment.
    let body = test_site
        .split_once("\nfn site_host")
        .map(|(_, rest)| format!("fn site_host{rest}"))
        .expect("the test block starts at site_host");
    format!(
        "{}{}{}",
        &checker[..from + begin.len()],
        body,
        &checker[to..]
    )
}
