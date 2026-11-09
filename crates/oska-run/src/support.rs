//! What the interpreter can run yet.
//!
//! Before a program starts, [`unsupported`] walks every function of the
//! checked program and finds the first node the interpreter cannot run:
//! one the checked program cannot express yet ([`ExprKind::Unsupported`]),
//! a call of a function the program lacks, or a call of an intrinsic that
//! has no runtime implementation. A program that has one is refused with
//! `E0380`, so it never fails halfway through for that reason.

use oska_check::program::{Callee, Expr, ExprKind, Part, Pattern, Program, Stmt};
use oska_syntax::{Diagnostic, Span};

use crate::intrinsics;
use crate::panics::codes;

/// The refusal for the first node in `program` the interpreter cannot
/// run, if any.
pub fn unsupported(program: &Program) -> Option<Diagnostic> {
    program
        .functions
        .values()
        .find_map(|f| first_in(program, &f.body))
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
    match &expr.kind {
        ExprKind::Unsupported => Some((expr.span, "this expression".to_string())),
        ExprKind::Unit
        | ExprKind::Str(_)
        | ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Local(_)
        | ExprKind::SelfValue => None,
        ExprKind::Interpolated(parts) => parts.iter().find_map(|part| match part {
            Part::Text(_) => None,
            Part::Hole { value, display } => {
                callee(program, display, value.span).or_else(|| first_in(program, value))
            }
        }),
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::Range {
            start: lhs,
            end: rhs,
        } => first_of(program, [lhs, rhs].map(|e| &**e)),
        ExprKind::Neg { operand, .. }
        | ExprKind::Not(operand)
        | ExprKind::Field { base: operand, .. } => first_in(program, operand),
        ExprKind::Construct { fields: items, .. } | ExprKind::List(items) => {
            first_of(program, items)
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => first_of(
            program,
            [cond, then_branch]
                .into_iter()
                .chain(else_branch)
                .map(|e| &**e),
        ),
        ExprKind::Call {
            callee: target,
            args,
        }
        | ExprKind::MutatingCall {
            callee: target,
            args,
            ..
        } => callee(program, target, expr.span).or_else(|| first_of(program, args)),
        ExprKind::Match { scrutinee, arms } => first_in(program, scrutinee).or_else(|| {
            arms.iter().find_map(|arm| {
                first_in_pattern(program, &arm.pattern)
                    .or_else(|| arm.guard.as_ref().and_then(|g| first_in(program, g)))
                    .or_else(|| first_in(program, &arm.body))
            })
        }),
        ExprKind::Block { stmts, value } => stmts
            .iter()
            .find_map(|stmt| first_in_stmt(program, stmt))
            .or_else(|| value.as_deref().and_then(|v| first_in(program, v))),
    }
}

fn first_in_pattern(program: &Program, pattern: &Pattern) -> Option<(Span, String)> {
    match pattern {
        Pattern::Wildcard | Pattern::Bind => None,
        Pattern::Literal(literal) => first_in(program, literal),
        Pattern::Construct { fields, .. } => {
            fields.iter().find_map(|f| first_in_pattern(program, f))
        }
    }
}

fn first_of<'e>(
    program: &Program,
    exprs: impl IntoIterator<Item = &'e Expr>,
) -> Option<(Span, String)> {
    exprs.into_iter().find_map(|e| first_in(program, e))
}

fn first_in_stmt(program: &Program, stmt: &Stmt) -> Option<(Span, String)> {
    match stmt {
        Stmt::Expr(value) | Stmt::Let { value, .. } | Stmt::Assign { value, .. } => {
            first_in(program, value)
        }
        Stmt::Return(value) => value.as_ref().and_then(|v| first_in(program, v)),
        Stmt::Break | Stmt::Continue => None,
        Stmt::While { cond, body } => first_in(program, cond).or_else(|| first_in(program, body)),
        Stmt::For { iter, body, .. } => first_in(program, iter).or_else(|| first_in(program, body)),
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
