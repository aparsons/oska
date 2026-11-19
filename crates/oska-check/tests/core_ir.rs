//! The core IR export (ADR 0018): a checked program in Oska Core as the
//! versioned form the Lean reference reads, and the refusal of any other
//! program, naming its first construct outside Oska Core.
//!
//! The goldens under `tests/core_ir/` pin the form. Refresh them with
//! `UPDATE_GOLDEN=1`, and only for an intentional change, which is a new
//! core IR version.

use std::path::PathBuf;

use oska_check::core_ir::{self, Refusal, CORE_IR_VERSION};
use oska_check::program::Program;

fn program(src: &str) -> Program {
    let report = oska_check::check_module(src);
    report
        .program
        .unwrap_or_else(|| panic!("{src} should check: {:#?}", report.diagnostics))
}

/// Compare the export of `src` with the golden `tests/core_ir/<name>.json`.
fn assert_golden(name: &str, src: &str) {
    let actual = core_ir::export(&program(src))
        .unwrap_or_else(|refusal| panic!("{src} should export, refused: {refusal}"));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("core_ir")
        .join(format!("{name}.json"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}; refresh with UPDATE_GOLDEN=1", path.display()));
    assert_eq!(
        actual, expected,
        "core IR of {name} differs from its golden"
    );
}

/// The refusal of `src`, with the source text its span covers.
fn refusal(src: &str) -> (String, String) {
    match core_ir::export(&program(src)) {
        Ok(json) => panic!("{src} should be refused, exported:\n{json}"),
        Err(Refusal { construct, span }) => (construct, src[span.start..span.end].to_string()),
    }
}

fn assert_refused(src: &str, construct: &str, text: &str) {
    assert_eq!(
        refusal(src),
        (construct.to_string(), text.to_string()),
        "refusal of {src:?}"
    );
}

#[test]
fn arithmetic_and_calls_export_to_the_golden_form() {
    assert_golden(
        "arithmetic",
        "fn add(a: Int, b: Int) -> Int {\n\
         \x20   a + b\n\
         }\n\
         \n\
         fn run() -> Int {\n\
         \x20   let x = add(1, -2)\n\
         \x20   let _ = x * 3\n\
         \x20   -(x - 4) / 2 % 5\n\
         }\n",
    );
}

#[test]
fn branches_and_booleans_export_to_the_golden_form() {
    assert_golden(
        "branches",
        "fn sign(n: Int) -> Int {\n\
         \x20   if n < 0 {\n\
         \x20       -1\n\
         \x20   } else if n == 0 {\n\
         \x20       0\n\
         \x20   } else {\n\
         \x20       1\n\
         \x20   }\n\
         }\n\
         \n\
         fn between(lo: Int, n: Int, hi: Int, _: Bool) -> Bool {\n\
         \x20   let ok = lo <= n && n <= hi\n\
         \x20   !(ok || false) implies n != hi\n\
         }\n\
         \n\
         fn nothing(flag: Bool) {\n\
         \x20   if flag {\n\
         \x20       sign(1)\n\
         \x20   }\n\
         }\n",
    );
}

#[test]
fn recursion_exports_to_the_golden_form() {
    assert_golden(
        "recursion",
        "fn fact(n: Int) -> Int {\n\
         \x20   if n <= 1 { 1 } else { n * fact(n - 1) }\n\
         }\n\
         \n\
         fn run() -> Int {\n\
         \x20   fact(5)\n\
         }\n",
    );
}

#[test]
fn the_export_carries_its_version_and_is_json() {
    assert_eq!(CORE_IR_VERSION, 1);
    let json = core_ir::export(&program("fn one() -> Int { 1 }\n"))
        .unwrap_or_else(|refusal| panic!("should export, refused: {refusal}"));
    let value = oska_syntax::parse_json(&json).unwrap_or_else(|e| panic!("{e}: {json}"));
    assert_eq!(
        value.get("core_ir_version").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        value
            .get("functions")
            .and_then(|f| f.as_array())
            .map(<[_]>::len),
        Some(1)
    );
}

#[test]
fn a_struct_is_refused_at_its_first_use() {
    assert_refused(
        "struct Point { x: Int }\n\
         fn origin() -> Int {\n\
         \x20   let p = Point(x: 0)\n\
         \x20   0\n\
         }\n",
        "the struct `Point`",
        "Point(x: 0)",
    );
}

#[test]
fn a_generic_function_is_refused_at_its_name() {
    assert_refused(
        "fn id<T>(x: T) -> T { x }\n",
        "the generic function `id`",
        "id",
    );
}

#[test]
fn a_capability_is_refused_naming_its_type() {
    assert_refused(
        "fn main(stdout: Stdout) -> Result<(), IoError> {\n\
         \x20   stdout.println(\"hi\")\n\
         }\n",
        "the type `Stdout`",
        "main",
    );
}

#[test]
fn a_built_in_call_is_refused_at_the_call() {
    assert_refused(
        "fn f(n: Int) -> Int {\n\
         \x20   let s = n.to_string()\n\
         \x20   n\n\
         }\n",
        "the built-in `Int.to_string`",
        "n.to_string()",
    );
}

#[test]
fn a_method_is_refused_at_its_name() {
    assert_refused(
        "struct Counter { n: Int }\n\
         impl Counter {\n\
         \x20   fn zero() -> Int { 0 }\n\
         }\n",
        "the associated function `Counter.zero`",
        "zero",
    );
    assert_refused(
        "struct Counter { n: Int }\n\
         impl Counter {\n\
         \x20   fn get(self) -> Int { 0 }\n\
         }\n",
        "the method `Counter.get`",
        "get",
    );
}

#[test]
fn a_contract_is_refused_at_its_clause() {
    assert_refused(
        "fn half(n: Int) -> Int\n\
         \x20   requires n >= 0\n\
         {\n\
         \x20   n / 2\n\
         }\n",
        "a contract",
        "requires n >= 0",
    );
}

#[test]
fn a_string_is_refused() {
    assert_refused(
        "fn f() -> Int {\n\
         \x20   let s = \"hi\"\n\
         \x20   1\n\
         }\n",
        "a string",
        "\"hi\"",
    );
}

#[test]
fn a_sized_integer_is_refused() {
    assert_refused(
        "fn f() -> Int {\n\
         \x20   let b: U8 = 7\n\
         \x20   1\n\
         }\n",
        "the integer type `U8`",
        "7",
    );
}

#[test]
fn a_loop_and_assignment_are_refused() {
    assert_refused(
        "fn f() -> Int {\n\
         \x20   var n = 0\n\
         \x20   n = 1\n\
         \x20   n\n\
         }\n",
        "an assignment",
        "1",
    );
    assert_refused(
        "fn f(go: Bool) -> Int {\n\
         \x20   while go {\n\
         \x20       break\n\
         \x20   }\n\
         \x20   1\n\
         }\n",
        "a `while` loop",
        "go",
    );
}

#[test]
fn return_closures_and_match_are_refused() {
    assert_refused(
        "fn f() -> Int {\n\
         \x20   return 1\n\
         }\n",
        "`return`",
        "return 1",
    );
    assert_refused(
        "fn f() -> Int {\n\
         \x20   let g = |x: Int| x\n\
         \x20   1\n\
         }\n",
        "a closure",
        "|x: Int| x",
    );
    assert_refused(
        "fn f(b: Bool) -> Int {\n\
         \x20   match b {\n\
         \x20       true => 1\n\
         \x20       false => 0\n\
         \x20   }\n\
         }\n",
        "`match`",
        "match b {\n        true => 1\n        false => 0\n    }",
    );
}

#[test]
fn imports_and_tests_are_refused() {
    assert_refused(
        "use std.sql\n\
         fn f() -> Int { 1 }\n",
        "the import of `std.sql`",
        "std.sql",
    );
    assert_refused(
        "fn f() -> Int { 1 }\n\
         test \"one\" {\n\
         \x20   t.assert(f() == 1)\n\
         }\n",
        "a `test` block",
        "test \"one\" {\n    t.assert(f() == 1)\n}",
    );
}

#[test]
fn the_first_construct_in_source_order_is_named() {
    // `b` sorts after `a` by name but comes first in the source.
    assert_refused(
        "fn b() -> Int {\n\
         \x20   let s = \"first\"\n\
         \x20   1\n\
         }\n\
         fn a() -> Int {\n\
         \x20   let s = \"second\"\n\
         \x20   2\n\
         }\n",
        "a string",
        "\"first\"",
    );
}
