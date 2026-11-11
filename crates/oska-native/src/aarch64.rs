//! Instruction selection and encoding for aarch64.
//!
//! Every temporary lives in a stack slot of its function's frame, at
//! `[sp + 8 * temp]`, and each instruction loads its operands into
//! scratch registers, computes, and stores its result: no register
//! allocation yet. That keeps the code obviously correct; a register
//! allocator replaces the slots later, behind the same [`emit`].
//!
//! # Calling convention
//!
//! The backend's own (nothing outside a program calls into it):
//!
//! - arguments in `x0`–`x7`, in order, the receiver first; the result in
//!   `x0`; every value is one word ([`crate::layout`]);
//! - `x8` holds a system call's number, and `svc #0` makes it;
//! - `x9`–`x11` are scratch within one instruction's code;
//! - `x29` is the frame pointer and `x30` the link register, saved in
//!   every frame;
//! - every register but `sp`, `x29` and `x30` is caller-saved, which
//!   costs nothing while temporaries live in the frame.
//!
//! A frame is `[sp]`: the temporaries, 16-byte aligned; above them the
//! caller's `x29` and `x30`, where `x29` points.
//!
//! All addressing is PC-relative (`adrp`/`add`, `bl`, `b`), so the
//! executable needs no relocation: `elf.rs` patches the addresses of data
//! ([`Fixup`]) once it has laid the file out.

use crate::ir::{BinOp, Cond, FnId, Inst, Module, Symbol, Width};

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
const X8: u32 = 8;
const X9: u32 = 9;
const X10: u32 = 10;
const FP: u32 = 29;
/// `sp` as a base or `add`/`sub` operand; `xzr` elsewhere.
const SP: u32 = 31;
const XZR: u32 = 31;

// Fixed instructions.
const BL: u32 = 0x9400_0000;
const B: u32 = 0x1400_0000;
const CBZ: u32 = 0xb400_0000;
const RET: u32 = 0xd65f_03c0;
const SVC_0: u32 = 0xd400_0001;
/// `stp x29, x30, [sp, #-16]!`
const PUSH_FRAME: u32 = 0xa9bf_7bfd;
/// `ldp x29, x30, [sp], #16`
const POP_FRAME: u32 = 0xa8c1_7bfd;

#[derive(Default)]
struct Emitter {
    words: Vec<u32>,
    fixups: Vec<Fixup>,
    /// `bl` instructions to patch once every function's start is known.
    calls: Vec<(usize, FnId)>,
}

impl Emitter {
    fn put(&mut self, word: u32) {
        self.words.push(word);
    }

    fn function(&mut self, function: &crate::ir::Function) -> Result<(), String> {
        let frame = (function.temps * 8).next_multiple_of(16);
        self.put(PUSH_FRAME);
        self.put(add_imm(FP, SP, 0, false));
        if frame >= 4096 {
            self.put(sub_imm(SP, SP, frame >> 12, true));
        }
        if frame & 0xfff != 0 {
            self.put(sub_imm(SP, SP, frame & 0xfff, false));
        }
        for param in 0..function.params.min(8) {
            self.put(store_slot(param, param));
        }
        let mut labels = vec![0usize; function.labels as usize];
        let mut jumps = Vec::new();
        for inst in &function.insts {
            match inst {
                Inst::Label(label) => {
                    if let Some(at) = labels.get_mut(*label as usize) {
                        *at = self.words.len();
                    }
                }
                Inst::Jump(label) => {
                    jumps.push((self.words.len(), *label));
                    self.put(B);
                }
                Inst::JumpIfZero { cond, target } => {
                    self.put(load_slot(X9, *cond));
                    jumps.push((self.words.len(), *target));
                    self.put(CBZ | X9);
                }
                other => self.inst(other),
            }
        }
        for (at, label) in jumps {
            let target = labels.get(label as usize).copied().unwrap_or_default();
            let offset = word_offset(at, target);
            let word = self.words[at];
            self.words[at] = if word == B {
                if !fits(offset, 26) {
                    return Err(format!("`{}` is too large to branch in", function.name));
                }
                B | (offset as u32 & 0x03ff_ffff)
            } else {
                if !fits(offset, 19) {
                    return Err(format!("`{}` is too large to branch in", function.name));
                }
                word | ((offset as u32 & 0x7_ffff) << 5)
            };
        }
        Ok(())
    }

    fn inst(&mut self, inst: &Inst) {
        match inst {
            Inst::Const { dst, value } => {
                self.constant(X9, *value);
                self.put(store_slot(X9, *dst));
            }
            Inst::Copy { dst, src } => {
                self.put(load_slot(X9, *src));
                self.put(store_slot(X9, *dst));
            }
            Inst::Addr { dst, symbol } => {
                self.fixups.push(Fixup {
                    at: self.words.len(),
                    symbol: *symbol,
                });
                // Patched by the file layout: `adrp x9, symbol`, then
                // `add x9, x9, :lo12:symbol`.
                self.put(ADRP | X9);
                self.put(add_imm(X9, X9, 0, false));
                self.put(store_slot(X9, *dst));
            }
            Inst::Binary { op, dst, lhs, rhs } => {
                self.put(load_slot(X9, *lhs));
                self.put(load_slot(X10, *rhs));
                let base = match op {
                    BinOp::Add => 0x8b00_0000,
                    BinOp::Sub => 0xcb00_0000,
                    BinOp::And => 0x8a00_0000,
                    BinOp::Or => 0xaa00_0000,
                    BinOp::Xor => 0xca00_0000,
                    BinOp::Shl => 0x9ac0_2000,
                    BinOp::Shr => 0x9ac0_2400,
                    BinOp::Sar => 0x9ac0_2800,
                };
                self.put(reg3(base, X9, X9, X10));
                self.put(store_slot(X9, *dst));
            }
            Inst::Compare {
                cond,
                dst,
                lhs,
                rhs,
            } => {
                self.put(load_slot(X9, *lhs));
                self.put(load_slot(X10, *rhs));
                // `cmp x9, x10`, then `cset x9, cond`.
                self.put(reg3(0xeb00_0000, XZR, X9, X10));
                self.put(0x9a9f_07e0 | ((condition(*cond) ^ 1) << 12) | X9);
                self.put(store_slot(X9, *dst));
            }
            Inst::Load {
                width,
                dst,
                base,
                offset,
            } => {
                self.put(load_slot(X9, *base));
                self.memory(*width, true, X9, *offset);
                self.put(store_slot(X9, *dst));
            }
            Inst::Store {
                width,
                base,
                offset,
                src,
            } => {
                self.put(load_slot(X9, *base));
                self.put(load_slot(X10, *src));
                // The value is in x10 for a store; `memory` addresses x9.
                self.memory(*width, false, X9, *offset);
            }
            Inst::Call { dst, callee, args } => {
                for (register, arg) in args.iter().enumerate() {
                    self.put(load_slot(register as u32, *arg));
                }
                self.calls.push((self.words.len(), *callee));
                self.put(BL);
                self.put(store_slot(X0, *dst));
            }
            Inst::Syscall { dst, number, args } => {
                for (register, arg) in args.iter().enumerate() {
                    self.put(load_slot(register as u32, *arg));
                }
                self.put(movz(X8, u32::from(*number), 0));
                self.put(SVC_0);
                self.put(store_slot(X0, *dst));
            }
            Inst::Return(value) => {
                self.put(load_slot(X0, *value));
                self.put(add_imm(SP, FP, 0, false));
                self.put(POP_FRAME);
                self.put(RET);
            }
            // Control flow is placed by `function`.
            Inst::Label(_) | Inst::Jump(_) | Inst::JumpIfZero { .. } => {}
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

    /// A load into `x9` from, or a store of `x10` to, `[base + offset]`.
    fn memory(&mut self, width: Width, load: bool, base: u32, offset: u32) {
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
        let rt = if load { X9 } else { X10 };
        if offset % size == 0 && offset / size < 4096 {
            self.put(immediate | (offset / size) << 10 | base << 5 | rt);
        } else {
            self.constant(11, i64::from(offset));
            self.put(register | 11 << 16 | base << 5 | rt);
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

/// `ldr rt, [sp, #8 * temp]`.
fn load_slot(rt: u32, temp: u32) -> u32 {
    0xf940_0000 | (temp & 0xfff) << 10 | SP << 5 | rt
}

/// `str rt, [sp, #8 * temp]`.
fn store_slot(rt: u32, temp: u32) -> u32 {
    0xf900_0000 | (temp & 0xfff) << 10 | SP << 5 | rt
}

/// The condition code of `cond`.
fn condition(cond: Cond) -> u32 {
    match cond {
        Cond::Eq => 0,
        Cond::Ne => 1,
        Cond::AboveEq => 2,
        Cond::Below => 3,
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
