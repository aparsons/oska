// SPDX-License-Identifier: Apache-2.0
//! Statements and loops.

use super::*;

impl Body<'_, '_> {
    pub(super) fn stmt(&mut self, stmt: &Stmt) -> Result<(), Diagnostic> {
        match stmt {
            Stmt::Expr(expr) => {
                let value = self.expr(expr)?;
                self.consume(value)?;
            }
            Stmt::Let {
                binds, ty, value, ..
            } => {
                let value = self.expr(value)?;
                if *binds {
                    // The local takes over the value's reference.
                    self.owned.remove(&value);
                    let temp = self.copy(value);
                    let release = self.counted(ty);
                    let ty = self.ty(ty);
                    self.slots.push(Slot { temp, release, ty });
                } else {
                    self.consume(value)?;
                }
            }
            Stmt::Assign { place, value, .. } => self.assign(place, value)?,
            Stmt::Return { value, span } => {
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => self.constant(0),
                };
                self.return_value(value, *span)?;
            }
            Stmt::Break(_) | Stmt::Continue(_) => {
                let Some(&innermost) = self.loops.last() else {
                    return Err(self.refuse(NOWHERE, "`break` outside a loop"));
                };
                self.release_slots(innermost.height)?;
                self.release_held(Some(self.loops.len()))?;
                let target = if matches!(stmt, Stmt::Break(_)) {
                    innermost.exit
                } else {
                    innermost.next
                };
                self.push(Inst::Jump(target));
            }
            Stmt::While { cond, body, span } => {
                let head = self.label();
                // Under `--max-steps`, the body and a `continue` go on
                // to the back-edge's step; without it, to the condition.
                let counted = self.step_limit().is_some();
                let next = if counted { self.label() } else { head };
                let exit = self.label();
                self.push(Inst::Label(head));
                self.branch_unless(cond, exit)?;
                self.loop_body(next, exit, body)?;
                if counted {
                    self.push(Inst::Label(next));
                    self.step(*span)?;
                }
                self.push(Inst::Jump(head));
                self.push(Inst::Label(exit));
            }
            Stmt::For {
                binds,
                ty,
                iter,
                body,
                span,
            } => {
                if let Some(text) = self.chars_of(iter) {
                    self.for_chars(*binds, ty, text, body, *span)?;
                } else if layout::list_item(&self.ty(&iter.ty)).is_some() {
                    self.for_list(*binds, ty, iter, body, *span)?;
                } else {
                    self.for_range(*binds, ty, iter, body, *span)?;
                }
            }
        }
        Ok(())
    }

    /// The body of a loop whose `continue` jumps to `next` and whose
    /// `break` jumps to `exit`.
    pub(super) fn loop_body(
        &mut self,
        next: Label,
        exit: Label,
        body: &Expr,
    ) -> Result<(), Diagnostic> {
        self.loops.push(Loop {
            next,
            exit,
            height: self.slots.len(),
        });
        let value = self.expr(body)?;
        self.consume(value)?;
        self.loops.pop();
        Ok(())
    }

    /// `for x in r { body }`, the statement at `span`, over a range: each
    /// integer from its start up to but excluding its end, both read
    /// once. A range written in place, `start..end`, is never built:
    /// `start` is evaluated first.
    pub(super) fn for_range(
        &mut self,
        binds: bool,
        ty: &Type,
        iter: &Expr,
        body: &Expr,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let (first, end) = match &iter.kind {
            ExprKind::Range { start, end } => {
                let first = self.expr(start)?;
                (first, self.expr(end)?)
            }
            _ => {
                let range = self.expr(iter)?;
                let first = self.load(range, layout::struct_field(0));
                let end = self.load(range, layout::struct_field(1));
                // Integers: the bounds outlive the range.
                self.consume(range)?;
                (first, end)
            }
        };
        let unsigned = matches!(ty, Type::Int(int) if int.min() == 0);
        let at = self.copy(first);
        let (head, next, exit) = (self.label(), self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: if unsigned { Cond::AboveEq } else { Cond::Ge },
            lhs: at,
            rhs: end,
            target: exit,
        });
        if binds {
            let temp = self.copy(at);
            self.slots.push(Slot {
                temp,
                release: false,
                ty: ty.clone(),
            });
        }
        self.loop_body(next, exit, body)?;
        if binds {
            self.slots.pop();
        }
        self.push(Inst::Label(next));
        self.step(span)?;
        // `at` is below `end`, so one more never overflows.
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(exit));
        Ok(())
    }
}
