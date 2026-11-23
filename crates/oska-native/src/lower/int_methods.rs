// SPDX-License-Identifier: Apache-2.0
//! Wrapping arithmetic on `U8`, `U32` and `U64`, and the conversions
//! between `Int`, `U8`, `U16`, `U32` and `U64`.
//!
//! An `Int` is held as a signed word and an unsigned value zero-extended
//! in its word (`bits.rs`), so the word's low bits are the value's low
//! bits whatever its type. `wrapping_add`, `wrapping_sub` and
//! `wrapping_mul` are one word operation each, whose low 64 bits are
//! right for signed and unsigned operands alike, masked back to the
//! type's width. `wrapping_to_<type>()` is the same mask.
//!
//! `to_<type>()` leaves the word as it is when every value of the source
//! fits the target, and otherwise compares it first and panics with
//! `P0001`, with the interpreter's message (`oska-run`'s
//! `int_methods.rs`). Into a narrower unsigned type the comparison is
//! unsigned, so a negative `Int` is out of range too; between `Int` and
//! `U64` a set top bit is the one value out of range either way, a
//! signed comparison with 0.

use oska_check::program::{Expr, IntType, Type};
use oska_syntax::{Diagnostic, Span};

use super::{panics, Body};
use crate::ir::{BinOp, Cond, Inst, Temp};
use crate::layout;

impl Body<'_, '_> {
    /// A call of a wrapping arithmetic method or a conversion of an
    /// integer type, whose arguments `args` were evaluated to `values`.
    /// `None` for any other intrinsic.
    pub(super) fn int_method_intrinsic(
        &mut self,
        name: &str,
        args: &[Expr],
        values: &[Temp],
        span: Span,
    ) -> Result<Option<Temp>, Diagnostic> {
        let Some((ty, method)) = name.split_once('.') else {
            return Ok(None);
        };
        let Some(Type::Int(from)) = args.first().map(|receiver| &receiver.ty) else {
            return Ok(None);
        };
        if from.name() != ty {
            return Ok(None);
        }
        let from = *from;
        let value = match (method, values) {
            ("wrapping_add", &[a, b]) => self.wrapping(BinOp::Add, a, b, from),
            ("wrapping_sub", &[a, b]) => self.wrapping(BinOp::Sub, a, b, from),
            ("wrapping_mul", &[a, b]) => self.wrapping(BinOp::Mul, a, b, from),
            (_, &[x]) => {
                if let Some(to) = method.strip_prefix("wrapping_to_").and_then(target) {
                    self.masked(x, to)
                } else if let Some(to) = method.strip_prefix("to_").and_then(target) {
                    self.convert(x, from, to, span)?
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    /// `a op b`, wrapped to the width of `int`.
    fn wrapping(&mut self, op: BinOp, a: Temp, b: Temp, int: IntType) -> Temp {
        let exact = self.binary_op(op, a, b);
        self.masked(exact, int)
    }

    /// `x`, of type `from`, as an integer of type `to`: `P0001` unless it
    /// fits.
    fn convert(
        &mut self,
        x: Temp,
        from: IntType,
        to: IntType,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let unsigned = !layout::int_kind(from).signed;
        let (cond, limit) = match to {
            // Every value of the source fits.
            _ if to.min() <= from.min() && from.max() <= to.max() => return Ok(x),
            // `Int` to `U64` or `U64` to `Int`: the top bit set.
            IntType::Int | IntType::U64 => (Cond::Lt, 0),
            // A narrower unsigned type: past its maximum, unsigned.
            _ => (Cond::Above, to.max() as i64),
        };
        let head = format!("`to_{}()` of ", to.name().to_lowercase());
        let tail = format!(
            " overflows `{}`, which ranges from {} to {}",
            to.name(),
            to.min(),
            to.max()
        );
        let overflow = self.panic_unary(panics::OVERFLOW, (&head, x, &tail), unsigned, span)?;
        let limit = self.constant(limit);
        self.push(Inst::Branch {
            cond,
            lhs: x,
            rhs: limit,
            target: overflow,
        });
        Ok(x)
    }
}

/// The type a conversion method converts to, by its suffix, such as `u8`.
fn target(suffix: &str) -> Option<IntType> {
    [
        IntType::Int,
        IntType::U8,
        IntType::U16,
        IntType::U32,
        IntType::U64,
    ]
    .into_iter()
    .find(|int| int.name().eq_ignore_ascii_case(suffix))
}
