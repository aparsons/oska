//! Trust propagation (#52): untrusted-ness carries through prelude text
//! operations, interpolation, and every route a value can take, while
//! results that are not text stay trusted.

mod common;

use common::{assert_errors, assert_ok};

/// `fn greet(name: String)`, a trusted `String` parameter, then `body`.
fn with_greet(body: &str) -> String {
    format!("fn greet(name: String) -> String {{\n    name\n}}\n{body}")
}

#[test]
fn a_text_method_on_untrusted_text_returns_untrusted_text() {
    let src = with_greet(
        "fn f(env: Env) -> Option<String> {\n    let home = env.get(\"HOME\")?\n    Some(greet(home.trim()))\n}",
    );
    assert_errors(&src, &[("E0300", "home.trim()")]);
}

#[test]
fn the_lines_of_untrusted_text_are_untrusted() {
    assert_errors(
        "fn f(fs: FsRead, path: Path) -> Result<List<String>, IoError> {\n    let text = fs.read_to_string(path)?\n    Ok(text.lines())\n}",
        &[("E0300", "text.lines()")],
    );
}

#[test]
fn a_text_method_on_trusted_text_returns_trusted_text() {
    assert_ok(&with_greet(
        "fn f(raw: String) -> List<String> {\n    let name = greet(raw.trim())\n    name.split(\",\")\n}",
    ));
}

#[test]
fn an_untrusted_argument_makes_the_text_result_untrusted() {
    assert_errors(
        "fn f(args: Args) -> Option<List<String>> {\n    let sep = args.get(1)?\n    Some(\"a,b\".split(sep))\n}",
        &[("E0300", "\"a,b\".split(sep)")],
    );
}

#[test]
fn non_text_results_of_untrusted_text_are_trusted() {
    assert_ok(
        "fn f(args: Args) -> Option<Int> {\n    let arg = args.get(1)?\n    \
         let n: Int = arg.len() + arg.lines().count()\n    \
         let found: Bool = arg.contains(\"x\")\n    \
         let same: Bool = arg == \"x\"\n    \
         arg.parse_int()\n}",
    );
}

#[test]
fn counting_the_lines_of_a_file_checks() {
    assert_ok(
        "fn count_lines(fs: FsRead, path: Path) -> Result<Int, IoError> {\n    \
         let text = fs.read_to_string(path)?\n    Ok(text.lines().count())\n}",
    );
}
