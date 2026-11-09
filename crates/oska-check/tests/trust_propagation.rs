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

// ---- Joins: trusted and untrusted text together is untrusted ----------

#[test]
fn if_branches_of_trusted_then_untrusted_text_are_untrusted() {
    let body = "fn f(args: Args, c: Bool) -> Option<String> {\n    let arg = args.get(1)?\n    \
                let pick = if c { \"default\" } else { arg }\n    Some(greet(pick))\n}";
    assert_errors(&with_greet(body), &[("E0300", "pick")]);
}

#[test]
fn if_branches_of_trusted_then_untrusted_text_fit_an_untrusted_slot() {
    assert_ok(
        "fn f(args: Args, c: Bool) -> Option<Untrusted<String>> {\n    let arg = args.get(1)?\n    \
         let pick = if c { \"default\" } else { arg }\n    Some(pick)\n}",
    );
}

#[test]
fn match_arms_of_trusted_then_untrusted_text_are_untrusted() {
    let body = "fn f(args: Args, c: Bool) -> Option<String> {\n    let arg = args.get(1)?\n    \
                let pick = match c {\n        true => \"default\"\n        false => arg\n    }\n    Some(greet(pick))\n}";
    assert_errors(&with_greet(body), &[("E0300", "pick")]);
}

#[test]
fn a_generic_function_joins_its_arguments() {
    let body = "fn first<T>(a: T, b: T) -> T {\n    a\n}\n\
                fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    \
                let picked = first(\"default\", arg)\n    Some(greet(picked))\n}";
    assert_errors(&with_greet(body), &[("E0300", "picked")]);
}

#[test]
fn a_generic_call_in_a_trusted_slot_rejects_an_untrusted_argument() {
    let body = "fn first<T>(a: T, b: T) -> T {\n    a\n}\n\
                fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    \
                Some(greet(first(\"default\", arg)))\n}";
    assert_errors(&with_greet(body), &[("E0300", "arg")]);
}

#[test]
fn a_generic_function_preserves_trust() {
    let body = "fn id<T>(x: T) -> T {\n    x\n}\n\
                fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    \
                let same = id(arg)\n    Some(greet(same))\n}";
    assert_errors(&with_greet(body), &[("E0300", "same")]);
}

#[test]
fn a_list_that_receives_untrusted_text_holds_untrusted_text() {
    let body = "fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    \
                var names = List.new()\n    names.push(\"default\")\n    names.push(arg)\n    \
                Some(greet(names.get(0)?))\n}";
    assert_errors(&with_greet(body), &[("E0300", "names.get(0)?")]);
}

#[test]
fn a_list_literal_of_mixed_trust_is_untrusted() {
    assert_errors(
        "fn f(args: Args) -> Option<List<String>> {\n    let arg = args.get(1)?\n    \
         let names = [\"default\", arg]\n    Some(names)\n}",
        &[("E0300", "names")],
    );
}

#[test]
fn text_that_flowed_into_a_trusted_slot_rejects_untrusted_text_later() {
    let body = "fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    \
                var names = List.new()\n    names.push(\"default\")\n    \
                let name = greet(names.get(0)?)\n    names.push(arg)\n    Some(name)\n}";
    assert_errors(&with_greet(body), &[("E0300", "arg")]);
}

#[test]
fn trusted_text_compares_with_untrusted_text() {
    assert_ok(
        "fn f(args: Args, name: String) -> Option<Bool> {\n    let arg = args.get(1)?\n    \
         Some(name == arg)\n}",
    );
}
