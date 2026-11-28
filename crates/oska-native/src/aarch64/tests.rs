// SPDX-License-Identifier: Apache-2.0
//! Every instruction form the aarch64 code generator emits, against its
//! encoding in the Arm Architecture Reference Manual for A-profile
//! (DDI 0487), A64 instruction set: the fixed bits and the operand fields
//! each instruction's encoding diagram gives (`sf`, `Rm`, `Rn`, `Rd`, `imm12`,
//! `imm19`, `imm26`, and so on), and the condition codes of its section on
//! conditional execution.
//!
//! The expected words are worked out from those diagrams, and each case is
//! named by the assembly it stands for. A test builds an [`Emitter`] with
//! the locations of its temporaries chosen (`Reg(n)` is `xn`, `Slot(n)` is
//! `[sp, #8 * n]`), runs one `Inst` through it, and compares the words.
//! A branch's offset is left zero by `inst` and patched by `function`; the
//! tests that follow cover the patching.

use super::*;
use crate::regalloc::Loc::{Reg, Slot};

/// An emitter that has run `inst` with the temporaries at `locs`.
fn emitted(locs: &[Loc], inst: &Inst) -> Emitter {
    let mut emitter = Emitter {
        locs: locs.to_vec(),
        extra_args: 0x40,
        ..Emitter::default()
    };
    emitter.inst(inst);
    emitter
}

/// `got` and `expected`, in hexadecimal, so a wrong bit is easy to see.
fn assert_words(got: &[u32], expected: &[u32], name: &str) {
    let hex = |words: &[u32]| words.iter().map(|w| format!("{w:08x}")).collect::<Vec<_>>();
    assert_eq!(hex(got), hex(expected), "{name}");
}

fn check(name: &str, locs: &[Loc], inst: Inst, expected: &[u32]) {
    assert_words(&emitted(locs, &inst).words, expected, name);
}

#[test]
fn a_constant_takes_the_shortest_sequence_that_builds_it() {
    // MOVZ sets one 16-bit chunk and zeroes the rest; MOVK keeps the
    // other bits; MOVN writes the inverted chunk, so a small negative is one
    // instruction. A chunk of zero past the first is skipped.
    check(
        "movz x2, #0 (value 0)",
        &[Reg(2)],
        Inst::Const { dst: 0, value: 0 },
        &[0xd280_0002],
    );
    check(
        "movz x2, #1 (value 1)",
        &[Reg(2)],
        Inst::Const { dst: 0, value: 1 },
        &[0xd280_0022],
    );
    check(
        "movz x2, #0xffff (value 65535)",
        &[Reg(2)],
        Inst::Const {
            dst: 0,
            value: 65535,
        },
        &[0xd29f_ffe2],
    );
    check(
        "movz x2, #0; movk x2, #1, lsl #16 (value 65536)",
        &[Reg(2)],
        Inst::Const {
            dst: 0,
            value: 65536,
        },
        &[0xd280_0002, 0xf2a0_0022],
    );
    check(
        "movz x2, #0x5678; movk x2, #0x1234, lsl #16 (value 305419896)",
        &[Reg(2)],
        Inst::Const {
            dst: 0,
            value: 305419896,
        },
        &[0xd28a_cf02, 0xf2a2_4682],
    );
    check(
        "movz x2, #0; movk x2, #1, lsl #32 (value 4294967296)",
        &[Reg(2)],
        Inst::Const {
            dst: 0,
            value: 4294967296,
        },
        &[0xd280_0002, 0xf2c0_0022],
    );
    check(
        "movz x2, #0xdef0; movk x2, #0x9abc, lsl #16; movk x2, #0x5678, lsl #32; movk x2, #0x1234, lsl #48 (value 1311768467463790320)",
        &[Reg(2)],
        Inst::Const { dst: 0, value: 1311768467463790320 },
        &[0xd29b_de02, 0xf2b3_5782, 0xf2ca_cf02, 0xf2e2_4682],
    );
    check(
        "movn x2, #0 (value -1)",
        &[Reg(2)],
        Inst::Const { dst: 0, value: -1 },
        &[0x9280_0002],
    );
    check(
        "movn x2, #1 (value -2)",
        &[Reg(2)],
        Inst::Const { dst: 0, value: -2 },
        &[0x9280_0022],
    );
    check(
        "movn x2, #0xffff (value -65536)",
        &[Reg(2)],
        Inst::Const {
            dst: 0,
            value: -65536,
        },
        &[0x929f_ffe2],
    );
    check(
        "movz x2, #0xffff; movk x2, #0xfffe, lsl #16; movk x2, #0xffff, lsl #32; movk x2, #0xffff, lsl #48 (value -65537)",
        &[Reg(2)],
        Inst::Const { dst: 0, value: -65537 },
        &[0xd29f_ffe2, 0xf2bf_ffc2, 0xf2df_ffe2, 0xf2ff_ffe2],
    );
    check(
        "movz x2, #0; movk x2, #0x8000, lsl #48 (value i64::MIN)",
        &[Reg(2)],
        Inst::Const {
            dst: 0,
            value: i64::MIN,
        },
        &[0xd280_0002, 0xf2f0_0002],
    );
    check(
        "movz x2, #0xffff; movk x2, #0xffff, lsl #16; movk x2, #0xffff, lsl #32; movk x2, #0x7fff, lsl #48 (value i64::MAX)",
        &[Reg(2)],
        Inst::Const { dst: 0, value: i64::MAX },
        &[0xd29f_ffe2, 0xf2bf_ffe2, 0xf2df_ffe2, 0xf2ef_ffe2],
    );
    check(
        "movz x21, #7",
        &[Reg(21)],
        Inst::Const { dst: 0, value: 7 },
        &[0xd280_00f5],
    );
    check(
        "movz x9, #7; str x9, [sp, #24]",
        &[Slot(3)],
        Inst::Const { dst: 0, value: 7 },
        &[0xd280_00e9, 0xf900_0fe9],
    );
}

#[test]
fn a_copy_is_an_orr_with_the_zero_register_and_a_slot_goes_through_a_scratch_register() {
    // MOV (register) is ORR Xd, XZR, Xm. A slot is LDR or STR (unsigned offset) with `sp` as the
    // base and the slot number as the scaled offset.
    check(
        "mov x2, x3 (Copy)",
        &[Reg(2), Reg(3)],
        Inst::Copy { dst: 0, src: 1 },
        &[0xaa03_03e2],
    );
    check(
        "mov x2, x3 (Secret)",
        &[Reg(2), Reg(3)],
        Inst::Secret { dst: 0, src: 1 },
        &[0xaa03_03e2],
    );
    check(
        "mov x2, x3 (Expose)",
        &[Reg(2), Reg(3)],
        Inst::Expose { dst: 0, src: 1 },
        &[0xaa03_03e2],
    );
    check(
        "mov x19, x27",
        &[Reg(19), Reg(27)],
        Inst::Copy { dst: 0, src: 1 },
        &[0xaa1b_03f3],
    );
    check(
        "ldr x9, [sp, #40]; mov x2, x9",
        &[Reg(2), Slot(5)],
        Inst::Copy { dst: 0, src: 1 },
        &[0xf940_17e9, 0xaa09_03e2],
    );
    check(
        "str x2, [sp, #40]",
        &[Slot(5), Reg(2)],
        Inst::Copy { dst: 0, src: 1 },
        &[0xf900_17e2],
    );
    check(
        "ldr x9, [sp, #16]; str x9, [sp, #8]",
        &[Slot(1), Slot(2)],
        Inst::Copy { dst: 0, src: 1 },
        &[0xf940_0be9, 0xf900_07e9],
    );
}

#[test]
fn a_copy_to_the_same_register_emits_nothing() {
    check(
        "nothing",
        &[Reg(4), Reg(4)],
        Inst::Copy { dst: 0, src: 1 },
        &[],
    );
}

#[test]
fn an_address_is_an_adrp_and_an_add_with_a_fixup() {
    // ADRP with its page offset zero, then ADD (immediate) of #0: the file
    // layout patches both (`link`), and the emitter records where.
    let mut e = emitted(
        &[Reg(2)],
        &Inst::Addr {
            dst: 0,
            symbol: Symbol::Rodata(16),
        },
    );
    assert_words(
        &e.words,
        &[0x9000_0002, 0x9100_0042],
        "adrp x2; add x2, x2, #0",
    );
    assert_eq!(e.fixups.len(), 1);
    assert_eq!(e.fixups[0].at, 0);
    assert_eq!(e.fixups[0].symbol, Symbol::Rodata(16));
    e = emitted(
        &[Slot(2)],
        &Inst::Addr {
            dst: 0,
            symbol: Symbol::Data(0),
        },
    );
    assert_words(
        &e.words,
        &[0x9000_0009, 0x9100_0129, 0xf900_0be9],
        "adrp x9; add x9, x9, #0; str x9, [sp, #16]",
    );
}

#[test]
fn a_binary_operation_is_one_three_register_instruction() {
    // ADD, SUB, AND, ORR, EOR (shifted register, shift 0), MUL (an alias of MADD with Xa = XZR),
    // and LSLV, LSRV, ASRV, all 64-bit. A slot operand is loaded into x9 (left) or x10 (right) first;
    // a slot result is computed in x9 and stored.
    check(
        "add x2, x3, x4 (Add)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::Add,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x8b04_0062],
    );
    check(
        "sub x2, x3, x4 (Sub)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::Sub,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xcb04_0062],
    );
    check(
        "mul x2, x3, x4 (Mul)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::Mul,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x9b04_7c62],
    );
    check(
        "and x2, x3, x4 (And)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::And,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x8a04_0062],
    );
    check(
        "orr x2, x3, x4 (Or)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::Or,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xaa04_0062],
    );
    check(
        "eor x2, x3, x4 (Xor)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::Xor,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xca04_0062],
    );
    check(
        "lsl x2, x3, x4 (Shl)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::Shl,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x9ac4_2062],
    );
    check(
        "lsr x2, x3, x4 (Shr)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::Shr,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x9ac4_2462],
    );
    check(
        "asr x2, x3, x4 (Sar)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Binary {
            op: BinOp::Sar,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x9ac4_2862],
    );
    check(
        "sub x19, x20, x21",
        &[Reg(19), Reg(20), Reg(21)],
        Inst::Binary {
            op: BinOp::Sub,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xcb15_0293],
    );
    check(
        "add x2, x3, x3",
        &[Reg(2), Reg(3), Reg(3)],
        Inst::Binary {
            op: BinOp::Add,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x8b03_0062],
    );
    check(
        "ldr x9, [sp, #16]; ldr x10, [sp, #24]; add x9, x9, x10; str x9, [sp, #8]",
        &[Slot(1), Slot(2), Slot(3)],
        Inst::Binary {
            op: BinOp::Add,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xf940_0be9, 0xf940_0fea, 0x8b0a_0129, 0xf900_07e9],
    );
    check(
        "ldr x9, [sp, #16]; lsl x2, x9, x4",
        &[Reg(2), Slot(2), Reg(4)],
        Inst::Binary {
            op: BinOp::Shl,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xf940_0be9, 0x9ac4_2122],
    );
}

#[test]
fn an_immediate_operation_is_an_add_a_sub_or_a_bitfield_move() {
    // ADD and SUB with a 12-bit immediate. A left
    // shift by n is UBFM Xd, Xn, #(-n mod 64), #(63 - n) (the LSL (immediate) alias); a
    // right shift is UBFM Xd, Xn, #n, #63 (LSR) and SBFM Xd, Xn, #n, #63.
    check(
        "add x2, x3, #5",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Add,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0x9100_1462],
    );
    check(
        "add x2, x3, #4095",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Add,
            dst: 0,
            lhs: 1,
            imm: 4095,
        },
        &[0x913f_fc62],
    );
    check(
        "add x19, x20, #1",
        &[Reg(19), Reg(20)],
        Inst::BinaryImm {
            op: BinOp::Add,
            dst: 0,
            lhs: 1,
            imm: 1,
        },
        &[0x9100_0693],
    );
    check(
        "sub x2, x3, #5",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Sub,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xd100_1462],
    );
    check(
        "sub x2, x3, #4095",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Sub,
            dst: 0,
            lhs: 1,
            imm: 4095,
        },
        &[0xd13f_fc62],
    );
    check(
        "lsl x2, x3, #3 (ubfm x2, x3, #61, #60)",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Shl,
            dst: 0,
            lhs: 1,
            imm: 3,
        },
        &[0xd37d_f062],
    );
    check(
        "lsl x2, x3, #0 (ubfm x2, x3, #0, #63)",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Shl,
            dst: 0,
            lhs: 1,
            imm: 0,
        },
        &[0xd340_fc62],
    );
    check(
        "lsl x2, x3, #63 (ubfm x2, x3, #1, #0)",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Shl,
            dst: 0,
            lhs: 1,
            imm: 63,
        },
        &[0xd341_0062],
    );
    check(
        "lsr x2, x3, #3",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Shr,
            dst: 0,
            lhs: 1,
            imm: 3,
        },
        &[0xd343_fc62],
    );
    check(
        "lsr x2, x3, #63",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Shr,
            dst: 0,
            lhs: 1,
            imm: 63,
        },
        &[0xd37f_fc62],
    );
    check(
        "asr x2, x3, #3",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Sar,
            dst: 0,
            lhs: 1,
            imm: 3,
        },
        &[0x9343_fc62],
    );
    check(
        "asr x2, x3, #63",
        &[Reg(2), Reg(3)],
        Inst::BinaryImm {
            op: BinOp::Sar,
            dst: 0,
            lhs: 1,
            imm: 63,
        },
        &[0x937f_fc62],
    );
    check(
        "ldr x9, [sp, #16]; add x9, x9, #8; str x9, [sp, #8]",
        &[Slot(1), Slot(2)],
        Inst::BinaryImm {
            op: BinOp::Add,
            dst: 0,
            lhs: 1,
            imm: 8,
        },
        &[0xf940_0be9, 0x9100_2129, 0xf900_07e9],
    );
}

#[test]
fn a_comparison_is_a_cmp_and_a_cset() {
    // CMP (register) is SUBS XZR, Xn, Xm; CSET is CSINC Xd, XZR, XZR, invert(cond)
    //. Condition codes : eq 0, ne 1, hs 2, lo 3, hi 8, ls 9, ge 10, lt 11, gt 12,
    // le 13.
    check(
        "cmp x3, x4; cset x2, eq (Eq)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::Eq,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_17e2],
    );
    check(
        "cmp x3, x4; cset x2, ne (Ne)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::Ne,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_07e2],
    );
    check(
        "cmp x3, x4; cset x2, lt (Lt)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::Lt,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_a7e2],
    );
    check(
        "cmp x3, x4; cset x2, le (Le)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::Le,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_c7e2],
    );
    check(
        "cmp x3, x4; cset x2, gt (Gt)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::Gt,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_d7e2],
    );
    check(
        "cmp x3, x4; cset x2, ge (Ge)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::Ge,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_b7e2],
    );
    check(
        "cmp x3, x4; cset x2, lo (Below)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::Below,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_27e2],
    );
    check(
        "cmp x3, x4; cset x2, ls (BelowEq)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::BelowEq,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_87e2],
    );
    check(
        "cmp x3, x4; cset x2, hi (Above)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::Above,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_97e2],
    );
    check(
        "cmp x3, x4; cset x2, hs (AboveEq)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Compare {
            cond: Cond::AboveEq,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0xeb04_007f, 0x9a9f_37e2],
    );
    check(
        "ldr x9, [sp, #16]; ldr x10, [sp, #24]; cmp x9, x10; cset x9, lt; str x9, [sp, #8]",
        &[Slot(1), Slot(2), Slot(3)],
        Inst::Compare {
            cond: Cond::Lt,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[
            0xf940_0be9,
            0xf940_0fea,
            0xeb0a_013f,
            0x9a9f_a7e9,
            0xf900_07e9,
        ],
    );
}

#[test]
fn a_comparison_with_an_immediate_is_a_cmp_or_cmn_and_a_cset() {
    // CMP (immediate) is SUBS XZR, Xn, #imm12; a negative immediate is CMN (immediate),
    // ADDS XZR, Xn, #-imm.
    check(
        "cmp x3, #5; cset x2, eq (Eq)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Eq,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_17e2],
    );
    check(
        "cmp x3, #5; cset x2, ne (Ne)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Ne,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_07e2],
    );
    check(
        "cmp x3, #5; cset x2, lt (Lt)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Lt,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_a7e2],
    );
    check(
        "cmp x3, #5; cset x2, le (Le)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Le,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_c7e2],
    );
    check(
        "cmp x3, #5; cset x2, gt (Gt)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Gt,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_d7e2],
    );
    check(
        "cmp x3, #5; cset x2, ge (Ge)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Ge,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_b7e2],
    );
    check(
        "cmp x3, #5; cset x2, lo (Below)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Below,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_27e2],
    );
    check(
        "cmp x3, #5; cset x2, ls (BelowEq)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::BelowEq,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_87e2],
    );
    check(
        "cmp x3, #5; cset x2, hi (Above)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Above,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_97e2],
    );
    check(
        "cmp x3, #5; cset x2, hs (AboveEq)",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::AboveEq,
            dst: 0,
            lhs: 1,
            imm: 5,
        },
        &[0xf100_147f, 0x9a9f_37e2],
    );
    check(
        "cmp x3, #4095; cset x2, eq",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Eq,
            dst: 0,
            lhs: 1,
            imm: 4095,
        },
        &[0xf13f_fc7f, 0x9a9f_17e2],
    );
    check(
        "cmn x3, #1; cset x2, eq",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Eq,
            dst: 0,
            lhs: 1,
            imm: -1,
        },
        &[0xb100_047f, 0x9a9f_17e2],
    );
    check(
        "cmn x3, #4095; cset x2, lt",
        &[Reg(2), Reg(3)],
        Inst::CompareImm {
            cond: Cond::Lt,
            dst: 0,
            lhs: 1,
            imm: -4095,
        },
        &[0xb13f_fc7f, 0x9a9f_a7e2],
    );
    check(
        "ldr x9, [sp, #16]; cmp x9, #0; cset x9, gt; str x9, [sp, #8]",
        &[Slot(1), Slot(2)],
        Inst::CompareImm {
            cond: Cond::Gt,
            dst: 0,
            lhs: 1,
            imm: 0,
        },
        &[0xf940_0be9, 0xf100_013f, 0x9a9f_d7e9, 0xf900_07e9],
    );
}

#[test]
fn a_branch_is_a_cmp_and_a_conditional_branch() {
    // B.cond with the offset left zero for `function` to patch: 0101010 0 imm19 0
    // cond.
    check(
        "cmp x3, x4; b.eq #0 (Eq)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::Eq,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_0000],
    );
    check(
        "cmp x3, x4; b.ne #0 (Ne)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::Ne,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_0001],
    );
    check(
        "cmp x3, x4; b.lt #0 (Lt)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::Lt,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_000b],
    );
    check(
        "cmp x3, x4; b.le #0 (Le)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::Le,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_000d],
    );
    check(
        "cmp x3, x4; b.gt #0 (Gt)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::Gt,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_000c],
    );
    check(
        "cmp x3, x4; b.ge #0 (Ge)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::Ge,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_000a],
    );
    check(
        "cmp x3, x4; b.lo #0 (Below)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::Below,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_0003],
    );
    check(
        "cmp x3, x4; b.ls #0 (BelowEq)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::BelowEq,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_0009],
    );
    check(
        "cmp x3, x4; b.hi #0 (Above)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::Above,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_0008],
    );
    check(
        "cmp x3, x4; b.hs #0 (AboveEq)",
        &[Reg(3), Reg(4)],
        Inst::Branch {
            cond: Cond::AboveEq,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xeb04_007f, 0x5400_0002],
    );
    check(
        "ldr x9, [sp, #8]; ldr x10, [sp, #16]; cmp x9, x10; b.eq #0",
        &[Slot(1), Slot(2)],
        Inst::Branch {
            cond: Cond::Eq,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        &[0xf940_07e9, 0xf940_0bea, 0xeb0a_013f, 0x5400_0000],
    );
}

#[test]
fn a_branch_on_an_immediate_is_a_cmp_immediate_and_a_conditional_branch() {
    check(
        "cmp x3, #7; b.eq #0 (Eq)",
        &[Reg(3)],
        Inst::BranchImm {
            cond: Cond::Eq,
            lhs: 0,
            imm: 7,
            target: 0,
        },
        &[0xf100_1c7f, 0x5400_0000],
    );
    check(
        "cmp x3, #7; b.lt #0 (Lt)",
        &[Reg(3)],
        Inst::BranchImm {
            cond: Cond::Lt,
            lhs: 0,
            imm: 7,
            target: 0,
        },
        &[0xf100_1c7f, 0x5400_000b],
    );
    check(
        "cmp x3, #7; b.lo #0 (Below)",
        &[Reg(3)],
        Inst::BranchImm {
            cond: Cond::Below,
            lhs: 0,
            imm: 7,
            target: 0,
        },
        &[0xf100_1c7f, 0x5400_0003],
    );
    check(
        "cmp x3, #7; b.hs #0 (AboveEq)",
        &[Reg(3)],
        Inst::BranchImm {
            cond: Cond::AboveEq,
            lhs: 0,
            imm: 7,
            target: 0,
        },
        &[0xf100_1c7f, 0x5400_0002],
    );
    check(
        "cmn x3, #7; b.ne #0",
        &[Reg(3)],
        Inst::BranchImm {
            cond: Cond::Ne,
            lhs: 0,
            imm: -7,
            target: 0,
        },
        &[0xb100_1c7f, 0x5400_0001],
    );
    check(
        "ldr x9, [sp, #16]; cmp x9, #0; b.ne #0",
        &[Slot(2)],
        Inst::BranchImm {
            cond: Cond::Ne,
            lhs: 0,
            imm: 0,
            target: 0,
        },
        &[0xf940_0be9, 0xf100_013f, 0x5400_0001],
    );
}

#[test]
fn a_jump_is_a_b_and_a_jump_if_zero_a_cbz() {
    // B and CBZ, the offset left zero for `function` to patch.
    check(
        "cbz x3, #0",
        &[Reg(3)],
        Inst::JumpIfZero { cond: 0, target: 0 },
        &[0xb400_0003],
    );
    check(
        "cbz x27, #0",
        &[Reg(27)],
        Inst::JumpIfZero { cond: 0, target: 0 },
        &[0xb400_001b],
    );
    check(
        "ldr x9, [sp, #32]; cbz x9, #0",
        &[Slot(4)],
        Inst::JumpIfZero { cond: 0, target: 0 },
        &[0xf940_13e9, 0xb400_0009],
    );
    check("b #0", &[], Inst::Jump(0), &[0x1400_0000]);
}

#[test]
fn a_checked_operation_with_an_immediate_is_an_adds_or_subs_and_a_branch_on_the_flag() {
    // ADDS and SUBS into x16, then B.VS (signed overflow) or B.HS /
    // B.LO (unsigned carry, and borrow) to the overflow label.
    check(
        "adds x16, x3, #5; b.vs #0; mov x2, x16",
        &[Reg(2), Reg(3)],
        Inst::ArithImm {
            op: Arith::Add,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            imm: 5,
            overflow: 0,
        },
        &[0xb100_1470, 0x5400_0006, 0xaa10_03e2],
    );
    check(
        "subs x16, x3, #5; b.vs #0; mov x2, x16",
        &[Reg(2), Reg(3)],
        Inst::ArithImm {
            op: Arith::Sub,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            imm: 5,
            overflow: 0,
        },
        &[0xf100_1470, 0x5400_0006, 0xaa10_03e2],
    );
    check(
        "adds x16, x3, #5; b.hs #0; mov x2, x16",
        &[Reg(2), Reg(3)],
        Inst::ArithImm {
            op: Arith::Add,
            int: IntKind {
                signed: false,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            imm: 5,
            overflow: 0,
        },
        &[0xb100_1470, 0x5400_0002, 0xaa10_03e2],
    );
    check(
        "subs x16, x3, #5; b.lo #0; mov x2, x16",
        &[Reg(2), Reg(3)],
        Inst::ArithImm {
            op: Arith::Sub,
            int: IntKind {
                signed: false,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            imm: 5,
            overflow: 0,
        },
        &[0xf100_1470, 0x5400_0003, 0xaa10_03e2],
    );
    check(
        "adds x16, x3, #4095; b.vs #0; mov x2, x16",
        &[Reg(2), Reg(3)],
        Inst::ArithImm {
            op: Arith::Add,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            imm: 4095,
            overflow: 0,
        },
        &[0xb13f_fc70, 0x5400_0006, 0xaa10_03e2],
    );
    check(
        "ldr x9, [sp, #16]; adds x16, x9, #5; b.hs #0; str x16, [sp, #8]",
        &[Slot(1), Slot(2)],
        Inst::ArithImm {
            op: Arith::Add,
            int: IntKind {
                signed: false,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            imm: 5,
            overflow: 0,
        },
        &[0xf940_0be9, 0xb100_1530, 0x5400_0002, 0xf900_07f0],
    );
}

#[test]
fn a_checked_operation_on_a_word_is_a_flag_setting_instruction_and_a_branch() {
    // ADDS, SUBS into x16; B.VS or B.HS / B.LO for the flag.
    // A signed multiply is MUL (MADD with XZR) and SMULH into x17, then
    // CMP x17, x16, ASR #63 (SUBS XZR with a shifted register): the high half must be the
    // low half's sign. An unsigned multiply is UMULH and CBNZ on the high
    // half. SDIV/UDIV; the remainder is MSUB of the quotient. The
    // one signed division that overflows is MIN / -1: CMN b, #1; B.NE over NEGS XZR, a (V when a is
    // MIN).
    check(
        "adds x16, x3, x4; b.vs #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0xab04_0070, 0x5400_0006, 0xaa10_03e2],
    );
    check(
        "subs x16, x3, x4; b.vs #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Sub,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0xeb04_0070, 0x5400_0006, 0xaa10_03e2],
    );
    check(
        "mul x16, x3, x4; smulh x17, x3, x4; cmp x17, x16, asr #63; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Mul,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0x9b04_7c70,
            0x9b44_7c71,
            0xeb90_fe3f,
            0x5400_0001,
            0xaa10_03e2,
        ],
    );
    check(
        "cmn x4, #1; b.ne #12; negs xzr, x3; b.vs #0; sdiv x16, x3, x4; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Div,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0xb100_049f,
            0x5400_0061,
            0xeb03_03ff,
            0x5400_0006,
            0x9ac4_0c70,
            0xaa10_03e2,
        ],
    );
    check(
        "sdiv x17, x3, x4; msub x16, x17, x4, x3; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Rem,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x9ac4_0c71, 0x9b04_8e30, 0xaa10_03e2],
    );
    check(
        "adds x16, x3, x4; b.hs #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: false,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0xab04_0070, 0x5400_0002, 0xaa10_03e2],
    );
    check(
        "subs x16, x3, x4; b.lo #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Sub,
            int: IntKind {
                signed: false,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0xeb04_0070, 0x5400_0003, 0xaa10_03e2],
    );
    check(
        "umulh x17, x3, x4; cbnz x17, #0; mul x16, x3, x4; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Mul,
            int: IntKind {
                signed: false,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x9bc4_7c71, 0xb500_0011, 0x9b04_7c70, 0xaa10_03e2],
    );
    check(
        "udiv x16, x3, x4; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Div,
            int: IntKind {
                signed: false,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x9ac4_0870, 0xaa10_03e2],
    );
    check(
        "udiv x17, x3, x4; msub x16, x17, x4, x3; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Rem,
            int: IntKind {
                signed: false,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x9ac4_0871, 0x9b04_8e30, 0xaa10_03e2],
    );
    check(
        "adds x16, x20, x21; b.vs #0; mov x19, x16",
        &[Reg(19), Reg(20), Reg(21)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0xab15_0290, 0x5400_0006, 0xaa10_03f3],
    );
    check(
        "ldr x9, [sp, #16]; ldr x10, [sp, #24]; adds x16, x9, x10; b.vs #0; str x16, [sp, #8]",
        &[Slot(1), Slot(2), Slot(3)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: true,
                bits: 64,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0xf940_0be9,
            0xf940_0fea,
            0xab0a_0130,
            0x5400_0006,
            0xf900_07f0,
        ],
    );
}

#[test]
fn a_checked_operation_on_a_narrow_type_is_a_word_operation_and_a_range_check() {
    // A sized type narrower than a word holds its values extended, so the word operation does
    // not overflow; the result fits when sign-extending its low `bits` (SBFM Xd, Xn, #0,
    // #bits-1: SXTB, SXTH, SXTW) gives it back, or, unsigned, when LSR by `bits`
    // leaves zero.
    check(
        "i8 Add: add x16, x3, x4; sbfm x17, x16, #0, #7; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: true,
                bits: 8,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0x8b04_0070,
            0x9340_1e11,
            0xeb10_023f,
            0x5400_0001,
            0xaa10_03e2,
        ],
    );
    check(
        "i8 Sub: sub x16, x3, x4; sbfm x17, x16, #0, #7; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Sub,
            int: IntKind {
                signed: true,
                bits: 8,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0xcb04_0070,
            0x9340_1e11,
            0xeb10_023f,
            0x5400_0001,
            0xaa10_03e2,
        ],
    );
    check(
        "i8 Mul: mul x16, x3, x4; sbfm x17, x16, #0, #7; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Mul,
            int: IntKind {
                signed: true,
                bits: 8,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0x9b04_7c70,
            0x9340_1e11,
            0xeb10_023f,
            0x5400_0001,
            0xaa10_03e2,
        ],
    );
    check(
        "i8 Div: sdiv x16, x3, x4; sbfm x17, x16, #0, #7; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Div,
            int: IntKind {
                signed: true,
                bits: 8,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0x9ac4_0c70,
            0x9340_1e11,
            0xeb10_023f,
            0x5400_0001,
            0xaa10_03e2,
        ],
    );
    check(
        "i8 Rem: sdiv x17, x3, x4; msub x16, x17, x4, x3; sbfm x17, x16, #0, #7; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith { op: Arith::Rem, int: IntKind { signed: true, bits: 8 }, dst: 0, lhs: 1, rhs: 2, overflow: 0 },
        &[0x9ac4_0c71, 0x9b04_8e30, 0x9340_1e11, 0xeb10_023f, 0x5400_0001, 0xaa10_03e2],
    );
    check(
        "i16 Add: add x16, x3, x4; sbfm x17, x16, #0, #15; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: true,
                bits: 16,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0x8b04_0070,
            0x9340_3e11,
            0xeb10_023f,
            0x5400_0001,
            0xaa10_03e2,
        ],
    );
    check(
        "i16 Rem: sdiv x17, x3, x4; msub x16, x17, x4, x3; sbfm x17, x16, #0, #15; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith { op: Arith::Rem, int: IntKind { signed: true, bits: 16 }, dst: 0, lhs: 1, rhs: 2, overflow: 0 },
        &[0x9ac4_0c71, 0x9b04_8e30, 0x9340_3e11, 0xeb10_023f, 0x5400_0001, 0xaa10_03e2],
    );
    check(
        "i32 Add: add x16, x3, x4; sbfm x17, x16, #0, #31; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: true,
                bits: 32,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0x8b04_0070,
            0x9340_7e11,
            0xeb10_023f,
            0x5400_0001,
            0xaa10_03e2,
        ],
    );
    check(
        "i32 Mul: mul x16, x3, x4; sbfm x17, x16, #0, #31; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Mul,
            int: IntKind {
                signed: true,
                bits: 32,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[
            0x9b04_7c70,
            0x9340_7e11,
            0xeb10_023f,
            0x5400_0001,
            0xaa10_03e2,
        ],
    );
    check(
        "i32 Rem: sdiv x17, x3, x4; msub x16, x17, x4, x3; sbfm x17, x16, #0, #31; cmp x17, x16; b.ne #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith { op: Arith::Rem, int: IntKind { signed: true, bits: 32 }, dst: 0, lhs: 1, rhs: 2, overflow: 0 },
        &[0x9ac4_0c71, 0x9b04_8e30, 0x9340_7e11, 0xeb10_023f, 0x5400_0001, 0xaa10_03e2],
    );
    check(
        "u8 Add: add x16, x3, x4; lsr x17, x16, #8; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: false,
                bits: 8,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x8b04_0070, 0xd348_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u8 Sub: sub x16, x3, x4; lsr x17, x16, #8; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Sub,
            int: IntKind {
                signed: false,
                bits: 8,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0xcb04_0070, 0xd348_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u8 Mul: mul x16, x3, x4; lsr x17, x16, #8; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Mul,
            int: IntKind {
                signed: false,
                bits: 8,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x9b04_7c70, 0xd348_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u8 Div: udiv x16, x3, x4; lsr x17, x16, #8; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Div,
            int: IntKind {
                signed: false,
                bits: 8,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x9ac4_0870, 0xd348_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u8 Rem: udiv x17, x3, x4; msub x16, x17, x4, x3; lsr x17, x16, #8; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith { op: Arith::Rem, int: IntKind { signed: false, bits: 8 }, dst: 0, lhs: 1, rhs: 2, overflow: 0 },
        &[0x9ac4_0871, 0x9b04_8e30, 0xd348_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u16 Add: add x16, x3, x4; lsr x17, x16, #16; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: false,
                bits: 16,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x8b04_0070, 0xd350_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u16 Rem: udiv x17, x3, x4; msub x16, x17, x4, x3; lsr x17, x16, #16; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith { op: Arith::Rem, int: IntKind { signed: false, bits: 16 }, dst: 0, lhs: 1, rhs: 2, overflow: 0 },
        &[0x9ac4_0871, 0x9b04_8e30, 0xd350_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u32 Add: add x16, x3, x4; lsr x17, x16, #32; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Add,
            int: IntKind {
                signed: false,
                bits: 32,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x8b04_0070, 0xd360_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u32 Mul: mul x16, x3, x4; lsr x17, x16, #32; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith {
            op: Arith::Mul,
            int: IntKind {
                signed: false,
                bits: 32,
            },
            dst: 0,
            lhs: 1,
            rhs: 2,
            overflow: 0,
        },
        &[0x9b04_7c70, 0xd360_fe11, 0xb500_0011, 0xaa10_03e2],
    );
    check(
        "u32 Rem: udiv x17, x3, x4; msub x16, x17, x4, x3; lsr x17, x16, #32; cbnz x17, #0; mov x2, x16",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Arith { op: Arith::Rem, int: IntKind { signed: false, bits: 32 }, dst: 0, lhs: 1, rhs: 2, overflow: 0 },
        &[0x9ac4_0871, 0x9b04_8e30, 0xd360_fe11, 0xb500_0011, 0xaa10_03e2],
    );
}

#[test]
fn a_load_is_an_ldr_of_its_width_at_a_scaled_offset_or_through_a_register() {
    // LDRB, LDRH, LDR: the offset is scaled by the
    // access size and 12 bits, so an offset that is not a multiple of it, or 4096 of it or more,
    // goes through x11 (MOVZ/MOVK) and LDR (register) with UXTX #0 (option 011, S 0).
    check(
        "ldrb w2, [x3]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W8,
            dst: 0,
            base: 1,
            offset: 0,
        },
        &[0x3940_0062],
    );
    check(
        "ldrb w2, [x3, #3]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W8,
            dst: 0,
            base: 1,
            offset: 3,
        },
        &[0x3940_0c62],
    );
    check(
        "ldrb w2, [x3, #4095]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W8,
            dst: 0,
            base: 1,
            offset: 4095,
        },
        &[0x397f_fc62],
    );
    check(
        "movz x11, #4096; ldrb w2, [x3, x11]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W8,
            dst: 0,
            base: 1,
            offset: 4096,
        },
        &[0xd282_000b, 0x386b_6862],
    );
    check(
        "ldrh w2, [x3]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W16,
            dst: 0,
            base: 1,
            offset: 0,
        },
        &[0x7940_0062],
    );
    check(
        "ldrh w2, [x3, #6]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W16,
            dst: 0,
            base: 1,
            offset: 6,
        },
        &[0x7940_0c62],
    );
    check(
        "ldrh w2, [x3, #8190]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W16,
            dst: 0,
            base: 1,
            offset: 8190,
        },
        &[0x797f_fc62],
    );
    check(
        "movz x11, #8192; ldrh w2, [x3, x11]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W16,
            dst: 0,
            base: 1,
            offset: 8192,
        },
        &[0xd284_000b, 0x786b_6862],
    );
    check(
        "movz x11, #1; ldrh w2, [x3, x11]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W16,
            dst: 0,
            base: 1,
            offset: 1,
        },
        &[0xd280_002b, 0x786b_6862],
    );
    check(
        "ldr w2, [x3]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W32,
            dst: 0,
            base: 1,
            offset: 0,
        },
        &[0xb940_0062],
    );
    check(
        "ldr w2, [x3, #12]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W32,
            dst: 0,
            base: 1,
            offset: 12,
        },
        &[0xb940_0c62],
    );
    check(
        "ldr w2, [x3, #16380]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W32,
            dst: 0,
            base: 1,
            offset: 16380,
        },
        &[0xb97f_fc62],
    );
    check(
        "movz x11, #16384; ldr w2, [x3, x11]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W32,
            dst: 0,
            base: 1,
            offset: 16384,
        },
        &[0xd288_000b, 0xb86b_6862],
    );
    check(
        "movz x11, #1; ldr w2, [x3, x11]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W32,
            dst: 0,
            base: 1,
            offset: 1,
        },
        &[0xd280_002b, 0xb86b_6862],
    );
    check(
        "ldr x2, [x3]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W64,
            dst: 0,
            base: 1,
            offset: 0,
        },
        &[0xf940_0062],
    );
    check(
        "ldr x2, [x3, #24]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W64,
            dst: 0,
            base: 1,
            offset: 24,
        },
        &[0xf940_0c62],
    );
    check(
        "ldr x2, [x3, #32760]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W64,
            dst: 0,
            base: 1,
            offset: 32760,
        },
        &[0xf97f_fc62],
    );
    check(
        "movz x11, #32768; ldr x2, [x3, x11]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W64,
            dst: 0,
            base: 1,
            offset: 32768,
        },
        &[0xd290_000b, 0xf86b_6862],
    );
    check(
        "movz x11, #1; ldr x2, [x3, x11]",
        &[Reg(2), Reg(3)],
        Inst::Load {
            width: Width::W64,
            dst: 0,
            base: 1,
            offset: 1,
        },
        &[0xd280_002b, 0xf86b_6862],
    );
    check(
        "ldr x19, [x20, #16]",
        &[Reg(19), Reg(20)],
        Inst::Load {
            width: Width::W64,
            dst: 0,
            base: 1,
            offset: 16,
        },
        &[0xf940_0a93],
    );
    check(
        "ldr x9, [sp, #16]; ldr x9, [x9, #16]; str x9, [sp, #8]",
        &[Slot(1), Slot(2)],
        Inst::Load {
            width: Width::W64,
            dst: 0,
            base: 1,
            offset: 16,
        },
        &[0xf940_0be9, 0xf940_0929, 0xf900_07e9],
    );
}

#[test]
fn a_store_is_a_str_of_its_width_at_a_scaled_offset_or_through_a_register() {
    // STRB, STRH, STR, as the loads.
    check(
        "strb w3, [x2]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W8,
            base: 0,
            offset: 0,
            src: 1,
        },
        &[0x3900_0043],
    );
    check(
        "strb w3, [x2, #3]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W8,
            base: 0,
            offset: 3,
            src: 1,
        },
        &[0x3900_0c43],
    );
    check(
        "strb w3, [x2, #4095]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W8,
            base: 0,
            offset: 4095,
            src: 1,
        },
        &[0x393f_fc43],
    );
    check(
        "movz x11, #4096; strb w3, [x2, x11]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W8,
            base: 0,
            offset: 4096,
            src: 1,
        },
        &[0xd282_000b, 0x382b_6843],
    );
    check(
        "strh w3, [x2]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W16,
            base: 0,
            offset: 0,
            src: 1,
        },
        &[0x7900_0043],
    );
    check(
        "strh w3, [x2, #6]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W16,
            base: 0,
            offset: 6,
            src: 1,
        },
        &[0x7900_0c43],
    );
    check(
        "strh w3, [x2, #8190]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W16,
            base: 0,
            offset: 8190,
            src: 1,
        },
        &[0x793f_fc43],
    );
    check(
        "movz x11, #8192; strh w3, [x2, x11]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W16,
            base: 0,
            offset: 8192,
            src: 1,
        },
        &[0xd284_000b, 0x782b_6843],
    );
    check(
        "movz x11, #1; strh w3, [x2, x11]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W16,
            base: 0,
            offset: 1,
            src: 1,
        },
        &[0xd280_002b, 0x782b_6843],
    );
    check(
        "str w3, [x2]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W32,
            base: 0,
            offset: 0,
            src: 1,
        },
        &[0xb900_0043],
    );
    check(
        "str w3, [x2, #12]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W32,
            base: 0,
            offset: 12,
            src: 1,
        },
        &[0xb900_0c43],
    );
    check(
        "str w3, [x2, #16380]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W32,
            base: 0,
            offset: 16380,
            src: 1,
        },
        &[0xb93f_fc43],
    );
    check(
        "movz x11, #16384; str w3, [x2, x11]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W32,
            base: 0,
            offset: 16384,
            src: 1,
        },
        &[0xd288_000b, 0xb82b_6843],
    );
    check(
        "movz x11, #1; str w3, [x2, x11]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W32,
            base: 0,
            offset: 1,
            src: 1,
        },
        &[0xd280_002b, 0xb82b_6843],
    );
    check(
        "str x3, [x2]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W64,
            base: 0,
            offset: 0,
            src: 1,
        },
        &[0xf900_0043],
    );
    check(
        "str x3, [x2, #24]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W64,
            base: 0,
            offset: 24,
            src: 1,
        },
        &[0xf900_0c43],
    );
    check(
        "str x3, [x2, #32760]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W64,
            base: 0,
            offset: 32760,
            src: 1,
        },
        &[0xf93f_fc43],
    );
    check(
        "movz x11, #32768; str x3, [x2, x11]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W64,
            base: 0,
            offset: 32768,
            src: 1,
        },
        &[0xd290_000b, 0xf82b_6843],
    );
    check(
        "movz x11, #1; str x3, [x2, x11]",
        &[Reg(2), Reg(3)],
        Inst::Store {
            width: Width::W64,
            base: 0,
            offset: 1,
            src: 1,
        },
        &[0xd280_002b, 0xf82b_6843],
    );
    check(
        "str x20, [x19, #16]",
        &[Reg(19), Reg(20)],
        Inst::Store {
            width: Width::W64,
            base: 0,
            offset: 16,
            src: 1,
        },
        &[0xf900_0a74],
    );
    check(
        "ldr x9, [sp, #8]; ldr x10, [sp, #16]; str x10, [x9, #16]",
        &[Slot(1), Slot(2)],
        Inst::Store {
            width: Width::W64,
            base: 0,
            offset: 16,
            src: 1,
        },
        &[0xf940_07e9, 0xf940_0bea, 0xf900_092a],
    );
}

#[test]
fn a_float_operation_runs_in_d0_and_d1_and_canonicalises_a_nan() {
    // FMOV Dd, Xn moves the bits in; FADD, FSUB, FMUL, FDIV (scalar, double) and FSQRT work on d0
    // and d1; FCMP d0, d0 sets V only for a NaN, so B.VC skips the next two instructions, which load
    // 0x7ff8000000000000 with MOVZ #0x7ff8, LSL #48 and FMOV d0, x11; FMOV Xd, Dn
    // moves the result out.
    check(
        "fmov d0, x3; fmov d1, x4; fadd d0, d0, d1; fcmp d0, d0; b.vc #12; movz x11, #0x7ff8, lsl #48; fmov d0, x11; fmov x2, d0",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Float { op: FloatOp::Add, dst: 0, lhs: 1, rhs: 2 },
        &[0x9e67_0060, 0x9e67_0081, 0x1e61_2800, 0x1e60_2000, 0x5400_0067, 0xd2ef_ff0b, 0x9e67_0160, 0x9e66_0002],
    );
    check(
        "fmov d0, x3; fmov d1, x4; fsub d0, d0, d1; fcmp d0, d0; b.vc #12; movz x11, #0x7ff8, lsl #48; fmov d0, x11; fmov x2, d0",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Float { op: FloatOp::Sub, dst: 0, lhs: 1, rhs: 2 },
        &[0x9e67_0060, 0x9e67_0081, 0x1e61_3800, 0x1e60_2000, 0x5400_0067, 0xd2ef_ff0b, 0x9e67_0160, 0x9e66_0002],
    );
    check(
        "fmov d0, x3; fmov d1, x4; fmul d0, d0, d1; fcmp d0, d0; b.vc #12; movz x11, #0x7ff8, lsl #48; fmov d0, x11; fmov x2, d0",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Float { op: FloatOp::Mul, dst: 0, lhs: 1, rhs: 2 },
        &[0x9e67_0060, 0x9e67_0081, 0x1e61_0800, 0x1e60_2000, 0x5400_0067, 0xd2ef_ff0b, 0x9e67_0160, 0x9e66_0002],
    );
    check(
        "fmov d0, x3; fmov d1, x4; fdiv d0, d0, d1; fcmp d0, d0; b.vc #12; movz x11, #0x7ff8, lsl #48; fmov d0, x11; fmov x2, d0",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Float { op: FloatOp::Div, dst: 0, lhs: 1, rhs: 2 },
        &[0x9e67_0060, 0x9e67_0081, 0x1e61_1800, 0x1e60_2000, 0x5400_0067, 0xd2ef_ff0b, 0x9e67_0160, 0x9e66_0002],
    );
    check(
        "fmov d0, x3; fsqrt d0, d0; fcmp d0, d0; b.vc #12; movz x11, #0x7ff8, lsl #48; fmov d0, x11; fmov x2, d0",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Float { op: FloatOp::Sqrt, dst: 0, lhs: 1, rhs: 2 },
        &[0x9e67_0060, 0x1e61_c000, 0x1e60_2000, 0x5400_0067, 0xd2ef_ff0b, 0x9e67_0160, 0x9e66_0002],
    );
    check(
        "fmov d0, x3; fcmp d0, d0; b.vc #12; movz x11, #0x7ff8, lsl #48; fmov d0, x11; fmov x2, d0",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::Float {
            op: FloatOp::Canonical,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[
            0x9e67_0060,
            0x1e60_2000,
            0x5400_0067,
            0xd2ef_ff0b,
            0x9e67_0160,
            0x9e66_0002,
        ],
    );
    check(
        "fmov d0, x20; fmov d1, x21; fadd d0, d0, d1; fcmp d0, d0; b.vc #12; movz x11, #0x7ff8, lsl #48; fmov d0, x11; fmov x19, d0",
        &[Reg(19), Reg(20), Reg(21)],
        Inst::Float { op: FloatOp::Add, dst: 0, lhs: 1, rhs: 2 },
        &[0x9e67_0280, 0x9e67_02a1, 0x1e61_2800, 0x1e60_2000, 0x5400_0067, 0xd2ef_ff0b, 0x9e67_0160, 0x9e66_0013],
    );
    check(
        "ldr x9, [sp, #16]; fmov d0, x9; ldr x10, [sp, #24]; fmov d1, x10; fmul d0, d0, d1; fcmp d0, d0; b.vc #12; movz x11, #0x7ff8, lsl #48; fmov d0, x11; fmov x9, d0; str x9, [sp, #8]",
        &[Slot(1), Slot(2), Slot(3)],
        Inst::Float { op: FloatOp::Mul, dst: 0, lhs: 1, rhs: 2 },
        &[0xf940_0be9, 0x9e67_0120, 0xf940_0fea, 0x9e67_0141, 0x1e61_0800, 0x1e60_2000, 0x5400_0067, 0xd2ef_ff0b, 0x9e67_0160, 0x9e66_0009, 0xf900_07e9],
    );
}

#[test]
fn a_float_comparison_is_an_fcmp_and_a_cset_that_is_false_for_a_nan() {
    // FCMP d0, d1 sets N, Z, C, V to 0011 for an unordered pair, so EQ (Z set), MI (N
    // set) and LS (C clear or Z set) are false for it, and NE is true; CSET reads them.
    check(
        "fmov d0, x3; fmov d1, x4; fcmp d0, d1; cset x2, eq (Eq)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::FloatCompare {
            cond: FloatCond::Eq,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x9e67_0060, 0x9e67_0081, 0x1e61_2000, 0x9a9f_17e2],
    );
    check(
        "fmov d0, x3; fmov d1, x4; fcmp d0, d1; cset x2, ne (Ne)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::FloatCompare {
            cond: FloatCond::Ne,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x9e67_0060, 0x9e67_0081, 0x1e61_2000, 0x9a9f_07e2],
    );
    check(
        "fmov d0, x3; fmov d1, x4; fcmp d0, d1; cset x2, mi (Lt)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::FloatCompare {
            cond: FloatCond::Lt,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x9e67_0060, 0x9e67_0081, 0x1e61_2000, 0x9a9f_57e2],
    );
    check(
        "fmov d0, x3; fmov d1, x4; fcmp d0, d1; cset x2, ls (Le)",
        &[Reg(2), Reg(3), Reg(4)],
        Inst::FloatCompare {
            cond: FloatCond::Le,
            dst: 0,
            lhs: 1,
            rhs: 2,
        },
        &[0x9e67_0060, 0x9e67_0081, 0x1e61_2000, 0x9a9f_87e2],
    );
    check(
        "ldr x9, [sp, #16]; ldr x10, [sp, #24]; fmov d0, x9; fmov d1, x10; fcmp d0, d1; cset x9, mi; str x9, [sp, #8]",
        &[Slot(1), Slot(2), Slot(3)],
        Inst::FloatCompare { cond: FloatCond::Lt, dst: 0, lhs: 1, rhs: 2 },
        &[0xf940_0be9, 0xf940_0fea, 0x9e67_0120, 0x9e67_0141, 0x1e61_2000, 0x9a9f_57e9, 0xf900_07e9],
    );
}

#[test]
fn a_call_is_a_bl_the_arguments_in_x0_on_and_the_result_from_x0() {
    // BL: 100101 imm26, the offset left zero for `emit` to patch (`calls` records where).
    // The arguments are moved by a parallel move (a cycle through x16).
    // No arguments.
    let e = emitted(
        &[Reg(2)],
        &Inst::Call {
            dst: 0,
            callee: 3,
            args: vec![],
        },
    );
    assert_words(&e.words, &[0x9400_0000, 0xaa00_03e2], "bl #0; mov x2, x0");
    assert_eq!(e.calls, &[(0, 3)]);
    // Two arguments, then the result.
    let e = emitted(
        &[Reg(5), Reg(2), Reg(3)],
        &Inst::Call {
            dst: 0,
            callee: 0,
            args: vec![1, 2],
        },
    );
    assert_words(
        &e.words,
        &[0xaa02_03e0, 0xaa03_03e1, 0x9400_0000, 0xaa00_03e5],
        "mov x0, x2; mov x1, x3; bl #0; mov x5, x0",
    );
    assert_eq!(e.calls, &[(2, 0)]);
    // A result in a slot.
    let e = emitted(
        &[Slot(4)],
        &Inst::Call {
            dst: 0,
            callee: 0,
            args: vec![],
        },
    );
    assert_words(
        &e.words,
        &[0x9400_0000, 0xf900_13e0],
        "bl #0; str x0, [sp, #32]",
    );
    // Arguments already in place move nowhere.
    let e = emitted(
        &[Reg(2), Reg(0), Reg(1)],
        &Inst::Call {
            dst: 0,
            callee: 0,
            args: vec![1, 2],
        },
    );
    assert_words(&e.words, &[0x9400_0000, 0xaa00_03e2], "bl #0; mov x2, x0");
    // Two arguments that swap: x0 and x1 trade through x16.
    let e = emitted(
        &[Reg(2), Reg(1), Reg(0)],
        &Inst::Call {
            dst: 0,
            callee: 0,
            args: vec![1, 2],
        },
    );
    assert_words(
        &e.words,
        &[
            0xaa01_03f0,
            0xaa00_03e1,
            0xaa10_03e0,
            0x9400_0000,
            0xaa00_03e2,
        ],
        "mov x16, x1; mov x1, x0; mov x0, x16; bl #0; mov x2, x0",
    );
    // An argument from a slot, and one to a slot-less register.
    let e = emitted(
        &[Reg(2), Slot(3), Reg(4)],
        &Inst::Call {
            dst: 0,
            callee: 0,
            args: vec![1, 2],
        },
    );
    assert_words(
        &e.words,
        &[0xf940_0fe0, 0xaa04_03e1, 0x9400_0000, 0xaa00_03e2],
        "ldr x0, [sp, #24]; mov x1, x4; bl #0; mov x2, x0",
    );
}

#[test]
fn a_call_with_more_than_eight_arguments_stores_the_rest_before_the_call() {
    // The ninth argument and those past it go in the writable data, at `extra_args` (here a fixup of an ADRP
    // and an ADD to x16), 8 bytes each, by STR (immediate) from x16.
    let locs = [
        Reg(14),
        Reg(2),
        Reg(3),
        Reg(4),
        Reg(5),
        Reg(6),
        Reg(7),
        Reg(8),
        Reg(12),
        Reg(13),
        Slot(1),
    ];
    let e = emitted(
        &locs,
        &Inst::Call {
            dst: 0,
            callee: 1,
            args: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        },
    );
    assert_words(
        &e.words,
        &[0x9000_0010, 0x9100_0210, 0xf900_020d, 0xf940_07f1, 0xf900_0611, 0xaa02_03e0, 0xaa03_03e1, 0xaa04_03e2, 0xaa05_03e3, 0xaa06_03e4, 0xaa07_03e5, 0xaa08_03e6, 0xaa0c_03e7, 0x9400_0000, 0xaa00_03ee],
        "adrp x16, #0; add x16, x16, #0; str x13, [x16]; ldr x17, [sp, #8]; str x17, [x16, #8]; mov x0, x2; ...; mov x7, x12; bl #0; mov x14, x0"
    );
    assert_eq!(e.fixups.len(), 1);
    assert_eq!(e.fixups[0].at, 0);
    assert_eq!(e.fixups[0].symbol, Symbol::Data(0x40));
    assert_eq!(e.calls, &[(13, 1)]);
}

#[test]
fn an_indirect_call_is_a_blr_of_x9() {
    // BLR: 1101011 0001 11111 000000 Rn 00000. The target goes to x9, which no argument
    // register and no temporary uses.
    let e = emitted(
        &[Reg(2), Reg(3), Reg(4)],
        &Inst::CallIndirect {
            dst: 0,
            target: 1,
            args: vec![2],
        },
    );
    assert_words(
        &e.words,
        &[0xaa03_03e9, 0xaa04_03e0, 0xd63f_0120, 0xaa00_03e2],
        "mov x9, x3; mov x0, x4; blr x9; mov x2, x0",
    );
    let e = emitted(
        &[Reg(2), Slot(3)],
        &Inst::CallIndirect {
            dst: 0,
            target: 1,
            args: vec![],
        },
    );
    assert_words(
        &e.words,
        &[0xf940_0fe9, 0xd63f_0120, 0xaa00_03e2],
        "ldr x9, [sp, #24]; blr x9; mov x2, x0",
    );
}

#[test]
fn a_system_call_is_a_movz_of_x8_and_an_svc() {
    // SVC #0: 11010100 000 imm16 000 01; the number in x8 by MOVZ.
    let e = emitted(
        &[Reg(2), Reg(3), Reg(4)],
        &Inst::Syscall {
            dst: 0,
            number: 64,
            args: vec![1, 2],
        },
    );
    assert_words(
        &e.words,
        &[
            0xaa03_03e0,
            0xaa04_03e1,
            0xd280_0808,
            0xd400_0001,
            0xaa00_03e2,
        ],
        "mov x0, x3; mov x1, x4; movz x8, #64; svc #0; mov x2, x0",
    );
    let e = emitted(
        &[Reg(2)],
        &Inst::Syscall {
            dst: 0,
            number: 93,
            args: vec![],
        },
    );
    assert_words(
        &e.words,
        &[0xd280_0ba8, 0xd400_0001, 0xaa00_03e2],
        "movz x8, #93; svc #0; mov x2, x0",
    );
    let e = emitted(
        &[Reg(2)],
        &Inst::Syscall {
            dst: 0,
            number: 0xffff,
            args: vec![],
        },
    );
    assert_words(
        &e.words,
        &[0xd29f_ffe8, 0xd400_0001, 0xaa00_03e2],
        "movz x8, #0xffff; svc #0; mov x2, x0",
    );
}

#[test]
fn a_library_call_loads_its_address_from_the_import_slot_and_extends_a_narrow_result() {
    // ADRP x16 and ADD x16, x16, #0 (patched to the slot), LDR x16, [x16], BLR x16. A C `int` result is
    // sign-extended by SXTW x0, w0 (SBFM x0, x0, #0, #31); a variadic argument (Apple's arm64 ABI) is
    // pushed by STR x9, [sp, #-16]! (pre-indexed) and popped by ADD sp, sp, #16.
    let locs = [Reg(2), Reg(3), Reg(4)];
    // A wide result, no variadic argument.
    let e = emitted(
        &locs,
        &Inst::LibCall {
            dst: 0,
            import: 5,
            args: vec![1],
            fixed: 1,
            wide: true,
        },
    );
    assert_words(
        &e.words,
        &[
            0xaa03_03e0,
            0x9000_0010,
            0x9100_0210,
            0xf940_0210,
            0xd63f_0200,
            0xaa00_03e2,
        ],
        "mov x0, x3; adrp x16, #0; add x16, x16, #0; ldr x16, [x16]; blr x16; mov x2, x0",
    );
    assert_eq!(e.fixups.len(), 1);
    assert_eq!(e.fixups[0].at, 1);
    assert_eq!(e.fixups[0].symbol, Symbol::Import(5));
    // A narrow result.
    let e = emitted(
        &locs,
        &Inst::LibCall {
            dst: 0,
            import: 0,
            args: vec![],
            fixed: 0,
            wide: false,
        },
    );
    assert_words(
        &e.words,
        &[
            0x9000_0010,
            0x9100_0210,
            0xf940_0210,
            0xd63f_0200,
            0x9340_7c00,
            0xaa00_03e2,
        ],
        "adrp x16, #0; add x16, x16, #0; ldr x16, [x16]; blr x16; sxtw x0, w0; mov x2, x0",
    );
    // A variadic argument, after one fixed.
    let e = emitted(
        &locs,
        &Inst::LibCall {
            dst: 0,
            import: 1,
            args: vec![1, 2],
            fixed: 1,
            wide: false,
        },
    );
    assert_words(
        &e.words,
        &[0xaa04_03e9, 0xaa03_03e0, 0xf81f_0fe9, 0x9000_0010, 0x9100_0210, 0xf940_0210, 0xd63f_0200, 0x9100_43ff, 0x9340_7c00, 0xaa00_03e2],
        "mov x9, x4; mov x0, x3; str x9, [sp, #-16]!; adrp x16, #0; add x16, x16, #0; ldr x16, [x16]; blr x16; add sp, sp, #16; sxtw x0, w0; mov x2, x0"
    );
}

#[test]
fn the_call_depth_the_second_result_and_the_entry_stack_pointer() {
    // Enter branches to its label when x28 is zero and decrements it (CBZ, SUB immediate); Leave
    // increments it (ADD immediate). x1 holds a second result; `_start`'s frame record is 16 bytes,
    // so the entry stack pointer is x29 + 16.
    check(
        "cbz x28, #0; sub x28, x28, #1",
        &[Reg(2)],
        Inst::Enter { exhausted: 0 },
        &[0xb400_001c, 0xd100_079c],
    );
    check("add x28, x28, #1", &[Reg(2)], Inst::Leave, &[0x9100_079c]);
    check(
        "mov x2, x28",
        &[Reg(2)],
        Inst::Depth { dst: 0 },
        &[0xaa1c_03e2],
    );
    check(
        "str x28, [sp, #16]",
        &[Slot(2)],
        Inst::Depth { dst: 0 },
        &[0xf900_0bfc],
    );
    check(
        "mov x2, x1",
        &[Reg(2)],
        Inst::Second { dst: 0 },
        &[0xaa01_03e2],
    );
    check(
        "str x1, [sp, #16]",
        &[Slot(2)],
        Inst::Second { dst: 0 },
        &[0xf900_0be1],
    );
    check(
        "add x2, x29, #16",
        &[Reg(2)],
        Inst::EntrySp { dst: 0 },
        &[0x9100_43a2],
    );
    check(
        "add x9, x29, #16; str x9, [sp, #16]",
        &[Slot(2)],
        Inst::EntrySp { dst: 0 },
        &[0x9100_43a9, 0xf900_0be9],
    );
}

#[test]
fn a_call_on_a_new_stack_moves_sp_sets_the_depth_and_calls() {
    // ADD sp, Xn, #0 (the MOV (to/from SP) alias) switches the stack; x28 gets the depth;
    // then BL.
    let e = emitted(
        &[Reg(3)],
        &Inst::CallOnStack {
            stack: 0,
            depth: 100,
            callee: 2,
        },
    );
    assert_words(
        &e.words,
        &[0x9100_007f, 0xd280_0c9c, 0x9400_0000],
        "mov sp, x3; movz x28, #100; bl #0",
    );
    assert_eq!(e.calls, &[(2, 2)]);
    let e = emitted(
        &[Slot(1)],
        &Inst::CallOnStack {
            stack: 0,
            depth: 70000,
            callee: 0,
        },
    );
    assert_words(
        &e.words,
        &[
            0xf940_07e9,
            0x9100_013f,
            0xd282_2e1c,
            0xf2a0_003c,
            0x9400_0000,
        ],
        "ldr x9, [sp, #8]; mov sp, x9; movz x28, #0x1170; movk x28, #1, lsl #16; bl #0",
    );
}

#[test]
fn a_return_restores_the_frame_and_ret() {
    // ADD sp, x29, #0, LDP x29, x30, [sp], #16, RET: the
    // result in x0, and a pair's second in x1.
    let e = emitted(&[Reg(2)], &Inst::Return(0));
    assert_words(
        &e.words,
        &[0xaa02_03e0, 0x9100_03bf, 0xa8c1_7bfd, 0xd65f_03c0],
        "mov x0, x2; mov sp, x29; ldp x29, x30, [sp], #16; ret",
    );
    let e = emitted(&[Slot(3)], &Inst::Return(0));
    assert_words(
        &e.words,
        &[0xf940_0fe0, 0x9100_03bf, 0xa8c1_7bfd, 0xd65f_03c0],
        "ldr x0, [sp, #24]; mov sp, x29; ldp x29, x30, [sp], #16; ret",
    );
    let e = emitted(
        &[Reg(2), Reg(3)],
        &Inst::ReturnPair {
            value: 0,
            second: 1,
        },
    );
    assert_words(
        &e.words,
        &[
            0xaa03_03e1,
            0xaa02_03e0,
            0x9100_03bf,
            0xa8c1_7bfd,
            0xd65f_03c0,
        ],
        "mov x1, x3; mov x0, x2; mov sp, x29; ldp x29, x30, [sp], #16; ret",
    );
    // The value is in x1 and the second in x0: they swap through x16 (the second's source is the one held).
    let e = emitted(
        &[Reg(1), Reg(0)],
        &Inst::ReturnPair {
            value: 0,
            second: 1,
        },
    );
    assert_words(
        &e.words,
        &[
            0xaa00_03f0,
            0xaa01_03e0,
            0xaa10_03e1,
            0x9100_03bf,
            0xa8c1_7bfd,
            0xd65f_03c0,
        ],
        "mov x16, x0; mov x0, x1; mov x1, x16; mov sp, x29; ldp x29, x30, [sp], #16; ret",
    );
    // Callee-saved registers come back, below the frame record.
    let mut e = Emitter {
        locs: vec![Reg(19)],
        saved: vec![19, 20, 21],
        ..Emitter::default()
    };
    e.inst(&Inst::Return(0));
    assert_words(
        &e.words,
        &[0xaa13_03e0, 0xa97f_4fb4, 0xf85e_83b5, 0x9100_03bf, 0xa8c1_7bfd, 0xd65f_03c0],
        "mov x0, x19; ldp x20, x19, [x29, #-16]; ldur x21, [x29, #-24]; mov sp, x29; ldp x29, x30, [sp], #16; ret"
    );
}

#[test]
fn the_immediate_arithmetic_helpers_put_the_immediate_at_bit_10_and_shift_it_12_when_asked() {
    // ADD and SUB (immediate): sf 0 0 100010 sh imm12 Rn Rd; the immediate is 12 bits.
    assert_words(&[add_imm(2, 3, 5, false)], &[0x9100_1462], "add x2, x3, #5");
    assert_words(
        &[add_imm(2, 3, 0xfff, false)],
        &[0x913f_fc62],
        "add x2, x3, #0xfff",
    );
    assert_words(
        &[add_imm(2, 3, 0x1005, false)],
        &[0x9100_1462],
        "add x2, x3, #5 (the immediate is masked to 12 bits)",
    );
    assert_words(
        &[add_imm(SP, SP, 1, true)],
        &[0x9140_07ff],
        "add sp, sp, #1, lsl #12",
    );
    assert_words(
        &[add_imm(FP, SP, 0, false)],
        &[0x9100_03fd],
        "add x29, sp, #0",
    );
    assert_words(&[sub_imm(2, 3, 5, false)], &[0xd100_1462], "sub x2, x3, #5");
    assert_words(
        &[sub_imm(SP, SP, 0xfff, false)],
        &[0xd13f_ffff],
        "sub sp, sp, #0xfff",
    );
    assert_words(
        &[sub_imm(SP, SP, 1, true)],
        &[0xd140_07ff],
        "sub sp, sp, #1, lsl #12",
    );
    assert_words(
        &[sub_imm(DEPTH, DEPTH, 1, false)],
        &[0xd100_079c],
        "sub x28, x28, #1",
    );
}

#[test]
fn movz_places_a_chunk_at_its_halfword() {
    // MOVZ Xd, #imm16, LSL #(16 * hw): 1 10 100101 hw imm16 Rd.
    assert_words(&[movz(2, 0x1234, 0)], &[0xd282_4682], "movz x2, #0x1234");
    assert_words(
        &[movz(2, 0x1234, 1)],
        &[0xd2a2_4682],
        "movz x2, #0x1234, lsl #16",
    );
    assert_words(
        &[movz(2, 0x1234, 2)],
        &[0xd2c2_4682],
        "movz x2, #0x1234, lsl #32",
    );
    assert_words(
        &[movz(2, 0x1234, 3)],
        &[0xd2e2_4682],
        "movz x2, #0x1234, lsl #48",
    );
    assert_words(&[movz(X8, 93, 0)], &[0xd280_0ba8], "movz x8, #93");
    assert_words(&[movz(X11, 0xffff, 0)], &[0xd29f_ffeb], "movz x11, #0xffff");
}

#[test]
fn the_multiply_helpers_put_the_addend_in_ra() {
    // MADD Xd, Xn, Xm, Xa: Xa + Xn * Xm; MSUB: Xa - Xn * Xm (sf 00 11011 000 Rm o0 Ra Rn Rd).
    assert_words(&[madd(1, 2, 3, 4)], &[0x9b03_1041], "madd x1, x2, x3, x4");
    assert_words(&[madd(16, 3, 4, XZR)], &[0x9b04_7c70], "mul x16, x3, x4");
    assert_words(&[msub(1, 2, 3, 4)], &[0x9b03_9041], "msub x1, x2, x3, x4");
    assert_words(
        &[msub(16, 17, 4, 3)],
        &[0x9b04_8e30],
        "msub x16, x17, x4, x3",
    );
}

#[test]
fn the_comparison_and_float_helpers() {
    // CMP (register) is SUBS XZR, Xn, Xm; CMP (immediate) is SUBS XZR, Xn, #imm and CMN (immediate)
    // is ADDS XZR, Xn, #imm. FMOV Dd, Xn and FCMP Dn, Dm.
    assert_words(&[cmp(3, 4)], &[0xeb04_007f], "cmp x3, x4");
    assert_words(&[cmp(9, 10)], &[0xeb0a_013f], "cmp x9, x10");
    assert_words(&[cmp_imm(3, 5)], &[0xf100_147f], "cmp x3, #5");
    assert_words(&[cmp_imm(3, 4095)], &[0xf13f_fc7f], "cmp x3, #4095");
    assert_words(&[cmp_imm(3, -5)], &[0xb100_147f], "cmn x3, #5");
    assert_words(&[cmp_imm(3, -4095)], &[0xb13f_fc7f], "cmn x3, #4095");
    assert_words(&[fmov_to_double(0, 3)], &[0x9e67_0060], "fmov d0, x3");
    assert_words(&[fmov_to_double(1, 30)], &[0x9e67_03c1], "fmov d1, x30");
    assert_words(&[fcmp(0, 0)], &[0x1e60_2000], "fcmp d0, d0");
    assert_words(&[fcmp(0, 1)], &[0x1e61_2000], "fcmp d0, d1");
}

#[test]
fn a_bitfield_move_puts_immr_at_bit_16_and_imms_at_bit_10() {
    // UBFM and SBFM, 64-bit (sf 10 100110 N immr imms Rn Rd, N = 1).
    assert_words(
        &[bitfield(UBFM, 2, 3, 1, 63)],
        &[0xd341_fc62],
        "ubfm x2, x3, #1, #63",
    );
    assert_words(
        &[bitfield(UBFM, 2, 3, 61, 60)],
        &[0xd37d_f062],
        "ubfm x2, x3, #61, #60",
    );
    assert_words(
        &[bitfield(SBFM, 2, 3, 1, 63)],
        &[0x9341_fc62],
        "sbfm x2, x3, #1, #63",
    );
    assert_words(
        &[bitfield(SBFM, X17, X16, 0, 7)],
        &[0x9340_1e11],
        "sbfm x17, x16, #0, #7",
    );
    assert_words(
        &[bitfield(SBFM, X0, X0, 0, 31)],
        &[0x9340_7c00],
        "sxtw x0, w0",
    );
}

#[test]
fn the_frame_accesses_use_signed_scaled_and_unscaled_offsets() {
    // STP and LDP (signed offset, 64-bit; the offset is a multiple of 8 in 7 bits, scaled), STUR and
    // LDUR (unscaled, a signed 9-bit offset), and LDR and STR (unsigned offset) from sp.
    assert_words(
        &[pair_access(STP, 20, 19, -16)],
        &[0xa93f_4fb4],
        "stp x20, x19, [x29, #-16]",
    );
    assert_words(
        &[pair_access(STP, 22, 21, -32)],
        &[0xa93e_57b6],
        "stp x22, x21, [x29, #-32]",
    );
    assert_words(
        &[pair_access(STP, 20, 19, -512)],
        &[0xa920_4fb4],
        "stp x20, x19, [x29, #-512]",
    );
    assert_words(
        &[pair_access(STP, 20, 19, 504)],
        &[0xa91f_cfb4],
        "stp x20, x19, [x29, #504]",
    );
    assert_words(
        &[pair_access(LDP, 20, 19, -16)],
        &[0xa97f_4fb4],
        "ldp x20, x19, [x29, #-16]",
    );
    assert_words(
        &[pair_access(LDP, 28, 27, -48)],
        &[0xa97d_6fbc],
        "ldp x28, x27, [x29, #-48]",
    );
    assert_words(
        &[stur(21, FP, -24)],
        &[0xf81e_83b5],
        "stur x21, [x29, #-24]",
    );
    assert_words(
        &[stur(21, FP, -256)],
        &[0xf810_03b5],
        "stur x21, [x29, #-256]",
    );
    assert_words(
        &[stur(21, FP, 255)],
        &[0xf80f_f3b5],
        "stur x21, [x29, #255]",
    );
    assert_words(
        &[ldur(21, FP, -24)],
        &[0xf85e_83b5],
        "ldur x21, [x29, #-24]",
    );
    assert_words(
        &[ldur(27, FP, -256)],
        &[0xf850_03bb],
        "ldur x27, [x29, #-256]",
    );
    assert_words(&[load_slot(2, 0)], &[0xf940_03e2], "ldr x2, [sp]");
    assert_words(&[load_slot(2, 5)], &[0xf940_17e2], "ldr x2, [sp, #40]");
    assert_words(
        &[load_slot(9, 4095)],
        &[0xf97f_ffe9],
        "ldr x9, [sp, #32760]",
    );
    assert_words(&[store_slot(2, 0)], &[0xf900_03e2], "str x2, [sp]");
    assert_words(&[store_slot(2, 5)], &[0xf900_17e2], "str x2, [sp, #40]");
    assert_words(
        &[store_slot(16, 4095)],
        &[0xf93f_fff0],
        "str x16, [sp, #32760]",
    );
}

#[test]
fn callee_saved_registers_are_stored_and_loaded_in_pairs_below_the_frame_record() {
    // Register i of `saved` is at [x29 - 8 * (i + 1)]: a pair is one STP or LDP, the higher register
    // first, and an odd one out is a STUR or LDUR.
    {
        let mut e = Emitter {
            saved: vec![19],
            ..Emitter::default()
        };
        e.save_registers(true);
        assert_words(&e.words, &[0xf81f_83b3], "stur x19, [x29, #-8]");
    }
    {
        let mut e = Emitter {
            saved: vec![19, 20],
            ..Emitter::default()
        };
        e.save_registers(true);
        assert_words(&e.words, &[0xa93f_4fb4], "stp x20, x19, [x29, #-16]");
    }
    {
        let mut e = Emitter {
            saved: vec![19, 20, 21],
            ..Emitter::default()
        };
        e.save_registers(true);
        assert_words(
            &e.words,
            &[0xa93f_4fb4, 0xf81e_83b5],
            "stp x20, x19, [x29, #-16]; stur x21, [x29, #-24]",
        );
    }
    {
        let mut e = Emitter {
            saved: vec![19, 20, 21, 22],
            ..Emitter::default()
        };
        e.save_registers(true);
        assert_words(
            &e.words,
            &[0xa93f_4fb4, 0xa93e_57b6],
            "stp x20, x19, [x29, #-16]; stp x22, x21, [x29, #-32]",
        );
    }
    {
        let mut e = Emitter {
            saved: vec![19, 20, 21, 22, 23, 24, 25, 26, 27],
            ..Emitter::default()
        };
        e.save_registers(true);
        assert_words(&e.words, &[0xa93f_4fb4, 0xa93e_57b6, 0xa93d_5fb8, 0xa93c_67ba, 0xf81b_83bb], "stp x20, x19, [x29, #-16]; stp x22, x21, [x29, #-32]; stp x24, x23, [x29, #-48]; stp x26, x25, [x29, #-64]; stur x27, [x29, #-72]");
    }
    {
        let mut e = Emitter {
            saved: vec![19],
            ..Emitter::default()
        };
        e.save_registers(false);
        assert_words(&e.words, &[0xf85f_83b3], "ldur x19, [x29, #-8]");
    }
    {
        let mut e = Emitter {
            saved: vec![19, 20, 21],
            ..Emitter::default()
        };
        e.save_registers(false);
        assert_words(
            &e.words,
            &[0xa97f_4fb4, 0xf85e_83b5],
            "ldp x20, x19, [x29, #-16]; ldur x21, [x29, #-24]",
        );
    }
    {
        let mut e = Emitter {
            saved: vec![19, 20, 21, 22, 23, 24, 25, 26, 27],
            ..Emitter::default()
        };
        e.save_registers(false);
        assert_words(&e.words, &[0xa97f_4fb4, 0xa97e_57b6, 0xa97d_5fb8, 0xa97c_67ba, 0xf85b_83bb], "ldp x20, x19, [x29, #-16]; ldp x22, x21, [x29, #-32]; ldp x24, x23, [x29, #-48]; ldp x26, x25, [x29, #-64]; ldur x27, [x29, #-72]");
    }
    let mut e = Emitter::default();
    e.save_registers(true);
    assert!(e.words.is_empty(), "no registers saved: no instructions");
}

#[test]
fn the_adrp_helper_puts_the_page_difference_in_immhi_and_immlo() {
    // ADRP Xd, label: 1 immlo 10000 immhi Rd, the signed page distance (the target's 4 KiB page
    // minus the instruction's) in 21 bits, immlo its low two.
    assert_words(&[adrp(5, 0x1000, 0x1000)], &[0x9000_0005], "adrp x5, #0");
    assert_words(
        &[adrp(5, 0x1000, 0x1fff)],
        &[0x9000_0005],
        "adrp x5, #0 (the same page)",
    );
    assert_words(
        &[adrp(5, 0x1000, 0x2000)],
        &[0xb000_0005],
        "adrp x5, #0x1000",
    );
    assert_words(
        &[adrp(5, 0x1ffc, 0x2000)],
        &[0xb000_0005],
        "adrp x5, #0x1000 (the pc is the page's last word)",
    );
    assert_words(
        &[adrp(5, 0x1000, 0x4abc)],
        &[0xf000_0005],
        "adrp x5, #0x3000",
    );
    assert_words(
        &[adrp(5, 0x1000, 0x5000)],
        &[0x9000_0025],
        "adrp x5, #0x4000",
    );
    assert_words(
        &[adrp(5, 0x5000, 0x1000)],
        &[0x90ff_ffe5],
        "adrp x5, #-0x4000",
    );
    assert_words(
        &[adrp(5, 0x2000, 0x1000)],
        &[0xf0ff_ffe5],
        "adrp x5, #-0x1000",
    );
    assert_words(
        &[adrp(30, 0x40_0000, 0x40_0000 + 0x1234_5000)],
        &[0xb009_1a3e],
        "adrp x30, #0x12345000",
    );
    assert_words(
        &[adrp(5, 0, 0xffff_f000)],
        &[0xf07f_ffe5],
        "adrp x5, #0xfffff000",
    );
    assert_words(
        &[adrp(5, 0x1_0000_0000, 0x1000)],
        &[0xb080_0005],
        "adrp x5, #-0xfffff000 (the farthest back)",
    );
}

#[test]
fn a_function_starts_with_the_frame_record_and_a_mov_of_the_frame_pointer() {
    // STP x29, x30, [sp, #-16]! (pre-indexed) pushes the frame record; MOV x29, sp is ADD x29, sp,
    // #0. A function with no temporaries has no frame to carve out.
    let mut e = Emitter::default();
    let function = Function {
        name: "f".to_string(),
        params: 0,
        temps: 0,
        labels: 0,
        insts: vec![Inst::Leave],
        cold: 1,
    };
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(
        &e.words,
        &[0xa9bf_7bfd, 0x9100_03fd, 0x9100_079c],
        "stp x29, x30, [sp, #-16]!; mov x29, sp; add x28, x28, #1",
    );
}

#[test]
fn a_function_that_returns_its_argument_moves_it_in_and_out_and_restores_the_frame() {
    // The parameter arrives in x0 and moves to the register `regalloc` chose for it (the first
    // caller-saved one, x12); `Return` moves it back to x0 and unwinds the frame.
    let mut e = Emitter::default();
    let function = Function {
        name: "id".to_string(),
        params: 1,
        temps: 1,
        labels: 0,
        insts: vec![Inst::Return(0)],
        cold: 1,
    };
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(&e.words, &[0xa9bf_7bfd, 0x9100_03fd, 0xaa00_03ec, 0xaa0c_03e0, 0x9100_03bf, 0xa8c1_7bfd, 0xd65f_03c0], "stp x29, x30, [sp, #-16]!; mov x29, sp; mov x12, x0; mov x0, x12; mov sp, x29; ldp x29, x30, [sp], #16; ret");
}

#[test]
fn a_jump_to_a_label_is_patched_with_the_word_distance_in_imm26() {
    // B: 000101 imm26, the signed distance in words from the branch to its target.
    // `b` forward over two words, then a third.
    let insts = vec![
        Inst::Jump(0),
        Inst::Leave,
        Inst::Leave,
        Inst::Label(0),
        Inst::Leave,
    ];
    let function = Function {
        name: "fwd".to_string(),
        params: 0,
        temps: 0,
        labels: 1,
        cold: insts.len(),
        insts,
    };
    let mut e = Emitter::default();
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(&e.words, &[0xa9bf_7bfd, 0x9100_03fd, 0x1400_0003, 0x9100_079c, 0x9100_079c, 0x9100_079c], "stp x29, x30, [sp, #-16]!; mov x29, sp; b #12; add x28, x28, #1; add x28, x28, #1; add x28, x28, #1");
    // `b` backward over two words.
    let insts = vec![Inst::Label(0), Inst::Leave, Inst::Leave, Inst::Jump(0)];
    let function = Function {
        name: "back".to_string(),
        params: 0,
        temps: 0,
        labels: 1,
        cold: insts.len(),
        insts,
    };
    let mut e = Emitter::default();
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(
        &e.words,
        &[
            0xa9bf_7bfd,
            0x9100_03fd,
            0x9100_079c,
            0x9100_079c,
            0x17ff_fffe,
        ],
        "stp x29, x30, [sp, #-16]!; mov x29, sp; add x28, x28, #1; add x28, x28, #1; b #-8",
    );
    // `b` to itself, and to the next word.
    let insts = vec![Inst::Label(0), Inst::Jump(0)];
    let function = Function {
        name: "self".to_string(),
        params: 0,
        temps: 0,
        labels: 1,
        cold: insts.len(),
        insts,
    };
    let mut e = Emitter::default();
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(
        &e.words,
        &[0xa9bf_7bfd, 0x9100_03fd, 0x1400_0000],
        "stp x29, x30, [sp, #-16]!; mov x29, sp; b #0",
    );
    let insts = vec![Inst::Jump(0), Inst::Label(0)];
    let function = Function {
        name: "next".to_string(),
        params: 0,
        temps: 0,
        labels: 1,
        cold: insts.len(),
        insts,
    };
    let mut e = Emitter::default();
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(
        &e.words,
        &[0xa9bf_7bfd, 0x9100_03fd, 0x1400_0001],
        "stp x29, x30, [sp, #-16]!; mov x29, sp; b #4",
    );
}

#[test]
fn a_short_branch_is_patched_with_the_word_distance_in_imm19() {
    // CBZ: sf 011010 0 imm19 Rt, the signed distance in words, at bit 5. `Enter` is a CBZ of x28.
    let insts = vec![Inst::Enter { exhausted: 0 }, Inst::Leave, Inst::Label(0)];
    let function = Function {
        name: "fwd".to_string(),
        params: 0,
        temps: 0,
        labels: 1,
        cold: insts.len(),
        insts,
    };
    let mut e = Emitter::default();
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(
        &e.words,
        &[
            0xa9bf_7bfd,
            0x9100_03fd,
            0xb400_007c,
            0xd100_079c,
            0x9100_079c,
        ],
        "stp x29, x30, [sp, #-16]!; mov x29, sp; cbz x28, #12; sub x28, x28, #1; add x28, x28, #1",
    );
    let insts = vec![Inst::Label(0), Inst::Leave, Inst::Enter { exhausted: 0 }];
    let function = Function {
        name: "back".to_string(),
        params: 0,
        temps: 0,
        labels: 1,
        cold: insts.len(),
        insts,
    };
    let mut e = Emitter::default();
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(
        &e.words,
        &[
            0xa9bf_7bfd,
            0x9100_03fd,
            0x9100_079c,
            0xb4ff_fffc,
            0xd100_079c,
        ],
        "stp x29, x30, [sp, #-16]!; mov x29, sp; add x28, x28, #1; cbz x28, #-4; sub x28, x28, #1",
    );
}

#[test]
fn a_conditional_branch_is_patched_with_the_word_distance_in_imm19() {
    // B.cond: 0101010 0 imm19 0 cond, the signed distance in words, at bit 5.
    let insts = vec![
        Inst::Branch {
            cond: Cond::Eq,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
        Inst::Leave,
        Inst::Label(0),
    ];
    let function = Function {
        name: "fwd".to_string(),
        params: 2,
        temps: 2,
        labels: 1,
        cold: insts.len(),
        insts,
    };
    let mut e = Emitter::default();
    e.function(&function)
        .expect("the function should be emitted");
    let at = e.words.len() - 2;
    assert_words(
        &e.words[at..],
        &[0x5400_0040, 0x9100_079c],
        "b.eq #8; add x28, x28, #1",
    );
    let insts = vec![
        Inst::Label(0),
        Inst::Branch {
            cond: Cond::Lt,
            lhs: 0,
            rhs: 1,
            target: 0,
        },
    ];
    let function = Function {
        name: "back".to_string(),
        params: 2,
        temps: 2,
        labels: 1,
        cold: insts.len(),
        insts,
    };
    let mut e = Emitter::default();
    e.function(&function)
        .expect("the function should be emitted");
    assert_words(
        &e.words[e.words.len() - 2..],
        &[0xeb0d_019f, 0x54ff_ffeb],
        "cmp x12, x13; b.lt #-4 (back to the cmp)",
    );
}

#[test]
fn a_short_branch_that_cannot_reach_its_label_is_an_error() {
    // A 19-bit signed word distance reaches -262144 to 262143 words (about +-1 MiB). A longer
    // one is refused rather than wrapped.
    // The forward distance of an `Enter` over `n` words is `n + 2`; 19 bits hold up to 2^18 - 1.
    let far = |n: usize, back: bool| {
        let mut insts = vec![Inst::Leave; n];
        if back {
            insts.insert(0, Inst::Label(0));
            insts.push(Inst::Enter { exhausted: 0 });
        } else {
            insts.insert(0, Inst::Enter { exhausted: 0 });
            insts.push(Inst::Label(0));
        }
        let function = Function {
            name: "far".to_string(),
            params: 0,
            temps: 0,
            labels: 1,
            cold: insts.len(),
            insts,
        };
        let mut e = Emitter::default();
        e.function(&function).map(|()| e.words)
    };
    // Forward: the farthest distance is 262143 words, 0x3ffff in imm19.
    let words = far(262_141, false).expect("a distance of 2^18 - 1 words fits");
    assert_words(&words[2..3], &[0xb47f_fffc], "cbz x28, #1048572");
    assert_eq!(
        far(262_142, false).expect_err("a distance of 2^18 words does not fit"),
        "`far` is too large to branch in"
    );
    // Backward: the farthest distance is -262144 words, 0x40000 in imm19.
    let words = far(262_144, true).expect("a distance of -2^18 words fits");
    assert_words(
        &words[words.len() - 2..words.len() - 1],
        &[0xb480_001c],
        "cbz x28, #-1048576",
    );
    assert_eq!(
        far(262_145, true).expect_err("a distance of -2^18 - 1 words does not fit"),
        "`far` is too large to branch in"
    );
}

#[test]
fn emit_patches_each_bl_with_the_word_distance_to_the_start_of_its_callee() {
    // BL: 100101 imm26, the signed distance in words from the call to the callee's first word.
    let call = |callee| Function {
        name: "f".to_string(),
        params: 0,
        temps: 1,
        labels: 0,
        insts: vec![Inst::Call {
            dst: 0,
            callee,
            args: vec![],
        }],
        cold: 1,
    };
    let leave = Function {
        name: "g".to_string(),
        params: 0,
        temps: 0,
        labels: 0,
        insts: vec![Inst::Leave],
        cold: 1,
    };
    let module = Module {
        functions: vec![call(1), leave, call(0)],
        entry: 1,
        rodata: Vec::new(),
        data_size: 0,
        extra_args: 0,
        callback: None,
        stack: 0,
    };
    let text = emit(&module).expect("the module should be emitted");
    assert_words(&text.words, &[0xa9bf_7bfd, 0x9100_03fd, 0x9400_0002, 0xaa00_03ec, 0xa9bf_7bfd, 0x9100_03fd, 0x9100_079c, 0xa9bf_7bfd, 0x9100_03fd, 0x97ff_fff7, 0xaa00_03ec], "stp x29, x30, [sp, #-16]!; mov x29, sp; bl #8; mov x12, x0; stp x29, x30, [sp, #-16]!; mov x29, sp; add x28, x28, #1; stp x29, x30, [sp, #-16]!; mov x29, sp; bl #-36; mov x12, x0");
    assert_eq!(text.size(), 44);
    assert_eq!(text.entry(), 16, "the second function starts at word 4");
}

#[test]
fn a_function_address_fixup_holds_the_byte_offset_of_the_function() {
    // The fixup of an `adrp` of a function holds the offset in bytes of its first word.
    let leave = Function {
        name: "g".to_string(),
        params: 0,
        temps: 0,
        labels: 0,
        insts: vec![Inst::Leave],
        cold: 1,
    };
    let address = Function {
        name: "f".to_string(),
        params: 0,
        temps: 1,
        labels: 0,
        insts: vec![Inst::Addr {
            dst: 0,
            symbol: Symbol::Function(1),
        }],
        cold: 1,
    };
    let module = Module {
        functions: vec![leave, address],
        entry: 0,
        rodata: Vec::new(),
        data_size: 0,
        extra_args: 0,
        callback: None,
        stack: 0,
    };
    let text = emit(&module).expect("the module should be emitted");
    assert_words(
        &text.words[3..],
        &[0xa9bf_7bfd, 0x9100_03fd, 0x9000_000c, 0x9100_018c],
        "stp x29, x30, [sp, #-16]!; mov x29, sp; adrp x12, #0; add x12, x12, #0",
    );
    assert_eq!(text.fixups.len(), 1);
    assert_eq!(text.fixups[0].at, 5);
    assert_eq!(
        text.fixups[0].symbol,
        Symbol::Function(12),
        "the second function is word 3, byte 12"
    );
}

#[test]
fn link_patches_each_adrp_and_add_with_the_page_and_the_low_twelve_bits_of_its_symbol() {
    // Layout: code at 0x1000, rodata 0x500, data 0x20000, the import table 0x30000 (8 bytes a slot);
    // a function symbol is the byte offset in the code. ADRP gets the page distance from the
    // instruction's page, ADD the low 12 bits.
    let text = Text {
        words: vec![
            ADRP | 2,
            add_imm(2, 2, 0, false),
            ADRP | 3,
            add_imm(3, 3, 0, false),
            ADRP | 4,
            add_imm(4, 4, 0, false),
            ADRP | 5,
            add_imm(5, 5, 0, false),
            RET,
        ],
        fixups: vec![
            Fixup {
                at: 0,
                symbol: Symbol::Data(0x123),
            },
            Fixup {
                at: 2,
                symbol: Symbol::Rodata(0x10),
            },
            Fixup {
                at: 4,
                symbol: Symbol::Import(3),
            },
            Fixup {
                at: 6,
                symbol: Symbol::Function(0x28),
            },
        ],
        entry: 0,
    };
    let layout = Layout {
        rodata: 0x500,
        code: 0x1000,
        data: 0x2_0000,
        got: 0x3_0000,
    };
    let words: Vec<u32> = text
        .link(&layout)
        .chunks(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    assert_words(&words, &[0xf000_00e2, 0x9104_8c42, 0xf0ff_ffe3, 0x9114_4063, 0xf000_0164, 0x9100_6084, 0x9000_0005, 0x9100_a0a5, 0xd65f_03c0], "adrp x2, #0x1f000; add x2, x2, #0x123; adrp x3, #-0x1000; add x3, x3, #0x510; adrp x4, #0x2f000; add x4, x4, #0x18; adrp x5, #0; add x5, x5, #0x28; ret");
}
