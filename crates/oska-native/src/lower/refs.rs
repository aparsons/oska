// SPDX-License-Identifier: Apache-2.0
//! Reference counts: retaining, releasing and leaving scopes.

use super::*;

impl Body<'_, '_> {
    /// Whether values of `ty` are counted, in the instance being lowered.
    pub(super) fn counted(&self, ty: &Type) -> bool {
        layout::is_counted(self.program(), &self.ty(ty))
    }

    // ---- Reference counts ------------------------------------------------

    /// Count one more reference to the object `value`, unless it is
    /// static.
    pub(super) fn retain(&mut self, value: Temp) {
        let count = self.load(value, 0);
        let zero = self.constant(0);
        let done = self.label();
        self.push(Inst::Branch {
            cond: Cond::Lt,
            lhs: count,
            rhs: zero,
            target: done,
        });
        let one = self.constant(1);
        let more = self.binary_op(BinOp::Add, count, one);
        self.push(Inst::Store {
            width: Width::W64,
            base: value,
            offset: 0,
            src: more,
        });
        self.push(Inst::Label(done));
    }

    /// Drop one reference to the object `value`, of the concrete type
    /// `ty`, unless it is static. When that was the last, release what
    /// its fields hold and free it.
    pub(super) fn release(&mut self, value: Temp, ty: &Type) -> Result<(), Diagnostic> {
        let count = self.load(value, 0);
        let zero = self.constant(0);
        let one = self.constant(1);
        let (done, shared) = (self.label(), self.label());
        self.push(Inst::Branch {
            cond: Cond::Lt,
            lhs: count,
            rhs: zero,
            target: done,
        });
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: count,
            rhs: one,
            target: shared,
        });
        if layout::is_directory(ty) {
            self.call_runtime("root_drop", vec![value], NOWHERE)?;
        } else if layout::is_socket(ty) {
            self.call_runtime("sock_drop", vec![value], NOWHERE)?;
        } else if matches!(ty, Type::Fn { .. }) {
            if self.dropping {
                let code = self.load(value, layout::FN_DROP);
                self.call_runtime("drop_later", vec![value, code], NOWHERE)?;
            } else {
                self.drop_function_value(value);
            }
        } else if layout::holds_counted(self.program(), ty) {
            let drop = Instance::Drop(self.source, ty.clone());
            let origin = self.origin.unwrap_or(NOWHERE);
            let id = self.lowering.request(drop, origin);
            if self.dropping {
                let code = self.function_address(id);
                self.call_runtime("drop_later", vec![value, code], NOWHERE)?;
            } else {
                self.call_id(id, vec![value], NOWHERE)?;
            }
        } else {
            self.call_runtime("free", vec![value], NOWHERE)?;
        }
        self.push(Inst::Jump(done));
        self.push(Inst::Label(shared));
        let less = self.binary_op(BinOp::Sub, count, one);
        self.push(Inst::Store {
            width: Width::W64,
            base: value,
            offset: 0,
            src: less,
        });
        self.push(Inst::Label(done));
        Ok(())
    }

    /// Release `value` if it holds a reference of its own.
    pub(super) fn consume(&mut self, value: Temp) -> Result<(), Diagnostic> {
        if let Some(ty) = self.owned.remove(&value) {
            self.release(value, &ty)?;
        }
        Ok(())
    }

    /// Release the slots above `height`, innermost first, without taking
    /// them out of scope: for code that leaves their scope.
    pub(super) fn release_slots(&mut self, height: usize) -> Result<(), Diagnostic> {
        let slots: Vec<Slot> = self.slots.get(height..).unwrap_or_default().to_vec();
        for slot in slots.iter().rev() {
            if slot.release {
                self.release(slot.temp, &slot.ty)?;
            }
        }
        Ok(())
    }

    /// Release the values held inside the innermost `loops` open loops
    /// (all of them for `None`), innermost first, without letting go of
    /// them: for code that leaves them.
    pub(super) fn release_held(&mut self, loops: Option<usize>) -> Result<(), Diagnostic> {
        let held: Vec<Held> = self.held.clone();
        for value in held.iter().rev() {
            if loops.is_none_or(|open| value.loops >= open) {
                if value.scope {
                    self.finish_scope(value.temp)?;
                }
                self.release(value.temp, &value.ty)?;
            }
        }
        Ok(())
    }

    /// End a scope: release its slots and take them out of scope.
    pub(super) fn close_scope(&mut self, height: usize) -> Result<(), Diagnostic> {
        self.release_slots(height)?;
        self.slots.truncate(height);
        Ok(())
    }

    /// Return `value`, whose reference passes to the caller, releasing
    /// every local, and every value an expression still holds: a `return`
    /// or `?` inside an argument abandons the arguments before it.
    pub(super) fn leave_function(&mut self, value: Temp) -> Result<(), Diagnostic> {
        self.owned.remove(&value);
        let pending: Vec<(Temp, Type)> = self
            .owned
            .iter()
            .rev()
            .map(|(&temp, ty)| (temp, ty.clone()))
            .collect();
        for (temp, ty) in pending {
            self.release(temp, &ty)?;
        }
        self.release_slots(0)?;
        self.release_held(None)?;
        if self.status {
            let went_ahead = self.constant(0);
            self.push(Inst::ReturnPair {
                value,
                second: went_ahead,
            });
            return Ok(());
        }
        match self.receiver.clone() {
            // A mutating method gives its receiver back.
            Some(Receiver {
                temp, owned: true, ..
            }) => self.push(Inst::ReturnPair {
                value,
                second: temp,
            }),
            _ => self.push(Inst::Return(value)),
        }
        Ok(())
    }

    /// Return `value`, at `at` (the `return`, or the end of the body),
    /// after its function's `ensures` clauses held: the first that is
    /// false panics with `P0007`, naming `at`. Every way out of a function
    /// goes through here.
    pub(super) fn return_value(&mut self, value: Temp, at: Span) -> Result<(), Diagnostic> {
        if let Some(function) = self.ensuring {
            // The clauses run in a frame of their own: the parameters
            // that take a slot there, then `r`.
            let frame = self.contract_slots(function);
            let outer = std::mem::replace(&mut self.slots, frame);
            let loops = std::mem::take(&mut self.loops);
            let height = self.slots.len();
            let (line, column) = self.program().line_col(at.start);
            for ensures in &function.contracts.ensures {
                if ensures.binds {
                    let ty = self.ty(&function.ret);
                    // Borrowed: the return still owns the value.
                    self.slots.push(Slot {
                        temp: value,
                        release: false,
                        ty,
                    });
                }
                let holds = self.expr(&ensures.clause.cond)?;
                self.slots.truncate(height);
                let message = self.worded(
                    panics::POSTCONDITION_VIOLATED,
                    &format!(
                        "postcondition violated: `{}` ensures `{}`, \
                         broken by the return at line {line}, column {column}",
                        function.name, ensures.clause.text
                    ),
                    ensures.clause.span,
                );
                let broken = self.panic_static(
                    panics::POSTCONDITION_VIOLATED,
                    &message,
                    ensures.clause.span,
                )?;
                self.push(Inst::JumpIfZero {
                    cond: holds,
                    target: broken,
                });
            }
            self.slots = outer;
            self.loops = loops;
        }
        self.leave_function(value)
    }
}
