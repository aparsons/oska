//! Result discipline (#37): a `Result` can't be dropped silently, and `?`
//! propagates a failure only into a function that returns the same kind.

mod common;

use common::{assert_errors, assert_ok};

/// A function returning a `Result`, shared by the snippets below.
const FALLIBLE: &str = "fn fallible() -> Result<Int, IoError> { Ok(1) }\n";

fn with_fallible(src: &str) -> String {
    format!("{FALLIBLE}{src}")
}

// ---- Ignored results ---------------------------------------------------------

#[test]
fn an_ignored_result_is_an_error() {
    let src = with_fallible("fn f() {\n    fallible()\n    let x = 1\n}");
    assert_errors(&src, &[("E0240", "fallible()")]);
}

#[test]
fn let_underscore_discards_a_result_deliberately() {
    assert_ok(&with_fallible("fn f() {\n    let _ = fallible()\n}"));
}

#[test]
fn a_result_from_a_method_call_cannot_be_ignored() {
    assert_errors(
        "fn main(io: Stdout) {\n    for i in 0..3 {\n        io.println(\"{i}\")\n    }\n}",
        &[("E0240", "io.println(\"{i}\")")],
    );
}

#[test]
fn a_discarded_non_result_value_is_fine() {
    assert_ok("fn one() -> Int { 1 }\nfn f() {\n    one()\n    let x = 1\n}");
}

// ---- `?` on Result -------------------------------------------------------------

#[test]
fn try_on_a_result_with_the_same_error_type_unwraps_the_success_value() {
    assert_ok(&with_fallible(
        "fn f() -> Result<String, IoError> {\n    let n: Int = fallible()?\n    Ok(\"{n}\")\n}",
    ));
}

#[test]
fn a_propagated_result_is_not_an_ignored_one() {
    assert_ok(
        "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")?\n    Ok(())\n}",
    );
}

#[test]
fn try_on_a_result_with_a_different_error_type_is_an_error() {
    let src = with_fallible(
        "struct AppError { message: String }\n\
         fn f() -> Result<Int, AppError> {\n    let n = fallible()?\n    Ok(n)\n}",
    );
    assert_errors(&src, &[("E0242", "fallible()?")]);
}

#[test]
fn try_converts_an_error_through_an_explicit_from_impl() {
    assert_ok(&with_fallible(
        "struct AppError { message: String }\n\
         impl From<IoError> for AppError {\n    fn from(err: IoError) -> AppError {\n        AppError(message: \"io\")\n    }\n}\n\
         fn f() -> Result<Int, AppError> {\n    let n = fallible()?\n    Ok(n)\n}",
    ));
}

// ---- `?` on Option -------------------------------------------------------------

#[test]
fn try_on_an_option_in_a_function_returning_option() {
    assert_ok("fn first(args: Args) -> Option<String> {\n    let s = args.get(0)?\n    Some(s)\n}");
}

#[test]
fn try_on_an_option_in_a_function_returning_result_is_an_error() {
    let src =
        "fn first(args: Args) -> Result<String, IoError> {\n    let s = args.get(0)?\n    Ok(s)\n}";
    assert_errors(src, &[("E0243", "args.get(0)?")]);
    let report = common::check(src);
    assert!(
        report.diagnostics[0].message.contains(".ok_or(err)?"),
        "{}",
        report.diagnostics[0].message
    );
}

#[test]
fn ok_or_converts_an_option_for_try_in_a_result_function() {
    assert_ok(
        "fn first(args: Args) -> Result<String, IoError> {\n    \
         let s = args.get(0).ok_or(IoError(message: \"missing\"))?\n    Ok(s)\n}",
    );
}

#[test]
fn try_on_a_result_in_a_function_returning_option_is_an_error() {
    let src = with_fallible("fn f() -> Option<Int> {\n    let n = fallible()?\n    Some(n)\n}");
    assert_errors(&src, &[("E0243", "fallible()?")]);
}

#[test]
fn try_in_a_function_returning_neither_is_an_error() {
    let src = with_fallible("fn f() {\n    let n = fallible()?\n}");
    assert_errors(&src, &[("E0243", "fallible()?")]);
}

// ---- What `?` applies to -------------------------------------------------------

#[test]
fn try_on_a_value_that_is_neither_result_nor_option_is_an_error() {
    assert_errors(
        "fn f() -> Option<Int> {\n    let n = 5\n    let m = n?\n    Some(m)\n}",
        &[("E0241", "n?")],
    );
}

#[test]
fn try_inside_a_closure_propagates_out_of_the_closure() {
    assert_ok(
        "fn f(xs: List<Args>) -> List<Option<String>> {\n    \
         xs.map(|a| Some(a.get(0)?))\n}",
    );
    assert_errors(
        "fn f(xs: List<Args>) -> List<Result<String, IoError>> {\n    \
         xs.map(|a| Ok(a.get(0)?))\n}",
        &[("E0243", "a.get(0)?")],
    );
}
