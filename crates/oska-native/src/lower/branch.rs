// SPDX-License-Identifier: Apache-2.0
//! Conditional branches and short-circuit operators.

use super::*;

impl Body<'_, '_> {
    /// Jump to `target` unless the condition `cond` holds: a comparison
    /// of integers or `Bool`s is one compare-and-branch, and `&&`, `||`,
    /// `implies` and `!` branch on their operands, without making a
    /// `Bool` first.
    pub(super) fn branch_unless(&mut self, cond: &Expr, target: Label) -> Result<(), Diagnostic> {
        self.branch_on(cond, false, target)
    }

    /// Jump to `target` when `cond` is `when`.
    pub(super) fn branch_on(
        &mut self,
        cond: &Expr,
        when: bool,
        target: Label,
    ) -> Result<(), Diagnostic> {
        match &cond.kind {
            ExprKind::Not(operand) => return self.branch_on(operand, !when, target),
            ExprKind::Binary { op, lhs, rhs, .. } => match op {
                // `a && b` is false when either is; `a || b` true when
                // either is; `a implies b` is `!a || b`.
                BinaryOp::And | BinaryOp::Or | BinaryOp::Implies => {
                    let (first_negated, decided) = match op {
                        BinaryOp::And => (false, false),
                        BinaryOp::Or => (false, true),
                        _ => (true, true),
                    };
                    if when == decided {
                        // Either operand decides: jump on each.
                        self.branch_on(lhs, when != first_negated, target)?;
                        return self.branch_on(rhs, when, target);
                    }
                    // Both must agree: skip past when the first decides
                    // the other way.
                    let skip = self.label();
                    self.branch_on(lhs, decided != first_negated, skip)?;
                    self.branch_on(rhs, when, target)?;
                    self.push(Inst::Label(skip));
                    return Ok(());
                }
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
                    if text_equality(*op, &lhs.ty).is_none() =>
                {
                    let unsigned = match &lhs.ty {
                        Type::Int(int) => int.min() == 0,
                        Type::Bool | Type::Unit => false,
                        _ => return self.branch_on_value(cond, when, target),
                    };
                    let mut test = comparison(*op, unsigned);
                    if !when {
                        test = test.negated();
                    }
                    let lhs = self.expr(lhs)?;
                    let rhs = self.expr(rhs)?;
                    self.push(Inst::Branch {
                        cond: test,
                        lhs,
                        rhs,
                        target,
                    });
                    return Ok(());
                }
                _ => {}
            },
            _ => {}
        }
        self.branch_on_value(cond, when, target)
    }

    /// Jump to `target` when the `Bool` `cond`, evaluated, is `when`.
    pub(super) fn branch_on_value(
        &mut self,
        cond: &Expr,
        when: bool,
        target: Label,
    ) -> Result<(), Diagnostic> {
        let value = self.expr(cond)?;
        if when {
            let zero = self.constant(0);
            self.push(Inst::Branch {
                cond: Cond::Ne,
                lhs: value,
                rhs: zero,
                target,
            });
        } else {
            self.push(Inst::JumpIfZero {
                cond: value,
                target,
            });
        }
        Ok(())
    }

    /// `&&`, `||` and `implies`, which evaluate `rhs` only when they need
    /// it.
    pub(super) fn short_circuit(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
    ) -> Result<Temp, Diagnostic> {
        let result = self.temp();
        let (skip, end) = (self.label(), self.label());
        let left = self.expr(lhs)?;
        // `a && b` and `a implies b` are decided without `b` when `a` is
        // false: `false` and `true`. `a || b` is, when `a` is true.
        let decided = match op {
            BinaryOp::Implies => 1,
            _ => 0,
        };
        if op == BinaryOp::Or {
            self.push(Inst::Copy {
                dst: result,
                src: left,
            });
            self.push(Inst::JumpIfZero {
                cond: left,
                target: skip,
            });
            self.push(Inst::Jump(end));
            self.push(Inst::Label(skip));
        } else {
            self.push(Inst::Const {
                dst: result,
                value: decided,
            });
            self.push(Inst::JumpIfZero {
                cond: left,
                target: end,
            });
        }
        let right = self.expr(rhs)?;
        self.push(Inst::Copy {
            dst: result,
            src: right,
        });
        self.push(Inst::Label(end));
        Ok(result)
    }
}
