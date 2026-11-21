//! The operators on secrets, lowered to the public operators on the
//! values inside them (see [`crate::program::secrets`]).

use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

use super::super::secrets::{conceal, int_name, reveal, secret_inner};
use super::super::{Callee, Expr, ExprKind, Type};
use super::Lowering;

impl Lowering<'_> {
    /// `kind`, of type `ty`, with an operator on secrets lowered to the
    /// public operator on the values inside them. Any other is returned
    /// as it is.
    pub(super) fn secret(&self, kind: ExprKind, ty: &Type, span: Span) -> ExprKind {
        let Some(inner) = secret_inner(ty).cloned() else {
            return kind;
        };
        match kind {
            ExprKind::Binary { op, lhs, rhs, int } => {
                let (lhs, rhs) = (reveal(*lhs), reveal(*rhs));
                let value = match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        let method = match op {
                            BinaryOp::Add => "wrapping_add",
                            BinaryOp::Sub => "wrapping_sub",
                            _ => "wrapping_mul",
                        };
                        let name = int_name(&inner).unwrap_or("U64");
                        Expr {
                            kind: ExprKind::Call {
                                callee: Callee::Intrinsic(format!("{name}.{method}")),
                                type_args: Vec::new(),
                                args: vec![lhs, rhs],
                            },
                            ty: inner,
                            span,
                        }
                    }
                    _ => Expr {
                        kind: ExprKind::Binary {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                            int,
                        },
                        ty: inner,
                        span,
                    },
                };
                conceal(value)
            }
            ExprKind::BitNot { operand, int } => conceal(Expr {
                kind: ExprKind::BitNot {
                    operand: Box::new(reveal(*operand)),
                    int,
                },
                ty: inner,
                span,
            }),
            kind => kind,
        }
    }
}
