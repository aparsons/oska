//! Clean-ups of a lowered function, before register allocation: the
//! lowering is simple and leaves copies and constants that cost an
//! instruction and a register each. Each pass keeps the code's meaning
//! and the rules `regalloc.rs` relies on (every temporary written before
//! it is read on every path; cold blocks as `ir::Function` describes).
//!
//! - **Copies.** A copy whose source is used once, straight after it is
//!   made, is folded into the instruction that made it; a copy used once
//!   further on, before its source changes, is replaced by its source.
//! - **Immediates.** A constant that fits an instruction's immediate
//!   field is encoded there ([`Inst::BinaryImm`], [`Inst::BranchImm`],
//!   [`Inst::CompareImm`], [`Inst::ArithImm`]).
//! - **Dead code.** An instruction with no effect but its result, whose
//!   result nothing reads, is removed.
//! - **Addresses.** A load or store at `x + constant` encodes the
//!   constant in its offset.
//! - **Inlining.** A call of a small function, such as `std.rt`'s
//!   `text_data`, is replaced by a copy of its code ([`module`]): one
//!   that runs straight through anywhere, and one with branches, loops
//!   and calls of its own (`alloc`, `write_small`) on the hot path, its
//!   labels renumbered and each `Return` a jump to its end. In rounds, so
//!   a function that became small enough is inlined in turn. A function
//!   that counts call depth, makes a system or indirect call, or has cold
//!   blocks is never inlined, nor one that marks a secret.
//!
//! **Secrets.** No pass removes or folds an [`Inst::Secret`] mark but as
//! dead code, whose result nothing reads; none turns a choice into a
//! branch or arithmetic into a table lookup; and a function that marks a
//! secret is never inlined, so a mark stays where `constant_time.rs`
//! expects it. That module checks the result.
//! - **Temporaries.** Those the code no longer uses are dropped and the
//!   rest renumbered, since each costs a stack slot in its frame and the
//!   stack is reserved for the deepest calls of the largest frame.
//!
//! Every pass works within a straight run of code: between two labels,
//! with no jump in between where it matters, so no other path can see a
//! changed temporary.

use crate::ir::{self, Arith, BinOp, FnId, Function, Inst, Temp};

/// The most instructions a function inlined at its calls may have.
const INLINE_SIZE: usize = 12;

/// The most instructions a function with branches inlined at its calls
/// may have.
const INLINE_BRANCHY_SIZE: usize = 72;

/// Clean up every function of a module, inline the small ones where they
/// are called, and clean up again.
pub fn module(functions: &mut [Function]) {
    for f in functions.iter_mut() {
        function(f);
    }
    // In rounds: a function whose only calls one round inlined may be
    // inlined in turn by the next (`text_data` into `digit_pairs` into
    // `write_small` into `int_put`).
    for _ in 0..INLINE_ROUNDS {
        let inlinable: Vec<Option<(Inlining, Function)>> = functions
            .iter()
            .map(|f| inlinable(f).map(|how| (how, f.clone())))
            .collect();
        let mut changed = false;
        for (id, f) in functions.iter_mut().enumerate() {
            if inline(f, id, &inlinable) {
                function(f);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Renumber `f`'s temporaries so those its code still reads or writes
/// are `0..temps`, its parameters first as they were: the passes leave
/// gaps, and every temporary counts toward the frame (a stack slot when
/// it gets no register) and the stack the program reserves for the
/// deepest calls.
fn compact_temps(f: &mut Function) {
    let mut used = vec![false; f.temps as usize];
    for t in 0..f.params {
        if let Some(u) = used.get_mut(t as usize) {
            *u = true;
        }
    }
    for inst in &f.insts {
        for t in inst.def().into_iter().chain(inst.uses()) {
            if let Some(u) = used.get_mut(t as usize) {
                *u = true;
            }
        }
    }
    let mut map = vec![0; used.len()];
    let mut next = 0;
    for (t, &u) in used.iter().enumerate() {
        if u {
            map[t] = next;
            next += 1;
        }
    }
    if next == f.temps {
        return;
    }
    for inst in &mut f.insts {
        inst.map_temps(|t| map.get(t as usize).copied().unwrap_or(t));
    }
    f.temps = next;
}

/// How many times calls are inlined, at most: how deep a chain of
/// inlined functions may be.
const INLINE_ROUNDS: usize = 4;

/// How calls of a function may be replaced by its code, if they may.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inlining {
    /// It runs straight through to its one `Return`, at the end.
    Straight,
    /// It has labels, branches and `Return`s of its own.
    Branchy,
}

/// Whether calls of `f` may be replaced by its code, and how: it is
/// short, has no cold blocks, leaves only by `Return`, and makes no call
/// but plain ones (no depth counting, system or indirect call).
fn inlinable(f: &Function) -> Option<Inlining> {
    if f.cold != f.insts.len() || !matches!(f.insts.last(), Some(Inst::Return(_))) {
        return None;
    }
    let simple = f.insts.iter().all(|inst| {
        !matches!(
            inst,
            Inst::CallIndirect { .. }
                | Inst::Syscall { .. }
                | Inst::LibCall { .. }
                | Inst::CallOnStack { .. }
                | Inst::ReturnPair { .. }
                | Inst::Second { .. }
                | Inst::Enter { .. }
                | Inst::Leave
                | Inst::Arith { .. }
                | Inst::ArithImm { .. }
                | Inst::EntrySp { .. }
                // A mark stays in the function that makes it
                // (`constant_time.rs`): `bytes_read` marks the bytes it
                // reads, and inlined into a caller of `Bytes.at` on public
                // bytes, its mark would make that caller's byte secret.
                | Inst::Secret { .. }
        )
    });
    if !simple {
        return None;
    }
    let (_, body) = f.insts.split_last()?;
    let straight = body
        .iter()
        .all(|inst| inst.target().is_none() && !matches!(inst, Inst::Label(_) | Inst::Return(_)));
    if straight && f.insts.len() <= INLINE_SIZE {
        Some(Inlining::Straight)
    } else if f.insts.len() <= INLINE_BRANCHY_SIZE {
        Some(Inlining::Branchy)
    } else {
        None
    }
}

/// Replace each call in `f` (whose id is `id`) of a function in
/// `inlinable` by that function's code, its temporaries renumbered after
/// `f`'s (and, for a branchy one, which is inlined only on the hot path,
/// its labels after `f`'s): whether any was.
fn inline(f: &mut Function, id: FnId, inlinable: &[Option<(Inlining, Function)>]) -> bool {
    let mut insts = Vec::with_capacity(f.insts.len());
    let mut cold = f.cold;
    let mut changed = false;
    for (at, inst) in std::mem::take(&mut f.insts).into_iter().enumerate() {
        let callee = match &inst {
            Inst::Call { callee, args, .. } if *callee != id => inlinable
                .get(*callee)
                .and_then(Option::as_ref)
                .filter(|(how, _)| *how == Inlining::Straight || at < f.cold)
                .filter(|(_, g)| g.params as usize == args.len())
                .filter(|(_, g)| f.temps + g.temps <= ir::MAX_TEMPS),
            _ => None,
        };
        let (Some((how, callee)), Inst::Call { dst, args, .. }) = (callee, &inst) else {
            insts.push(inst);
            continue;
        };
        let branchy = *how == Inlining::Branchy;
        let base = f.temps;
        f.temps += callee.temps;
        let labels = f.labels;
        let end = labels + callee.labels;
        if branchy {
            f.labels = end + 1;
        }
        let before = insts.len();
        for (param, &arg) in args.iter().enumerate() {
            insts.push(Inst::Copy {
                dst: base + param as Temp,
                src: arg,
            });
        }
        let last = callee.insts.len() - 1;
        for (i, inst) in callee.insts.iter().enumerate() {
            match inst {
                Inst::Return(value) => {
                    insts.push(Inst::Copy {
                        dst: *dst,
                        src: base + value,
                    });
                    if i != last {
                        insts.push(Inst::Jump(end));
                    }
                }
                other => {
                    let mut copy = other.clone();
                    copy.map_temps(|t| base + t);
                    if branchy {
                        map_labels(&mut copy, |l| labels + l);
                    }
                    insts.push(copy);
                }
            }
        }
        if branchy {
            insts.push(Inst::Label(end));
        }
        if at < f.cold {
            // The call was one instruction; its code is this many.
            cold += insts.len() - before - 1;
        }
        changed = true;
    }
    f.insts = insts;
    f.cold = cold;
    changed
}

/// Renumber with `map` the labels `inst` places or jumps to: those of an
/// inlined function, which [`inlinable`] allows only in labels, jumps and
/// branches.
fn map_labels(inst: &mut Inst, map: impl Fn(ir::Label) -> ir::Label) {
    match inst {
        Inst::Label(label) | Inst::Jump(label) => *label = map(*label),
        Inst::JumpIfZero { target, .. }
        | Inst::Branch { target, .. }
        | Inst::BranchImm { target, .. } => *target = map(*target),
        _ => {}
    }
}

/// Clean up `function` in place.
pub fn function(function: &mut Function) {
    let mut code = Code::new(function);
    code.propagate_copies();
    code.coalesce_copies();
    code.immediates();
    code.fold_offsets();
    code.remove_dead();
    code.write_back(function);
    compact_temps(function);
}

/// A function's instructions, each with a flag for whether it is still
/// there: a removed one is skipped, so indices stay valid while passes run.
struct Code {
    insts: Vec<Option<Inst>>,
    cold: usize,
    temps: usize,
}

impl Code {
    fn new(function: &Function) -> Code {
        Code {
            insts: function.insts.iter().cloned().map(Some).collect(),
            cold: function.cold,
            temps: function.temps as usize,
        }
    }

    fn write_back(self, function: &mut Function) {
        let cold = self.insts[..self.cold.min(self.insts.len())]
            .iter()
            .filter(|i| i.is_some())
            .count();
        function.insts = self.insts.into_iter().flatten().collect();
        function.cold = cold;
    }

    /// How many instructions write each temporary, and how many reads
    /// there are of each.
    fn counts(&self) -> (Vec<u32>, Vec<u32>) {
        let mut defs = vec![0u32; self.temps];
        let mut uses = vec![0u32; self.temps];
        for inst in self.insts.iter().flatten() {
            if let Some(d) = inst.def() {
                if let Some(n) = defs.get_mut(d as usize) {
                    *n += 1;
                }
            }
            for u in inst.uses() {
                if let Some(n) = uses.get_mut(u as usize) {
                    *n += 1;
                }
            }
        }
        (defs, uses)
    }

    /// `dst = copy src`, then one read of `dst` further on in the same
    /// straight run, before anything writes `src`: the read takes `src`
    /// and the copy goes. Reads in the cold blocks are left alone.
    fn propagate_copies(&mut self) {
        let (defs, uses) = self.counts();
        for at in 0..self.cold.min(self.insts.len()) {
            let Some(Inst::Copy { dst, src }) = self.insts[at] else {
                continue;
            };
            if dst == src || defs[dst as usize] != 1 || uses[dst as usize] != 1 {
                continue;
            }
            let mut next = at + 1;
            while next < self.cold {
                let Some(inst) = &self.insts[next] else {
                    next += 1;
                    continue;
                };
                if matches!(inst, Inst::Label(_)) {
                    break;
                }
                if inst.uses().contains(&dst) {
                    if let Some(inst) = self.insts[next].as_mut() {
                        rename_uses(inst, dst, src);
                    }
                    self.insts[at] = None;
                    break;
                }
                // Past a write of `src`, the copy holds the old value.
                if inst.def() == Some(src) {
                    break;
                }
                next += 1;
            }
        }
    }

    /// `t = ...`, then `x = copy t` with nothing reading or writing `x` in
    /// between, nor any jump: the instruction writes `x` itself, when `t`
    /// is read only by the copy.
    fn coalesce_copies(&mut self) {
        let (defs, uses) = self.counts();
        // Where the one write of each temporary written once is.
        let mut written_at = vec![None; self.temps];
        for (at, inst) in self.insts.iter().enumerate() {
            if let Some(d) = inst.as_ref().and_then(Inst::def) {
                if defs[d as usize] == 1 {
                    written_at[d as usize] = Some(at);
                }
            }
        }
        for at in 0..self.cold.min(self.insts.len()) {
            let Some(Inst::Copy { dst, src }) = self.insts[at] else {
                continue;
            };
            if dst == src || uses[src as usize] != 1 {
                continue;
            }
            let Some(def_at) = written_at[src as usize] else {
                continue;
            };
            if def_at >= at || !renamable(self.insts[def_at].as_ref()) {
                continue;
            }
            let clear = self.insts[def_at + 1..at].iter().flatten().all(|inst| {
                !matches!(inst, Inst::Label(_))
                    && inst.target().is_none()
                    && inst.def() != Some(dst)
                    && !inst.uses().contains(&dst)
            });
            if !clear {
                continue;
            }
            if let Some(inst) = self.insts[def_at].as_mut() {
                rename_def(inst, dst);
            }
            self.insts[at] = None;
        }
    }

    /// Encode constants in the instructions that read them, where they
    /// fit.
    fn immediates(&mut self) {
        let (defs, _) = self.counts();
        let mut constant = vec![None; self.temps];
        for inst in self.insts.iter().flatten() {
            if let Inst::Const { dst, value } = inst {
                if defs[*dst as usize] == 1 {
                    constant[*dst as usize] = Some(*value);
                }
            }
        }
        let value = |t: Temp| constant.get(t as usize).copied().flatten();
        for slot in self.insts.iter_mut() {
            let Some(inst) = slot.as_mut() else {
                continue;
            };
            let replacement = match *inst {
                Inst::Binary { op, dst, lhs, rhs } => {
                    let (lhs, imm) = match (value(lhs), value(rhs)) {
                        (_, Some(imm)) => (lhs, imm),
                        // Addition is the same either way round.
                        (Some(imm), None) if op == BinOp::Add => (rhs, imm),
                        _ => continue,
                    };
                    match op {
                        BinOp::Add | BinOp::Sub if (-4095..=4095).contains(&imm) => {
                            let (op, imm) = match (op, imm < 0) {
                                (BinOp::Add, true) => (BinOp::Sub, -imm),
                                (BinOp::Sub, true) => (BinOp::Add, -imm),
                                _ => (op, imm),
                            };
                            Inst::BinaryImm { op, dst, lhs, imm }
                        }
                        BinOp::Shl | BinOp::Shr | BinOp::Sar if (0..64).contains(&imm) => {
                            Inst::BinaryImm { op, dst, lhs, imm }
                        }
                        _ => continue,
                    }
                }
                Inst::Branch {
                    cond,
                    lhs,
                    rhs,
                    target,
                } => match value(rhs) {
                    Some(imm) if (-4095..=4095).contains(&imm) => Inst::BranchImm {
                        cond,
                        lhs,
                        imm,
                        target,
                    },
                    _ => continue,
                },
                Inst::Compare {
                    cond,
                    dst,
                    lhs,
                    rhs,
                } => match value(rhs) {
                    Some(imm) if (-4095..=4095).contains(&imm) => Inst::CompareImm {
                        cond,
                        dst,
                        lhs,
                        imm,
                    },
                    _ => continue,
                },
                Inst::Arith {
                    op: op @ (Arith::Add | Arith::Sub),
                    int,
                    dst,
                    lhs,
                    rhs,
                    overflow,
                } if int.bits == 64 => match value(rhs) {
                    Some(imm) if (0..=4095).contains(&imm) => Inst::ArithImm {
                        op,
                        int,
                        dst,
                        lhs,
                        imm,
                        overflow,
                    },
                    _ => continue,
                },
                _ => continue,
            };
            *inst = replacement;
        }
    }

    /// `b = x + imm`, then a load or store at `b + offset` further on in
    /// the same straight run, before anything writes `x` or `b` again: it
    /// addresses `x + (imm + offset)` instead, which the instruction
    /// encodes, so the addition may go (`std.rt`'s word-at-a-time loops
    /// read `at + 8`, `at + 16` and so on).
    fn fold_offsets(&mut self) {
        let end = self.cold.min(self.insts.len());
        for at in 0..end {
            let Some(Inst::BinaryImm {
                op: BinOp::Add,
                dst,
                lhs,
                imm,
            }) = self.insts[at]
            else {
                continue;
            };
            if dst == lhs || !(0..=32760).contains(&imm) {
                continue;
            }
            for slot in &mut self.insts[at + 1..end] {
                let Some(inst) = slot.as_mut() else {
                    continue;
                };
                if matches!(inst, Inst::Label(_)) {
                    break;
                }
                match inst {
                    Inst::Load {
                        width,
                        base,
                        offset,
                        ..
                    }
                    | Inst::Store {
                        width,
                        base,
                        offset,
                        ..
                    } if *base == dst => {
                        let size = match width {
                            ir::Width::W8 => 1,
                            ir::Width::W16 => 2,
                            ir::Width::W32 => 4,
                            ir::Width::W64 => 8,
                        };
                        let folded = *offset + imm as u32;
                        if folded % size == 0 && folded / size < 4096 {
                            *base = lhs;
                            *offset = folded;
                        }
                    }
                    _ => {}
                }
                let inst = slot.as_ref();
                let def = inst.and_then(Inst::def);
                if def == Some(lhs)
                    || def == Some(dst)
                    || inst.is_some_and(|i| i.target().is_some())
                {
                    break;
                }
            }
        }
    }

    /// Remove instructions whose only effect is a result nothing reads,
    /// until none is left.
    fn remove_dead(&mut self) {
        loop {
            let (_, uses) = self.counts();
            let mut removed = false;
            for slot in self.insts.iter_mut() {
                let dead = slot.as_ref().is_some_and(|inst| {
                    pure(inst) && inst.def().is_some_and(|d| uses[d as usize] == 0)
                });
                if dead {
                    *slot = None;
                    removed = true;
                }
            }
            if !removed {
                return;
            }
        }
    }
}

/// Whether `inst` does nothing but write its result.
fn pure(inst: &Inst) -> bool {
    matches!(
        inst,
        Inst::Const { .. }
            | Inst::Copy { .. }
            | Inst::Secret { .. }
            | Inst::Expose { .. }
            | Inst::Addr { .. }
            | Inst::Binary { .. }
            | Inst::BinaryImm { .. }
            | Inst::Compare { .. }
            | Inst::CompareImm { .. }
            | Inst::Load { .. }
    )
}

/// Whether the result of `inst` may be written to another temporary:
/// any instruction that writes one, but [`Inst::Second`], which must
/// follow its call as it is.
fn renamable(inst: Option<&Inst>) -> bool {
    inst.is_some_and(|inst| inst.def().is_some() && !matches!(inst, Inst::Second { .. }))
}

/// Make `inst` write `to` in place of its result.
fn rename_def(inst: &mut Inst, to: Temp) {
    match inst {
        Inst::Const { dst, .. }
        | Inst::Copy { dst, .. }
        | Inst::Secret { dst, .. }
        | Inst::Expose { dst, .. }
        | Inst::Addr { dst, .. }
        | Inst::Binary { dst, .. }
        | Inst::BinaryImm { dst, .. }
        | Inst::CompareImm { dst, .. }
        | Inst::ArithImm { dst, .. }
        | Inst::Compare { dst, .. }
        | Inst::Load { dst, .. }
        | Inst::Call { dst, .. }
        | Inst::CallIndirect { dst, .. }
        | Inst::Syscall { dst, .. }
        | Inst::LibCall { dst, .. }
        | Inst::Arith { dst, .. }
        | Inst::Second { dst }
        | Inst::Depth { dst }
        | Inst::EntrySp { dst } => *dst = to,
        Inst::Store { .. }
        | Inst::Label(_)
        | Inst::Jump(_)
        | Inst::JumpIfZero { .. }
        | Inst::Branch { .. }
        | Inst::BranchImm { .. }
        | Inst::Enter { .. }
        | Inst::Leave
        | Inst::CallOnStack { .. }
        | Inst::Return(_)
        | Inst::ReturnPair { .. } => {}
    }
}

/// Make `inst` read `to` wherever it reads `from`.
fn rename_uses(inst: &mut Inst, from: Temp, to: Temp) {
    let swap = |t: &mut Temp| {
        if *t == from {
            *t = to;
        }
    };
    match inst {
        Inst::Copy { src, .. } | Inst::Secret { src, .. } | Inst::Expose { src, .. } => swap(src),
        Inst::Binary { lhs, rhs, .. }
        | Inst::Compare { lhs, rhs, .. }
        | Inst::Arith { lhs, rhs, .. }
        | Inst::Branch { lhs, rhs, .. } => {
            swap(lhs);
            swap(rhs);
        }
        Inst::BinaryImm { lhs, .. }
        | Inst::BranchImm { lhs, .. }
        | Inst::CompareImm { lhs, .. }
        | Inst::ArithImm { lhs, .. } => swap(lhs),
        Inst::Load { base, .. } => swap(base),
        Inst::Store { base, src, .. } => {
            swap(base);
            swap(src);
        }
        Inst::Call { args, .. } | Inst::Syscall { args, .. } | Inst::LibCall { args, .. } => {
            args.iter_mut().for_each(swap)
        }
        Inst::CallIndirect { target, args, .. } => {
            swap(target);
            args.iter_mut().for_each(swap);
        }
        Inst::JumpIfZero { cond, .. } => swap(cond),
        Inst::CallOnStack { stack, .. } => swap(stack),
        Inst::Return(value) => swap(value),
        Inst::ReturnPair { value, second } => {
            swap(value);
            swap(second);
        }
        Inst::Const { .. }
        | Inst::Second { .. }
        | Inst::EntrySp { .. }
        | Inst::Depth { .. }
        | Inst::Addr { .. }
        | Inst::Label(_)
        | Inst::Jump(_)
        | Inst::Enter { .. }
        | Inst::Leave => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{module, Code};
    use crate::constant_time::secrets;
    use crate::ir::{BinOp, Cond, Function, Inst, Width};

    /// In a loop over the public bound `t1`: `t3`, a secret read from the
    /// `Secret` at the parameter `t0`, copied, added to a constant, and
    /// made the condition of a choice (`ct_select`'s mask sequence)
    /// between itself and the sum, stored back through an address one
    /// addition away, next to a dead constant. Something for every pass
    /// to do, a choice it could make a branch, and a loop it must leave.
    fn secret_code() -> Function {
        Function {
            name: "f".to_string(),
            params: 2,
            temps: 20,
            labels: 2,
            insts: vec![
                Inst::Const { dst: 2, value: 0 },
                Inst::Const { dst: 9, value: 0 },
                Inst::Label(0),
                Inst::Branch {
                    cond: Cond::Ge,
                    lhs: 2,
                    rhs: 1,
                    target: 1,
                },
                Inst::Load {
                    width: Width::W64,
                    dst: 4,
                    base: 0,
                    offset: 8,
                },
                Inst::Secret { dst: 3, src: 4 },
                Inst::Copy { dst: 5, src: 3 },
                Inst::Const { dst: 6, value: 5 },
                Inst::Binary {
                    op: BinOp::Add,
                    dst: 7,
                    lhs: 5,
                    rhs: 6,
                },
                // The choice: `mask = 0 - pick`, then
                // `b ^ ((a ^ b) & mask)`.
                Inst::Const { dst: 8, value: 1 },
                Inst::Binary {
                    op: BinOp::And,
                    dst: 10,
                    lhs: 3,
                    rhs: 8,
                },
                Inst::Binary {
                    op: BinOp::Sub,
                    dst: 11,
                    lhs: 9,
                    rhs: 10,
                },
                Inst::Binary {
                    op: BinOp::Xor,
                    dst: 12,
                    lhs: 7,
                    rhs: 3,
                },
                Inst::Binary {
                    op: BinOp::And,
                    dst: 13,
                    lhs: 12,
                    rhs: 11,
                },
                Inst::Binary {
                    op: BinOp::Xor,
                    dst: 14,
                    lhs: 3,
                    rhs: 13,
                },
                Inst::BinaryImm {
                    op: BinOp::Add,
                    dst: 15,
                    lhs: 0,
                    imm: 8,
                },
                Inst::Store {
                    width: Width::W64,
                    base: 15,
                    offset: 0,
                    src: 14,
                },
                Inst::Copy { dst: 16, src: 14 },
                Inst::Const { dst: 17, value: 0 },
                Inst::BinaryImm {
                    op: BinOp::Add,
                    dst: 2,
                    lhs: 2,
                    imm: 1,
                },
                Inst::Jump(0),
                Inst::Label(1),
                Inst::Return(16),
            ],
            cold: 23,
        }
    }

    /// The mark is still there, the value stored and the value returned
    /// are still secret, the guard finds no secret branch or address, and
    /// nothing branches but the loop.
    fn assert_marked(f: &Function, pass: &str) {
        let marks = f
            .insts
            .iter()
            .filter(|i| matches!(i, Inst::Secret { .. }))
            .count();
        assert_eq!(marks, 1, "{pass}: {:?}", f.insts);
        let marked = secrets(std::slice::from_ref(f))
            .unwrap_or_else(|leak| panic!("{pass}: {leak:?}"))
            .remove(0);
        for inst in &f.insts {
            let read = match inst {
                Inst::Store { src, .. } => *src,
                Inst::Return(value) => *value,
                _ => continue,
            };
            assert!(marked[read as usize], "{pass}: {inst:?} in {:?}", f.insts);
        }
        // Only the loop's own branch and jump: the choice is no branch.
        let branches = f.insts.iter().filter(|i| i.target().is_some()).count();
        assert_eq!(branches, 2, "{pass}: {:?}", f.insts);
    }

    #[test]
    fn each_pass_leaves_a_secret_marked() {
        type Pass = fn(&mut Code);
        let passes: [(&str, Pass); 5] = [
            ("propagate_copies", Code::propagate_copies),
            ("coalesce_copies", Code::coalesce_copies),
            ("immediates", Code::immediates),
            ("fold_offsets", Code::fold_offsets),
            ("remove_dead", Code::remove_dead),
        ];
        for (name, pass) in passes {
            let mut f = secret_code();
            let mut code = Code::new(&f);
            pass(&mut code);
            code.write_back(&mut f);
            assert_marked(&f, name);
        }
        let mut f = secret_code();
        super::function(&mut f);
        assert_marked(&f, "all of them");
    }

    /// A function that marks a secret is never inlined, so its mark stays
    /// its own; one that computes on a secret argument is, and what it
    /// computes from the caller's secret stays secret there.
    #[test]
    fn inlining_keeps_marks_where_they_are_made() {
        let caller = Function {
            name: "caller".to_string(),
            params: 1,
            temps: 5,
            labels: 0,
            insts: vec![
                Inst::Load {
                    width: Width::W64,
                    dst: 3,
                    base: 0,
                    offset: 8,
                },
                Inst::Secret { dst: 4, src: 3 },
                Inst::Call {
                    dst: 1,
                    callee: 1,
                    args: vec![0],
                },
                Inst::Call {
                    dst: 2,
                    callee: 2,
                    args: vec![4],
                },
                Inst::Store {
                    width: Width::W64,
                    base: 0,
                    offset: 8,
                    src: 2,
                },
                Inst::Return(1),
            ],
            cold: 6,
        };
        let marks = Function {
            name: "marks".to_string(),
            params: 1,
            temps: 3,
            labels: 0,
            insts: vec![
                Inst::Load {
                    width: Width::W8,
                    dst: 1,
                    base: 0,
                    offset: 24,
                },
                Inst::Secret { dst: 2, src: 1 },
                Inst::Return(2),
            ],
            cold: 3,
        };
        let computes = Function {
            name: "computes".to_string(),
            params: 1,
            temps: 2,
            labels: 0,
            insts: vec![
                Inst::BinaryImm {
                    op: BinOp::Shl,
                    dst: 1,
                    lhs: 0,
                    imm: 3,
                },
                Inst::Return(1),
            ],
            cold: 2,
        };
        let mut functions = [caller, marks, computes];
        module(&mut functions);
        let caller = &functions[0];
        assert!(
            caller
                .insts
                .iter()
                .any(|i| matches!(i, Inst::Call { callee: 1, .. })),
            "the marking function is called, not inlined: {:?}",
            caller.insts
        );
        assert!(
            !caller
                .insts
                .iter()
                .any(|i| matches!(i, Inst::Call { callee: 2, .. })),
            "the computing function is inlined: {:?}",
            caller.insts
        );
        let marked = secrets(&functions).expect("the code is constant-time");
        let stored = caller.insts.iter().find_map(|i| match i {
            Inst::Store { src, .. } => Some(*src),
            _ => None,
        });
        assert!(
            stored.is_some_and(|t| marked[0][t as usize]),
            "{:?}",
            functions[0].insts
        );
        assert!(marked[1][2], "the mark is in its own function");
    }
}
