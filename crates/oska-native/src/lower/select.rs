//! `ExprKind::Select`, the choice `ct_select`, `and` and `or` on a secret
//! lower to (#224): every operand is evaluated, and no branch decides
//! which value is taken.
//!
//! A `Bool` or an integer is a word, so the choice is arithmetic on
//! words: with `mask` all ones when the condition holds (`0 - 1`) and
//! zero when it does not, `if_false ^ ((if_true ^ if_false) & mask)`.
//! Two `Bytes` are picked byte by byte the same way into a new buffer, by
//! `std.rt`'s `bytes_select`, which gives the word 0 for buffers of
//! different lengths, for the generated code to panic (`P0042`).
//!
//! The constant-time guarantee of native executables (that no secret
//! value reaches a branch or an address after optimization) is #225's.

use oska_check::program::{Callee, Expr, ExprKind, Type};
use oska_syntax::Diagnostic;

use super::Body;
use crate::ir::{BinOp, Temp};

impl Body<'_, '_> {
    /// `cond`, `if_true` and `if_false`, all evaluated, and the value of
    /// `if_true` when `cond` holds, otherwise of `if_false`.
    pub(super) fn select(
        &mut self,
        expr: &Expr,
        cond: &Expr,
        if_true: &Expr,
        if_false: &Expr,
    ) -> Result<Temp, Diagnostic> {
        if !matches!(expr.ty, Type::Bool | Type::Int(_)) {
            let call = Expr {
                kind: ExprKind::Call {
                    callee: Callee::Intrinsic("Bytes.ct_select".to_string()),
                    type_args: Vec::new(),
                    args: vec![cond.clone(), if_true.clone(), if_false.clone()],
                },
                ty: expr.ty.clone(),
                span: expr.span,
            };
            return self.expr(&call);
        }
        let pick = self.expr(cond)?;
        let a = self.expr(if_true)?;
        let b = self.expr(if_false)?;
        let zero = self.constant(0);
        let mask = self.binary_op(BinOp::Sub, zero, pick);
        let differ = self.binary_op(BinOp::Xor, a, b);
        let kept = self.binary_op(BinOp::And, differ, mask);
        Ok(self.binary_op(BinOp::Xor, b, kept))
    }
}
