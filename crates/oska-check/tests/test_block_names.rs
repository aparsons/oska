//! How the authority report writes a test's name (#150, spec #184): as an
//! Oska string, so a name with quotes, escapes or text outside ASCII reads
//! as it was written, and JSON-safe in the JSON report. A test with no
//! name text is told by its line, never `test ""`.

mod common;

use common::check;

/// The human report's first line for `src`, whose one test narrows `t.fs`.
fn heading(src: &str) -> String {
    let human = check(src).authority_to_human("prog.osk");
    human.lines().next().expect("a heading").to_string()
}

#[test]
fn a_tests_name_is_written_as_an_oska_string() {
    // Quotes, a newline, a backslash and braces are escaped as Oska
    // spells them; text outside ASCII is kept.
    let src = "test \"say \\\"hi\\\"\\n \\\\ \\{x\\} caf\u{e9} \u{65e5}\u{672c}\" {\n    let r = t.fs.read_only()\n}";
    assert_eq!(
        heading(src),
        "test \"say \\\"hi\\\"\\n \\\\ \\{x\\} caf\u{e9} \u{65e5}\u{672c}\":"
    );
}

#[test]
fn a_tests_name_in_the_json_is_json_safe() {
    let src = "test \"a\\tb \\\"q\\\" \\{x\\} \u{65e5}\" {\n    let r = t.fs.read_only()\n}";
    let json = check(src).to_json();
    // The label `test "a\tb \"q\" \{x\} 日`, escaped once more for JSON.
    let expected = "\"function\":\"test \\\"a\\\\tb \\\\\\\"q\\\\\\\" \\\\{x\\\\} \u{65e5}\\\"\"";
    assert!(json.contains(expected), "{json}");
}

#[test]
fn a_control_character_in_a_name_cannot_split_the_report() {
    let src = "test \"a\\0b\\rc\" {\n    let r = t.fs.read_only()\n}";
    let human = check(src).authority_to_human("prog.osk");
    assert_eq!(human.lines().count(), 2, "{human:?}");
    assert_eq!(human.lines().next(), Some("test \"a\\0b\\rc\":"));
}

#[test]
fn an_empty_name_falls_back_to_the_tests_line() {
    let src = "\n\n\ntest \"\" {\n    let r = t.fs.read_only()\n}";
    assert_eq!(heading(src), "test at line 4:");
    let json = check(src).to_json();
    assert!(json.contains("\"function\":\"test at line 4\""), "{json}");
    assert!(!json.contains("test \\\"\\\""), "{json}");
}

#[test]
fn two_tests_with_empty_names_are_told_apart_by_their_lines() {
    let src = "test \"\" {\n    let a = t.fs.read_only()\n}\n\ntest \"\" {\n    let b = t.fs.write_only()\n}";
    let human = check(src).authority_to_human("prog.osk");
    let headings: Vec<&str> = human.lines().filter(|l| !l.starts_with(' ')).collect();
    assert_eq!(headings, ["test at line 1:", "test at line 5:"]);
}

#[test]
fn an_empty_name_in_a_package_gives_the_line_in_its_own_file() {
    let util =
        "pub(pkg) fn one() -> Int {\n    1\n}\n\ntest \"\" {\n    let r = t.fs.read_only()\n}\n";
    let main = "fn main() {\n}\n";
    let report = common::check_modules(&[("main", main), ("util", util)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let names: Vec<String> = report
        .attenuations
        .iter()
        .map(|a| a.function.to_string())
        .collect();
    assert_eq!(names, ["test at line 5"]);
}
