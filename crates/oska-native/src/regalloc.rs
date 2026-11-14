//! Register allocation: where each temporary of a function lives, a
//! register or a stack slot, for the whole function.
//!
//! A linear scan over live intervals:
//!
//! - A temporary's interval runs from the first to the last instruction
//!   of the hot path that writes or reads it (the parameters are written
//!   on entry). An interval that overlaps a loop, without lying inside it,
//!   is stretched over the whole loop, since its value is needed again
//!   after the backward jump. Every temporary the lowering makes is
//!   written before it is read on every path, so an interval inside a
//!   loop carries nothing around it.
//! - A cold block ([`Function::cold`]) reads the hot path's temporaries
//!   exactly as they were at the one jump to it, so its reads count at
//!   that jump. A temporary a cold block writes gets a slot.
//! - An interval that spans a call gets one of the target's callee-saved
//!   registers ([`Registers::callee_saved`]); one that does not may also
//!   get a caller-saved one ([`Registers::caller_saved`]). When none is
//!   free, the interval used least (each use weighted by how deeply it is
//!   nested in loops) goes to a stack slot.

use crate::ir::{Function, Inst, Temp};

/// Where a temporary lives for the whole function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loc {
    /// A register, by the target's number for it: one of its
    /// [`Registers`].
    Reg(u32),
    /// A stack slot, at `[sp + 8 * slot]`.
    Slot(u32),
}

/// The registers a target gives temporaries.
#[derive(Debug, Clone, Copy)]
pub struct Registers {
    /// Those that hold temporaries across calls, saved by the function
    /// that uses them.
    pub callee_saved: &'static [u32],
    /// Those that hold temporaries between calls, tried first.
    pub caller_saved: &'static [u32],
}

/// The locations of a function's temporaries.
#[derive(Debug)]
pub struct Allocation {
    /// By temporary.
    pub locs: Vec<Loc>,
    /// How many stack slots the frame needs.
    pub slots: u32,
    /// The callee-saved registers used, which the function saves.
    pub saved: Vec<u32>,
}

/// One temporary's live interval, in positions: the entry is 0, and
/// instruction `i` of the hot path is `i + 1`.
#[derive(Debug, Clone, Copy)]
struct Interval {
    start: usize,
    end: usize,
    /// How much a register for it saves: its uses, each weighted by how
    /// deeply it is nested in loops.
    weight: u64,
    /// Whether a call happens inside it.
    crosses_call: bool,
}

/// Choose a location for every temporary of `function` among
/// `registers` and stack slots.
pub fn allocate(function: &Function, registers: &Registers) -> Allocation {
    let intervals = intervals(function);
    let mut locs: Vec<Option<u32>> = vec![None; function.temps as usize];

    let mut order: Vec<Temp> = (0..function.temps)
        .filter(|&t| intervals[t as usize].is_some())
        .collect();
    order.sort_by_key(|&t| intervals[t as usize].map(|i| i.start));
    let interval = |t: Temp| {
        intervals[t as usize].unwrap_or(Interval {
            start: 0,
            end: 0,
            weight: 0,
            crosses_call: false,
        })
    };

    let mut active: Vec<(Temp, u32)> = Vec::new();
    for t in order {
        let here = interval(t);
        active.retain(|&(a, _)| interval(a).end > here.start);
        let taken =
            |register: u32, active: &[(Temp, u32)]| active.iter().any(|&(_, r)| r == register);
        let allowed: Vec<u32> = if here.crosses_call {
            registers.callee_saved.to_vec()
        } else {
            registers
                .caller_saved
                .iter()
                .chain(registers.callee_saved)
                .copied()
                .collect()
        };
        if let Some(&register) = allowed.iter().find(|&&r| !taken(r, &active)) {
            locs[t as usize] = Some(register);
            active.push((t, register));
            continue;
        }
        // No register is free: the least used of this interval and those
        // holding a register it may take goes to a slot.
        let victim = active
            .iter()
            .enumerate()
            .filter(|(_, (_, r))| allowed.contains(r))
            .min_by_key(|(_, (a, _))| interval(*a).weight);
        if let Some((index, &(a, register))) = victim {
            if interval(a).weight < here.weight {
                locs[a as usize] = None;
                active.remove(index);
                locs[t as usize] = Some(register);
                active.push((t, register));
            }
        }
    }

    let mut slots = 0;
    let mut saved = Vec::new();
    let locs = locs
        .into_iter()
        .map(|loc| match loc {
            Some(register) => {
                if registers.callee_saved.contains(&register) && !saved.contains(&register) {
                    saved.push(register);
                }
                Loc::Reg(register)
            }
            None => {
                slots += 1;
                Loc::Slot(slots - 1)
            }
        })
        .collect();
    saved.sort_unstable();
    Allocation { locs, slots, saved }
}

/// The live interval of each temporary that the hot path writes or
/// reads; `None` for those only cold blocks use, which get slots.
fn intervals(function: &Function) -> Vec<Option<Interval>> {
    let insts = &function.insts;
    let cold = function.cold.min(insts.len());
    let hot = &insts[..cold];
    let labels = function.labels as usize;

    // Where each label is in the hot path, and the hot instruction that
    // jumps to each.
    let mut label_at = vec![None; labels];
    let mut jumped_from = vec![None; labels];
    let mut loops = Vec::new();
    for (i, inst) in hot.iter().enumerate() {
        if let Inst::Label(label) = inst {
            if let Some(at) = label_at.get_mut(*label as usize) {
                *at = Some(i + 1);
            }
        }
    }
    for (i, inst) in hot.iter().enumerate() {
        if let Some(target) = inst.target() {
            if let Some(from) = jumped_from.get_mut(target as usize) {
                from.get_or_insert(i + 1);
            }
            if let Some(Some(head)) = label_at.get(target as usize) {
                if *head <= i + 1 {
                    loops.push((*head, i + 1));
                }
            }
        }
    }
    let depth = |position: usize| {
        loops
            .iter()
            .filter(|&&(head, back)| head <= position && position <= back)
            .count()
    };

    let mut ranges: Vec<Option<Interval>> = vec![None; function.temps as usize];
    let mut touch = |t: Temp, position: usize, weight: u64| {
        if let Some(slot) = ranges.get_mut(t as usize) {
            let range = slot.get_or_insert(Interval {
                start: position,
                end: position,
                weight: 0,
                crosses_call: false,
            });
            range.start = range.start.min(position);
            range.end = range.end.max(position);
            range.weight = range.weight.saturating_add(weight);
        }
    };
    for param in 0..function.params {
        touch(param, 0, 1);
    }
    for (i, inst) in hot.iter().enumerate() {
        let weight = 10u64.pow(depth(i + 1).min(6) as u32);
        for t in inst.def().into_iter().chain(inst.uses()) {
            touch(t, i + 1, weight);
        }
    }
    // A cold block's reads of the hot path's temporaries count at its
    // jump; what it writes lives in slots.
    let written_cold: Vec<Temp> = insts[cold..].iter().filter_map(Inst::def).collect();
    let mut jump = None;
    for inst in &insts[cold..] {
        if let Inst::Label(label) = inst {
            jump = jumped_from.get(*label as usize).copied().flatten();
        }
        if let Some(position) = jump {
            for t in inst.uses() {
                if !written_cold.contains(&t) {
                    touch(t, position, 1);
                }
            }
        }
    }
    for t in written_cold {
        if let Some(slot) = ranges.get_mut(t as usize) {
            *slot = None;
        }
    }

    // Stretch intervals over the loops they overlap, until none changes.
    let mut changed = true;
    while changed {
        changed = false;
        for range in ranges.iter_mut().flatten() {
            for &(head, back) in &loops {
                let overlaps = range.start <= back && head <= range.end;
                let inside = head <= range.start && range.end <= back;
                if overlaps && !inside && (range.start > head || range.end < back) {
                    range.start = range.start.min(head);
                    range.end = range.end.max(back);
                    changed = true;
                }
            }
        }
    }

    let calls: Vec<usize> = hot
        .iter()
        .enumerate()
        .filter(|(_, inst)| {
            matches!(
                inst,
                Inst::Call { .. } | Inst::CallIndirect { .. } | Inst::Syscall { .. }
            )
        })
        .map(|(i, _)| i + 1)
        .collect();
    for range in ranges.iter_mut().flatten() {
        range.crosses_call = calls.iter().any(|&c| range.start < c && c < range.end);
    }
    ranges
}
