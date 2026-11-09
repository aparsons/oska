//! Declaration rules (#35): `cap` on capability-bearing types, `main` as
//! the root of authority, capitalization (ADR 0005), recursive types, and
//! `dbg` under `--release`.

mod common;

use common::{assert_errors, assert_fix, assert_ok};
use oska_check::{check_module_with, CheckOptions};

// ---- `cap` -------------------------------------------------------------

#[test]
fn a_struct_holding_a_capability_must_be_declared_cap() {
    assert_errors(
        "struct Logger { out: Stdout }",
        &[("E0210", "Logger")],
    );
}

#[test]
fn the_missing_cap_fix_inserts_cap() {
    assert_fix(
        "pub struct Logger { out: Stdout }",
        "E0210",
        "pub cap struct Logger { out: Stdout }",
    );
}

#[test]
fn an_enum_holding_a_capability_must_be_declared_cap() {
    let src = "enum Sink {\n    Out(Stdout),\n    Nowhere,\n}";
    assert_errors(src, &[("E0210", "Sink")]);
    assert_fix(
        src,
        "E0210",
        "cap enum Sink {\n    Out(Stdout),\n    Nowhere,\n}",
    );
}

#[test]
fn a_cap_struct_may_hold_a_capability() {
    assert_ok("cap struct Logger { out: Stdout }");
    assert_ok("cap enum Sink {\n    Out(Stdout),\n    Nowhere,\n}");
}

#[test]
fn a_field_of_a_cap_type_or_cap_fn_needs_cap() {
    assert_errors(
        "cap struct Logger { out: Stdout }\nstruct App { log: Logger }",
        &[("E0210", "App")],
    );
    assert_errors("struct Hook { run: cap fn(Int) }", &[("E0210", "Hook")]);
}

#[test]
fn a_capability_inside_a_written_type_argument_needs_cap() {
    assert_errors(
        "struct Fanout { outs: List<Stdout> }",
        &[("E0210", "Fanout")],
    );
}

#[test]
fn generic_containers_and_plain_data_need_no_cap() {
    assert_ok("struct Box<T> { value: T }\nfn f(b: Box<Stdout>) { }");
    assert_ok("struct Scores { all: List<Int>, best: Option<Int>, hook: fn(Int) -> Int }");
}

// ---- `main` ------------------------------------------------------------

#[test]
fn main_may_take_built_in_capabilities_and_args() {
    assert_ok("fn main(io: Stdout, fs: FsRead, net: Net, args: Args) { }");
    assert_ok("fn main() -> () { }");
}

#[test]
fn main_parameters_must_be_capabilities_or_args() {
    assert_errors(
        "cap struct Logger { out: Stdout }\nfn main(log: Logger, n: Int, outs: List<Stdout>) { }",
        &[
            ("E0211", "Logger"),
            ("E0211", "Int"),
            ("E0211", "List<Stdout>"),
        ],
    );
}

#[test]
fn a_misspelled_main_parameter_is_reported_once() {
    assert_errors("fn main(io: Stdot) { }", &[("E0200", "Stdot")]);
}

#[test]
fn main_may_return_a_result_whose_error_is_display() {
    assert_ok("fn main(io: Stdout) -> Result<(), IoError> { }");
    assert_ok("fn main() -> Result<(), String> { }");
    assert_ok(
        "struct AppError { message: String }\n\
         impl Display for AppError {\n    fn to_string(self) -> String { self.message }\n}\n\
         fn main() -> Result<(), AppError> { }",
    );
}

#[test]
fn main_must_return_unit_or_a_result_of_unit() {
    assert_errors("fn main() -> Int { 0 }", &[("E0212", "Int")]);
    assert_errors(
        "fn main() -> Result<Int, String> { }",
        &[("E0212", "Result<Int, String>")],
    );
}

#[test]
fn mains_error_type_must_be_display() {
    assert_errors(
        "struct AppError { message: String }\nfn main() -> Result<(), AppError> { }",
        &[("E0212", "AppError")],
    );
}

// ---- Capitalization (ADR 0005) -----------------------------------------

#[test]
fn well_capitalized_declarations_check() {
    assert_ok(
        "enum Shape<T> {\n    Circle { radius: T },\n    Empty,\n}\n\
         trait Area {\n    fn area(self) -> Int\n}\n\
         fn total_area(shape_count: Int, _unused: Int) -> Int { shape_count }",
    );
}

#[test]
fn types_variants_traits_and_type_parameters_start_uppercase() {
    assert_errors(
        "struct point { x: Int }\n\
         enum Shape {\n    circle,\n}\n\
         trait area {\n    fn area(self) -> Int\n}\n\
         fn first<t>(xs: List<t>) { }",
        &[
            ("E0213", "point"),
            ("E0213", "circle"),
            ("E0213", "area"),
            ("E0213", "t"),
        ],
    );
}

#[test]
fn functions_fields_and_parameters_start_lowercase() {
    assert_errors(
        "struct Point { X: Int }\n\
         enum Shape {\n    Circle { Radius: Int },\n}\n\
         impl Point {\n    fn Norm(self) -> Int { 0 }\n}\n\
         fn Area(Width: Int) -> Int { 0 }",
        &[
            ("E0214", "X"),
            ("E0214", "Radius"),
            ("E0214", "Norm"),
            ("E0214", "Area"),
            ("E0214", "Width"),
        ],
    );
}

// ---- Recursive types ---------------------------------------------------

#[test]
fn recursion_through_option_or_an_enum_needs_no_boxing() {
    assert_ok("struct Node { value: Int, next: Option<Node> }");
    assert_ok("enum Tree {\n    Leaf,\n    Branch(Tree, Tree),\n}");
    assert_ok("struct Forest { trees: List<Forest> }");
    assert_ok(
        "struct Wrap<T> { inner: T }\n\
         struct Twice { value: Wrap<Wrap<Int>> }",
    );
}

#[test]
fn a_recursive_type_with_no_base_case_is_an_error() {
    assert_errors("struct Loop { me: Loop }", &[("E0215", "Loop")]);
    assert_errors(
        "enum Chain {\n    Link(Int, Chain),\n}",
        &[("E0215", "Chain")],
    );
}

#[test]
fn mutual_recursion_with_no_base_case_reports_each_type() {
    assert_errors(
        "struct Ping { pong: Pong }\nstruct Pong { ping: Ping }",
        &[("E0215", "Ping"), ("E0215", "Pong")],
    );
}

#[test]
fn recursion_through_a_generic_struct_has_no_base_case() {
    assert_errors(
        "struct Wrap<T> { inner: T }\nstruct Loop { me: Wrap<Loop> }",
        &[("E0215", "Loop")],
    );
}

// ---- `dbg` under `--release` --------------------------------------------

/// `(code, spanned text)` for each diagnostic of `src` checked with
/// `--release`.
fn release_errors(src: &str) -> Vec<(&'static str, &str)> {
    check_module_with(src, CheckOptions { release: true })
        .diagnostics
        .iter()
        .map(|d| (d.code, &src[d.span.start..d.span.end]))
        .collect()
}

const DBG: &str = "fn twice(n: Int) -> Int {\n    let m = dbg(n * 2)\n    if m > 4 {\n        dbg(m)\n    }\n    m\n}\n\
                   struct Point { x: Int }\n\
                   impl Point {\n    fn show(self) -> Int { dbg(self.x) }\n}";

#[test]
fn dbg_is_allowed_outside_release() {
    assert_ok(DBG);
}

#[test]
fn dbg_is_an_error_under_release() {
    assert_eq!(
        release_errors(DBG),
        [
            ("E0216", "dbg(n * 2)"),
            ("E0216", "dbg(m)"),
            ("E0216", "dbg(self.x)"),
        ]
    );
}

#[test]
fn dbg_in_tests_is_an_error_under_release() {
    assert_eq!(
        release_errors("test \"debugging\" {\n    dbg(1)\n}"),
        [("E0216", "dbg(1)")]
    );
}

#[test]
fn only_the_free_function_main_is_a_root_of_authority() {
    assert_ok("struct App { n: Int }\nimpl App {\n    fn main(self, n: Int) -> Int { n }\n}");
}
