//! `match` and patterns (#38): patterns are checked against the
//! scrutinee, uppercase names are variants (ADR 0005), and every `match`
//! is exhaustive with no unreachable arms.

mod common;

use common::{assert_errors, assert_fix, assert_ok};

// ---- Pattern types -----------------------------------------------------------

#[test]
fn a_pattern_binding_takes_its_type_from_the_scrutinee() {
    assert_ok(
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(n) => n\n        None => 0\n    }\n}",
    );
    assert_errors(
        "fn f(o: Option<Int>) -> String {\n    match o {\n        Some(n) => n\n        None => \"\"\n    }\n}",
        &[("E0220", "n")],
    );
}

#[test]
fn a_pattern_of_the_wrong_type_is_a_mismatch() {
    assert_errors(
        "fn f(n: Int) -> Int {\n    match n {\n        Some(x) => x\n        _ => 0\n    }\n}",
        &[("E0220", "Some(x)")],
    );
    assert_errors(
        "fn f(n: Int) -> Int {\n    match n {\n        \"a\" => 1\n        _ => 0\n    }\n}",
        &[("E0220", "\"a\"")],
    );
}

// ---- Capitalization (ADR 0005) --------------------------------------------

const SHAPE: &str = "enum Shape {\n    Circle { radius: Int },\n    Square(Int),\n    Empty,\n}\n";

#[test]
fn a_misspelled_variant_is_an_error_not_a_binding() {
    let src = "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(x) => x\n        Nnoe => 0\n    }\n}";
    assert_errors(src, &[("E0250", "Nnoe")]);
    assert_fix(
        src,
        "E0250",
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(x) => x\n        None => 0\n    }\n}",
    );
}

#[test]
fn an_uppercase_name_inside_a_pattern_is_a_variant_too() {
    assert_errors(
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(X) => 1\n        _ => 0\n    }\n}",
        &[("E0250", "X")],
    );
}

#[test]
fn a_failed_pattern_still_binds_its_names() {
    assert_errors(
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Smoe(x) => x\n        _ => 0\n    }\n}",
        &[("E0250", "Smoe")],
    );
}

#[test]
fn a_bare_variant_of_a_user_enum_suggests_its_qualified_name() {
    let src = format!(
        "{SHAPE}fn f(s: Shape) -> Int {{\n    match s {{\n        Empty => 0\n        _ => 1\n    }}\n}}"
    );
    assert_errors(&src, &[("E0250", "Empty")]);
    assert_fix(
        &src,
        "E0250",
        &src.replace("        Empty =>", "        Shape.Empty =>"),
    );
}

#[test]
fn a_misspelled_qualified_variant_is_an_error() {
    let src = format!(
        "{SHAPE}fn f(s: Shape) -> Int {{\n    match s {{\n        Shape.Emtpy => 0\n        _ => 1\n    }}\n}}"
    );
    assert_errors(&src, &[("E0250", "Emtpy")]);
    assert_fix(&src, "E0250", &src.replace("Emtpy", "Empty"));
}

#[test]
fn a_local_name_must_start_lowercase() {
    assert_errors("fn f() {\n    let Total = 1\n}", &[("E0214", "Total")]);
    assert_errors("fn f() {\n    var Count = 1\n}", &[("E0214", "Count")]);
    assert_errors(
        "fn f(xs: List<Int>) {\n    for Item in xs {\n    }\n}",
        &[("E0214", "Item")],
    );
    assert_ok("fn f() {\n    let total = 1\n    let _unused = 2\n}");
}

#[test]
fn a_lowercase_name_in_a_pattern_is_a_binding() {
    assert_ok(&format!(
        "{SHAPE}fn f(s: Shape) -> Int {{\n    match s {{\n        Shape.Square(side) => side\n        other => 0\n    }}\n}}"
    ));
}

// ---- Constructor patterns -----------------------------------------------------

#[test]
fn a_constructor_pattern_matches_the_variant_fields() {
    assert_ok(&format!(
        "{SHAPE}fn f(s: Shape) -> Int {{\n    match s {{\n        Shape.Circle(radius: r) => r\n        Shape.Square(side) => side\n        Shape.Empty => 0\n    }}\n}}"
    ));
    assert_ok(
        "struct Point { x: Int, y: Int }\nfn f(p: Point) -> Int {\n    match p {\n        Point(x: a, y: b) => a + b\n    }\n}",
    );
}

#[test]
fn a_constructor_pattern_with_the_wrong_number_of_fields_is_an_error() {
    assert_errors(
        &format!("{SHAPE}fn f(s: Shape) -> Int {{\n    match s {{\n        Shape.Square(a, b) => a\n        _ => 0\n    }}\n}}"),
        &[("E0251", "Shape.Square(a, b)")],
    );
    assert_errors(
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some => 1\n        None => 0\n    }\n}",
        &[("E0251", "Some")],
    );
}

#[test]
fn a_named_field_must_be_matched_with_its_label() {
    let src = format!(
        "{SHAPE}fn f(s: Shape) -> Int {{\n    match s {{\n        Shape.Circle(r) => r\n        _ => 0\n    }}\n}}"
    );
    assert_errors(&src, &[("E0252", "r")]);
    assert_fix(
        &src,
        "E0252",
        &src.replace("Circle(r)", "Circle(radius: r)"),
    );
    let wrong = src.replace("Circle(r)", "Circle(radios: r)");
    assert_errors(&wrong, &[("E0252", "radios")]);
    assert_fix(
        &wrong,
        "E0252",
        &src.replace("Circle(r)", "Circle(radius: r)"),
    );
}

// ---- Exhaustiveness -------------------------------------------------------------

#[test]
fn a_match_missing_a_variant_is_an_error_with_a_fix() {
    let src = "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(x) => x\n    }\n}";
    assert_errors(src, &[("E0253", "o")]);
    assert_fix(
        src,
        "E0253",
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(x) => x\n        None => todo()\n    }\n}",
    );
}

#[test]
fn a_nested_non_exhaustive_match_lists_the_missing_patterns() {
    let src = "fn f(r: Result<Option<Int>, String>) -> Int {\n    match r {\n        Ok(Some(x)) => x\n        Err(e) => 0\n    }\n}";
    assert_errors(src, &[("E0253", "r")]);
    let fixed = "fn f(r: Result<Option<Int>, String>) -> Int {\n    match r {\n        Ok(Some(x)) => x\n        Err(e) => 0\n        Ok(None) => todo()\n    }\n}";
    assert_fix(src, "E0253", fixed);
    assert_ok(fixed);
}

#[test]
fn missing_named_fields_are_written_with_their_labels() {
    let src = format!(
        "{SHAPE}fn f(s: Shape) -> Int {{\n    match s {{\n        Shape.Empty => 0\n    }}\n}}"
    );
    assert_fix(
        &src,
        "E0253",
        &src.replace(
            "        Shape.Empty => 0\n",
            "        Shape.Empty => 0\n        Shape.Circle(radius: _) => todo()\n        Shape.Square(_) => todo()\n",
        ),
    );
}

#[test]
fn a_match_with_every_variant_is_exhaustive() {
    assert_ok(
        "fn f(r: Result<Option<Int>, String>) -> Int {\n    match r {\n        Ok(Some(x)) => x\n        Ok(None) => 0\n        Err(e) => 1\n    }\n}",
    );
    assert_ok(
        "fn f(b: Bool) -> Int {\n    match b {\n        true => 1\n        false => 0\n    }\n}",
    );
}

#[test]
fn guards_do_not_count_towards_exhaustiveness() {
    let src = "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(x) if x > 0 => x\n        None => 0\n    }\n}";
    assert_errors(src, &[("E0253", "o")]);
    assert_fix(
        src,
        "E0253",
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(x) if x > 0 => x\n        None => 0\n        Some(_) => todo()\n    }\n}",
    );
    assert_ok(
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(x) if x > 0 => x\n        Some(x) => 0 - x\n        None => 0\n    }\n}",
    );
}

#[test]
fn a_literal_match_needs_a_wildcard_or_binding_arm() {
    let src = "fn f(n: Int) -> String {\n    match n {\n        0 => \"zero\"\n        1 => \"one\"\n    }\n}";
    assert_errors(src, &[("E0253", "n")]);
    assert_fix(
        src,
        "E0253",
        "fn f(n: Int) -> String {\n    match n {\n        0 => \"zero\"\n        1 => \"one\"\n        _ => todo()\n    }\n}",
    );
    assert_errors(
        "fn f(s: String) -> Int {\n    match s {\n        \"a\" => 1\n    }\n}",
        &[("E0253", "s")],
    );
    assert_ok("fn f(n: Int) -> String {\n    match n {\n        0 => \"zero\"\n        other => \"{other}\"\n    }\n}");
    assert_ok(
        "fn f(s: String) -> Int {\n    match s {\n        \"a\" => 1\n        _ => 0\n    }\n}",
    );
}

#[test]
fn a_match_with_no_arms_gets_arms_for_every_variant() {
    let src = "fn f(o: Option<Int>) -> Int {\n    match o {}\n}";
    assert_fix(
        src,
        "E0253",
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(_) => todo()\n        None => todo()\n    }\n}",
    );
}

// ---- Unreachable arms -----------------------------------------------------------

#[test]
fn an_arm_after_a_wildcard_or_binding_is_unreachable() {
    assert_errors(
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        _ => 0\n        Some(x) => x\n    }\n}",
        &[("E0254", "Some(x)")],
    );
    assert_errors(
        "fn f(n: Int) -> Int {\n    match n {\n        m => m\n        0 => 1\n    }\n}",
        &[("E0254", "0")],
    );
}

#[test]
fn an_arm_covered_by_earlier_arms_is_unreachable() {
    assert_errors(
        "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(x) => x\n        None => 0\n        Some(1) => 1\n    }\n}",
        &[("E0254", "Some(1)")],
    );
    assert_errors(
        "fn f(n: Int) -> Int {\n    match n {\n        1 => 1\n        1 => 2\n        _ => 0\n    }\n}",
        &[("E0254", "1")],
    );
}

#[test]
fn an_arm_after_a_guarded_wildcard_is_reachable() {
    assert_ok(
        "fn f(n: Int) -> Int {\n    match n {\n        m if m > 0 => m\n        _ => 0\n    }\n}",
    );
}
