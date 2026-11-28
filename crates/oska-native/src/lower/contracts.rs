// SPDX-License-Identifier: Apache-2.0
//! `requires` contracts.

use super::*;

impl Body<'_, '_> {
    /// The slots of a function's contracts' frame, before `r`: its
    /// parameters that take one there.
    pub(super) fn contract_slots(&self, function: &Function) -> Vec<Slot> {
        let mut slots = Vec::new();
        for (index, &takes) in function.contracts.params.iter().enumerate() {
            if let (true, Some(param), Some(&temp)) = (
                takes,
                function.params.get(index),
                self.param_temps.get(index),
            ) {
                slots.push(Slot {
                    temp,
                    release: false,
                    ty: self.ty(&param.ty),
                });
            }
        }
        slots
    }

    /// The body of [`Instance::Requires`]: evaluate each clause in order,
    /// returning the number (from 1) of the first that is false, or 0.
    pub(super) fn requires_function(&mut self, function: &Function) -> Result<(), Diagnostic> {
        self.slots = self.contract_slots(function);
        for (index, clause) in function.contracts.requires.iter().enumerate() {
            let holds = self.expr(&clause.cond)?;
            let ok = self.label();
            let zero = self.constant(0);
            self.push(Inst::Branch {
                cond: Cond::Ne,
                lhs: holds,
                rhs: zero,
                target: ok,
            });
            let failed = self.constant(index as i64 + 1);
            self.push(Inst::Return(failed));
            self.push(Inst::Label(ok));
        }
        let none = self.constant(0);
        self.push(Inst::Return(none));
        Ok(())
    }

    /// Before a call of the user function `name`, with `args` (evaluated
    /// already) at `span`: evaluate its `requires` clauses, and panic with
    /// `P0006` at the call if one is false.
    pub(super) fn check_requires(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &[Temp],
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some(function) = self.program().functions.get(name) else {
            return Ok(());
        };
        if function.contracts.requires.is_empty() {
            return Ok(());
        }
        let instance = Instance::Requires {
            source: self.source,
            name: name.to_string(),
            type_args: type_args.to_vec(),
        };
        let id = self.lowering.request(instance, self.origin.unwrap_or(span));
        let failed = self.call_id(id, args.to_vec(), span)?;
        let (before, after) = self.report(panics::PRECONDITION_VIOLATED, span);
        let clauses = &function.contracts.requires;
        let broken = self.cold_block(|body| {
            // Read the result first: it is a hot temporary.
            let failed = body.copy(failed);
            for (index, clause) in clauses.iter().enumerate() {
                let next = body.label();
                if index + 1 < clauses.len() {
                    let this = body.constant(index as i64 + 1);
                    body.push(Inst::Branch {
                        cond: Cond::Ne,
                        lhs: failed,
                        rhs: this,
                        target: next,
                    });
                }
                let message = body.worded(
                    panics::PRECONDITION_VIOLATED,
                    &format!(
                        "precondition violated: `{}` requires `{}`",
                        function.name, clause.text
                    ),
                    span,
                );
                let report = body.text(&format!("{before}{message}{after}"));
                // The caller broke it: the call is not counted yet, as the
                // interpreter's report has it.
                body.push(Inst::Leave);
                let unit = body.call_runtime("panic", vec![report], NOWHERE)?;
                body.push(Inst::Return(unit));
                body.push(Inst::Label(next));
            }
            Ok(())
        })?;
        let zero = self.constant(0);
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: failed,
            rhs: zero,
            target: broken,
        });
        Ok(())
    }

    // ---- Panics ------------------------------------------------------------
}
