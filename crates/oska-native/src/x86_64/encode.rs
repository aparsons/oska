//! x86_64 instruction encoding: each instruction form the code generator
//! emits, as the Intel 64 and IA-32 Architectures Software Developer's
//! Manual, Volume 2, encodes it. Every form operates on 64-bit registers
//! unless its name says otherwise; `tests.rs` holds each against the
//! manual's bytes.
//!
//! A register is its number in the manual's Table 3-1, 0 to 15: the low
//! three bits go in ModRM or SIB, the high one in a REX prefix.

pub const RAX: u8 = 0;
pub const RCX: u8 = 1;
pub const RDX: u8 = 2;
pub const RBX: u8 = 3;
pub const RSP: u8 = 4;
pub const RBP: u8 = 5;
pub const RSI: u8 = 6;
pub const RDI: u8 = 7;
pub const R8: u8 = 8;
pub const R9: u8 = 9;
pub const R10: u8 = 10;
pub const R11: u8 = 11;
pub const R12: u8 = 12;
pub const R13: u8 = 13;
pub const R14: u8 = 14;
pub const R15: u8 = 15;

// Condition codes (the manual's Table B-10, `tttn`).
pub const O: u8 = 0x0;
pub const B: u8 = 0x2;
pub const AE: u8 = 0x3;
pub const E: u8 = 0x4;
pub const NE: u8 = 0x5;
pub const BE: u8 = 0x6;
pub const A: u8 = 0x7;
pub const L: u8 = 0xc;
pub const GE: u8 = 0xd;
pub const LE: u8 = 0xe;
pub const G: u8 = 0xf;

/// A two-operand arithmetic or logic instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alu {
    Add,
    Or,
    And,
    Sub,
    Xor,
    Cmp,
}

impl Alu {
    /// The `/digit` of its immediate forms (`81` and `83`).
    fn digit(self) -> u8 {
        match self {
            Alu::Add => 0,
            Alu::Or => 1,
            Alu::And => 4,
            Alu::Sub => 5,
            Alu::Xor => 6,
            Alu::Cmp => 7,
        }
    }

    /// The opcode of its `op r64, r/m64` form.
    fn opcode(self) -> u8 {
        self.digit() << 3 | 3
    }
}

/// A shift of a whole word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Shl,
    Shr,
    Sar,
}

impl Shift {
    fn digit(self) -> u8 {
        match self {
            Shift::Shl => 4,
            Shift::Shr => 5,
            Shift::Sar => 7,
        }
    }
}

/// A group-3 instruction (`F7 /digit`) of one register operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unary {
    Neg,
    /// `rdx:rax = rax * r`, unsigned.
    Mul,
    /// `rax, rdx = rdx:rax / r, rdx:rax % r`, unsigned.
    Div,
    /// The same, signed.
    Idiv,
}

impl Unary {
    fn digit(self) -> u8 {
        match self {
            Unary::Neg => 3,
            Unary::Mul => 4,
            Unary::Div => 6,
            Unary::Idiv => 7,
        }
    }
}

/// The width of a memory access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    B,
    W,
    D,
    Q,
}

/// A memory operand: `[base + index + disp]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mem {
    pub base: u8,
    /// Never `rsp`.
    pub index: Option<u8>,
    pub disp: i32,
}

/// Machine code being written.
#[derive(Debug, Default)]
pub struct Asm {
    pub bytes: Vec<u8>,
}

/// What a REX prefix carries: W for a 64-bit operand, and the high bits
/// of ModRM.reg (R), SIB.index (X) and ModRM.rm or SIB.base (B).
fn rex(w: bool, reg: u8, index: u8, base: u8) -> u8 {
    0x40 | u8::from(w) << 3 | (reg >> 3) << 2 | (index >> 3) << 1 | base >> 3
}

fn fits_i8(value: i32) -> bool {
    i8::try_from(value).is_ok()
}

impl Asm {
    fn put(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// The prefix for `w`, `reg` and `base`, when it says anything. A
    /// byte register 4 to 7 (`spl` to `dil`, not `ah` to `bh`) needs one
    /// even with no bit set (`byte`).
    fn prefix(&mut self, w: bool, reg: u8, index: u8, base: u8, byte: bool) {
        let prefix = rex(w, reg, index, base);
        if prefix != 0x40 || byte {
            self.put(&[prefix]);
        }
    }

    /// `opcode` with ModRM addressing the register `rm`, `reg` in its reg
    /// field.
    fn op_rr(&mut self, w: bool, opcode: &[u8], reg: u8, rm: u8) {
        self.prefix(w, reg, 0, rm, false);
        self.put(opcode);
        self.put(&[0xc0 | (reg & 7) << 3 | (rm & 7)]);
    }

    /// `opcode` with ModRM (and SIB and displacement) addressing `mem`,
    /// `reg` in its reg field. `byte` when `reg` is a byte register.
    fn op_mem(&mut self, w: bool, opcode: &[u8], reg: u8, mem: Mem, byte: bool) {
        let index = mem.index.unwrap_or(0);
        let byte = byte && (4..8).contains(&reg);
        self.prefix(w, reg, index, mem.base, byte);
        self.put(opcode);
        // `rbp` and `r13` as a base with mod 00 would mean RIP or disp32.
        let mode = if mem.disp == 0 && mem.base & 7 != 5 {
            0
        } else if fits_i8(mem.disp) {
            1
        } else {
            2
        };
        let reg = (reg & 7) << 3;
        match mem.index {
            Some(index) => {
                self.put(&[mode << 6 | reg | 4, (index & 7) << 3 | (mem.base & 7)]);
            }
            // `rsp` and `r12` as a base need a SIB byte with no index.
            None if mem.base & 7 == 4 => self.put(&[mode << 6 | reg | 4, 0x24]),
            None => self.put(&[mode << 6 | reg | (mem.base & 7)]),
        }
        match mode {
            1 => self.put(&[mem.disp as u8]),
            2 => self.put(&mem.disp.to_le_bytes()),
            _ => {}
        }
    }

    /// `mov dst, src`.
    pub fn mov(&mut self, dst: u8, src: u8) {
        self.op_rr(true, &[0x8b], dst, src);
    }

    /// `dst = value`, by the shortest form that holds it.
    pub fn constant(&mut self, dst: u8, value: i64) {
        if value == 0 {
            // `xor dst32, dst32`.
            self.op_rr(false, &[0x33], dst, dst);
        } else if let Ok(value) = u32::try_from(value) {
            self.prefix(false, 0, 0, dst, false);
            self.put(&[0xb8 + (dst & 7)]);
            self.put(&value.to_le_bytes());
        } else if let Ok(value) = i32::try_from(value) {
            self.op_rr(true, &[0xc7], 0, dst);
            self.put(&value.to_le_bytes());
        } else {
            self.prefix(true, 0, 0, dst, false);
            self.put(&[0xb8 + (dst & 7)]);
            self.put(&value.to_le_bytes());
        }
    }

    /// `dst =` the low `bits` (8, 16 or 32) of `src`, sign-extended.
    pub fn sign_extend(&mut self, dst: u8, src: u8, bits: u32) {
        match bits {
            8 => self.op_rr(true, &[0x0f, 0xbe], dst, src),
            16 => self.op_rr(true, &[0x0f, 0xbf], dst, src),
            _ => self.op_rr(true, &[0x63], dst, src),
        }
    }

    /// `op dst, src`.
    pub fn alu(&mut self, op: Alu, dst: u8, src: u8) {
        self.op_rr(true, &[op.opcode()], dst, src);
    }

    /// `op dst, imm`.
    pub fn alu_imm(&mut self, op: Alu, dst: u8, imm: i32) {
        if fits_i8(imm) {
            self.op_rr(true, &[0x83], op.digit(), dst);
            self.put(&[imm as u8]);
        } else {
            self.op_rr(true, &[0x81], op.digit(), dst);
            self.put(&imm.to_le_bytes());
        }
    }

    /// `imul dst, src`: the low word of the product.
    pub fn imul(&mut self, dst: u8, src: u8) {
        self.op_rr(true, &[0x0f, 0xaf], dst, src);
    }

    /// `test a, b`.
    pub fn test(&mut self, a: u8, b: u8) {
        self.op_rr(true, &[0x85], b, a);
    }

    /// `op dst, cl`.
    pub fn shift_cl(&mut self, op: Shift, dst: u8) {
        self.op_rr(true, &[0xd3], op.digit(), dst);
    }

    /// `op dst, n`.
    pub fn shift_imm(&mut self, op: Shift, dst: u8, n: u8) {
        self.op_rr(true, &[0xc1], op.digit(), dst);
        self.put(&[n]);
    }

    /// `op r`.
    pub fn unary(&mut self, op: Unary, r: u8) {
        self.op_rr(true, &[0xf7], op.digit(), r);
    }

    /// `cqo`: `rdx =` the sign of `rax`, for a signed division.
    pub fn cqo(&mut self) {
        self.put(&[0x48, 0x99]);
    }

    /// `setcc r8`: the low byte of `r` = 1 when `cc` holds, else 0.
    pub fn setcc(&mut self, cc: u8, r: u8) {
        self.prefix(false, 0, 0, r, (4..8).contains(&r));
        self.put(&[0x0f, 0x90 | cc, 0xc0 | (r & 7)]);
    }

    /// `movzx r32, r8`: `r =` its own low byte.
    pub fn zero_extend_byte(&mut self, r: u8) {
        self.prefix(false, r, 0, r, (4..8).contains(&r));
        self.put(&[0x0f, 0xb6, 0xc0 | (r & 7) << 3 | (r & 7)]);
    }

    /// `dst =` the `size` bits at `mem`, zero-extended.
    pub fn load(&mut self, size: Size, dst: u8, mem: Mem) {
        match size {
            Size::B => self.op_mem(false, &[0x0f, 0xb6], dst, mem, false),
            Size::W => self.op_mem(false, &[0x0f, 0xb7], dst, mem, false),
            Size::D => self.op_mem(false, &[0x8b], dst, mem, false),
            Size::Q => self.op_mem(true, &[0x8b], dst, mem, false),
        }
    }

    /// Write the low `size` bits of `src` at `mem`.
    pub fn store(&mut self, size: Size, mem: Mem, src: u8) {
        match size {
            Size::B => self.op_mem(false, &[0x88], src, mem, true),
            Size::W => {
                self.put(&[0x66]);
                self.op_mem(false, &[0x89], src, mem, false);
            }
            Size::D => self.op_mem(false, &[0x89], src, mem, false),
            Size::Q => self.op_mem(true, &[0x89], src, mem, false),
        }
    }

    /// `lea dst, mem`.
    pub fn lea(&mut self, dst: u8, mem: Mem) {
        self.op_mem(true, &[0x8d], dst, mem, false);
    }

    /// `lea dst, [rip + disp32]`: where its displacement is, to patch.
    pub fn lea_rip(&mut self, dst: u8) -> usize {
        self.prefix(true, dst, 0, 0, false);
        self.put(&[0x8d, (dst & 7) << 3 | 5]);
        self.rel32()
    }

    /// A zero 32-bit displacement: where it is.
    fn rel32(&mut self) -> usize {
        let at = self.bytes.len();
        self.put(&[0; 4]);
        at
    }

    /// `jmp rel32`: where its displacement is.
    pub fn jmp(&mut self) -> usize {
        self.put(&[0xe9]);
        self.rel32()
    }

    /// `jcc rel32`: where its displacement is.
    pub fn jcc(&mut self, cc: u8) -> usize {
        self.put(&[0x0f, 0x80 | cc]);
        self.rel32()
    }

    /// `call rel32`: where its displacement is.
    pub fn call(&mut self) -> usize {
        self.put(&[0xe8]);
        self.rel32()
    }

    /// `jcc rel8`: where its displacement is.
    pub fn jcc8(&mut self, cc: u8) -> usize {
        self.put(&[0x70 | cc, 0]);
        self.bytes.len() - 1
    }

    /// `jmp rel8`: where its displacement is.
    pub fn jmp8(&mut self) -> usize {
        self.put(&[0xeb, 0]);
        self.bytes.len() - 1
    }

    /// Point the 8-bit displacement at `at` to here.
    pub fn land8(&mut self, at: usize) {
        self.bytes[at] = (self.bytes.len() - (at + 1)) as u8;
    }

    /// Write `value` as the 32-bit displacement at `at`.
    pub fn patch32(&mut self, at: usize, value: i32) {
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// `call r`.
    pub fn call_reg(&mut self, r: u8) {
        self.op_rr(false, &[0xff], 2, r);
    }

    /// `push r`.
    pub fn push(&mut self, r: u8) {
        self.prefix(false, 0, 0, r, false);
        self.put(&[0x50 | (r & 7)]);
    }

    /// `leave`: `mov rsp, rbp`, then `pop rbp`.
    pub fn leave(&mut self) {
        self.put(&[0xc9]);
    }

    pub fn ret(&mut self) {
        self.put(&[0xc3]);
    }

    pub fn syscall(&mut self) {
        self.put(&[0x0f, 0x05]);
    }
}

#[cfg(test)]
mod tests;
