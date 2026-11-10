//! The `cap fn` join (#45, #109): a plain `fn` and a `cap fn` meeting in
//! one position join to `cap fn`, in the positions where trusted and
//! untrusted text join to untrusted: list elements, `if` and `match`
//! branches, and the arguments of a generic for one type parameter. The
//! join never turns a `cap fn` into a plain `fn`, an explicit annotation
//! still fixes the flag, and a value that has already been used as a
//! plain `fn` rejects a later `cap fn`.

mod common;

use common::{assert_authority, assert_errors, assert_ok, check};

/// A pure function, a function that calls a `cap fn`, one that calls a
/// plain `fn`, and the same for lists of them.
const FNS: &str = "fn pure() -> Int {\n    1\n}\n\
                   fn run_cap(f: cap fn() -> Int) -> Int {\n    f()\n}\n\
                   fn run(f: fn() -> Int) -> Int {\n    f()\n}\n\
                   fn all_cap(fs: List<cap fn() -> Int>) -> Int {\n    fs.len()\n}\n\
                   fn all(fs: List<fn() -> Int>) -> Int {\n    fs.len()\n}\n";

/// A closure that captures `out`, so it is a `cap fn`.
const CAPTURING: &str = "|| {\n        let _ = out\n        2\n    }";

/// `FNS`, then `fn f(out: Stdout, c: Bool) -> Int` with `body`.
fn program(body: &str) -> String {
    format!("{FNS}fn f(out: Stdout, c: Bool) -> Int {{\n{body}\n}}")
}

/// The message of the only diagnostic `src` reports, which must have
/// `code`.
fn only_message(src: &str, code: &str) -> String {
    let report = check(src);
    match report.diagnostics.as_slice() {
        [d] if d.code == code => d.message.clone(),
        other => panic!("{src:?} should report one {code}, found {other:#?}"),
    }
}

/// Assert that `value`, bound in `body`, has the type `ty`, as the
/// mismatch message for using it as an `Int` shows.
fn assert_type(body: &str, value: &str, ty: &str) {
    let src = program(&format!("{body}\n    let n: Int = {value}\n    n"));
    let message = only_message(&src, "E0220");
    assert!(
        message.contains(&format!("found `{ty}`")),
        "{value} should be `{ty}`: {message}"
    );
}

// ---- Lists -------------------------------------------------------------------

#[test]
fn a_plain_fn_and_a_cap_fn_in_one_list_join_to_a_list_of_cap_fn() {
    let body = format!("    let fs = [pure, {CAPTURING}]");
    assert_type(&body, "fs", "List<cap fn() -> Int>");
}

#[test]
fn the_join_does_not_depend_on_the_order_of_the_elements() {
    let body = format!("    let fs = [{CAPTURING}, pure, || 3]");
    assert_type(&body, "fs", "List<cap fn() -> Int>");
}

#[test]
fn a_list_of_only_plain_fns_stays_plain() {
    assert_type("    let fs = [pure, || 3]", "fs", "List<fn() -> Int>");
    assert_ok(&program("    all([pure, || 3])"));
}

#[test]
fn a_joined_list_may_be_passed_as_a_list_of_cap_fn() {
    assert_ok(&program(&format!(
        "    let fs = [pure, {CAPTURING}]\n    all_cap(fs)"
    )));
}

#[test]
fn a_joined_list_cannot_be_passed_as_a_list_of_plain_fn() {
    let src = program(&format!("    let fs = [pure, {CAPTURING}]\n    all(fs)"));
    assert_errors(&src, &[("E0270", "fs")]);
}

#[test]
fn an_annotation_fixes_the_list_as_plain() {
    let src = program(&format!(
        "    let fs: List<fn() -> Int> = [pure, {CAPTURING}]\n    fs.len()"
    ));
    assert_errors(&src, &[("E0270", CAPTURING)]);
}

#[test]
fn an_element_of_a_joined_list_is_a_cap_fn() {
    let src = program(&format!(
        "    let fs = [pure, {CAPTURING}]\n    var n = 0\n    for g in fs {{\n        n = n + run(g)\n    }}\n    n"
    ));
    assert_errors(&src, &[("E0270", "g")]);
    assert_ok(&program(&format!(
        "    let fs = [pure, {CAPTURING}]\n    var n = 0\n    for g in fs {{\n        n = n + run_cap(g)\n    }}\n    n"
    )));
}

#[test]
fn an_element_taken_out_of_a_joined_list_is_a_cap_fn() {
    let src = program(&format!(
        "    let fs = [pure, {CAPTURING}]\n    match fs.get(0) {{\n        Some(g) => run(g)\n        None => 0\n    }}"
    ));
    assert_errors(&src, &[("E0270", "g")]);
}

#[test]
fn a_list_of_lists_joins_inside_its_type_argument() {
    let body = format!("    let fss = [[pure], [{CAPTURING}]]");
    assert_type(&body, "fss", "List<List<cap fn() -> Int>>");
    let src = format!(
        "{FNS}fn nested(fss: List<List<fn() -> Int>>) -> Int {{\n    fss.len()\n}}\n\
         fn f(out: Stdout) -> Int {{\n    let fss = [[pure], [{CAPTURING}]]\n    nested(fss)\n}}"
    );
    assert_errors(&src, &[("E0270", "fss")]);
}

// ---- `if` and `match` branches -----------------------------------------------

#[test]
fn if_branches_of_a_plain_fn_and_a_cap_fn_join_to_cap_fn() {
    let body = format!("    let g = if c {{ pure }} else {{ {CAPTURING} }}");
    assert_type(&body, "g", "cap fn() -> Int");
    let src = program(&format!("{body}\n    run(g)"));
    assert_errors(&src, &[("E0270", "g")]);
    assert_ok(&program(&format!("{body}\n    run_cap(g)")));
}

#[test]
fn closures_returned_from_if_branches_join_to_cap_fn() {
    let body = format!("    let g = if c {{ || 1 }} else {{ {CAPTURING} }}");
    assert_type(&body, "g", "cap fn() -> Int");
    let body = format!("    let g = if c {{ {CAPTURING} }} else {{ || 1 }}");
    assert_type(&body, "g", "cap fn() -> Int");
}

#[test]
fn match_arms_of_a_plain_fn_and_a_cap_fn_join_to_cap_fn() {
    let body = format!(
        "    let g = match c {{\n        true => pure\n        false => {CAPTURING}\n    }}"
    );
    assert_type(&body, "g", "cap fn() -> Int");
    let src = program(&format!("{body}\n    run(g)"));
    assert_errors(&src, &[("E0270", "g")]);
}

#[test]
fn branches_join_inside_an_option() {
    let body = format!("    let o = if c {{ Some(pure) }} else {{ Some({CAPTURING}) }}");
    assert_type(&body, "o", "Option<cap fn() -> Int>");
    let src = format!(
        "{FNS}fn maybe(o: Option<fn() -> Int>) -> Int {{\n    0\n}}\n\
         fn f(out: Stdout, c: Bool) -> Int {{\n{body}\n    maybe(o)\n}}"
    );
    assert_errors(&src, &[("E0270", "o")]);
}

#[test]
fn a_branch_checked_against_a_plain_fn_return_type_is_e0270() {
    let src = format!(
        "{FNS}fn pick(out: Stdout, c: Bool) -> fn() -> Int {{\n    if c {{ pure }} else {{ {CAPTURING} }}\n}}"
    );
    assert_errors(&src, &[("E0270", CAPTURING)]);
}

// ---- Generic arguments -------------------------------------------------------

const PICK: &str = "fn pick<T>(a: T, b: T) -> T {\n    a\n}\n";

#[test]
fn generic_arguments_for_one_parameter_join_to_cap_fn() {
    let body = format!("    let g = pick(pure, {CAPTURING})");
    let src = format!("{PICK}{}", program(&format!("{body}\n    run_cap(g)")));
    assert_ok(&src);
    let src = format!("{PICK}{}", program(&format!("{body}\n    run(g)")));
    assert_errors(&src, &[("E0270", "g")]);
}

#[test]
fn generic_arguments_join_inside_type_arguments() {
    let src = format!(
        "{PICK}{}",
        program(&format!(
            "    let fs = pick([pure], [{CAPTURING}])\n    all(fs)"
        ))
    );
    assert_errors(&src, &[("E0270", "fs")]);
    let src = format!(
        "{PICK}{}",
        program(&format!(
            "    let o = pick(Some(pure), Some({CAPTURING}))\n    match o {{\n        Some(g) => run(g)\n        None => 0\n    }}"
        ))
    );
    assert_errors(&src, &[("E0270", "g")]);
}

#[test]
fn a_generic_identity_does_not_launder_a_joined_cap_fn() {
    let src = format!(
        "fn id<T>(x: T) -> T {{\n    x\n}}\n{}",
        program(&format!(
            "    let g = if c {{ pure }} else {{ {CAPTURING} }}\n    run(id(g))"
        ))
    );
    assert_errors(&src, &[("E0270", "g")]);
}

// ---- Every route out keeps the `cap fn` --------------------------------------

#[test]
fn a_joined_cap_fn_cannot_be_returned_as_a_plain_fn() {
    let src = format!(
        "{FNS}fn make(out: Stdout, c: Bool) -> fn() -> Int {{\n    let g = if c {{ pure }} else {{ {CAPTURING} }}\n    g\n}}"
    );
    assert_errors(&src, &[("E0270", "g")]);
    assert_ok(&format!(
        "{FNS}fn make(out: Stdout, c: Bool) -> cap fn() -> Int {{\n    let g = if c {{ pure }} else {{ {CAPTURING} }}\n    g\n}}"
    ));
}

#[test]
fn a_joined_list_cannot_be_stored_in_a_plain_fn_field() {
    let src = format!(
        "{FNS}struct Handlers {{ fs: List<fn() -> Int> }}\n\
         fn f(out: Stdout) -> Handlers {{\n    let fs = [pure, {CAPTURING}]\n    Handlers(fs: fs)\n}}"
    );
    assert_errors(&src, &[("E0270", "fs")]);
}

#[test]
fn a_joined_cap_fn_cannot_leave_through_question_mark_as_a_plain_fn() {
    let src = format!(
        "{FNS}fn f(out: Stdout, c: Bool) -> Result<fn() -> Int, IoError> {{\n    let r = if c {{ Ok(pure) }} else {{ Ok({CAPTURING}) }}\n    let g = r?\n    Ok(g)\n}}"
    );
    assert_errors(&src, &[("E0270", "g")]);
}

#[test]
fn a_closure_capturing_a_joined_cap_fn_is_a_cap_fn() {
    let src = program(&format!(
        "    let g = if c {{ pure }} else {{ {CAPTURING} }}\n    run(|| g())"
    ));
    assert_errors(&src, &[("E0270", "|| g()")]);
}

#[test]
fn calling_a_joined_list_needs_cap_fn_in_the_signature() {
    let src = "fn call_all(fs: List<cap fn() -> Int>) -> Int {\n    var n = 0\n    for g in fs {\n        n = n + g()\n    }\n    n\n}";
    assert_ok(src);
    assert_authority(src, "call_all", &[], false);
}

// ---- A flag already used as a plain `fn` ------------------------------------

#[test]
fn a_list_already_passed_as_plain_fns_rejects_a_later_cap_fn() {
    let src = program(&format!(
        "    var fs = [pure]\n    let n = all(fs)\n    fs = [{CAPTURING}]\n    n"
    ));
    assert_errors(&src, &[("E0270", CAPTURING)]);
}

#[test]
fn a_list_not_yet_used_as_plain_fns_joins_on_assignment() {
    let src = program(&format!(
        "    var fs = [pure]\n    fs = [{CAPTURING}]\n    all_cap(fs)"
    ));
    assert_ok(&src);
    let src = program(&format!(
        "    var fs = [pure]\n    fs = [{CAPTURING}]\n    all(fs)"
    ));
    assert_errors(&src, &[("E0270", "fs")]);
}

#[test]
fn a_mutating_method_cannot_add_a_cap_fn_to_a_list_of_plain_fns() {
    let src = program(&format!(
        "    var fs: List<fn() -> Int> = [pure]\n    fs.push({CAPTURING})\n    fs.len()"
    ));
    assert_errors(&src, &[("E0270", CAPTURING)]);
    let src = program(&format!(
        "    var fs = [pure]\n    let n = all(fs)\n    fs.push({CAPTURING})\n    n"
    ));
    assert_errors(&src, &[("E0270", CAPTURING)]);
}

#[test]
fn a_mutating_method_on_an_unpinned_list_joins() {
    let src = program(&format!(
        "    var fs = [pure]\n    fs.push({CAPTURING})\n    all(fs)"
    ));
    assert_errors(&src, &[("E0270", "fs")]);
}

#[test]
fn a_mutating_method_cannot_add_untrusted_text_to_a_list_of_trusted_text() {
    assert_errors(
        "fn f(u: Untrusted<String>) -> List<String> {\n    var xs: List<String> = []\n    xs.push(u)\n    xs\n}",
        &[("E0300", "u")],
    );
}

#[test]
fn a_closure_that_read_a_flag_as_plain_fixes_it_across_loop_iterations() {
    // The closure is a plain `fn` because `fs` holds no `cap fn` yet; on
    // the loop's second pass it would capture one.
    let src = program(&format!(
        "    var fs = [pure]\n    var n = 0\n    for i in 0..2 {{\n        n = n + run(|| all_cap(fs))\n        fs = [{CAPTURING}]\n    }}\n    n"
    ));
    assert_errors(&src, &[("E0270", CAPTURING)]);
}

#[test]
fn a_contract_that_read_a_flag_as_plain_fixes_it_across_loop_iterations() {
    let src = format!(
        "{FNS}fn f(out: Stdout) -> Int\n    requires {{\n        var g = if true {{ pure }} else {{ pure }}\n        var ok = true\n        for i in 0..2 {{\n            ok = g() > 0\n            g = {CAPTURING}\n        }}\n        ok\n    }}\n{{\n    1\n}}"
    );
    // The call read `g` as a plain `fn`, so it is pure; on the loop's
    // second pass it would call a `cap fn`.
    assert_errors(&src, &[("E0270", CAPTURING)]);
}

// ---- More positions ----------------------------------------------------------

#[test]
fn a_generic_struct_joins_its_fields_for_one_parameter() {
    let src = format!(
        "{FNS}struct Pair<T> {{ a: T, b: T }}\n\
         fn take(p: Pair<fn() -> Int>) -> Int {{\n    0\n}}\n\
         fn f(out: Stdout) -> Int {{\n    let p = Pair(a: pure, b: {CAPTURING})\n    take(p)\n}}"
    );
    assert_errors(&src, &[("E0270", "p")]);
}

#[test]
fn a_join_in_parameter_position_accepts_only_what_every_branch_accepts() {
    // One branch accepts only a plain `fn`, so the joined function does
    // too: a `cap fn` argument could reach the branch that cannot take it.
    let body = format!(
        "    let h = if c {{ |g: fn() -> Int| g() }} else {{ |g: cap fn() -> Int| g() }}\n    h({CAPTURING})"
    );
    assert_errors(&program(&body), &[("E0270", CAPTURING)]);
    assert_ok(&program(
        "    let h = if c { |g: fn() -> Int| g() } else { |g: cap fn() -> Int| g() }\n    h(pure)",
    ));
}
