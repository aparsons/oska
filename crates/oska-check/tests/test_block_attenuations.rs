//! Attenuations inside a `test` block (#150): the authority report names
//! the test as `test "<name>"`, in the human report's heading and in the
//! JSON `function` field, so a reviewer can tell which test narrows what.

mod common;

use common::check;

#[test]
fn an_attenuation_in_a_test_is_named_by_the_test() {
    let report = check(
        "test \"reads a note\" {\n    let notes = t.fs.read_only().restrict_to(\"notes\")\n}",
    );
    let functions: Vec<&str> = report
        .attenuations
        .iter()
        .map(|a| a.function.as_str())
        .collect();
    assert_eq!(
        functions,
        ["test \"reads a note\"", "test \"reads a note\""]
    );
}

#[test]
fn the_human_report_lists_a_tests_attenuations_under_the_test() {
    let src = "test \"reads a note\" {\n    let notes = t.fs.read_only()\n}";
    let human = check(src).authority_to_human("prog.osk");
    let start = src.find("t.fs.read_only()").expect("in the source");
    let expected = format!(
        "test \"reads a note\":\n\
         \x20 Fs -> FsRead: read_only (bytes {}..{})\n",
        start,
        start + "t.fs.read_only()".len()
    );
    assert_eq!(human, expected);
}

#[test]
fn the_json_report_escapes_the_tests_name() {
    let report = check("test \"says \\\"hi\\\"\" {\n    let r = t.fs.read_only()\n}");
    assert!(
        report
            .to_json()
            .contains(r#""function":"test \"says \\\"hi\\\"\"""#),
        "{}",
        report.to_json()
    );
}
