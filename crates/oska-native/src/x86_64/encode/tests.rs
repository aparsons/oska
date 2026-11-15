//! Seam 2 of spec #153: every instruction form the x86_64 code generator
//! emits, against its encoding in the Intel 64 and IA-32 Architectures
//! Software Developer's Manual, Volume 2 (the opcode, `/r` or `/digit`,
//! REX, ModRM, SIB and displacement as its instruction pages give them).
//! The expected bytes are worked out from the manual by hand, register
//! numbers from its Table 3-1 (`rax` 0, `rcx` 1, `rdx` 2, `rbx` 3, `rsp` 4,
//! `rbp` 5, `rsi` 6, `rdi` 7, `r8`–`r15` 8–15, the high bit in REX).

use super::*;

/// The bytes `build` emits.
fn bytes(build: impl FnOnce(&mut Asm)) -> Vec<u8> {
    let mut asm = Asm::default();
    build(&mut asm);
    asm.bytes
}

fn check(name: &str, build: impl FnOnce(&mut Asm), expected: &[u8]) {
    assert_eq!(bytes(build), expected, "{name}");
}

fn at(base: u8, disp: i32) -> Mem {
    Mem {
        base,
        index: None,
        disp,
    }
}

// ---- Moves ---------------------------------------------------------------------

#[test]
fn mov_r64_rm64_is_rex_w_8b_r() {
    // MOV r64, r/m64: REX.W + 8B /r, the destination in ModRM.reg.
    check("mov rax, rbx", |a| a.mov(RAX, RBX), &[0x48, 0x8b, 0xc3]);
    check("mov r12, rsi", |a| a.mov(R12, RSI), &[0x4c, 0x8b, 0xe6]);
    check("mov rdi, r15", |a| a.mov(RDI, R15), &[0x49, 0x8b, 0xff]);
    check("mov rbp, rsp", |a| a.mov(RBP, RSP), &[0x48, 0x8b, 0xec]);
    check("mov rsp, rax", |a| a.mov(RSP, RAX), &[0x48, 0x8b, 0xe0]);
    check("mov r11, r10", |a| a.mov(R11, R10), &[0x4d, 0x8b, 0xda]);
}

#[test]
fn a_constant_takes_the_shortest_mov_that_holds_it() {
    // Zero: XOR r32, r/m32 (33 /r), which clears the whole register.
    check("xor eax, eax", |a| a.constant(RAX, 0), &[0x33, 0xc0]);
    check("xor r9d, r9d", |a| a.constant(R9, 0), &[0x45, 0x33, 0xc9]);
    // Up to 2^32 - 1: MOV r32, imm32 (B8+rd id), zero-extended.
    check("mov eax, 1", |a| a.constant(RAX, 1), &[0xb8, 0x01, 0, 0, 0]);
    check(
        "mov r11d, 0x12345678",
        |a| a.constant(R11, 0x1234_5678),
        &[0x41, 0xbb, 0x78, 0x56, 0x34, 0x12],
    );
    check(
        "mov esi, 0xffffffff",
        |a| a.constant(RSI, 0xffff_ffff),
        &[0xbe, 0xff, 0xff, 0xff, 0xff],
    );
    // A negative that fits 32 bits: MOV r/m64, imm32 (REX.W + C7 /0 id),
    // sign-extended.
    check(
        "mov rax, -1",
        |a| a.constant(RAX, -1),
        &[0x48, 0xc7, 0xc0, 0xff, 0xff, 0xff, 0xff],
    );
    check(
        "mov r13, -4096",
        |a| a.constant(R13, -4096),
        &[0x49, 0xc7, 0xc5, 0x00, 0xf0, 0xff, 0xff],
    );
    // Anything else: MOV r64, imm64 (REX.W + B8+rd io).
    check(
        "mov r10, 0x1122334455667788",
        |a| a.constant(R10, 0x1122_3344_5566_7788),
        &[0x49, 0xba, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11],
    );
    check(
        "mov rbx, i64::MIN",
        |a| a.constant(RBX, i64::MIN),
        &[0x48, 0xbb, 0, 0, 0, 0, 0, 0, 0, 0x80],
    );
}

#[test]
fn sign_extension_is_movsx_and_movsxd() {
    // MOVSX r64, r/m8: REX.W + 0F BE /r; r/m16: REX.W + 0F BF /r;
    // MOVSXD r64, r/m32: REX.W + 63 /r.
    check(
        "movsx rax, r11b",
        |a| a.sign_extend(RAX, R11, 8),
        &[0x49, 0x0f, 0xbe, 0xc3],
    );
    check(
        "movsx rax, r11w",
        |a| a.sign_extend(RAX, R11, 16),
        &[0x49, 0x0f, 0xbf, 0xc3],
    );
    check(
        "movsxd rax, r11d",
        |a| a.sign_extend(RAX, R11, 32),
        &[0x49, 0x63, 0xc3],
    );
}

// ---- Arithmetic ----------------------------------------------------------------

#[test]
fn two_register_arithmetic_is_the_op_r64_rm64_form() {
    // ADD 03 /r, OR 0B /r, AND 23 /r, SUB 2B /r, XOR 33 /r, CMP 3B /r, all
    // REX.W, the destination in ModRM.reg.
    check(
        "add rax, rcx",
        |a| a.alu(Alu::Add, RAX, RCX),
        &[0x48, 0x03, 0xc1],
    );
    check(
        "or rbx, rdx",
        |a| a.alu(Alu::Or, RBX, RDX),
        &[0x48, 0x0b, 0xda],
    );
    check(
        "and rsi, rdi",
        |a| a.alu(Alu::And, RSI, RDI),
        &[0x48, 0x23, 0xf7],
    );
    check(
        "sub r9, r10",
        |a| a.alu(Alu::Sub, R9, R10),
        &[0x4d, 0x2b, 0xca],
    );
    check(
        "xor r8, rax",
        |a| a.alu(Alu::Xor, R8, RAX),
        &[0x4c, 0x33, 0xc0],
    );
    check(
        "cmp rbx, r13",
        |a| a.alu(Alu::Cmp, RBX, R13),
        &[0x49, 0x3b, 0xdd],
    );
    // IMUL r64, r/m64: REX.W + 0F AF /r.
    check(
        "imul rdx, rsi",
        |a| a.imul(RDX, RSI),
        &[0x48, 0x0f, 0xaf, 0xd6],
    );
    check(
        "imul r11, r14",
        |a| a.imul(R11, R14),
        &[0x4d, 0x0f, 0xaf, 0xde],
    );
    // TEST r/m64, r64: REX.W + 85 /r.
    check("test rcx, rcx", |a| a.test(RCX, RCX), &[0x48, 0x85, 0xc9]);
    check("test r15, r15", |a| a.test(R15, R15), &[0x4d, 0x85, 0xff]);
}

#[test]
fn an_immediate_takes_imm8_when_it_fits_and_imm32_otherwise() {
    // 83 /digit ib (sign-extended imm8), 81 /digit id: ADD /0, OR /1,
    // AND /4, SUB /5, XOR /6, CMP /7.
    check(
        "add rsp, 8",
        |a| a.alu_imm(Alu::Add, RSP, 8),
        &[0x48, 0x83, 0xc4, 0x08],
    );
    check(
        "sub rsp, 0x1000",
        |a| a.alu_imm(Alu::Sub, RSP, 0x1000),
        &[0x48, 0x81, 0xec, 0x00, 0x10, 0x00, 0x00],
    );
    check(
        "cmp r11, -1",
        |a| a.alu_imm(Alu::Cmp, R11, -1),
        &[0x49, 0x83, 0xfb, 0xff],
    );
    check(
        "cmp rax, 4095",
        |a| a.alu_imm(Alu::Cmp, RAX, 4095),
        &[0x48, 0x81, 0xf8, 0xff, 0x0f, 0x00, 0x00],
    );
    check(
        "cmp rdi, -128",
        |a| a.alu_imm(Alu::Cmp, RDI, -128),
        &[0x48, 0x83, 0xff, 0x80],
    );
    check(
        "add r15, 1",
        |a| a.alu_imm(Alu::Add, R15, 1),
        &[0x49, 0x83, 0xc7, 0x01],
    );
    check(
        "sub r15, 1",
        |a| a.alu_imm(Alu::Sub, R15, 1),
        &[0x49, 0x83, 0xef, 0x01],
    );
    check(
        "add rbx, 128",
        |a| a.alu_imm(Alu::Add, RBX, 128),
        &[0x48, 0x81, 0xc3, 0x80, 0x00, 0x00, 0x00],
    );
}

#[test]
fn shifts_take_cl_or_an_imm8() {
    // SHL r/m64, CL: REX.W + D3 /4; SHR /5; SAR /7. By imm8: C1 /digit ib.
    check(
        "shl rax, cl",
        |a| a.shift_cl(Shift::Shl, RAX),
        &[0x48, 0xd3, 0xe0],
    );
    check(
        "shr r11, cl",
        |a| a.shift_cl(Shift::Shr, R11),
        &[0x49, 0xd3, 0xeb],
    );
    check(
        "sar rdx, cl",
        |a| a.shift_cl(Shift::Sar, RDX),
        &[0x48, 0xd3, 0xfa],
    );
    check(
        "shl rbx, 3",
        |a| a.shift_imm(Shift::Shl, RBX, 3),
        &[0x48, 0xc1, 0xe3, 0x03],
    );
    check(
        "shr rax, 32",
        |a| a.shift_imm(Shift::Shr, RAX, 32),
        &[0x48, 0xc1, 0xe8, 0x20],
    );
    check(
        "sar r12, 63",
        |a| a.shift_imm(Shift::Sar, R12, 63),
        &[0x49, 0xc1, 0xfc, 0x3f],
    );
}

#[test]
fn negation_multiplication_and_division_are_group_3() {
    // F7 /3 NEG, /4 MUL, /6 DIV, /7 IDIV, all REX.W; CQO is REX.W + 99.
    check("neg r11", |a| a.unary(Unary::Neg, R11), &[0x49, 0xf7, 0xdb]);
    check("mul rcx", |a| a.unary(Unary::Mul, RCX), &[0x48, 0xf7, 0xe1]);
    check("div r12", |a| a.unary(Unary::Div, R12), &[0x49, 0xf7, 0xf4]);
    check(
        "idiv rbx",
        |a| a.unary(Unary::Idiv, RBX),
        &[0x48, 0xf7, 0xfb],
    );
    check("cqo", Asm::cqo, &[0x48, 0x99]);
}

#[test]
fn a_condition_becomes_a_word_through_setcc_and_movzx() {
    // SETcc r/m8: 0F 90+cc /0, with a REX prefix for sil, dil and r8b on
    // (without one, 4 to 7 are ah, ch, dh and bh). MOVZX r32, r/m8: 0F B6 /r.
    check("sete al", |a| a.setcc(E, RAX), &[0x0f, 0x94, 0xc0]);
    check("setl sil", |a| a.setcc(L, RSI), &[0x40, 0x0f, 0x9c, 0xc6]);
    check("setb r9b", |a| a.setcc(B, R9), &[0x41, 0x0f, 0x92, 0xc1]);
    check("setg dil", |a| a.setcc(G, RDI), &[0x40, 0x0f, 0x9f, 0xc7]);
    check(
        "movzx eax, al",
        |a| a.zero_extend_byte(RAX),
        &[0x0f, 0xb6, 0xc0],
    );
    check(
        "movzx esi, sil",
        |a| a.zero_extend_byte(RSI),
        &[0x40, 0x0f, 0xb6, 0xf6],
    );
    check(
        "movzx r9d, r9b",
        |a| a.zero_extend_byte(R9),
        &[0x45, 0x0f, 0xb6, 0xc9],
    );
}

#[test]
fn condition_codes_are_the_manuals_tttn() {
    // Table B-10: O 0, B 2, AE 3, E 4, NE 5, BE 6, A 7, L C, GE D, LE E, G F.
    assert_eq!(
        [O, B, AE, E, NE, BE, A, L, GE, LE, G],
        [0x0, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0xc, 0xd, 0xe, 0xf]
    );
}

// ---- Memory --------------------------------------------------------------------

#[test]
fn a_load_zero_extends_into_the_word() {
    // MOV r64, r/m64 (REX.W + 8B /r); MOV r32, r/m32 (8B /r), which clears
    // the upper half; MOVZX r32, r/m8 (0F B6 /r) and r/m16 (0F B7 /r).
    check(
        "mov rax, [rbx + 8]",
        |a| a.load(Size::Q, RAX, at(RBX, 8)),
        &[0x48, 0x8b, 0x43, 0x08],
    );
    check(
        "mov eax, [rdi + 4]",
        |a| a.load(Size::D, RAX, at(RDI, 4)),
        &[0x8b, 0x47, 0x04],
    );
    check(
        "movzx eax, byte [rdi + 1]",
        |a| a.load(Size::B, RAX, at(RDI, 1)),
        &[0x0f, 0xb6, 0x47, 0x01],
    );
    check(
        "movzx r8d, word [rsi]",
        |a| a.load(Size::W, R8, at(RSI, 0)),
        &[0x44, 0x0f, 0xb7, 0x06],
    );
    check(
        "mov rcx, [rdi]",
        |a| a.load(Size::Q, RCX, at(RDI, 0)),
        &[0x48, 0x8b, 0x0f],
    );
}

#[test]
fn a_store_writes_the_low_bits() {
    // MOV r/m8, r8 (88 /r, with REX for sil and dil); MOV r/m16, r16
    // (66 89 /r); MOV r/m32, r32 (89 /r); MOV r/m64, r64 (REX.W + 89 /r).
    check(
        "mov [rbx + 8], rax",
        |a| a.store(Size::Q, at(RBX, 8), RAX),
        &[0x48, 0x89, 0x43, 0x08],
    );
    check(
        "mov byte [rdi], al",
        |a| a.store(Size::B, at(RDI, 0), RAX),
        &[0x88, 0x07],
    );
    check(
        "mov byte [rdi], sil",
        |a| a.store(Size::B, at(RDI, 0), RSI),
        &[0x40, 0x88, 0x37],
    );
    check(
        "mov word [rdi + 2], ax",
        |a| a.store(Size::W, at(RDI, 2), RAX),
        &[0x66, 0x89, 0x47, 0x02],
    );
    check(
        "mov dword [rdi], r8d",
        |a| a.store(Size::D, at(RDI, 0), R8),
        &[0x44, 0x89, 0x07],
    );
    check(
        "mov word [r9 + 6], r10w",
        |a| a.store(Size::W, at(R9, 6), R10),
        &[0x66, 0x45, 0x89, 0x51, 0x06],
    );
}

#[test]
fn addressing_takes_the_modrm_and_sib_forms_its_base_needs() {
    // `rsp` and `r12` as a base need a SIB byte (24: no index); `rbp` and
    // `r13` with no displacement need a zero disp8 (mod 00 means RIP or
    // disp32); a displacement past an i8 is a disp32.
    check(
        "mov rax, [rsp + 16]",
        |a| a.load(Size::Q, RAX, at(RSP, 16)),
        &[0x48, 0x8b, 0x44, 0x24, 0x10],
    );
    check(
        "mov rax, [r12]",
        |a| a.load(Size::Q, RAX, at(R12, 0)),
        &[0x49, 0x8b, 0x04, 0x24],
    );
    check(
        "mov rax, [rbp]",
        |a| a.load(Size::Q, RAX, at(RBP, 0)),
        &[0x48, 0x8b, 0x45, 0x00],
    );
    check(
        "mov rax, [r13]",
        |a| a.load(Size::Q, RAX, at(R13, 0)),
        &[0x49, 0x8b, 0x45, 0x00],
    );
    check(
        "mov rax, [rbx + 0x1000]",
        |a| a.load(Size::Q, RAX, at(RBX, 0x1000)),
        &[0x48, 0x8b, 0x83, 0x00, 0x10, 0x00, 0x00],
    );
    check(
        "mov [rsp + 8], r12",
        |a| a.store(Size::Q, at(RSP, 8), R12),
        &[0x4c, 0x89, 0x64, 0x24, 0x08],
    );
    check(
        "mov [rsp + 0x7ff8], rax",
        |a| a.store(Size::Q, at(RSP, 0x7ff8), RAX),
        &[0x48, 0x89, 0x84, 0x24, 0xf8, 0x7f, 0x00, 0x00],
    );
    check(
        "mov [rbp - 8], rbx",
        |a| a.store(Size::Q, at(RBP, -8), RBX),
        &[0x48, 0x89, 0x5d, 0xf8],
    );
    check(
        "mov r14, [rbp - 32]",
        |a| a.load(Size::Q, R14, at(RBP, -32)),
        &[0x4c, 0x8b, 0x75, 0xe0],
    );
    // An index register: SIB with scale 1, REX.X for r8 on.
    let indexed = |base, disp| Mem {
        base,
        index: Some(R11),
        disp,
    };
    check(
        "mov rax, [rbx + r11]",
        |a| a.load(Size::Q, RAX, indexed(RBX, 0)),
        &[0x4a, 0x8b, 0x04, 0x1b],
    );
    check(
        "mov rax, [r13 + r11]",
        |a| a.load(Size::Q, RAX, indexed(R13, 0)),
        &[0x4b, 0x8b, 0x44, 0x1d, 0x00],
    );
    check(
        "mov byte [rsp + r11], dil",
        |a| a.store(Size::B, indexed(RSP, 0), RDI),
        &[0x42, 0x88, 0x3c, 0x1c],
    );
}

#[test]
fn an_address_is_lea() {
    // LEA r64, m: REX.W + 8D /r; RIP-relative is mod 00, r/m 101, disp32.
    check(
        "lea rax, [rbp + 8]",
        |a| a.lea(RAX, at(RBP, 8)),
        &[0x48, 0x8d, 0x45, 0x08],
    );
    let mut asm = Asm::default();
    let disp = asm.lea_rip(R11);
    assert_eq!(asm.bytes, [0x4c, 0x8d, 0x1d, 0, 0, 0, 0], "lea r11, [rip]");
    assert_eq!(disp, 3, "the displacement to patch");
    let mut asm = Asm::default();
    asm.lea_rip(RCX);
    assert_eq!(asm.bytes, [0x48, 0x8d, 0x0d, 0, 0, 0, 0], "lea rcx, [rip]");
}

// ---- Control -------------------------------------------------------------------

#[test]
fn jumps_and_calls_leave_their_displacement_to_patch() {
    let mut asm = Asm::default();
    assert_eq!(asm.jmp(), 1);
    assert_eq!(asm.jcc(NE), 7);
    assert_eq!(asm.call(), 12);
    assert_eq!(asm.jcc8(NE), 17);
    assert_eq!(asm.jmp8(), 19);
    assert_eq!(
        asm.bytes,
        [
            0xe9, 0, 0, 0, 0, // JMP rel32: E9 cd
            0x0f, 0x85, 0, 0, 0, 0, // JNE rel32: 0F 85 cd
            0xe8, 0, 0, 0, 0, // CALL rel32: E8 cd
            0x75, 0, // JNE rel8: 75 cb
            0xeb, 0, // JMP rel8: EB cb
        ]
    );
    asm.patch32(1, -5);
    assert_eq!(asm.bytes[1..5], [0xfb, 0xff, 0xff, 0xff]);
}

#[test]
fn the_fixed_instructions() {
    check("call rax", |a| a.call_reg(RAX), &[0xff, 0xd0]);
    check("call r11", |a| a.call_reg(R11), &[0x41, 0xff, 0xd3]);
    check("push rbp", |a| a.push(RBP), &[0x55]);
    check("push r12", |a| a.push(R12), &[0x41, 0x54]);
    check("leave", Asm::leave, &[0xc9]);
    check("ret", Asm::ret, &[0xc3]);
    check("syscall", Asm::syscall, &[0x0f, 0x05]);
}
