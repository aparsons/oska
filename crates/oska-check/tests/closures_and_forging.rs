//! Capturing closures and capability forging (#40): authority cannot hide
//! inside a closure, and built-in capabilities cannot be constructed.

mod common;

use common::{assert_errors, assert_ok};

const RUN: &str = "fn run(f: fn() -> Int) -> Int {\n    f()\n}\n";

// ---- Closures ----------------------------------------------------------------

#[test]
fn a_pure_closure_can_be_passed_as_a_plain_fn() {
    assert_ok(&format!(
        "{RUN}fn f(n: Int) -> Int {{\n    run(|| n + 1)\n}}"
    ));
}

#[test]
fn a_closure_capturing_a_capability_cannot_be_passed_as_a_plain_fn() {
    let src = format!(
        "{RUN}fn f(out: Stdout) -> Int {{\n    run(|| {{\n        let _ = out.println(\"hi\")\n        1\n    }})\n}}"
    );
    assert_errors(
        &src,
        &[(
            "E0270",
            "|| {\n        let _ = out.println(\"hi\")\n        1\n    }",
        )],
    );
}

#[test]
fn a_closure_capturing_a_capability_can_be_passed_as_a_cap_fn() {
    assert_ok(
        "fn run(f: cap fn() -> Int) -> Int {\n    f()\n}\n\
         fn f(out: Stdout) -> Int {\n    run(|| {\n        let _ = out.println(\"hi\")\n        1\n    })\n}",
    );
}

#[test]
fn a_capturing_closure_bound_to_a_name_is_still_a_cap_fn() {
    let src = format!(
        "{RUN}fn f(out: Stdout) -> Int {{\n    let g = || {{\n        let _ = out.println(\"hi\")\n        1\n    }}\n    run(g)\n}}"
    );
    assert_errors(&src, &[("E0270", "g")]);
}

#[test]
fn a_cap_fn_parameter_cannot_be_passed_on_as_a_plain_fn() {
    let src = format!("{RUN}fn f(g: cap fn() -> Int) -> Int {{\n    run(g)\n}}");
    assert_errors(&src, &[("E0270", "g")]);
}

#[test]
fn capturing_a_value_that_holds_a_capability_makes_a_cap_fn() {
    let src = format!(
        "{RUN}cap struct Logger {{ out: Stdout }}\n\
         fn f(log: Logger) -> Int {{\n    run(|| {{\n        let _ = log\n        1\n    }})\n}}"
    );
    assert_errors(
        &src,
        &[("E0270", "|| {\n        let _ = log\n        1\n    }")],
    );
}

#[test]
fn capturing_a_list_of_capabilities_makes_a_cap_fn() {
    let src = format!("{RUN}fn f(outs: List<Stdout>) -> Int {{\n    run(|| outs.len())\n}}");
    assert_errors(&src, &[("E0270", "|| outs.len()")]);
}

#[test]
fn capturing_plain_data_keeps_a_closure_pure() {
    assert_ok(&format!(
        "{RUN}fn f(xs: List<Int>) -> Int {{\n    run(|| xs.len())\n}}"
    ));
}

#[test]
fn a_closure_parameter_is_not_a_capture() {
    assert_ok(
        "fn each(f: fn(Stdout) -> Int, out: Stdout) -> Int {\n    f(out)\n}\n\
         fn g(out: Stdout) -> Int {\n    each(|o| {\n        let _ = o.println(\"hi\")\n        1\n    }, out)\n}",
    );
}

#[test]
fn a_closure_capturing_a_capability_bearing_self_is_a_cap_fn() {
    let src = format!(
        "{RUN}cap struct Logger {{ out: Stdout }}\n\
         impl Logger {{\n    fn f(self) -> Int {{\n        run(|| {{\n            let _ = self\n            1\n        }})\n    }}\n}}"
    );
    assert_errors(
        &src,
        &[("E0270", "|| {\n            let _ = self\n            1\n        }")],
    );
}

#[test]
fn a_nested_capture_is_a_capture_of_the_outer_closure_too() {
    let src = format!(
        "{RUN}fn wrap(f: cap fn() -> Int) -> cap fn() -> Int {{\n    f\n}}\n\
         fn f(out: Stdout) -> Int {{\n    run(|| {{\n        let inner = wrap(|| {{\n            let _ = out\n            1\n        }})\n        inner()\n    }})\n}}"
    );
    assert_errors(
        &src,
        &[(
            "E0270",
            "|| {\n        let inner = wrap(|| {\n            let _ = out\n            1\n        })\n        inner()\n    }",
        )],
    );
}

// ---- Forging -----------------------------------------------------------------

#[test]
fn a_built_in_capability_cannot_be_constructed() {
    assert_errors(
        "fn f() -> Stdout {\n    Stdout(handle: 1)\n}",
        &[("E0271", "Stdout")],
    );
}

#[test]
fn a_prelude_type_with_private_fields_cannot_be_constructed() {
    assert_errors(
        "fn f() -> Args {\n    Args(handle: 1)\n}",
        &[("E0271", "Args")],
    );
}

#[test]
fn a_prelude_type_with_public_fields_can_be_constructed() {
    assert_ok("fn f() -> Range<Int> {\n    Range(start: 0, end: 1)\n}");
}

#[test]
fn a_private_field_of_a_prelude_type_cannot_be_read() {
    assert_errors(
        "fn f(out: Stdout) -> Int {\n    out.handle\n}",
        &[("E0272", "handle")],
    );
}

#[test]
fn a_modules_own_private_fields_are_visible_in_it() {
    assert_ok("struct Point { x: Int }\nfn f() -> Int {\n    let p = Point(x: 1)\n    p.x\n}");
}
