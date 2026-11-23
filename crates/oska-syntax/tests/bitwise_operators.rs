// SPDX-License-Identifier: Apache-2.0
//! Bitwise and shift operators (ticket #220): `& | ^ ~ << >>`, their
//! precedence, `E0108` where one meets a different binary operator
//! without parentheses, `>>` still closing nested generic arguments, and
//! `|` as a closure in prefix position and bitwise or in infix position.

mod common;

use common::module_err;
use oska_syntax::ast::{ExprKind, ItemKind, StmtKind};
use oska_syntax::{parse_module, Applicability, Diagnostic, Span};

/// The expression `expr` as the parser grouped it: every operation in
/// parentheses.
fn grouped(expr: &str) -> String {
    let src = format!("fn f() {{\n    let v = {expr}\n}}");
    let module = parse_module(&src).unwrap_or_else(|e| panic!("{src:?} should parse: {e}"));
    let ItemKind::Fn(f) = &module.items[0].kind else {
        panic!("expected a function");
    };
    let StmtKind::Let { value, .. } = &f.body.as_ref().expect("a body").stmts[0].kind else {
        panic!("expected a `let`");
    };
    show(value)
}

fn show(expr: &oska_syntax::ast::Expr) -> String {
    match &expr.kind {
        ExprKind::Name(ident) => ident.name.clone(),
        ExprKind::Int(digits) => digits.clone(),
        ExprKind::Unary { op, operand } => format!("({}{})", op.text(), show(operand)),
        ExprKind::Binary { op, lhs, rhs } => {
            format!("({} {} {})", show(lhs), op.text(), show(rhs))
        }
        ExprKind::Range { start, end } => format!("({}..{})", show(start), show(end)),
        ExprKind::Closure { params, body } => {
            let names: Vec<&str> = params.iter().map(|p| p.name.name.as_str()).collect();
            format!("(|{}| {})", names.join(", "), show(body))
        }
        ExprKind::Call { callee, args } => {
            let args: Vec<String> = args.iter().map(|a| show(&a.value)).collect();
            format!("{}({})", show(callee), args.join(", "))
        }
        ExprKind::Field { base, name } => format!("{}.{}", show(base), name.name),
        other => panic!("not shown: {other:?}"),
    }
}

/// Apply every edit of `err`'s fix to `src`.
fn apply_fix(src: &str, err: &Diagnostic) -> String {
    let fix = err
        .fix
        .as_ref()
        .unwrap_or_else(|| panic!("{} should carry a fix", err.code));
    assert_eq!(fix.applicability, Applicability::Machine, "{err:?}");
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut out = src.to_string();
    for edit in edits {
        out.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    out
}

// ---- Each operator ------------------------------------------------------------

#[test]
fn each_operator_parses_as_its_binary_or_unary_node() {
    assert_eq!(grouped("a & b"), "(a & b)");
    assert_eq!(grouped("a | b"), "(a | b)");
    assert_eq!(grouped("a ^ b"), "(a ^ b)");
    assert_eq!(grouped("a << 3"), "(a << 3)");
    assert_eq!(grouped("a >> 3"), "(a >> 3)");
    assert_eq!(grouped("~a"), "(~a)");
    assert_eq!(grouped("~~a"), "(~(~a))");
}

#[test]
fn a_chain_of_one_associative_operator_needs_no_parentheses_and_groups_left() {
    assert_eq!(grouped("a & b & c"), "((a & b) & c)");
    assert_eq!(grouped("a | b | c"), "((a | b) | c)");
    assert_eq!(grouped("a ^ b ^ c"), "((a ^ b) ^ c)");
    assert_eq!(grouped("(a << 1) << 2"), "((a << 1) << 2)");
    assert_eq!(grouped("a >> (1 >> 2)"), "(a >> (1 >> 2))");
}

#[test]
fn prefix_operators_bind_tighter_than_bitwise_ones() {
    assert_eq!(grouped("~a & b"), "((~a) & b)");
    assert_eq!(grouped("a ^ ~b"), "(a ^ (~b))");
    assert_eq!(grouped("~x.rotl(3)"), "(~x.rotl(3))");
}

#[test]
fn parentheses_make_any_mix_legal() {
    assert_eq!(grouped("(x & mask) == 0"), "((x & mask) == 0)");
    assert_eq!(grouped("(a + b) << 3"), "((a + b) << 3)");
    assert_eq!(grouped("a + (b << 3)"), "(a + (b << 3))");
    assert_eq!(grouped("(a & b) | c"), "((a & b) | c)");
    assert_eq!(grouped("a | (b ^ c)"), "(a | (b ^ c))");
    assert_eq!(grouped("(a << b) >> c"), "((a << b) >> c)");
    assert_eq!(grouped("(a | b) == c && d"), "(((a | b) == c) && d)");
    assert_eq!(grouped("0..(n & 3)"), "(0..(n & 3))");
}

#[test]
fn operators_without_bitwise_ones_keep_their_precedence() {
    assert_eq!(grouped("a + b * c"), "(a + (b * c))");
    assert_eq!(grouped("a < b && c"), "((a < b) && c)");
}

// ---- E0108: mixing without parentheses ---------------------------------------------

/// The `E0108` that `let v = {expr}` reports: its span covers `spanned`,
/// and its fix rewrites the expression as `fixed`.
fn assert_mixed(expr: &str, spanned: &str, fixed: &str) {
    let src = format!("fn f() {{\n    let v = {expr}\n}}");
    let err = module_err(&src);
    assert_eq!(err.code, "E0108", "{expr}: {err:?}");
    let start = src.find(spanned).expect("the span's text is in the source");
    assert_eq!(
        err.span,
        Span {
            start,
            end: start + spanned.len()
        },
        "{expr}: {err:?}"
    );
    assert_eq!(
        apply_fix(&src, &err),
        format!("fn f() {{\n    let v = {fixed}\n}}"),
        "{expr}"
    );
}

#[test]
fn a_bitwise_operator_in_a_comparison_needs_parentheses() {
    assert_mixed("x & mask == 0", "x & mask == 0", "(x & mask) == 0");
    assert_mixed("0 != x | y", "0 != x | y", "0 != (x | y)");
    assert_mixed("x >> 4 < y", "x >> 4 < y", "(x >> 4) < y");
}

#[test]
fn arithmetic_in_a_shift_or_bitwise_operand_needs_parentheses() {
    assert_mixed("a + b << 3", "a + b << 3", "(a + b) << 3");
    assert_mixed("a << b - 1", "a << b - 1", "a << (b - 1)");
    assert_mixed("a * b & c", "a * b & c", "(a * b) & c");
    assert_mixed("a + b << c + d", "a + b << c + d", "(a + b) << (c + d)");
}

#[test]
fn two_different_bitwise_operators_need_parentheses() {
    assert_mixed("a & b | c", "a & b | c", "(a & b) | c");
    assert_mixed("a | b ^ c", "a | b ^ c", "a | (b ^ c)");
    assert_mixed("a ^ b & c", "a ^ b & c", "a ^ (b & c)");
    assert_mixed("a << b >> c", "a << b >> c", "(a << b) >> c");
    assert_mixed("a & b << 2", "a & b << 2", "a & (b << 2)");
}

#[test]
fn a_chain_of_one_shift_needs_parentheses_since_shifts_do_not_associate() {
    assert_mixed("a << 1 << 2", "a << 1 << 2", "(a << 1) << 2");
    assert_mixed("a >> 1 >> 2", "a >> 1 >> 2", "(a >> 1) >> 2");
    let err = module_err("fn f() {\n    let v = a << 1 << 2\n}");
    assert_eq!(
        err.message,
        "`<<` does not chain without parentheses; \
         write `(a << 1) << 2` to say which applies first"
    );
}

#[test]
fn a_bitwise_operator_beside_logic_or_a_range_needs_parentheses() {
    assert_mixed("a & b && c", "a & b && c", "(a & b) && c");
    assert_mixed("c || a | b", "c || a | b", "c || (a | b)");
    assert_mixed("0..n & 3", "0..n & 3", "0..(n & 3)");
}

#[test]
fn the_innermost_mix_is_reported_first() {
    // `b + c << 1` is the first operation the parser finishes.
    assert_mixed("a == b + c << 1", "b + c << 1", "a == (b + c) << 1");
    // The fixed source still mixes `<<` and `==`, which a second fix
    // settles.
    assert_mixed(
        "a == (b + c) << 1",
        "a == (b + c) << 1",
        "a == ((b + c) << 1)",
    );
}

#[test]
fn every_fixed_expression_parses_and_keeps_rusts_grouping() {
    assert_eq!(grouped("(x & mask) == 0"), "((x & mask) == 0)");
    assert_eq!(grouped("(a + b) << (c + d)"), "((a + b) << (c + d))");
    assert_eq!(grouped("a | (b ^ c)"), "(a | (b ^ c))");
}

#[test]
fn the_message_names_both_operators_and_the_fixed_expression() {
    let err = module_err("fn f() {\n    let v = x & mask == 0\n}");
    assert_eq!(
        err.message,
        "`&` cannot be mixed with `==` without parentheses; \
         write `(x & mask) == 0` to say which applies first"
    );
}

// ---- `>>` and generics, `|` and closures ------------------------------------------

#[test]
fn nested_generic_arguments_still_close_with_two_angle_brackets() {
    let src = "fn f(x: List<List<U8>>) -> List<List<U8>> {\n    \
               let y: List<List<U8>> = x\n    \
               let g = |v: List<List<U8>>| v\n    \
               y\n}";
    assert!(parse_module(src).is_ok(), "{:?}", parse_module(src).err());
}

#[test]
fn angle_brackets_apart_are_not_a_shift() {
    let err = module_err("fn f() {\n    let v = a > > b\n}");
    assert_eq!(err.code, "E0100", "{err:?}");
    let err = module_err("fn f() {\n    let v = a < < b\n}");
    assert_eq!(err.code, "E0100", "{err:?}");
}

#[test]
fn a_pipe_in_prefix_position_starts_a_closure_and_in_infix_position_is_or() {
    assert_eq!(grouped("|x| x | 1"), "(|x| (x | 1))");
    assert_eq!(grouped("a | b"), "(a | b)");
    assert_eq!(grouped("f(|x| x, a | b)"), "f((|x| x), (a | b))");
    assert_eq!(grouped("|| a ^ b"), "(|| (a ^ b))");
}

#[test]
fn the_new_punctuation_lexes() {
    assert_eq!(grouped("a&b"), "(a & b)");
    assert_eq!(grouped("a^b"), "(a ^ b)");
    assert_eq!(grouped("a<<b"), "(a << b)");
    assert_eq!(grouped("a>>b"), "(a >> b)");
    // `&&` stays logical and.
    assert_eq!(grouped("a && b"), "(a && b)");
}
