//! `Bytes`, the prelude's packed buffer of bytes (ticket #222): each
//! method's signature, the mutating methods' need for a `var`, the
//! private representation, and that no program of Oska Core can reach a
//! `Bytes`.

mod common;

use common::{assert_errors, assert_ok, check};

/// Assert that `src` reports one error, `code` at `text`, whose message
/// contains `fragment`.
fn assert_error(src: &str, code: &str, text: &str, fragment: &str) {
    assert_errors(src, &[(code, text)]);
    let message = &check(src).diagnostics[0].message;
    assert!(message.contains(fragment), "{src}: {message}");
}

/// Every operation of `Bytes`, one statement each, with `b` a `Bytes`
/// held by a `var`, `n` an `Int`, `x` a `U8`, `w16`, `w32` and `w64`
/// unsigned integers, and the type each gives.
const OPERATIONS: &[(&str, &str)] = &[
    ("Bytes.new()", "Bytes"),
    ("Bytes.zeroed(n)", "Bytes"),
    ("Bytes.from_hex(\"00ff\")", "Option<Bytes>"),
    ("b.len()", "Int"),
    ("b.get(n)", "Option<U8>"),
    ("b.at(n)", "U8"),
    ("b.slice(n, n)", "Bytes"),
    ("b.read_u16_be(n)", "U16"),
    ("b.read_u16_le(n)", "U16"),
    ("b.read_u32_be(n)", "U32"),
    ("b.read_u32_le(n)", "U32"),
    ("b.read_u64_be(n)", "U64"),
    ("b.read_u64_le(n)", "U64"),
    ("b.to_hex()", "String"),
    ("b.to_text()", "Option<String>"),
    ("\"text\".to_bytes()", "Bytes"),
    ("b == b", "Bool"),
    ("b != b", "Bool"),
    ("b.hash()", "Int"),
    ("b.push(x)", "()"),
    ("b.set(n, x)", "()"),
    ("b.append(b)", "()"),
    ("b.push_u16_be(w16)", "()"),
    ("b.push_u16_le(w16)", "()"),
    ("b.push_u32_be(w32)", "()"),
    ("b.push_u32_le(w32)", "()"),
    ("b.push_u64_be(w64)", "()"),
    ("b.push_u64_le(w64)", "()"),
];

/// A program that binds what [`OPERATIONS`] use, then `body`.
fn program(body: &str) -> String {
    format!(
        "fn main() {{\n    var b = Bytes.new()\n    let n = 0\n    let x: U8 = 1\n    \
         let w16: U16 = 1\n    let w32: U32 = 1\n    let w64: U64 = 1\n{body}\n}}"
    )
}

#[test]
fn each_operation_gives_its_type() {
    for (operation, ty) in OPERATIONS {
        assert_ok(&program(&format!("    let v: {ty} = {operation}")));
    }
}

#[test]
fn a_mutating_method_needs_a_var() {
    for method in ["push(1)", "set(0, 1)", "append(b)", "push_u32_be(w32)"] {
        let src = format!(
            "fn main() {{\n    let b = Bytes.new()\n    let w32: U32 = 1\n    b.{method}\n}}"
        );
        assert_error(&src, "E0230", "b", "declared with `let`");
    }
}

#[test]
fn bytes_and_offsets_take_their_own_types() {
    // A byte is a `U8`, never an `Int`; an index or offset is an `Int`.
    assert_error(
        &program("    b.push(n)"),
        "E0220",
        "n",
        "expected `U8`, found `Int`",
    );
    assert_error(
        &program("    b.push(256)"),
        "E0222",
        "256",
        "does not fit in `U8`",
    );
    assert_error(
        &program("    let v = b.at(x)"),
        "E0220",
        "x",
        "expected `Int`, found `U8`",
    );
    assert_error(
        &program("    b.push_u32_be(w64)"),
        "E0220",
        "w64",
        "expected `U32`, found `U64`",
    );
    assert_error(
        &program("    let v: U32 = b.read_u16_be(0)"),
        "E0220",
        "b.read_u16_be(0)",
        "expected `U32`, found `U16`",
    );
    assert_error(
        &program("    b.append(\"text\")"),
        "E0220",
        "\"text\"",
        "expected `Bytes`, found `String`",
    );
}

#[test]
fn bytes_have_no_display_and_a_private_representation() {
    assert_error(
        &program("    let s = \"{b}\""),
        "E0265",
        "{b}",
        "`Bytes` does not implement `Display`",
    );
    assert_error(
        "fn main() {\n    let b = Bytes(handle: 1)\n}",
        "E0271",
        "Bytes",
        "cannot be constructed outside the prelude",
    );
    assert_error(
        &program("    let h = b.handle"),
        "E0272",
        "handle",
        "the field `handle` of `Bytes` is private",
    );
}

#[test]
fn bytes_are_eq_and_hash_in_generic_code() {
    assert_ok(
        "fn same<T: Eq>(a: T, b: T) -> Bool {\n    a == b\n}\n\n\
         fn key<T: Hash>(a: T) -> Int {\n    a.hash()\n}\n\n\
         fn main() {\n    let b = Bytes.new()\n    let s = same(b, b)\n    let k = key(b)\n}",
    );
}

/// The Lean model of Oska Core needs no change for `Bytes`: the core IR
/// export refuses a value of the type wherever it appears (it names only
/// the module's own types), so no program it accepts can reach any
/// operation of `Bytes`.
#[test]
fn no_oska_core_program_can_reach_a_bytes_operation() {
    let export = |src: &str| {
        let report = oska_check::check_module(src);
        let program = report
            .program
            .unwrap_or_else(|| panic!("{src} should check: {:#?}", report.diagnostics));
        oska_check::core_ir::export(&program)
    };
    assert!(export("fn main() {\n    let n = 0\n}").is_ok());
    let refusal = export("fn main() {\n    let b = Bytes.new()\n}")
        .expect_err("a `Bytes` is outside Oska Core");
    assert!(
        refusal.construct.contains("`Bytes"),
        "{}",
        refusal.construct
    );
    for (operation, ty) in OPERATIONS {
        assert!(
            export(&program(&format!("    let v = {operation}"))).is_err(),
            "{operation} should be outside Oska Core"
        );
        // Even where no `Bytes` is bound to a name: every operation but
        // the mutating ones, which need a place.
        if ty != &"()" {
            let alone = operation
                .split(' ')
                .map(|word| word.replacen("b.", "Bytes.new().", 1))
                .map(|word| {
                    if word == "b" {
                        "Bytes.new()".to_string()
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            let src = program(&format!("    let v = {alone}"))
                .replace("var b = Bytes.new()", "let m = 0");
            assert!(export(&src).is_err(), "{alone} should be outside Oska Core");
        }
    }
}
