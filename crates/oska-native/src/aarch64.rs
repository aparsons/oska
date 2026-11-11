//! Instruction selection and encoding for aarch64.
//!
//! Each temporary has one location for the whole function, which
//! `regalloc.rs` chooses: a register, or a stack slot of its frame at
//! `[sp + 8 * slot]`. Each instruction reads its operands from their
//! locations (a slot's through a scratch register), computes, and writes
//! its result to its location.
//!
//! # Calling convention
//!
//! The backend's own (nothing outside a program calls into it):
//!
//! - arguments in `x0`–`x7`, in order, the receiver first; the result in
//!   `x0`, and a mutating method's receiver, its second result, in `x1`
//!   ([`Inst::ReturnPair`]); every value is one word ([`crate::layout`]);
//! - `x8` holds a system call's number, and `svc #0` makes it;
//! - `x9`–`x11`, `x16` and `x17` are scratch within one instruction's
//!   code;
//! - `x19`–`x27` hold temporaries ([`regalloc::CALLEE_SAVED`]) and are
//!   callee-saved: a function saves those it uses; `x2`–`x8` and
//!   `x12`–`x15` hold temporaries no call happens during
//!   ([`regalloc::CALLER_SAVED`]): arguments and parameters move to and
//!   from them as one parallel move ([`Emitter::parallel_move`]);
//! - `x28` counts how many more calls may nest ([`Inst::Enter`]): the
//!   entry point sets it, and nothing else writes it;
//! - `x29` is the frame pointer and `x30` the link register, saved in
//!   every frame;
//! - every other register is caller-saved.
//!
//! A frame is `[sp]`: the stack slots; above them the callee-saved
//! registers the function uses; above those the caller's `x29` and
//! `x30`, where `x29` points. Its size is a multiple of 16.
//!
//! All addressing is PC-relative (`adrp`/`add`, `bl`, `b`), so the
//! executable needs no relocation: `elf.rs` patches the addresses of data
//! ([`Fixup`]) once it has laid the file out.

use crate::ir::{Arith, BinOp, Cond, FnId, Function, Inst, IntKind, Module, Symbol, Temp, Width};
use crate::regalloc::{self, Loc};

/// The machine code of a module.
#[derive(Debug)]
pub struct Text {
    /// The instructions, each one word.
    pub words: Vec<u32>,
    /// The instructions that address data, for the file layout to patch.
    pub fixups: Vec<Fixup>,
    /// The instruction the process starts at, by index.
    pub entry: usize,
}

/// An `adrp` at `at`, followed by an `add`, that must point at `symbol`.
#[derive(Debug, Clone, Copy)]
pub struct Fixup {
    pub at: usize,
    pub symbol: Symbol,
}

/// Select and encode `module`'s instructions. Fails only when the code is
/// too large for a branch to reach.
pub fn emit(module: &Module) -> Result<Text, String> {
    let mut emitter = Emitter::default();
    let mut starts = Vec::with_capacity(module.functions.len());
    for function in &module.functions {
        starts.push(emitter.words.len());
        emitter.function(function)?;
    }
    for &(at, callee) in &emitter.calls {
        let target = starts.get(callee).copied().unwrap_or_default();
        let offset = word_offset(at, target);
        if !fits(offset, 26) {
            return Err("the program's code is too large to call across".to_string());
        }
        emitter.words[at] = BL | (offset as u32 & 0x03ff_ffff);
    }
    Ok(Text {
        words: emitter.words,
        fixups: emitter.fixups,
        entry: starts.get(module.entry).copied().unwrap_or_default(),
    })
}

// Registers.
const X0: u32 = 0;
const X1: u32 = 1;
const X8: u32 = 8;
const X9: u32 = 9;
const X10: u32 = 10;
const X11: u32 = 11;
const X16: u32 = 16;
const X17: u32 = 17;
/// How many more calls may nest.
const DEPTH: u32 = 28;
const FP: u32 = 29;
/// `sp` as a base or `add`/`sub` operand; `xzr` elsewhere.
const SP: u32 = 31;
const XZR: u32 = 31;

// Fixed instructions.
const BL: u32 = 0x9400_0000;
const B: u32 = 0x1400_0000;
const B_COND: u32 = 0x5400_0000;
const CBZ: u32 = 0xb400_0000;
const CBNZ: u32 = 0xb500_0000;
const RET: u32 = 0xd65f_03c0;
const SVC_0: u32 = 0xd400_0001;
/// `stp x29, x30, [sp, #-16]!`
const PUSH_FRAME: u32 = 0xa9bf_7bfd;
/// `ldp x29, x30, [sp], #16`
const POP_FRAME: u32 = 0xa8c1_7bfd;

// Condition codes.
const NE: u32 = 1;
const HS: u32 = 2;
const LO: u32 = 3;
const VS: u32 = 6;

/// How a branch to a label is encoded, for patching.
#[derive(Clone, Copy)]
enum Jump {
    /// `b`: a 26-bit offset.
    Always,
    /// `cbz`, `cbnz` or `b.cond`: a 19-bit offset at bit 5.
    Short,
}

#[derive(Default)]
struct Emitter {
    words: Vec<u32>,
    fixups: Vec<Fixup>,
    /// `bl` instructions to patch once every function's start is known.
    calls: Vec<(usize, FnId)>,
    /// The location of each temporary of the function being emitted.
    locs: Vec<Loc>,
    /// The callee-saved registers it uses.
    saved: Vec<u32>,
    /// Branches to labels, to patch once the function's labels are placed.
    jumps: Vec<(usize, u32, Jump)>,
}

impl Emitter {
    fn put(&mut self, word: u32) {
        self.words.push(word);
    }

    fn function(&mut self, function: &Function) -> Result<(), String> {
        let allocation = regalloc::allocate(function);
        self.locs = allocation.locs;
        self.saved = allocation.saved;
        let frame = (8 * (allocation.slots + self.saved.len() as u32)).next_multiple_of(16);
        self.put(PUSH_FRAME);
        self.put(add_imm(FP, SP, 0, false));
        if frame >= 4096 {
            self.put(sub_imm(SP, SP, frame >> 12, true));
        }
        if frame & 0xfff != 0 {
            self.put(sub_imm(SP, SP, frame & 0xfff, false));
        }
        self.save_registers(true);
        let params: Vec<(Loc, Loc)> = (0..function.params.min(8))
            .map(|param| (self.loc(param), Loc::Reg(param)))
            .collect();
        self.parallel_move(params);
        let mut labels = vec![0usize; function.labels as usize];
        self.jumps.clear();
        for inst in &function.insts {
            match inst {
                Inst::Label(label) => {
                    if let Some(at) = labels.get_mut(*label as usize) {
                        *at = self.words.len();
                    }
                }
                other => self.inst(other),
            }
        }
        for &(at, label, kind) in &self.jumps {
            let target = labels.get(label as usize).copied().unwrap_or_default();
            let offset = word_offset(at, target);
            let word = self.words[at];
            self.words[at] = match kind {
                Jump::Always if fits(offset, 26) => word | (offset as u32 & 0x03ff_ffff),
                Jump::Short if fits(offset, 19) => word | ((offset as u32 & 0x7_ffff) << 5),
                _ => return Err(format!("`{}` is too large to branch in", function.name)),
            };
        }
        Ok(())
    }

    /// A branch to `label`, encoded as `word` before its offset.
    fn jump(&mut self, word: u32, label: u32, kind: Jump) {
        self.jumps.push((self.words.len(), label, kind));
        self.put(word);
    }

    /// The register holding `temp`: its own, or `scratch` loaded from its
    /// slot.
    fn read(&mut self, temp: Temp, scratch: u32) -> u32 {
        match self.loc(temp) {
            Loc::Reg(register) => register,
            Loc::Slot(slot) => {
                self.put(load_slot(scratch, slot));
                scratch
            }
        }
    }

    /// Read `temp` into the register `target` itself.
    fn read_into(&mut self, target: u32, temp: Temp) {
        match self.loc(temp) {
            Loc::Reg(register) => self.mov(target, register),
            Loc::Slot(slot) => self.put(load_slot(target, slot)),
        }
    }

    /// Put `args` in the argument registers, `x0` on.
    fn arguments(&mut self, args: &[Temp]) {
        let moves = args
            .iter()
            .enumerate()
            .map(|(register, &arg)| (Loc::Reg(register as u32), self.loc(arg)))
            .collect();
        self.parallel_move(moves);
    }

    /// Make each `(to, from)` move as if all at once: a register that is
    /// another move's source is written only after that move is made,
    /// and a cycle of registers is broken through `x16`.
    fn parallel_move(&mut self, moves: Vec<(Loc, Loc)>) {
        let mut pending: Vec<(Loc, Loc)> =
            moves.into_iter().filter(|(to, from)| to != from).collect();
        while !pending.is_empty() {
            let free = (0..pending.len()).find(|&i| {
                let to = pending[i].0;
                !matches!(to, Loc::Reg(_))
                    || pending
                        .iter()
                        .enumerate()
                        .all(|(j, &(_, from))| j == i || from != to)
            });
            match free {
                Some(i) => {
                    let (to, from) = pending.remove(i);
                    self.move_loc(to, from);
                }
                None => {
                    // Every destination is a pending source: a cycle.
                    let held = pending[0].1;
                    self.move_loc(Loc::Reg(X16), held);
                    for (_, from) in pending.iter_mut() {
                        if *from == held {
                            *from = Loc::Reg(X16);
                        }
                    }
                }
            }
        }
    }

    /// One move between locations; a slot to a slot goes through `x17`.
    fn move_loc(&mut self, to: Loc, from: Loc) {
        match (to, from) {
            (Loc::Reg(to), Loc::Reg(from)) => self.mov(to, from),
            (Loc::Reg(to), Loc::Slot(slot)) => self.put(load_slot(to, slot)),
            (Loc::Slot(slot), Loc::Reg(from)) => self.put(store_slot(from, slot)),
            (Loc::Slot(to), Loc::Slot(from)) => {
                if to != from {
                    self.put(load_slot(X17, from));
                    self.put(store_slot(X17, to));
                }
            }
        }
    }

    /// The register to compute `temp` in: its own, or `scratch`, which
    /// [`Emitter::write`] then stores.
    fn target(&self, temp: Temp, scratch: u32) -> u32 {
        match self.loc(temp) {
            Loc::Reg(register) => register,
            Loc::Slot(_) => scratch,
        }
    }

    /// Put the value in `register` in `temp`'s location.
    fn write(&mut self, temp: Temp, register: u32) {
        match self.loc(temp) {
            Loc::Reg(own) => self.mov(own, register),
            Loc::Slot(slot) => self.put(store_slot(register, slot)),
        }
    }

    fn loc(&self, temp: Temp) -> Loc {
        self.locs
            .get(temp as usize)
            .copied()
            .unwrap_or(Loc::Slot(0))
    }

    fn mov(&mut self, rd: u32, rm: u32) {
        if rd != rm {
            self.put(reg3(0xaa00_0000, rd, XZR, rm));
        }
    }

    fn inst(&mut self, inst: &Inst) {
        match inst {
            Inst::Const { dst, value } => {
                let r = self.target(*dst, X9);
                self.constant(r, *value);
                self.write(*dst, r);
            }
            Inst::Copy { dst, src } => {
                let s = self.read(*src, X9);
                self.write(*dst, s);
            }
            Inst::Addr { dst, symbol } => {
                let r = self.target(*dst, X9);
                self.fixups.push(Fixup {
                    at: self.words.len(),
                    symbol: *symbol,
                });
                // Patched by the file layout: `adrp r, symbol`, then
                // `add r, r, :lo12:symbol`.
                self.put(ADRP | r);
                self.put(add_imm(r, r, 0, false));
                self.write(*dst, r);
            }
            Inst::Binary { op, dst, lhs, rhs } => {
                let a = self.read(*lhs, X9);
                let b = self.read(*rhs, X10);
                let r = self.target(*dst, X9);
                let base = match op {
                    BinOp::Add => 0x8b00_0000,
                    BinOp::Sub => 0xcb00_0000,
                    BinOp::Mul => 0x9b00_7c00,
                    BinOp::And => 0x8a00_0000,
                    BinOp::Or => 0xaa00_0000,
                    BinOp::Xor => 0xca00_0000,
                    BinOp::Shl => 0x9ac0_2000,
                    BinOp::Shr => 0x9ac0_2400,
                    BinOp::Sar => 0x9ac0_2800,
                };
                self.put(reg3(base, r, a, b));
                self.write(*dst, r);
            }
            Inst::BinaryImm { op, dst, lhs, imm } => {
                let a = self.read(*lhs, X9);
                let r = self.target(*dst, X9);
                let n = (*imm as u32) & 0xfff;
                let word = match op {
                    BinOp::Sub => sub_imm(r, a, n, false),
                    // `lsl` is `ubfm r, a, #(-n mod 64), #(63 - n)`.
                    BinOp::Shl => bitfield(UBFM, r, a, (64 - n) % 64, 63 - (n & 63)),
                    BinOp::Shr => bitfield(UBFM, r, a, n & 63, 63),
                    BinOp::Sar => bitfield(SBFM, r, a, n & 63, 63),
                    _ => add_imm(r, a, n, false),
                };
                self.put(word);
                self.write(*dst, r);
            }
            Inst::CompareImm {
                cond,
                dst,
                lhs,
                imm,
            } => {
                let a = self.read(*lhs, X9);
                let r = self.target(*dst, X9);
                self.put(cmp_imm(a, *imm));
                self.put(0x9a9f_07e0 | ((condition(*cond) ^ 1) << 12) | r);
                self.write(*dst, r);
            }
            Inst::BranchImm {
                cond,
                lhs,
                imm,
                target,
            } => {
                let a = self.read(*lhs, X9);
                self.put(cmp_imm(a, *imm));
                self.jump(B_COND | condition(*cond), *target, Jump::Short);
            }
            Inst::ArithImm {
                op,
                int,
                dst,
                lhs,
                imm,
                overflow,
            } => {
                let a = self.read(*lhs, X9);
                let n = (*imm as u32) & 0xfff;
                // `adds`/`subs x16, a, #n`, then the flag that says the
                // result does not fit.
                let (word, flag) = match (op, int.signed) {
                    (Arith::Sub, true) => (0xf100_0000, VS),
                    (Arith::Sub, false) => (0xf100_0000, LO),
                    (_, true) => (0xb100_0000, VS),
                    (_, false) => (0xb100_0000, HS),
                };
                self.put(word | n << 10 | a << 5 | X16);
                self.jump(B_COND | flag, *overflow, Jump::Short);
                self.write(*dst, X16);
            }
            Inst::Compare {
                cond,
                dst,
                lhs,
                rhs,
            } => {
                let a = self.read(*lhs, X9);
                let b = self.read(*rhs, X10);
                let r = self.target(*dst, X9);
                self.put(cmp(a, b));
                // `cset r, cond`.
                self.put(0x9a9f_07e0 | ((condition(*cond) ^ 1) << 12) | r);
                self.write(*dst, r);
            }
            Inst::Load {
                width,
                dst,
                base,
                offset,
            } => {
                let b = self.read(*base, X9);
                let r = self.target(*dst, X9);
                self.memory(*width, true, r, b, *offset);
                self.write(*dst, r);
            }
            Inst::Store {
                width,
                base,
                offset,
                src,
            } => {
                let b = self.read(*base, X9);
                let v = self.read(*src, X10);
                self.memory(*width, false, v, b, *offset);
            }
            Inst::Call { dst, callee, args } => {
                self.arguments(args);
                self.calls.push((self.words.len(), *callee));
                self.put(BL);
                self.write(*dst, X0);
            }
            Inst::Syscall { dst, number, args } => {
                self.arguments(args);
                self.put(movz(X8, u32::from(*number), 0));
                self.put(SVC_0);
                self.write(*dst, X0);
            }
            Inst::Arith {
                op,
                int,
                dst,
                lhs,
                rhs,
                overflow,
            } => {
                let a = self.read(*lhs, X9);
                let b = self.read(*rhs, X10);
                self.arith(*op, *int, a, b, *overflow);
                // The result is in x16; nothing was written before the
                // check, so on overflow every operand is intact.
                self.write(*dst, X16);
            }
            Inst::Jump(label) => self.jump(B, *label, Jump::Always),
            Inst::JumpIfZero { cond, target } => {
                let c = self.read(*cond, X9);
                self.jump(CBZ | c, *target, Jump::Short);
            }
            Inst::Branch {
                cond,
                lhs,
                rhs,
                target,
            } => {
                let a = self.read(*lhs, X9);
                let b = self.read(*rhs, X10);
                self.put(cmp(a, b));
                self.jump(B_COND | condition(*cond), *target, Jump::Short);
            }
            Inst::Enter { exhausted } => {
                self.jump(CBZ | DEPTH, *exhausted, Jump::Short);
                self.put(sub_imm(DEPTH, DEPTH, 1, false));
            }
            Inst::Leave => self.put(add_imm(DEPTH, DEPTH, 1, false)),
            Inst::CallOnStack {
                stack,
                depth,
                callee,
            } => {
                let s = self.read(*stack, X9);
                self.put(add_imm(SP, s, 0, false));
                self.constant(DEPTH, i64::from(*depth));
                self.calls.push((self.words.len(), *callee));
                self.put(BL);
            }
            Inst::Return(value) => {
                self.read_into(X0, *value);
                self.epilogue();
            }
            Inst::ReturnPair { value, second } => {
                self.parallel_move(vec![
                    (Loc::Reg(X1), self.loc(*second)),
                    (Loc::Reg(X0), self.loc(*value)),
                ]);
                self.epilogue();
            }
            // Straight after the call, before anything touches `x1`.
            Inst::Second { dst } => self.write(*dst, X1),
            // `_start`'s prologue pushed the frame record (16 bytes) on
            // the kernel's stack pointer, and `x29` points at it.
            Inst::EntrySp { dst } => {
                let r = self.target(*dst, X9);
                self.put(add_imm(r, FP, 16, false));
                self.write(*dst, r);
            }
            // Placed by `function`.
            Inst::Label(_) => {}
        }
    }

    /// Store (or, when not `store`, load) the callee-saved registers the
    /// function uses, below the frame record: register `i` of
    /// [`Emitter::saved`] at `[x29 - 8 * (i + 1)]`, two at a time.
    fn save_registers(&mut self, store: bool) {
        let saved = self.saved.clone();
        for (pair, registers) in saved.chunks(2).enumerate() {
            let offset = -16 * (pair as i32 + 1);
            let word = match (registers, store) {
                (&[low, high], true) => pair_access(STP, high, low, offset),
                (&[low, high], false) => pair_access(LDP, high, low, offset),
                (&[only], true) => stur(only, FP, offset + 8),
                (&[only], false) => ldur(only, FP, offset + 8),
                _ => continue,
            };
            self.put(word);
        }
    }

    /// Restore the callee-saved registers and the caller's frame, and
    /// return.
    fn epilogue(&mut self) {
        self.save_registers(false);
        self.put(add_imm(SP, FP, 0, false));
        self.put(POP_FRAME);
        self.put(RET);
    }

    /// `x16 = a op b`, checked against `int`: a branch to `overflow` when
    /// the result does not fit. Writes only scratch registers.
    fn arith(&mut self, op: Arith, int: IntKind, a: u32, b: u32, overflow: u32) {
        let short = |e: &mut Emitter, word: u32| e.jump(word, overflow, Jump::Short);
        match (op, int.signed, int.bits) {
            (Arith::Add, true, 64) => {
                self.put(reg3(0xab00_0000, X16, a, b));
                short(self, B_COND | VS);
            }
            (Arith::Sub, true, 64) => {
                self.put(reg3(0xeb00_0000, X16, a, b));
                short(self, B_COND | VS);
            }
            (Arith::Mul, true, 64) => {
                // The high half of the 128-bit product must be the low
                // half's sign.
                self.put(madd(X16, a, b, XZR));
                self.put(reg3(0x9b40_7c00, X17, a, b));
                // `cmp x17, x16, asr #63`
                self.put(0xeb80_0000 | X16 << 16 | 63 << 10 | X17 << 5 | XZR);
                short(self, B_COND | NE);
            }
            (Arith::Div, true, 64) => {
                // MIN / -1 is the one quotient that overflows.
                // `cmn b, #1`; `b.ne +3`; `negs xzr, a` (V when a is MIN).
                self.put(0xb100_0400 | b << 5 | XZR);
                self.put(B_COND | NE | 3 << 5);
                self.put(reg3(0xeb00_0000, XZR, XZR, a));
                short(self, B_COND | VS);
                self.put(reg3(0x9ac0_0c00, X16, a, b));
            }
            (Arith::Add, false, 64) => {
                self.put(reg3(0xab00_0000, X16, a, b));
                short(self, B_COND | HS);
            }
            (Arith::Sub, false, 64) => {
                self.put(reg3(0xeb00_0000, X16, a, b));
                short(self, B_COND | LO);
            }
            (Arith::Mul, false, 64) => {
                self.put(reg3(0x9bc0_7c00, X17, a, b));
                short(self, CBNZ | X17);
                self.put(madd(X16, a, b, XZR));
            }
            (Arith::Div, false, 64) => self.put(reg3(0x9ac0_0800, X16, a, b)),
            (Arith::Rem, signed, 64) => {
                let div = if signed { 0x9ac0_0c00 } else { 0x9ac0_0800 };
                self.put(reg3(div, X17, a, b));
                self.put(msub(X16, X17, b, a));
            }
            (op, signed, bits) => {
                // A sized type narrower than a word: its values are
                // extended words, so the word operation cannot overflow
                // and the result fits exactly when extending its low
                // `bits` gives it back.
                match op {
                    Arith::Add => self.put(reg3(0x8b00_0000, X16, a, b)),
                    Arith::Sub => self.put(reg3(0xcb00_0000, X16, a, b)),
                    Arith::Mul => self.put(madd(X16, a, b, XZR)),
                    Arith::Div if signed => self.put(reg3(0x9ac0_0c00, X16, a, b)),
                    Arith::Div => self.put(reg3(0x9ac0_0800, X16, a, b)),
                    Arith::Rem => {
                        let div = if signed { 0x9ac0_0c00 } else { 0x9ac0_0800 };
                        self.put(reg3(div, X17, a, b));
                        self.put(msub(X16, X17, b, a));
                    }
                }
                if signed {
                    // `sbfm x17, x16, #0, #bits-1`, then `cmp x17, x16`.
                    self.put(0x9340_0000 | (bits - 1) << 10 | X16 << 5 | X17);
                    self.put(cmp(X17, X16));
                    short(self, B_COND | NE);
                } else {
                    // `lsr x17, x16, #bits`: anything left overflowed.
                    self.put(0xd340_fc00 | bits << 16 | X16 << 5 | X17);
                    short(self, CBNZ | X17);
                }
            }
        }
    }

    /// Put `value` in `rd`.
    fn constant(&mut self, rd: u32, value: i64) {
        let bits = value as u64;
        let chunk = |i: u32| ((bits >> (16 * i)) & 0xffff) as u32;
        if (!bits) >> 16 == 0 {
            // A small negative: one `movn`.
            self.put(0x9280_0000 | ((!bits & 0xffff) as u32) << 5 | rd);
            return;
        }
        self.put(movz(rd, chunk(0), 0));
        for i in 1..4 {
            if chunk(i) != 0 {
                self.put(0xf280_0000 | i << 21 | chunk(i) << 5 | rd);
            }
        }
    }

    /// A load into, or a store from, `rt` at `[base + offset]`.
    fn memory(&mut self, width: Width, load: bool, rt: u32, base: u32, offset: u32) {
        let (size, immediate, register) = match (width, load) {
            (Width::W8, true) => (1, 0x3940_0000, 0x3860_6800),
            (Width::W16, true) => (2, 0x7940_0000, 0x7860_6800),
            (Width::W32, true) => (4, 0xb940_0000, 0xb860_6800),
            (Width::W64, true) => (8, 0xf940_0000, 0xf860_6800),
            (Width::W8, false) => (1, 0x3900_0000, 0x3820_6800),
            (Width::W16, false) => (2, 0x7900_0000, 0x7820_6800),
            (Width::W32, false) => (4, 0xb900_0000, 0xb820_6800),
            (Width::W64, false) => (8, 0xf900_0000, 0xf820_6800),
        };
        if offset % size == 0 && offset / size < 4096 {
            self.put(immediate | (offset / size) << 10 | base << 5 | rt);
        } else {
            self.constant(X11, i64::from(offset));
            self.put(register | X11 << 16 | base << 5 | rt);
        }
    }
}

/// `adrp`, before its page offset is patched in.
pub const ADRP: u32 = 0x9000_0000;

/// The `adrp` that puts the page of `target` in `rd`, for an instruction
/// at `pc`.
pub fn adrp(rd: u32, pc: u64, target: u64) -> u32 {
    let pages = (target >> 12) as i64 - (pc >> 12) as i64;
    let pages = pages as u32;
    ADRP | (pages & 3) << 29 | ((pages >> 2) & 0x7_ffff) << 5 | rd
}

/// `add rd, rn, #imm12` (shifted left 12 when `shifted`).
pub fn add_imm(rd: u32, rn: u32, imm12: u32, shifted: bool) -> u32 {
    0x9100_0000 | u32::from(shifted) << 22 | (imm12 & 0xfff) << 10 | rn << 5 | rd
}

fn sub_imm(rd: u32, rn: u32, imm12: u32, shifted: bool) -> u32 {
    0xd100_0000 | u32::from(shifted) << 22 | (imm12 & 0xfff) << 10 | rn << 5 | rd
}

fn movz(rd: u32, imm16: u32, hw: u32) -> u32 {
    0xd280_0000 | hw << 21 | (imm16 & 0xffff) << 5 | rd
}

fn reg3(base: u32, rd: u32, rn: u32, rm: u32) -> u32 {
    base | rm << 16 | rn << 5 | rd
}

/// `cmp rn, rm`.
fn cmp(rn: u32, rm: u32) -> u32 {
    reg3(0xeb00_0000, XZR, rn, rm)
}

/// `cmp rn, #imm`, or `cmn rn, #-imm` for a negative `imm`; `|imm|` is
/// below 4096.
fn cmp_imm(rn: u32, imm: i64) -> u32 {
    let (base, n) = if imm < 0 {
        (0xb100_0000, (-imm) as u32)
    } else {
        (0xf100_0000, imm as u32)
    };
    base | (n & 0xfff) << 10 | rn << 5 | XZR
}

/// `ubfm`, 64-bit.
const UBFM: u32 = 0xd340_0000;
/// `sbfm`, 64-bit.
const SBFM: u32 = 0x9340_0000;

/// A bitfield move (`UBFM` or `SBFM`) of `rn` to `rd` with `immr` and
/// `imms`.
fn bitfield(base: u32, rd: u32, rn: u32, immr: u32, imms: u32) -> u32 {
    base | (immr & 63) << 16 | (imms & 63) << 10 | rn << 5 | rd
}

/// `madd rd, rn, rm, ra`: `ra + rn * rm`.
fn madd(rd: u32, rn: u32, rm: u32, ra: u32) -> u32 {
    0x9b00_0000 | rm << 16 | ra << 10 | rn << 5 | rd
}

/// `msub rd, rn, rm, ra`: `ra - rn * rm`.
fn msub(rd: u32, rn: u32, rm: u32, ra: u32) -> u32 {
    0x9b00_8000 | rm << 16 | ra << 10 | rn << 5 | rd
}

/// `stp`, 64-bit, signed offset.
const STP: u32 = 0xa900_0000;
/// `ldp`, 64-bit, signed offset.
const LDP: u32 = 0xa940_0000;

/// `stp`/`ldp first, second, [x29, #offset]`: `first` at `offset`,
/// `second` 8 bytes above, for an offset a multiple of 8 from -512 to 504.
fn pair_access(base: u32, first: u32, second: u32, offset: i32) -> u32 {
    base | (((offset / 8) as u32) & 0x7f) << 15 | second << 10 | FP << 5 | first
}

/// `stur rt, [rn, #offset]`, for an offset from -256 to 255.
fn stur(rt: u32, rn: u32, offset: i32) -> u32 {
    0xf800_0000 | ((offset as u32) & 0x1ff) << 12 | rn << 5 | rt
}

/// `ldur rt, [rn, #offset]`, for an offset from -256 to 255.
fn ldur(rt: u32, rn: u32, offset: i32) -> u32 {
    0xf840_0000 | ((offset as u32) & 0x1ff) << 12 | rn << 5 | rt
}

/// `ldr rt, [sp, #8 * slot]`.
fn load_slot(rt: u32, slot: u32) -> u32 {
    0xf940_0000 | (slot & 0xfff) << 10 | SP << 5 | rt
}

/// `str rt, [sp, #8 * slot]`.
fn store_slot(rt: u32, slot: u32) -> u32 {
    0xf900_0000 | (slot & 0xfff) << 10 | SP << 5 | rt
}

/// The condition code of `cond`.
fn condition(cond: Cond) -> u32 {
    match cond {
        Cond::Eq => 0,
        Cond::Ne => NE,
        Cond::AboveEq => HS,
        Cond::Below => LO,
        Cond::Above => 8,
        Cond::BelowEq => 9,
        Cond::Ge => 10,
        Cond::Lt => 11,
        Cond::Gt => 12,
        Cond::Le => 13,
    }
}

/// The distance in instructions from `from` to `to`.
fn word_offset(from: usize, to: usize) -> i64 {
    to as i64 - from as i64
}

/// Whether `offset` fits a signed field of `bits` bits.
fn fits(offset: i64, bits: u32) -> bool {
    let limit = 1i64 << (bits - 1);
    (-limit..limit).contains(&offset)
}
