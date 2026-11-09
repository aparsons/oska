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

#[test]
fn to_string_of_untrusted_text_is_untrusted() {
    assert_errors(
        &with_greet(
            "fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    Some(greet(arg.to_string()))\n}",
        ),
        &[("E0300", "arg.to_string()")],
    );
}

#[test]
fn to_string_of_a_non_text_value_is_trusted() {
    assert_ok(&with_greet(
        "fn f(n: Int) -> String {\n    greet(n.to_string())\n}",
    ));
}

#[test]
fn a_method_that_is_not_trust_polymorphic_rejects_an_untrusted_receiver() {
    assert_errors(
        "fn f(args: Args, fs: Fs) -> Option<Fs> {\n    let dir = args.get(1)?\n    Some(fs.restrict_to(dir))\n}",
        &[("E0300", "dir")],
    );
}

#[test]
fn user_code_cannot_declare_a_trust_polymorphic_function() {
    assert_errors(
        "#[follows_trust]\nfn f(s: String) -> String {\n    s\n}",
        &[("E0132", "follows_trust")],
    );
}

// ---- `Untrusted<...>` is only for text ---------------------------------

#[test]
fn untrusted_of_a_non_text_type_is_an_error() {
    assert_errors(
        "fn f(n: Untrusted<Int>) -> Int {\n    1\n}",
        &[("E0320", "Untrusted<Int>")],
    );
}

#[test]
fn untrusted_of_a_container_is_an_error() {
    assert_errors(
        "struct Form { names: Untrusted<List<String>> }",
        &[("E0320", "Untrusted<List<String>>")],
    );
}

#[test]
fn untrusted_of_a_generic_parameter_is_an_error() {
    assert_errors(
        "fn f<T>(x: Untrusted<T>) -> Int {\n    1\n}",
        &[("E0320", "Untrusted<T>")],
    );
}

#[test]
fn untrusted_of_a_non_text_type_in_a_let_is_an_error() {
    assert_errors(
        "fn f() -> Int {\n    let n: Untrusted<Int> = 1\n    1\n}",
        &[("E0320", "Untrusted<Int>")],
    );
}

#[test]
fn untrusted_text_inside_a_container_is_fine() {
    assert_ok("fn f(names: List<Untrusted<String>>) -> Int {\n    names.len()\n}");
}

// ---- Interpolation ------------------------------------------------------

#[test]
fn interpolating_untrusted_text_makes_the_string_untrusted() {
    assert_errors(
        &with_greet(
            "fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    Some(greet(\"hello {arg}\"))\n}",
        ),
        &[("E0300", "\"hello {arg}\"")],
    );
}

#[test]
fn interpolating_an_untrusted_field_makes_the_string_untrusted() {
    assert_errors(
        "struct Form { name: Untrusted<String> }\n\
         fn f(form: Form) -> String {\n    \"hello {form.name}\"\n}",
        &[("E0300", "\"hello {form.name}\"")],
    );
}

#[test]
fn interpolating_trusted_values_gives_trusted_text() {
    assert_ok(&with_greet(
        "fn f(name: String, n: Int) -> String {\n    greet(\"{name} has {n}\")\n}",
    ));
}

#[test]
fn an_interpolated_string_of_untrusted_text_fits_a_data_position() {
    assert_ok(
        "fn f(io: Stdout, args: Args) -> Result<(), IoError> {\n    \
         let arg = args.get(1).ok_or(IoError(message: \"usage\"))?\n    \
         io.println(\"hello {arg}\")\n}",
    );
}

// ---- Other routes -------------------------------------------------------

#[test]
fn an_untrusted_field_cannot_be_passed_as_trusted_text() {
    assert_errors(
        &with_greet(
            "struct Form { name: Untrusted<String> }\n\
             fn f(form: Form) -> String {\n    greet(form.name)\n}",
        ),
        &[("E0300", "form.name")],
    );
}

#[test]
fn untrusted_text_in_an_option_stays_untrusted_through_question_mark() {
    assert_errors(
        &with_greet(
            "fn f(args: Args) -> Option<String> {\n    let found = Some(args.get(1)?)\n    Some(greet(found?))\n}",
        ),
        &[("E0300", "found?")],
    );
}

#[test]
fn untrusted_text_stays_untrusted_through_a_list() {
    assert_errors(
        &with_greet(
            "fn f(args: Args) -> Option<String> {\n    let all = [args.get(1)?]\n    Some(greet(all.get(0)?))\n}",
        ),
        &[("E0300", "all.get(0)?")],
    );
}

#[test]
fn untrusted_text_mapped_through_a_list_stays_untrusted() {
    assert_errors(
        &with_greet(
            "fn f(args: Args) -> Option<List<String>> {\n    let arg = args.get(1)?\n    \
             Some(arg.lines().map(|line| line.trim()))\n}",
        ),
        &[("E0300", "line.trim()")],
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
