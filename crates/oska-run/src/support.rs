//! What the interpreter can run yet.
//!
//! Before a program starts, [`unsupported`] walks every function of the
//! checked program, its contracts included, and finds the first node the interpreter cannot run:
//! one the checked program cannot express yet ([`ExprKind::Unsupported`]),
//! a call of a function the program lacks, or a call of an intrinsic that
//! has no runtime implementation. A program that has one is
//! refused with `E0380`, so it never fails halfway through for that
//! reason.

use std::ops::ControlFlow;

use oska_check::program::{walk, Callee, Expr, ExprKind, Function, Program, Visit};
use oska_syntax::{Diagnostic, Span};

use crate::intrinsics;
use crate::panics::codes;

/// The refusal for the first node in `program`'s functions the
/// interpreter cannot run, if any.
pub fn unsupported(program: &Program) -> Option<Diagnostic> {
    refusal(program, program.functions.values().flat_map(Function::code))
}

/// The refusal for the first node in `program`'s functions or `test`
/// blocks the interpreter cannot run, if any.
pub fn unsupported_with_tests(program: &Program) -> Option<Diagnostic> {
    unsupported(program).or_else(|| refusal(program, program.tests.iter().map(|t| &t.body)))
}

fn refusal<'e>(
    program: &Program,
    bodies: impl IntoIterator<Item = &'e Expr>,
) -> Option<Diagnostic> {
    bodies
        .into_iter()
        .find_map(|body| first_in(program, body))
        .map(|(span, what)| {
            Diagnostic::new(
                codes::UNSUPPORTED,
                format!("the interpreter cannot run {what} yet"),
                span,
            )
        })
}

/// The first unsupported node in `expr`, with what it is.
fn first_in(program: &Program, expr: &Expr) -> Option<(Span, String)> {
    let found = walk(expr, &mut |visit| match visit {
        Visit::Expr(Expr {
            kind: ExprKind::Unsupported(what),
            span,
        }) => ControlFlow::Break((*span, what.clone())),
        Visit::Expr(_) => ControlFlow::Continue(()),
        Visit::Callee(target, span) => match callee(program, target, span) {
            Some(found) => ControlFlow::Break(found),
            None => ControlFlow::Continue(()),
        },
    });
    match found {
        ControlFlow::Break(found) => Some(found),
        ControlFlow::Continue(()) => None,
    }
}

/// A call of `target` at `span` that cannot run.
fn callee(program: &Program, target: &Callee, span: Span) -> Option<(Span, String)> {
    match target {
        Callee::Function(name) if !program.functions.contains_key(name) => {
            Some((span, format!("a call of `{name}`")))
        }
        Callee::Intrinsic(name) if !intrinsics::implemented(name) => {
            Some((span, format!("`{name}`")))
        }
        _ => None,
    }
}
