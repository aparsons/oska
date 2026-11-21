//! Secrets and Oska Core (#224): secrets are outside Oska Core, so the
//! core IR export refuses every program that uses one, and the Lean model
//! is unchanged. The new refusals (`E0353` to `E0356`) only reject
//! programs in which a value of a `Secret` type meets a condition, an
//! index, an operator or a comparison, and no type in Oska Core is a
//! `Secret`, so every Oska Core program is typed and run as before. These
//! tests pin both halves: a secret type, and the choice without a branch
//! that `ct_select` lowers to, are each refused by the export.

use oska_check::core_ir::{self, Refusal};
use oska_check::program::{Expr, ExprKind, Program};

fn program(src: &str) -> Program {
    let report = oska_check::check_module(src);
    report
        .program
        .unwrap_or_else(|| panic!("{src} should check: {:#?}", report.diagnostics))
}

/// The refusal of `program`, checked from `src` and perhaps changed
/// since, with the source text its span covers.
fn refusal_of(src: &str, program: &Program) -> (String, String) {
    match core_ir::export(program) {
        Ok(json) => panic!("{src} should be refused, exported:\n{json}"),
        Err(Refusal { construct, span }) => (construct, src[span.start..span.end].to_string()),
    }
}

#[test]
fn a_secret_integer_is_outside_oska_core() {
    let src = "fn f(k: Secret<U32>) -> Int {\n    let x = k ^ 1\n    0\n}\n";
    let (construct, _) = refusal_of(src, &program(src));
    assert!(
        construct.contains("Secret<U32>"),
        "the refusal should name the secret type: {construct}"
    );
}

#[test]
fn a_secret_bool_is_outside_oska_core() {
    let src = "fn f(c: Secret<Bool>) -> Int {\n    0\n}\n";
    let (construct, _) = refusal_of(src, &program(src));
    assert!(
        construct.contains("Secret<Bool>"),
        "the refusal should name the secret type: {construct}"
    );
}

#[test]
fn a_choice_without_a_branch_is_outside_oska_core() {
    let src = "fn f(c: Bool, a: Int, b: Int) -> Int {\n    if c { a } else { b }\n}\n";
    let mut program = program(src);
    core_ir::export(&program).expect("the `if` is in Oska Core");
    // The same choice as `ct_select` makes it: every operand evaluated.
    let body = &mut program.functions.get_mut("f").expect("f").body;
    let ExprKind::Block {
        value: Some(value), ..
    } = &mut body.kind
    else {
        panic!("a block with a value: {body:#?}");
    };
    let ExprKind::If {
        cond,
        then_branch,
        else_branch: Some(else_branch),
    } = value.kind.clone()
    else {
        panic!("an `if`: {value:#?}");
    };
    value.kind = ExprKind::Select {
        cond,
        if_true: then_branch,
        if_false: else_branch,
    };
    let Expr { span, .. } = **value;
    let (construct, text) = refusal_of(src, &program);
    assert_eq!(construct, "a choice without a branch (`ct_select`)");
    assert_eq!(text, &src[span.start..span.end]);
}
