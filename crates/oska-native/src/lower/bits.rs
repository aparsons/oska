// SPDX-License-Identifier: Apache-2.0
//! The bitwise and shift operators and the rotations of `U8`, `U32` and
//! `U64`.
//!
//! A value of these types is held zero-extended in its word, so `&`, `|`
//! and `^` are one word operation each, and `>>` is a logical shift. `<<`
//! and `~` are masked back to the type's width afterwards (not for
//! `U64`, which fills the word). The word shifts take their amount
//! modulo 64, and aarch64 and x86_64 would mask a narrower type's amount
//! differently again, so every shift and rotation compares its amount with
//! the type's width first and panics with `P0008` outside 0 to the width
//! less 1, with the interpreter's message (`oska-run`'s `ops.rs`): the
//! hardware's masking never shows. An amount is compared unsigned, so a
//! negative rotation amount, an `Int`, is out of range too.
//!
//! A rotation by `n` is `(x << n) | (x >> (width - n))`, masked, or the
//! mirror image for `rotr`. When `n` is 0 the second shift is by the
//! width: for `U64` that is 64, taken modulo 64 as 0, so both halves are
//! `x`; for a narrower type it shifts every bit of `x` out. Either way the
//! result is `x`.

use oska_check::program::{Expr, IntType, Type};
use oska_syntax::ast::BinaryOp;
use oska_syntax::{Diagnostic, Span};

use super::{panics, Body};
use crate::ir::{BinOp, Cond, Inst, Temp};
use crate::layout;

impl Body<'_, '_> {
    /// `lhs op rhs` for a bitwise or shift operator `op` on integers of
    /// type `int`.
    pub(super) fn bitwise(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        int: IntType,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let a = self.expr(lhs)?;
        let b = self.expr(rhs)?;
        let value = match op {
            BinaryOp::BitAnd => self.binary_op(BinOp::And, a, b),
            BinaryOp::BitOr => self.binary_op(BinOp::Or, a, b),
            BinaryOp::BitXor => self.binary_op(BinOp::Xor, a, b),
            BinaryOp::Shl => {
                self.check_amount(op.text(), b, true, int, span)?;
                let shifted = self.binary_op(BinOp::Shl, a, b);
                self.masked(shifted, int)
            }
            BinaryOp::Shr => {
                self.check_amount(op.text(), b, true, int, span)?;
                self.binary_op(BinOp::Shr, a, b)
            }
            _ => return Err(self.refuse(span, &format!("the operator `{}` here", op.text()))),
        };
        Ok(value)
    }

    /// `~operand` on an integer of type `int`.
    pub(super) fn bit_not(&mut self, operand: &Expr, int: IntType) -> Result<Temp, Diagnostic> {
        let value = self.expr(operand)?;
        let ones = self.constant(mask(int));
        Ok(self.binary_op(BinOp::Xor, value, ones))
    }

    /// A call of `U8.rotl`, `U8.rotr` or `U32`'s or `U64`'s, whose
    /// arguments `args` were evaluated to `values`. `None` for any other
    /// intrinsic.
    pub(super) fn rotation_intrinsic(
        &mut self,
        name: &str,
        args: &[Expr],
        values: &[Temp],
        span: Span,
    ) -> Result<Option<Temp>, Diagnostic> {
        let (Some((ty, method)), [receiver, _], &[x, n]) = (name.split_once('.'), args, values)
        else {
            return Ok(None);
        };
        let left = match method {
            "rotl" => true,
            "rotr" => false,
            _ => return Ok(None),
        };
        let Type::Int(int) = receiver.ty else {
            return Ok(None);
        };
        if int.name() != ty || !matches!(int, IntType::U8 | IntType::U32 | IntType::U64) {
            return Ok(None);
        }
        self.check_amount(method, n, false, int, span)?;
        let width = self.constant(i64::from(layout::int_kind(int).bits));
        let back = self.binary_op(BinOp::Sub, width, n);
        let (first, second) = if left {
            (BinOp::Shl, BinOp::Shr)
        } else {
            (BinOp::Shr, BinOp::Shl)
        };
        let one = self.binary_op(first, x, n);
        let other = self.binary_op(second, x, back);
        let both = self.binary_op(BinOp::Or, one, other);
        Ok(Some(self.masked(both, int)))
    }

    /// Panic with `P0008` unless `amount`, the amount of the shift or
    /// rotation `op` of an integer of type `int`, is from 0 to the type's
    /// width less 1. `unsigned` says how the message prints it.
    fn check_amount(
        &mut self,
        op: &str,
        amount: Temp,
        unsigned: bool,
        int: IntType,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let bits = layout::int_kind(int).bits;
        let head = format!("`{op}` by ");
        let tail = format!(
            ": the amount must be from 0 to {} for `{}`",
            bits - 1,
            int.name()
        );
        let out_of_range = self.panic_unary(
            panics::SHIFT_OUT_OF_RANGE,
            (&head, amount, &tail),
            unsigned,
            span,
        )?;
        let width = self.constant(i64::from(bits));
        self.push(Inst::Branch {
            cond: Cond::AboveEq,
            lhs: amount,
            rhs: width,
            target: out_of_range,
        });
        Ok(())
    }

    /// `value` with the bits above `int`'s width cleared.
    pub(super) fn masked(&mut self, value: Temp, int: IntType) -> Temp {
        if layout::int_kind(int).bits == 64 {
            return value;
        }
        let ones = self.constant(mask(int));
        self.binary_op(BinOp::And, value, ones)
    }
}

/// Every bit of an integer of type `int` set, as a word.
fn mask(int: IntType) -> i64 {
    match layout::int_kind(int).bits {
        64 => -1,
        bits => (1i64 << bits) - 1,
    }
}
