// SPDX-License-Identifier: Apache-2.0
//! `Float`: IEEE 754 binary64 (ADR 0027). A value is one word, the bits
//! of the number. Each of `+ - * /` and `sqrt` is one [`Inst::Float`],
//! which each target selects as the hardware instruction (`addsd` on
//! x86_64, `fadd` on aarch64) and makes a NaN result the canonical NaN.
//! Nothing here fuses one operation with another: `a * b + c` is a
//! multiplication and then an addition, as the interpreter does it.
//! Comparisons are [`Inst::FloatCompare`], IEEE 754's: a NaN is
//! unordered.

use oska_check::program::Expr;
use oska_syntax::ast::BinaryOp;
use oska_syntax::{Diagnostic, Span};

use super::{panics, Body};
use crate::ir::{Cond, FloatCond, FloatOp, Inst, Temp};

impl Body<'_, '_> {
    /// `lhs op rhs` for the float operators: a `Float` for `+ - * /`, a
    /// `Bool` for a comparison.
    pub(super) fn float_binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
    ) -> Result<Temp, Diagnostic> {
        let a = self.expr(lhs)?;
        let b = self.expr(rhs)?;
        Ok(match op {
            BinaryOp::Add => self.float_op(FloatOp::Add, a, b),
            BinaryOp::Sub => self.float_op(FloatOp::Sub, a, b),
            BinaryOp::Mul => self.float_op(FloatOp::Mul, a, b),
            BinaryOp::Div => self.float_op(FloatOp::Div, a, b),
            BinaryOp::Eq => self.float_compare(FloatCond::Eq, a, b),
            BinaryOp::Ne => self.float_compare(FloatCond::Ne, a, b),
            BinaryOp::Lt => self.float_compare(FloatCond::Lt, a, b),
            BinaryOp::Le => self.float_compare(FloatCond::Le, a, b),
            // `a > b` is `b < a`: the same answer for every pair, a NaN
            // included.
            BinaryOp::Gt => self.float_compare(FloatCond::Lt, b, a),
            BinaryOp::Ge => self.float_compare(FloatCond::Le, b, a),
            other => unreachable!("the checker allows no `{}` on a `Float`", other.text()),
        })
    }

    pub(super) fn float_op(&mut self, op: FloatOp, lhs: Temp, rhs: Temp) -> Temp {
        let dst = self.temp();
        self.push(Inst::Float { op, dst, lhs, rhs });
        dst
    }

    pub(super) fn float_compare(&mut self, cond: FloatCond, lhs: Temp, rhs: Temp) -> Temp {
        let dst = self.temp();
        self.push(Inst::FloatCompare {
            cond,
            dst,
            lhs,
            rhs,
        });
        dst
    }

    /// A call of a `Float` method, or a conversion between `Int` and
    /// `Float`, whose arguments were evaluated to `values`. `None` for
    /// any other intrinsic.
    pub(super) fn float_intrinsic(
        &mut self,
        name: &str,
        values: &[Temp],
        span: Span,
    ) -> Result<Option<Temp>, Diagnostic> {
        let value = match (name, values) {
            ("Float.sqrt", &[x]) => self.float_op(FloatOp::Sqrt, x, x),
            // A `Float`'s bits are already canonical.
            ("Float.to_bits", &[x]) => self.copy(x),
            ("Float.from_bits", &[x]) => self.float_op(FloatOp::Canonical, x, x),
            // NaN, an infinity or a value out of range panics (`P0044`).
            ("Float.to_int", &[x]) => {
                let fits = self.call_runtime("float_fits_int", vec![x], span)?;
                let label =
                    self.panic_text(panics::FLOAT_TO_INT, "panic_float_to_int", &[x], span)?;
                self.push(Inst::JumpIfZero {
                    cond: fits,
                    target: label,
                });
                self.call_runtime("float_to_int", vec![x], span)?
            }
            // A value that would round panics (`P0045`).
            ("Int.to_float", &[n]) => {
                let exact = self.call_runtime("int_fits_float", vec![n], span)?;
                let label =
                    self.panic_text(panics::INT_TO_FLOAT, "panic_int_to_float", &[n], span)?;
                self.push(Inst::JumpIfZero {
                    cond: exact,
                    target: label,
                });
                self.call_runtime("int_to_float_rounded", vec![n], span)?
            }
            // A count of digits outside 0 to 340 panics (`P0046`): above,
            // unsigned, is also below 0.
            ("Float.fmt", &[x, digits]) => {
                let label =
                    self.panic_text(panics::FMT_DIGITS, "panic_float_fmt", &[digits], span)?;
                let limit = self.constant(340);
                self.push(Inst::Branch {
                    cond: Cond::Above,
                    lhs: digits,
                    rhs: limit,
                    target: label,
                });
                self.call_runtime("float_fmt", vec![x, digits], span)?
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
}
