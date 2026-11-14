//! Instruction selection for x86_64; `encode.rs` encodes each instruction.
//!
//! Each temporary has one location for the whole function, which
//! `regalloc.rs` chooses: a register, or a stack slot of its frame at
//! `[rsp + 8 * slot]`. Each instruction reads its operands from their
//! locations (a slot's through a scratch register), computes, and writes
//! its result to its location, as on aarch64 (`aarch64.rs`).
//!
//! # Calling convention
//!
//! The backend's own, in the System V style (nothing outside a program
//! calls into it):
//!
//! - arguments in `rdi`, `rsi`, `rdx`, `rcx`, `r8` and `r9`, in order,
//!   the receiver first, and any past the sixth in a fixed area of the
//!   writable data ([`Module::extra_args`]): the caller stores them just
//!   before its `call`, and the callee's entry loads them before any
//!   other code runs; the result in `rax`, and a mutating method's
//!   receiver, its second result, in `rdx` ([`Inst::ReturnPair`]);
//! - a system call takes its number in `rax` and its arguments in `rdi`,
//!   `rsi`, `rdx`, `r10`, `r8` and `r9`, and `syscall` makes it;
//! - `rax`, `rcx`, `rdx`, `r10` and `r11` are scratch within one
//!   instruction's code: division takes `rdx:rax`, a shift its count in
//!   `cl`, and `syscall` writes `rcx` and `r11`;
//! - `rbx` and `r12`–`r14` hold temporaries ([`CALLEE_SAVED`]) and are
//!   callee-saved: a function saves those it uses; `rsi`, `rdi`, `r8` and
//!   `r9` hold temporaries no call happens during ([`CALLER_SAVED`]):
//!   arguments and parameters move to and from them as one parallel move
//!   ([`Emitter::parallel_move`]);
//! - `r15` counts how many more calls may nest ([`Inst::Enter`]): the
//!   entry point sets it, and nothing else writes it;
//! - `rbp` is the frame pointer, and `rsp` the stack pointer.
//!
//! A frame is `[rsp]`: the stack slots; above them the callee-saved
//! registers the function uses; above those the caller's `rbp`, where
//! `rbp` points, and the return address. Its size, without those two, is a
//! multiple of 16.
//!
//! All addressing is relative to `rip` (`lea`, `call`, `jmp`), so the
//! executable needs no relocation: [`Text::link`] patches the addresses
//! of data ([`Fixup`]) once the file has been laid out.

mod encode;

use encode::{
    Alu, Asm, Mem, Shift, Size, Unary, A, AE, B, BE, E, G, GE, L, LE, NE, O, R10, R11, R12, R13,
    R14, R15, R8, R9, RAX, RBP, RBX, RCX, RDI, RDX, RSI, RSP,
};

use crate::ir::{Arith, BinOp, Cond, FnId, Function, Inst, IntKind, Module, Symbol, Temp, Width};
use crate::regalloc::{self, Loc, Registers};
use crate::target::{Code, Layout};

/// How many arguments a call passes in registers ([`ARGS`]).
pub const REGISTER_ARGS: usize = 6;

/// The argument registers, in order.
const ARGS: [u8; REGISTER_ARGS] = [RDI, RSI, RDX, RCX, R8, R9];

/// A system call's argument registers, in order.
const SYSCALL_ARGS: [u8; 6] = [RDI, RSI, RDX, R10, R8, R9];

/// The registers that hold temporaries across calls.
const CALLEE_SAVED: [u32; 4] = [RBX as u32, R12 as u32, R13 as u32, R14 as u32];

/// The registers that hold temporaries between calls: argument registers
/// that are no instruction's scratch.
const CALLER_SAVED: [u32; 4] = [RSI as u32, RDI as u32, R8 as u32, R9 as u32];

const REGISTERS: Registers = Registers {
    callee_saved: &CALLEE_SAVED,
    caller_saved: &CALLER_SAVED,
};

/// How many more calls may nest.
const DEPTH: u8 = R15;

/// The machine code of a module.
#[derive(Debug)]
pub struct Text {
    pub bytes: Vec<u8>,
    /// The `rip`-relative displacements that address data or code, for
    /// the file layout to patch. A [`Symbol::Function`] here holds the
    /// offset of the function's first byte, not its id.
    pub fixups: Vec<Fixup>,
    /// Where the process starts, in bytes.
    pub entry: usize,
}

impl Code for Text {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn entry(&self) -> u64 {
        self.entry as u64
    }

    fn link(&self, layout: &Layout) -> Vec<u8> {
        let mut bytes = self.bytes.clone();
        for fixup in &self.fixups {
            let target = match fixup.symbol {
                Symbol::Rodata(at) => layout.rodata + u64::from(at),
                Symbol::Data(at) => layout.data + u64::from(at),
                Symbol::Function(start) => layout.code + start as u64,
            };
            // Relative to the end of the instruction, which the
            // displacement ends.
            let next = layout.code + fixup.at as u64 + 4;
            let disp = target.wrapping_sub(next) as i32;
            bytes[fixup.at..fixup.at + 4].copy_from_slice(&disp.to_le_bytes());
        }
        bytes
    }
}

/// A `rip`-relative displacement at `at` that must point at `symbol`.
#[derive(Debug, Clone, Copy)]
pub struct Fixup {
    pub at: usize,
    pub symbol: Symbol,
}

/// Select and encode `module`'s instructions. Fails only when the code is
/// too large for a branch to reach.
pub fn emit(module: &Module) -> Result<Text, String> {
    let mut emitter = Emitter {
        extra_args: module.extra_args,
        ..Emitter::default()
    };
    let mut starts = Vec::with_capacity(module.functions.len());
    for function in &module.functions {
        starts.push(emitter.asm.bytes.len());
        emitter.function(function)?;
    }
    for &(at, callee) in &emitter.calls {
        let target = starts.get(callee).copied().unwrap_or_default();
        let offset = displacement(at, target)
            .ok_or_else(|| "the program's code is too large to call across".to_string())?;
        emitter.asm.patch32(at, offset);
    }
    for fixup in &mut emitter.fixups {
        if let Symbol::Function(callee) = fixup.symbol {
            let start = starts.get(callee).copied().unwrap_or_default();
            fixup.symbol = Symbol::Function(start);
        }
    }
    Ok(Text {
        bytes: emitter.asm.bytes,
        fixups: emitter.fixups,
        entry: starts.get(module.entry).copied().unwrap_or_default(),
    })
}

/// The displacement from the end of the 4 bytes at `at` to `target`, if
/// it fits 32 bits.
fn displacement(at: usize, target: usize) -> Option<i32> {
    i32::try_from(target as i64 - (at as i64 + 4)).ok()
}

#[derive(Default)]
struct Emitter {
    asm: Asm,
    fixups: Vec<Fixup>,
    /// `call` displacements to patch once every function's start is known.
    calls: Vec<(usize, FnId)>,
    /// The location of each temporary of the function being emitted.
    locs: Vec<Loc>,
    /// The callee-saved registers it uses.
    saved: Vec<u8>,
    /// Jump displacements to patch once the function's labels are placed.
    jumps: Vec<(usize, u32)>,
    /// [`Module::extra_args`].
    extra_args: u32,
}

impl Emitter {
    fn function(&mut self, function: &Function) -> Result<(), String> {
        let allocation = regalloc::allocate(function, &REGISTERS);
        self.locs = allocation.locs;
        self.saved = allocation.saved.iter().map(|&r| r as u8).collect();
        let frame = (8 * (allocation.slots + self.saved.len() as u32)).next_multiple_of(16);
        self.asm.push(RBP);
        self.asm.mov(RBP, RSP);
        if frame != 0 {
            self.asm.alu_imm(Alu::Sub, RSP, frame as i32);
        }
        for (i, &register) in self.saved.clone().iter().enumerate() {
            self.asm.store(Size::Q, saved_at(i), register);
        }
        // A parameter no code reads may share its location with a later
        // one, whose move is the one to make: the last move into each
        // location stays.
        let mut params: Vec<(Loc, Loc)> = Vec::new();
        for param in (0..function.params.min(REGISTER_ARGS as u32)).rev() {
            let to = self.loc(param);
            if params.iter().all(|&(taken, _)| taken != to) {
                params.push((to, reg(ARGS[param as usize])));
            }
        }
        self.parallel_move(params);
        // The parameters past the sixth, from where the caller stored
        // them. A parameter no code reads may share its location with a
        // later one, so they are loaded in order, after the registers.
        if function.params > REGISTER_ARGS as u32 {
            self.extra_args_address();
            for (index, param) in (REGISTER_ARGS as u32..function.params).enumerate() {
                self.asm.load(Size::Q, R10, offset(R11, 8 * index as i32));
                let to = self.loc(param);
                self.move_loc(to, reg(R10));
            }
        }
        let mut labels = vec![0usize; function.labels as usize];
        self.jumps.clear();
        for inst in &function.insts {
            match inst {
                Inst::Label(label) => {
                    if let Some(at) = labels.get_mut(*label as usize) {
                        *at = self.asm.bytes.len();
                    }
                }
                other => self.inst(other),
            }
        }
        for &(at, label) in &self.jumps {
            let target = labels.get(label as usize).copied().unwrap_or_default();
            let offset = displacement(at, target)
                .ok_or_else(|| format!("`{}` is too large to branch in", function.name))?;
            self.asm.patch32(at, offset);
        }
        Ok(())
    }

    /// A jump to `label` when `cc` holds, or always without one.
    fn jump(&mut self, cc: Option<u8>, label: u32) {
        let at = match cc {
            Some(cc) => self.asm.jcc(cc),
            None => self.asm.jmp(),
        };
        self.jumps.push((at, label));
    }

    fn loc(&self, temp: Temp) -> Loc {
        self.locs
            .get(temp as usize)
            .copied()
            .unwrap_or(Loc::Slot(0))
    }

    /// The register holding `temp`: its own, or `scratch` loaded from its
    /// slot.
    fn read(&mut self, temp: Temp, scratch: u8) -> u8 {
        match self.loc(temp) {
            Loc::Reg(register) => register as u8,
            Loc::Slot(slot) => {
                self.asm.load(Size::Q, scratch, slot_at(slot));
                scratch
            }
        }
    }

    /// Read `temp` into the register `target` itself.
    fn read_into(&mut self, target: u8, temp: Temp) {
        let from = self.loc(temp);
        self.move_loc(reg(target), from);
    }

    /// The register to compute `temp` in: its own, or `scratch`, which
    /// [`Emitter::write`] then stores.
    fn target(&self, temp: Temp, scratch: u8) -> u8 {
        match self.loc(temp) {
            Loc::Reg(register) => register as u8,
            Loc::Slot(_) => scratch,
        }
    }

    /// Put the value in `register` in `temp`'s location.
    fn write(&mut self, temp: Temp, register: u8) {
        let to = self.loc(temp);
        self.move_loc(to, reg(register));
    }

    fn mov(&mut self, dst: u8, src: u8) {
        if dst != src {
            self.asm.mov(dst, src);
        }
    }

    /// One move between locations; a slot to a slot goes through `r10`.
    fn move_loc(&mut self, to: Loc, from: Loc) {
        match (to, from) {
            (Loc::Reg(to), Loc::Reg(from)) => self.mov(to as u8, from as u8),
            (Loc::Reg(to), Loc::Slot(slot)) => self.asm.load(Size::Q, to as u8, slot_at(slot)),
            (Loc::Slot(slot), Loc::Reg(from)) => self.asm.store(Size::Q, slot_at(slot), from as u8),
            (Loc::Slot(to), Loc::Slot(from)) => {
                if to != from {
                    self.asm.load(Size::Q, R10, slot_at(from));
                    self.asm.store(Size::Q, slot_at(to), R10);
                }
            }
        }
    }

    /// Make each `(to, from)` move as if all at once: a register that is
    /// another move's source is written only after that move is made,
    /// and a cycle of registers is broken through `r11`.
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
                    self.move_loc(reg(R11), held);
                    for (_, from) in pending.iter_mut() {
                        if *from == held {
                            *from = reg(R11);
                        }
                    }
                }
            }
        }
    }

    /// Put `args` in the argument registers, and those past the sixth in
    /// the extra arguments' area, first, while every argument is still in
    /// its own location.
    fn arguments(&mut self, args: &[Temp]) {
        if let Some(extra) = args.get(REGISTER_ARGS..).filter(|extra| !extra.is_empty()) {
            self.extra_args_address();
            for (index, &arg) in extra.iter().enumerate() {
                let value = self.read(arg, R10);
                self.asm
                    .store(Size::Q, offset(R11, 8 * index as i32), value);
            }
        }
        let moves = args
            .iter()
            .zip(ARGS)
            .map(|(&arg, register)| (reg(register), self.loc(arg)))
            .collect();
        self.parallel_move(moves);
    }

    /// `r11 =` the address of the extra arguments' area.
    fn extra_args_address(&mut self) {
        let at = self.asm.lea_rip(R11);
        self.fixups.push(Fixup {
            at,
            symbol: Symbol::Data(self.extra_args),
        });
    }

    fn inst(&mut self, inst: &Inst) {
        match inst {
            Inst::Const { dst, value } => {
                let r = self.target(*dst, RAX);
                self.asm.constant(r, *value);
                self.write(*dst, r);
            }
            Inst::Copy { dst, src } => {
                let s = self.read(*src, RAX);
                self.write(*dst, s);
            }
            Inst::Addr { dst, symbol } => {
                let r = self.target(*dst, RAX);
                let at = self.asm.lea_rip(r);
                self.fixups.push(Fixup {
                    at,
                    symbol: *symbol,
                });
                self.write(*dst, r);
            }
            Inst::Binary { op, dst, lhs, rhs } => self.binary(*op, *dst, *lhs, *rhs),
            Inst::BinaryImm { op, dst, lhs, imm } => {
                let a = self.read(*lhs, RAX);
                let r = self.target(*dst, RAX);
                self.mov(r, a);
                match op {
                    BinOp::Sub => self.asm.alu_imm(Alu::Sub, r, *imm as i32),
                    BinOp::Shl => self.asm.shift_imm(Shift::Shl, r, (*imm & 63) as u8),
                    BinOp::Shr => self.asm.shift_imm(Shift::Shr, r, (*imm & 63) as u8),
                    BinOp::Sar => self.asm.shift_imm(Shift::Sar, r, (*imm & 63) as u8),
                    _ => self.asm.alu_imm(Alu::Add, r, *imm as i32),
                }
                self.write(*dst, r);
            }
            Inst::CompareImm {
                cond,
                dst,
                lhs,
                imm,
            } => {
                let a = self.read(*lhs, RAX);
                self.asm.alu_imm(Alu::Cmp, a, *imm as i32);
                self.set(*cond, *dst);
            }
            Inst::BranchImm {
                cond,
                lhs,
                imm,
                target,
            } => {
                let a = self.read(*lhs, RAX);
                self.asm.alu_imm(Alu::Cmp, a, *imm as i32);
                self.jump(Some(condition(*cond)), *target);
            }
            Inst::ArithImm {
                op,
                int,
                dst,
                lhs,
                imm,
                overflow,
            } => {
                // In `r11`, then the flag that says the result does not
                // fit; nothing is written before the check, so on
                // overflow every operand is intact.
                let a = self.read(*lhs, RAX);
                self.asm.mov(R11, a);
                let alu = if *op == Arith::Sub {
                    Alu::Sub
                } else {
                    Alu::Add
                };
                self.asm.alu_imm(alu, R11, *imm as i32);
                self.jump(Some(if int.signed { O } else { B }), *overflow);
                self.write(*dst, R11);
            }
            Inst::Compare {
                cond,
                dst,
                lhs,
                rhs,
            } => {
                let a = self.read(*lhs, RAX);
                let b = self.read(*rhs, RCX);
                self.asm.alu(Alu::Cmp, a, b);
                self.set(*cond, *dst);
            }
            Inst::Load {
                width,
                dst,
                base,
                offset,
            } => {
                let b = self.read(*base, R10);
                let r = self.target(*dst, RAX);
                let mem = self.address(b, *offset);
                self.asm.load(size(*width), r, mem);
                self.write(*dst, r);
            }
            Inst::Store {
                width,
                base,
                offset,
                src,
            } => {
                let b = self.read(*base, R10);
                let v = self.read(*src, RAX);
                let mem = self.address(b, *offset);
                self.asm.store(size(*width), mem, v);
            }
            Inst::Call { dst, callee, args } => {
                self.arguments(args);
                let at = self.asm.call();
                self.calls.push((at, *callee));
                self.write(*dst, RAX);
            }
            Inst::CallIndirect { dst, target, args } => {
                // `rax` is no temporary's and no argument's: the moves
                // into the argument registers leave it alone.
                self.read_into(RAX, *target);
                self.arguments(args);
                self.asm.call_reg(RAX);
                self.write(*dst, RAX);
            }
            Inst::Syscall { dst, number, args } => {
                let moves = args
                    .iter()
                    .zip(SYSCALL_ARGS)
                    .map(|(&arg, register)| (reg(register), self.loc(arg)))
                    .collect();
                self.parallel_move(moves);
                self.asm.constant(RAX, i64::from(*number));
                self.asm.syscall();
                self.write(*dst, RAX);
            }
            Inst::Arith {
                op,
                int,
                dst,
                lhs,
                rhs,
                overflow,
            } => {
                let a = self.read(*lhs, R10);
                let b = self.read(*rhs, RCX);
                self.arith(*op, *int, a, b, *overflow);
                // The result is in r11; nothing was written before the
                // check, so on overflow every operand is intact.
                self.write(*dst, R11);
            }
            Inst::Jump(label) => self.jump(None, *label),
            Inst::JumpIfZero { cond, target } => {
                let c = self.read(*cond, RAX);
                self.asm.test(c, c);
                self.jump(Some(E), *target);
            }
            Inst::Branch {
                cond,
                lhs,
                rhs,
                target,
            } => {
                let a = self.read(*lhs, RAX);
                let b = self.read(*rhs, RCX);
                self.asm.alu(Alu::Cmp, a, b);
                self.jump(Some(condition(*cond)), *target);
            }
            Inst::Enter { exhausted } => {
                self.asm.test(DEPTH, DEPTH);
                self.jump(Some(E), *exhausted);
                self.asm.alu_imm(Alu::Sub, DEPTH, 1);
            }
            Inst::Leave => self.asm.alu_imm(Alu::Add, DEPTH, 1),
            Inst::CallOnStack {
                stack,
                depth,
                callee,
            } => {
                let s = self.read(*stack, RAX);
                self.asm.mov(RSP, s);
                self.asm.constant(DEPTH, i64::from(*depth));
                let at = self.asm.call();
                self.calls.push((at, *callee));
            }
            Inst::Return(value) => {
                self.read_into(RAX, *value);
                self.epilogue();
            }
            Inst::ReturnPair { value, second } => {
                self.parallel_move(vec![
                    (reg(RDX), self.loc(*second)),
                    (reg(RAX), self.loc(*value)),
                ]);
                self.epilogue();
            }
            // Straight after the call, before anything touches `rdx`.
            Inst::Second { dst } => self.write(*dst, RDX),
            // `_start`'s prologue pushed `rbp` on the kernel's stack
            // pointer, and `rbp` points at it.
            Inst::EntrySp { dst } => {
                let r = self.target(*dst, RAX);
                self.asm.lea(r, offset(RBP, 8));
                self.write(*dst, r);
            }
            // Placed by `function`.
            Inst::Label(_) => {}
        }
    }

    /// `dst = lhs op rhs`, in `dst`'s register when it has one.
    fn binary(&mut self, op: BinOp, dst: Temp, lhs: Temp, rhs: Temp) {
        let shift = match op {
            BinOp::Shl => Some(Shift::Shl),
            BinOp::Shr => Some(Shift::Shr),
            BinOp::Sar => Some(Shift::Sar),
            _ => None,
        };
        if let Some(shift) = shift {
            // The amount in `cl`, which holds no temporary.
            self.read_into(RCX, rhs);
            let a = self.read(lhs, RAX);
            let r = self.target(dst, RAX);
            self.mov(r, a);
            self.asm.shift_cl(shift, r);
            self.write(dst, r);
            return;
        }
        let a = self.read(lhs, RAX);
        let b = self.read(rhs, RCX);
        let mut r = self.target(dst, RAX);
        let commutes = !matches!(op, BinOp::Sub);
        let (first, second) = if r == b && r != a {
            if commutes {
                (b, a)
            } else {
                // `r` holds `b`, which `a - b` still needs.
                r = R11;
                (a, b)
            }
        } else {
            (a, b)
        };
        self.mov(r, first);
        match op {
            BinOp::Mul => self.asm.imul(r, second),
            BinOp::Sub => self.asm.alu(Alu::Sub, r, second),
            BinOp::And => self.asm.alu(Alu::And, r, second),
            BinOp::Or => self.asm.alu(Alu::Or, r, second),
            BinOp::Xor => self.asm.alu(Alu::Xor, r, second),
            _ => self.asm.alu(Alu::Add, r, second),
        }
        self.write(dst, r);
    }

    /// `dst = 1` if the flags say `cond`, else 0.
    fn set(&mut self, cond: Cond, dst: Temp) {
        let r = self.target(dst, RAX);
        self.asm.setcc(condition(cond), r);
        self.asm.zero_extend_byte(r);
        self.write(dst, r);
    }

    /// The memory operand `[base + offset]`: an offset past a 32-bit
    /// displacement goes in `r11`.
    fn address(&mut self, base: u8, offset: u32) -> Mem {
        match i32::try_from(offset) {
            Ok(disp) => self::offset(base, disp),
            Err(_) => {
                self.asm.constant(R11, i64::from(offset));
                Mem {
                    base,
                    index: Some(R11),
                    disp: 0,
                }
            }
        }
    }

    /// Restore the callee-saved registers and the caller's frame, and
    /// return.
    fn epilogue(&mut self) {
        for (i, &register) in self.saved.clone().iter().enumerate() {
            self.asm.load(Size::Q, register, saved_at(i));
        }
        self.asm.leave();
        self.asm.ret();
    }

    /// `r11 = a op b`, checked against `int`: a jump to `overflow` when the
    /// result does not fit. Writes only scratch registers; `a` and `b` are
    /// none of `rax`, `rdx` and `r11`.
    fn arith(&mut self, op: Arith, int: IntKind, a: u8, b: u8, overflow: u32) {
        match (op, int.signed, int.bits) {
            (Arith::Add | Arith::Sub, signed, 64) => {
                let alu = if op == Arith::Add { Alu::Add } else { Alu::Sub };
                self.asm.mov(R11, a);
                self.asm.alu(alu, R11, b);
                // Signed overflow sets OF; an unsigned carry or borrow, CF.
                self.jump(Some(if signed { O } else { B }), overflow);
            }
            (Arith::Mul, true, 64) => {
                // OF when the product does not fit a word.
                self.asm.mov(R11, a);
                self.asm.imul(R11, b);
                self.jump(Some(O), overflow);
            }
            (Arith::Mul, false, 64) => {
                // OF when the high word of `rdx:rax` is not zero.
                self.asm.mov(RAX, a);
                self.asm.unary(Unary::Mul, b);
                self.jump(Some(O), overflow);
                self.asm.mov(R11, RAX);
            }
            (Arith::Div, true, 64) => {
                // MIN / -1 is the one quotient that overflows (and `idiv`
                // traps on it): when `b` is -1, `-a` must not overflow.
                self.asm.alu_imm(Alu::Cmp, b, -1);
                let skip = self.asm.jcc8(NE);
                self.asm.mov(R11, a);
                self.asm.unary(Unary::Neg, R11);
                self.jump(Some(O), overflow);
                self.asm.land8(skip);
                self.divide(true, a, b);
                self.asm.mov(R11, RAX);
            }
            (Arith::Rem, true, 64) => {
                // Any remainder by -1 is 0; `idiv` would trap on MIN.
                self.asm.alu_imm(Alu::Cmp, b, -1);
                let divide = self.asm.jcc8(NE);
                self.asm.constant(R11, 0);
                let done = self.asm.jmp8();
                self.asm.land8(divide);
                self.divide(true, a, b);
                self.asm.mov(R11, RDX);
                self.asm.land8(done);
            }
            (Arith::Div, false, 64) => {
                self.divide(false, a, b);
                self.asm.mov(R11, RAX);
            }
            (Arith::Rem, false, 64) => {
                self.divide(false, a, b);
                self.asm.mov(R11, RDX);
            }
            (op, signed, bits) => {
                // A sized type narrower than a word: its values are
                // extended words, so the word operation cannot overflow
                // (nor a division trap) and the result fits exactly when
                // extending its low `bits` gives it back.
                match op {
                    Arith::Add | Arith::Sub | Arith::Mul => {
                        self.asm.mov(R11, a);
                        match op {
                            Arith::Add => self.asm.alu(Alu::Add, R11, b),
                            Arith::Sub => self.asm.alu(Alu::Sub, R11, b),
                            _ => self.asm.imul(R11, b),
                        }
                    }
                    Arith::Div => {
                        self.divide(signed, a, b);
                        self.asm.mov(R11, RAX);
                    }
                    Arith::Rem => {
                        self.divide(signed, a, b);
                        self.asm.mov(R11, RDX);
                    }
                }
                if signed {
                    self.asm.sign_extend(RAX, R11, bits);
                    self.asm.alu(Alu::Cmp, RAX, R11);
                } else {
                    // Anything left above `bits` overflowed.
                    self.asm.mov(RAX, R11);
                    self.asm.shift_imm(Shift::Shr, RAX, bits as u8);
                }
                self.jump(Some(NE), overflow);
            }
        }
    }

    /// `rax = a / b` and `rdx = a % b`, signed or not.
    fn divide(&mut self, signed: bool, a: u8, b: u8) {
        self.asm.mov(RAX, a);
        if signed {
            self.asm.cqo();
            self.asm.unary(Unary::Idiv, b);
        } else {
            self.asm.constant(RDX, 0);
            self.asm.unary(Unary::Div, b);
        }
    }
}

fn reg(register: u8) -> Loc {
    Loc::Reg(u32::from(register))
}

fn offset(base: u8, disp: i32) -> Mem {
    Mem {
        base,
        index: None,
        disp,
    }
}

/// Stack slot `slot`: `[rsp + 8 * slot]`.
fn slot_at(slot: u32) -> Mem {
    offset(RSP, 8 * slot as i32)
}

/// Where the `i`th saved callee-saved register is: `[rbp - 8 * (i + 1)]`.
fn saved_at(i: usize) -> Mem {
    offset(RBP, -8 * (i as i32 + 1))
}

fn size(width: Width) -> Size {
    match width {
        Width::W8 => Size::B,
        Width::W16 => Size::W,
        Width::W32 => Size::D,
        Width::W64 => Size::Q,
    }
}

/// The condition code of `cond`, after `cmp lhs, rhs`.
fn condition(cond: Cond) -> u8 {
    match cond {
        Cond::Eq => E,
        Cond::Ne => NE,
        Cond::Lt => L,
        Cond::Le => LE,
        Cond::Gt => G,
        Cond::Ge => GE,
        Cond::Below => B,
        Cond::BelowEq => BE,
        Cond::Above => A,
        Cond::AboveEq => AE,
    }
}
