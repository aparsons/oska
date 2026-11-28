// SPDX-License-Identifier: Apache-2.0
//! `--max-steps` counting.

use super::*;

impl Body<'_, '_> {
    /// Under `--max-steps`, give the program its steps: the limit, in the
    /// step counter, before `main` runs. Without the flag nothing is
    /// generated.
    pub(super) fn start_steps(&mut self) {
        let Some(limit) = self.lowering.limits.max_steps else {
            return;
        };
        let counter = self.steps_counter();
        let limit = self.limit_constant(limit);
        self.store(counter, 0, limit);
    }

    /// The address of the step counter.
    pub(super) fn steps_counter(&mut self) -> Temp {
        let dst = self.temp();
        self.push(Inst::Addr {
            dst,
            symbol: Symbol::Data(STEPS_WORD * layout::WORD),
        });
        dst
    }

    /// Under `--max-steps`, take one **step** (ADR 0021) at `span`: a call
    /// of a function, method or closure written in Oska, or a loop's
    /// back-edge, as the interpreter's `Interpreter::step` counts them.
    /// With no step left it panics with `P0038` at `span` instead. The
    /// counter is a word in the writable data that only counts down, read
    /// and written with the backend's plain loads and stores before
    /// register allocation, so every target counts alike, with no system
    /// call (ADR 0013). Without the flag nothing is generated, and
    /// nothing in `std.rt` is a step.
    pub(super) fn step(&mut self, span: Span) -> Result<(), Diagnostic> {
        let Some(limit) = self.step_limit() else {
            return Ok(());
        };
        let over = self.panic_static(Limit::Steps.code(), &Limit::Steps.message(limit), span)?;
        self.spend_step(over);
        Ok(())
    }

    /// The `--max-steps` limit this code counts steps under: none
    /// without the flag, and none in `std.rt`, where nothing is a step.
    pub(super) fn step_limit(&self) -> Option<u64> {
        self.lowering
            .limits
            .max_steps
            .filter(|_| self.source == Source::User)
    }

    /// Under `--max-steps`, `span`: where a function the backend generates
    /// and shares between callers, such as the comparison of two lists,
    /// takes the steps it calls the program's code for, so that each
    /// caller gets its own copy that reports them at its own span, as the
    /// interpreter does. `None` without the flag: one copy for all.
    pub(super) fn step_site(&self, span: Span) -> Option<Span> {
        self.lowering.limits.max_steps.map(|_| span)
    }

    /// Count one step down, or jump to `over` when none is left.
    pub(super) fn spend_step(&mut self, over: Label) {
        let counter = self.steps_counter();
        let left = self.load(counter, 0);
        self.push(Inst::JumpIfZero {
            cond: left,
            target: over,
        });
        let one = self.constant(1);
        let left = self.binary_op(BinOp::Sub, left, one);
        self.store(counter, 0, left);
    }
}
