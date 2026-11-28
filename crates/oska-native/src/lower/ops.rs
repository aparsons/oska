// SPDX-License-Identifier: Apache-2.0
//! Binary and unary operators.

use super::*;

impl Body<'_, '_> {
    pub(super) fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        int: Option<IntType>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        match (op, int) {
            (BinaryOp::And | BinaryOp::Or | BinaryOp::Implies, _) => {
                self.short_circuit(op, lhs, rhs)
            }
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem,
                Some(int),
            ) => self.arithmetic(op, lhs, rhs, int, span),
            (
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr,
                Some(int),
            ) => self.bitwise(op, lhs, rhs, int, span),
            (
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge,
                None,
            ) => {
                if let Some(function) = text_equality(op, &lhs.ty) {
                    let operands = self.borrowed_args(&[lhs, rhs])?;
                    let [left, right] = operands[..] else {
                        return Err(self.refuse(span, "comparing texts"));
                    };
                    let equal = self.call_runtime(function, vec![left, right], span)?;
                    self.consume(left)?;
                    self.consume(right)?;
                    if op == BinaryOp::Ne {
                        let one = self.constant(1);
                        return Ok(self.binary_op(BinOp::Xor, equal, one));
                    }
                    return Ok(equal);
                }
                if lhs.ty == Type::String {
                    // `<`, `<=`, `>` and `>=` on text (ADR 0034): the order
                    // of its bytes, as -1, 0 or 1, against 0.
                    let operands = self.borrowed_args(&[lhs, rhs])?;
                    let [left, right] = operands[..] else {
                        return Err(self.refuse(span, "comparing texts"));
                    };
                    let order = self.call_runtime("string_compare", vec![left, right], span)?;
                    self.consume(left)?;
                    self.consume(right)?;
                    let zero = self.constant(0);
                    let dst = self.temp();
                    self.push(Inst::Compare {
                        cond: comparison(op, false),
                        dst,
                        lhs: order,
                        rhs: zero,
                    });
                    return Ok(dst);
                }
                let unsigned = match &lhs.ty {
                    Type::Int(int) => int.min() == 0,
                    Type::Bool | Type::Unit => false,
                    other => {
                        return Err(self.refuse(span, &format!("comparing values of `{other}`")))
                    }
                };
                let cond = comparison(op, unsigned);
                let lhs = self.expr(lhs)?;
                let rhs = self.expr(rhs)?;
                let dst = self.temp();
                self.push(Inst::Compare {
                    cond,
                    dst,
                    lhs,
                    rhs,
                });
                Ok(dst)
            }
            _ => Err(self.refuse(span, &format!("the operator `{}` here", op.text()))),
        }
    }

    /// `lhs op rhs` on integers of type `int`, checked as the interpreter
    /// checks it: division by zero panics with `P0004`, a result outside
    /// the type with `P0001`, with the interpreter's messages.
    pub(super) fn arithmetic(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        int: IntType,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let a = self.expr(lhs)?;
        let b = self.expr(rhs)?;
        let kind = layout::int_kind(int);
        let unsigned = !kind.signed;
        let arith = match op {
            BinaryOp::Add => Arith::Add,
            BinaryOp::Sub => Arith::Sub,
            BinaryOp::Mul => Arith::Mul,
            BinaryOp::Div => Arith::Div,
            _ => Arith::Rem,
        };
        // A divisor written as a literal other than 0 needs no check.
        let nonzero = matches!(rhs.kind, ExprKind::Int(value, _) if value != 0);
        if matches!(arith, Arith::Div | Arith::Rem) && !nonzero {
            let tail = format!(" {} 0`", op.text());
            let by_zero = self.panic_unary(
                panics::DIVISION_BY_ZERO,
                ("division by zero: `", a, &tail),
                unsigned,
                span,
            )?;
            let zero = self.constant(0);
            self.push(Inst::Branch {
                cond: Cond::Eq,
                lhs: b,
                rhs: zero,
                target: by_zero,
            });
        }
        let mid = format!(" {} ", op.text());
        let tail = format!(
            "` overflows `{}`, which ranges from {} to {}",
            int.name(),
            int.min(),
            int.max()
        );
        let overflow =
            self.panic_binary(panics::OVERFLOW, ("`", a, &mid, b, &tail), unsigned, span)?;
        let dst = self.temp();
        self.push(Inst::Arith {
            op: arith,
            int: kind,
            dst,
            lhs: a,
            rhs: b,
            overflow,
        });
        Ok(dst)
    }

    /// `-operand` on an integer of type `int`: `P0001` when the result
    /// does not fit, as for the most negative value of a signed type, or
    /// anything but 0 of an unsigned one.
    pub(super) fn negate(
        &mut self,
        operand: &Expr,
        int: IntType,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let a = self.expr(operand)?;
        let kind = layout::int_kind(int);
        let tail = format!("` overflows `{}`", int.name());
        let overflow = self.panic_unary(panics::OVERFLOW, ("`-", a, &tail), !kind.signed, span)?;
        let zero = self.constant(0);
        let dst = self.temp();
        self.push(Inst::Arith {
            op: Arith::Sub,
            int: kind,
            dst,
            lhs: zero,
            rhs: a,
            overflow,
        });
        Ok(dst)
    }
}
