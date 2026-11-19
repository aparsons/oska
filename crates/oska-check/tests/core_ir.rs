//! The core IR export (ADR 0018): a checked program in Oska Core as the
//! versioned form the Lean reference reads, and the refusal of any other
//! program, naming its first construct outside Oska Core.
//!
//! The goldens under `tests/core_ir/` pin the form. Refresh them with
//! `UPDATE_GOLDEN=1`, and only for an intentional change, which is a new
//! core IR version.

use std::ops::ControlFlow;
use std::path::PathBuf;

use oska_check::core_ir::{self, Refusal, BUILTINS, CORE_IR_VERSION};
use oska_check::program::{
    walk, DeclarationKind, Expr, ExprKind, IntType, Pattern, Program, Stmt, Type, Visit,
};
use oska_syntax::Span;

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
    refusal_of(src, &program(src))
}

/// The refusal of `program`, checked from `src` and perhaps changed
/// since, with the source text its span covers.
fn refusal_of(src: &str, program: &Program) -> (String, String) {
    match core_ir::export(program) {
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
fn structs_enums_and_match_export_to_the_golden_form() {
    assert_golden(
        "data",
        "struct Point { x: Int, y: Int }\n\
         \n\
         enum Shape {\n\
         \x20   Dot,\n\
         \x20   Square(Int),\n\
         \x20   At { p: Point, big: Bool },\n\
         }\n\
         \n\
         fn size(s: Shape) -> Int {\n\
         \x20   match s {\n\
         \x20       Shape.Dot => 0\n\
         \x20       Shape.Square(n) if n > 9 => n\n\
         \x20       Shape.Square(_) => -1\n\
         \x20       Shape.At(p: Point(x: 0, y: _), big: true) => 1\n\
         \x20       Shape.At(p: q, big: _) => q.x + q.y\n\
         \x20   }\n\
         }\n\
         \n\
         fn run() -> Int {\n\
         \x20   let p = Point(x: 1, y: 2)\n\
         \x20   size(Shape.At(p: p, big: false)) + size(Shape.Dot)\n\
         }\n",
    );
}

#[test]
fn the_export_carries_its_version_and_is_json() {
    assert_eq!(CORE_IR_VERSION, 5);
    let json = core_ir::export(&program("fn one() -> Int { 1 }\n"))
        .unwrap_or_else(|refusal| panic!("should export, refused: {refusal}"));
    let value = oska_syntax::parse_json(&json).unwrap_or_else(|e| panic!("{e}: {json}"));
    assert_eq!(
        value.get("core_ir_version").and_then(|v| v.as_u64()),
        Some(5)
    );
    assert_eq!(
        value
            .get("types")
            .and_then(|f| f.as_array())
            .map(<[_]>::len),
        Some(0)
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
fn a_struct_or_enum_outside_the_core_is_refused_at_its_declaration() {
    assert_refused(
        "struct Named { name: U8 }\n\
         fn f() -> Int { 1 }\n",
        "the type `U8`",
        "struct Named { name: U8 }",
    );
    assert_refused(
        "fn f() -> Int { 1 }\n\
         enum Maybe<T> { Nothing, Just(T) }\n",
        "the generic enum `Maybe`",
        "enum Maybe<T> { Nothing, Just(T) }",
    );
    assert_refused(
        "struct Wrap<T> { inner: T }\n\
         struct Uses { w: Wrap<Int> }\n",
        "the generic struct `Wrap`",
        "struct Wrap<T> { inner: T }",
    );
}

#[test]
fn a_prelude_type_is_refused() {
    assert_refused(
        "fn f(o: Option<Int>) -> Int {\n\
         \x20   match o {\n\
         \x20       Some(n) => n\n\
         \x20       None => 0\n\
         \x20   }\n\
         }\n",
        "the type `Option<Int>`",
        "Option<Int>",
    );
    assert_refused(
        "fn f() -> Int {\n\
         \x20   match Some(1) {\n\
         \x20       Some(n) => n\n\
         \x20       None => 0\n\
         \x20   }\n\
         }\n",
        "the enum variant `Option.Some`",
        "Some(1)",
    );
}

#[test]
fn a_string_literal_pattern_is_refused() {
    let src = "fn f(n: Int) -> Int {\n\
               \x20   match n {\n\
               \x20       1 => 2\n\
               \x20       _ => 0\n\
               \x20   }\n\
               }\n";
    let mut program = program(src);
    // The checker never puts a string literal in a pattern on an `Int`;
    // this is what a program that slipped past it would look like.
    let body = &mut program
        .functions
        .get_mut("f")
        .unwrap_or_else(|| panic!("no `f`"))
        .body;
    let ExprKind::Block {
        value: Some(value), ..
    } = &mut body.kind
    else {
        panic!("{body:?}")
    };
    let ExprKind::Match { arms, .. } = &mut value.kind else {
        panic!("{value:?}")
    };
    let Pattern::Literal(literal) = &mut arms[0].pattern else {
        panic!("{:?}", arms[0].pattern)
    };
    literal.kind = ExprKind::Str("1".to_string());
    assert_eq!(
        refusal_of(src, &program),
        ("this literal pattern".to_string(), "1".to_string())
    );
}

#[test]
fn a_trait_is_refused_even_when_nothing_uses_it() {
    assert_refused(
        "trait Named {\n\
         \x20   fn name(self) -> Int { 1 }\n\
         }\n\
         fn f() -> Int { 1 }\n",
        "the trait `Named`",
        "trait Named {\n    fn name(self) -> Int { 1 }\n}",
    );
}

#[test]
fn a_constant_is_refused() {
    assert_refused(
        "const LIMIT: Int = 3\n\
         fn f() -> Int { LIMIT }\n",
        "the constant `LIMIT`",
        "const LIMIT: Int = 3",
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
fn a_main_the_runtime_would_not_run_as_written_is_refused() {
    // `main` returning a `Result` prints its error, which Oska Core does
    // not model.
    assert_refused(
        "fn main(stdout: Stdout) -> Result<(), IoError> {\n\
         \x20   stdout.println(\"hi\")\n\
         }\n",
        "a `main` that returns `Result<(), IoError>`",
        "Result<(), IoError>",
    );
    // Only attenuation makes a directory capability: the runtime never
    // grants one to `main`. The checker refuses such a `main` (`E0217`);
    // this is what one that slipped past it would look like.
    let src = "fn main(io: Stdout) {\n\
               \x20   let _ = io.println(\"hi\")\n\
               }\n";
    let mut program = program(src);
    program
        .functions
        .get_mut("main")
        .unwrap_or_else(|| panic!("no `main`"))
        .params[0]
        .ty = Type::Named {
        name: "DirWrite".to_string(),
        args: Vec::new(),
    };
    assert_eq!(
        refusal_of(src, &program),
        (
            "a `main` that takes `DirWrite`".to_string(),
            "Stdout".to_string()
        )
    );
}

#[test]
fn a_capability_outside_the_core_is_refused_naming_its_type() {
    assert_refused(
        "fn main(stdin: Stdin) {\n\
         \x20   let _ = stdin.read_line()\n\
         }\n",
        "the type `Stdin`",
        "Stdin",
    );
}

#[test]
fn a_capability_field_is_refused_at_its_declaration() {
    assert_refused(
        "cap struct Logger { out: Stdout }\n\
         fn f() -> Int { 1 }\n",
        "the capability field type `Stdout`",
        "cap struct Logger { out: Stdout }",
    );
}

#[test]
fn equality_on_text_is_refused() {
    assert_refused(
        "fn same(a: String, b: String) -> Bool {\n\
         \x20   a == b\n\
         }\n",
        "the operator `==` on `String`",
        "a == b",
    );
}

#[test]
fn capabilities_and_their_effects_export_to_the_golden_form() {
    assert_golden(
        "capabilities",
        "fn say(io: Stdout, line: Untrusted<String>) {\n\
         \x20   let _ = io.println(line)\n\
         }\n\
         \n\
         fn main(io: Stdout, fs: Fs) {\n\
         \x20   let root = fs.restrict_to(\".\")\n\
         \x20   let made = root.write_only().create_dir(Path.from(\"box\"))\n\
         \x20   let boxed = root.restrict_to(Path.new(\"box\"))\n\
         \x20   let wrote = boxed.write_only().write(Path.from(\"a.txt\"), \"hi\")\n\
         \x20   let text = boxed.read_only().read_to_string(Path.from(\"a.txt\")).unwrap_or(\"none\")\n\
         \x20   if made.is_ok() && wrote.is_ok() {\n\
         \x20       say(io, text)\n\
         \x20   }\n\
         }\n",
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
fn an_impl_block_is_refused_with_its_type() {
    let src = "struct Counter { n: Int }\n\
               impl Counter {\n\
               \x20   fn get(self) -> Int { 0 }\n\
               }\n";
    // The struct is in Oska Core; its impl is not.
    assert_refused(
        src,
        "the `impl` of `Counter`",
        "impl Counter {\n    fn get(self) -> Int { 0 }\n}",
    );
    // Without the impl's declaration, its method is the first refusal.
    let mut program = program(src);
    program
        .declarations
        .retain(|d| d.kind != DeclarationKind::Impl);
    assert_eq!(
        refusal_of(src, &program),
        ("the method `Counter.get`".to_string(), "get".to_string())
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
fn an_interpolated_string_is_refused() {
    assert_refused(
        "fn f(n: Int) -> Int {\n\
         \x20   let s = \"n is {n}\"\n\
         \x20   1\n\
         }\n",
        "an interpolated string",
        "\"n is {n}\"",
    );
}

#[test]
fn a_let_annotation_outside_the_core_is_refused_at_the_annotation() {
    assert_refused(
        "fn f() -> Int {\n\
         \x20   let b: U8 = 7\n\
         \x20   1\n\
         }\n",
        "the type `U8`",
        "U8",
    );
    // The annotation comes before the value in the source.
    assert_refused(
        "fn f() -> Int {\n\
         \x20   let s: Option<Int> = None\n\
         \x20   1\n\
         }\n",
        "the type `Option<Int>`",
        "Option<Int>",
    );
}

/// Give `-n + 7`, the body of `f`, these integer types: the checker
/// never does for a program in Oska Core, so this is what a program that
/// slipped past it would look like.
fn set_int_types(program: &mut Program, sum: Option<IntType>, neg: IntType) {
    let body = &mut program
        .functions
        .get_mut("f")
        .unwrap_or_else(|| panic!("no `f`"))
        .body;
    let ExprKind::Block {
        value: Some(value), ..
    } = &mut body.kind
    else {
        panic!("{body:?}")
    };
    let ExprKind::Binary { lhs, int, .. } = &mut value.kind else {
        panic!("{value:?}")
    };
    *int = sum;
    let ExprKind::Neg { int, .. } = &mut lhs.kind else {
        panic!("{lhs:?}")
    };
    *int = neg;
}

#[test]
fn sized_integer_operations_are_refused() {
    let src = "fn f(n: Int) -> Int {\n\
               \x20   -n + 7\n\
               }\n";
    let mut program = program(src);
    set_int_types(&mut program, Some(IntType::U8), IntType::Int);
    assert_eq!(
        refusal_of(src, &program),
        ("the integer type `U8`".to_string(), "-n + 7".to_string())
    );
    set_int_types(&mut program, Some(IntType::Int), IntType::I32);
    assert_eq!(
        refusal_of(src, &program),
        ("the integer type `I32`".to_string(), "-n".to_string())
    );
}

#[test]
fn an_unsolved_type_is_refused() {
    let src = "fn f() -> Int { 1 }\n";
    let mut program = program(src);
    program.unsolved.push(Span { start: 16, end: 17 });
    assert_eq!(
        refusal_of(src, &program),
        (
            "a type the checker left unsolved".to_string(),
            "1".to_string()
        )
    );
}

#[test]
fn var_and_loops_are_refused_at_their_statement() {
    assert_refused(
        "fn f() -> Int {\n\
         \x20   var n = 0\n\
         \x20   n = 1\n\
         \x20   n\n\
         }\n",
        "a `var` binding",
        "var n = 0",
    );
    assert_refused(
        "fn f() -> Int {\n\
         \x20   var n = 0\n\
         \x20   n\n\
         }\n",
        "a `var` binding",
        "var n = 0",
    );
    assert_refused(
        "fn f(go: Bool) -> Int {\n\
         \x20   while go {\n\
         \x20       break\n\
         \x20   }\n\
         \x20   1\n\
         }\n",
        "a `while` loop",
        "while go {\n        break\n    }",
    );
    assert_refused(
        "fn f() -> Int {\n\
         \x20   for i in 0..3 {\n\
         \x20       continue\n\
         \x20   }\n\
         \x20   1\n\
         }\n",
        "a `for` loop",
        "for i in 0..3 {\n        continue\n    }",
    );
}

#[test]
fn break_and_continue_carry_their_keyword_span() {
    let src = "fn f(go: Bool) -> Int {\n\
               \x20   while go {\n\
               \x20       if go { break }\n\
               \x20       continue\n\
               \x20   }\n\
               \x20   1\n\
               }\n";
    let program = program(src);
    let mut found = Vec::new();
    let _ = walk(&program.functions["f"].body, &mut |visit| {
        if let Visit::Expr(Expr {
            kind: ExprKind::Block { stmts, .. },
            ..
        }) = visit
        {
            for stmt in stmts {
                if let Stmt::Break(span) | Stmt::Continue(span) = stmt {
                    found.push(&src[span.start..span.end]);
                }
            }
        }
        ControlFlow::<()>::Continue(())
    });
    found.sort_unstable();
    assert_eq!(found, ["break", "continue"]);
}

#[test]
fn return_is_refused() {
    assert_refused(
        "fn f() -> Int {\n\
         \x20   return 1\n\
         }\n",
        "`return`",
        "return 1",
    );
}

#[test]
fn closures_and_calls_of_function_values_export_to_the_golden_form() {
    assert_golden(
        "closures",
        "fn twice(f: fn(Int) -> Int, x: Int) -> Int {\n\
         \x20   f(f(x))\n\
         }\n\
         fn adder(n: Int) -> fn(Int) -> Int {\n\
         \x20   |x: Int| x + n\n\
         }\n\
         fn run(_: Bool) -> Int {\n\
         \x20   let step = 3\n\
         \x20   let add = |x: Int| x + step\n\
         \x20   let nested = |a: Int, _: Bool| {\n\
         \x20       let inner = |b: Int| a + b + step\n\
         \x20       inner(a)\n\
         \x20   }\n\
         \x20   let add_ten = adder(10)\n\
         \x20   twice(add, 1) + add_ten(2) + nested(4, true) + twice(|x| x * 2, 5)\n\
         }\n",
    );
}

/// The program of the `results` golden: `Result`, `Ok`, `Err` and their
/// patterns, `?` with and without a `From` conversion, in a function and
/// in a closure, and `is_ok` and `unwrap_or` on a `Result` of the module's
/// own types.
const RESULTS: &str = "struct AppError { code: Int }\n\
     \n\
     impl From<Int> for AppError {\n\
     \x20   fn from(code: Int) -> AppError { AppError(code: code) }\n\
     }\n\
     \n\
     fn half(n: Int) -> Result<Int, Int> {\n\
     \x20   if n % 2 == 0 { Ok(n / 2) } else { Err(n) }\n\
     }\n\
     \n\
     fn quarter(n: Int) -> Result<Int, AppError> {\n\
     \x20   let h = half(n)?\n\
     \x20   Ok(half(h)?)\n\
     }\n\
     \n\
     fn code(r: Result<Int, AppError>) -> Int {\n\
     \x20   match r {\n\
     \x20       Ok(n) => n\n\
     \x20       Err(AppError(code: c)) => 0 - c\n\
     \x20   }\n\
     }\n\
     \n\
     fn run() -> Int {\n\
     \x20   let eighth = |n: Int| {\n\
     \x20       let q = half(half(n)?)?\n\
     \x20       half(q)\n\
     \x20   }\n\
     \x20   let nested: Result<Result<Int, Int>, Bool> = Ok(eighth(8))\n\
     \x20   let inner = match nested {\n\
     \x20       Ok(Ok(n)) => n\n\
     \x20       Ok(Err(_)) => -1\n\
     \x20       Err(_) => -2\n\
     \x20   }\n\
     \x20   let fine = if quarter(12).is_ok() { 1 } else { 0 }\n\
     \x20   code(quarter(12)) + code(quarter(6)) + inner + eighth(4).unwrap_or(fine)\n\
     }\n";

#[test]
fn results_and_the_question_mark_export_to_the_golden_form() {
    assert_golden("results", RESULTS);
}

#[test]
fn a_question_mark_on_an_option_is_refused() {
    assert_refused(
        "fn first(o: Option<Int>) -> Option<Int> {\n\
         \x20   let n = o?\n\
         \x20   Some(n)\n\
         }\n",
        "the type `Option<Int>`",
        "Option<Int>",
    );
}

#[test]
fn a_result_that_holds_a_capability_or_function_is_refused() {
    assert_refused(
        "fn f(io: Stdout) -> Int {\n\
         \x20   let r: Result<Stdout, Int> = Ok(io)\n\
         \x20   1\n\
         }\n",
        "the type `Result<Stdout, Int>`, a `Result` that holds a capability or a function",
        "Result<Stdout, Int>",
    );
    assert_refused(
        "fn f() -> Int {\n\
         \x20   let r: Result<Int, Result<Int, fn() -> Int>> = Ok(1)\n\
         \x20   1\n\
         }\n",
        "the type `Result<Int, fn() -> Int>`, a `Result` that holds a capability or a function",
        "Result<Int, Result<Int, fn() -> Int>>",
    );
}

#[test]
fn a_field_of_an_io_error_is_refused() {
    assert_refused(
        "fn f(io: Stdout) -> Int {\n\
         \x20   match io.println(\"hi\") {\n\
         \x20       Ok(_) => 0\n\
         \x20       Err(e) => {\n\
         \x20           let m = e.message\n\
         \x20           1\n\
         \x20       }\n\
         \x20   }\n\
         }\n",
        "a field of `IoError`",
        "e.message",
    );
}

#[test]
fn an_impl_of_another_trait_than_from_is_refused() {
    let src = "struct AppError { code: Int }\n\
               impl Display for AppError {\n\
               \x20   fn to_string(self) -> String { \"app\" }\n\
               }\n";
    assert_refused(
        src,
        "the `impl` of `AppError`",
        "impl Display for AppError {\n    fn to_string(self) -> String { \"app\" }\n}",
    );
}

#[test]
fn a_function_used_as_a_value_is_refused() {
    assert_refused(
        "fn double(x: Int) -> Int { x * 2 }\n\
         fn f() -> Int {\n\
         \x20   let g = double\n\
         \x20   g(1)\n\
         }\n",
        "a function used as a value",
        "double",
    );
}

#[test]
fn a_function_field_is_refused_at_its_declaration() {
    assert_refused(
        "struct Holder { f: fn() -> Int }\n\
         fn f() -> Int { 1 }\n",
        "the function field type `fn() -> Int`",
        "struct Holder { f: fn() -> Int }",
    );
}

/// `checks` is whether `src` checks with no error.
fn checks(src: &str) -> bool {
    oska_check::check_module(src).program.is_some()
}

/// The programs `formal/lean/OskaFormal/Core/Examples.lean` shows the Lean
/// typing of Oska Core rejects, one per example, are ones the checker
/// rejects: a closure called with the wrong arity or argument type, a
/// call of a value that is not a function, and a closure that escapes at
/// a type it does not have. Each is beside a variant the checker accepts,
/// so the rejection is for the reason named.
#[test]
fn the_closures_the_lean_typing_rejects_the_checker_rejects() {
    let pairs = [
        (
            "fn f() -> Int {\n    let g = |x: Int| x + 1\n    g(1, 2)\n}\n",
            "fn f() -> Int {\n    let g = |x: Int| x + 1\n    g(1)\n}\n",
        ),
        (
            "fn f() -> Int {\n    let g = |x: Int| x + 1\n    g(true)\n}\n",
            "fn f() -> Int {\n    let g = |x: Int| x + 1\n    g(2)\n}\n",
        ),
        (
            "fn f() -> Int {\n    let n = 1\n    n(2)\n}\n",
            "fn f() -> Int {\n    let n = |x: Int| x\n    n(2)\n}\n",
        ),
        (
            "fn make() -> fn(Int) -> Bool {\n    |x: Int| x + 1\n}\n",
            "fn make() -> fn(Int) -> Int {\n    |x: Int| x + 1\n}\n",
        ),
    ];
    for (rejected, accepted) in pairs {
        assert!(!checks(rejected), "the checker should reject {rejected:?}");
        assert!(checks(accepted), "the checker should accept {accepted:?}");
        core_ir::export(&program(accepted))
            .unwrap_or_else(|refusal| panic!("{accepted:?} should export, refused: {refusal}"));
    }
}

/// The programs `formal/lean/OskaFormal/Core/Examples.lean` shows the Lean
/// typing of Oska Core rejects for `?`: one in a function whose result
/// type is not a `Result` (`E0243`), and one whose error type is not the
/// function's, with no `From` conversion between them (`E0242`). With the
/// conversion the checker accepts the second, and the export names the
/// `from` it chose, as the Lean example does.
#[test]
fn the_question_marks_the_lean_typing_rejects_the_checker_rejects() {
    let g = "fn g() -> Result<Int, Int> { Err(1) }\n";
    let not_result = format!("{g}fn f() -> Int {{\n    g()?\n}}\n");
    let result = format!("{g}fn f() -> Result<Int, Int> {{\n    Ok(g()?)\n}}\n");
    let app = "struct AppError { code: Int }\n";
    let from = "impl From<Int> for AppError {\n\
                \x20   fn from(value: Int) -> AppError { AppError(code: value) }\n\
                }\n";
    let f = "fn f() -> Result<Int, AppError> {\n    let x = g()?\n    Ok(x)\n}\n";
    let unconverted = format!("{g}{app}{f}");
    let converted = format!("{g}{app}{from}{f}");
    for (rejected, accepted) in [(&not_result, &result), (&unconverted, &converted)] {
        assert!(!checks(rejected), "the checker should reject {rejected:?}");
        assert!(checks(accepted), "the checker should accept {accepted:?}");
        core_ir::export(&program(accepted))
            .unwrap_or_else(|refusal| panic!("{accepted:?} should export, refused: {refusal}"));
    }
    let json = core_ir::export(&program(&converted))
        .unwrap_or_else(|refusal| panic!("should export, refused: {refusal}"));
    assert!(
        json.contains("\"convert\": \"<AppError as From<Int>>.from\"")
            && json.contains("\"name\": \"<AppError as From<Int>>.from\""),
        "the export should name and define the conversion:\n{json}"
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
        "fn b(n: Int) -> Int {\n\
         \x20   let s = \"first {n}\"\n\
         \x20   1\n\
         }\n\
         fn a(n: Int) -> Int {\n\
         \x20   let s = \"second {n}\"\n\
         \x20   2\n\
         }\n",
        "an interpolated string",
        "\"first {n}\"",
    );
}

/// The names the Lean reader's `builtin` table in
/// `formal/lean/OskaFormal/Core/CoreIR.lean` reads: each `| "NAME" =>`
/// arm between `def builtin` and its catch-all.
fn lean_builtins() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../formal/lean/OskaFormal/Core/CoreIR.lean");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let start = text
        .find("\ndef builtin :")
        .unwrap_or_else(|| panic!("{} has no `def builtin`", path.display()));
    let mut names = Vec::new();
    for line in text[start + 1..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with("| _ =>") {
            return names;
        }
        let name = line
            .strip_prefix("| \"")
            .and_then(|rest| rest.split_once('"'))
            .map(|(name, _)| name.to_string())
            .unwrap_or_else(|| panic!("unexpected line in the Lean `builtin` table: {line:?}"));
        names.push(name);
    }
    panic!(
        "the Lean `builtin` table in {} has no catch-all",
        path.display()
    )
}

#[test]
fn every_built_in_the_export_writes_is_one_the_lean_reader_reads() {
    let lean = lean_builtins();
    let missing: Vec<&str> = BUILTINS
        .iter()
        .copied()
        .filter(|name| !lean.iter().any(|l| l == name))
        .collect();
    assert!(
        missing.is_empty(),
        "the Lean reader does not read these built-ins of `BUILTINS`: {missing:?}"
    );
    let extra: Vec<&String> = lean
        .iter()
        .filter(|name| !BUILTINS.contains(&name.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "the Lean reader reads these built-ins that `BUILTINS` lacks: {extra:?}"
    );
}
