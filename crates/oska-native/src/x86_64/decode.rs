// SPDX-License-Identifier: Apache-2.0
//! Enough of an x86_64 decoder to read back what `encode.rs` writes, for
//! the constant-time tests (`constant_time.rs`): each instruction's length,
//! whether it is a conditional jump, and the base register of a memory
//! operand that reads or writes memory. Decoding the bytes, not trusting
//! the encoder's own account, sees whatever the code generator emitted.
//! An opcode the encoder never writes fails the test.

use super::encode::{RBP, RSP};
use crate::target::Selected;

/// What one instruction is, as the tests see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    ConditionalJump,
    /// A load or store at `[base + ...]`, or at `[rip + ...]` (`None`).
    Memory(Option<u8>),
    Other,
}

/// The instructions in `bytes`, which must be whole ones, counted into a
/// [`Selected`].
pub fn selected(bytes: &[u8]) -> Selected {
    let mut selected = Selected::default();
    let mut at = 0;
    while at < bytes.len() {
        let (length, kind) = instruction(&bytes[at..]);
        match kind {
            Kind::ConditionalJump => selected.conditional_branches += 1,
            Kind::Memory(Some(RSP | RBP)) => selected.stack_accesses += 1,
            Kind::Memory(_) => selected.other_accesses += 1,
            Kind::Other => {}
        }
        at += length;
    }
    assert_eq!(at, bytes.len(), "an instruction runs past its code");
    selected
}

/// The length and kind of the instruction at the start of `code`.
fn instruction(code: &[u8]) -> (usize, Kind) {
    let byte = |i: usize| -> u8 {
        *code
            .get(i)
            .unwrap_or_else(|| panic!("an instruction cut short: {code:02x?}"))
    };
    let mut at = 0;
    if byte(at) == 0x66 {
        at += 1;
    }
    let mut rex = 0;
    if (0x40..0x50).contains(&byte(at)) {
        rex = byte(at);
        at += 1;
    }
    let wide = rex & 8 != 0;
    let opcode = byte(at);
    at += 1;
    // The ModRM operand at `at`, then `imm` bytes of immediate. `lea`
    // computes an address without reading memory.
    let modrm = |at: usize, imm: usize, reads: bool| -> (usize, Kind) {
        let m = byte(at);
        let (mode, rm) = (m >> 6, m & 7);
        let mut length = at + 1;
        if mode == 3 {
            return (length + imm, Kind::Other);
        }
        let base = if rm == 4 {
            let sib = byte(length);
            length += 1;
            assert!(
                !(mode == 0 && sib & 7 == 5),
                "a SIB with no base: {code:02x?}"
            );
            Some(sib & 7 | (rex & 1) << 3)
        } else if rm == 5 && mode == 0 {
            length += 4;
            None
        } else {
            Some(rm | (rex & 1) << 3)
        };
        length += match mode {
            1 => 1,
            2 => 4,
            _ => 0,
        };
        let kind = if reads {
            Kind::Memory(base)
        } else {
            Kind::Other
        };
        (length + imm, kind)
    };
    match opcode {
        0x0f => {
            let second = byte(at);
            at += 1;
            match second {
                0x80..=0x8f => (at + 4, Kind::ConditionalJump),
                0x05 => (at, Kind::Other),
                0x90..=0x9f | 0xaf | 0xb6 | 0xb7 | 0xbe | 0xbf => modrm(at, 0, true),
                _ => panic!("an opcode the encoder does not write: 0f {second:02x}"),
            }
        }
        0x70..=0x7f => (at + 1, Kind::ConditionalJump),
        0x03 | 0x0b | 0x23 | 0x2b | 0x33 | 0x3b | 0x63 | 0x85 | 0x88 | 0x89 | 0x8b | 0xd3
        | 0xf7 | 0xff => modrm(at, 0, true),
        0x8d => modrm(at, 0, false),
        0x83 | 0xc1 => modrm(at, 1, true),
        0x81 | 0xc7 => modrm(at, 4, true),
        0xb8..=0xbf => (at + if wide { 8 } else { 4 }, Kind::Other),
        0x99 | 0x50..=0x5f | 0xc3 | 0xc9 => (at, Kind::Other),
        0xe8 | 0xe9 => (at + 4, Kind::Other),
        0xeb => (at + 1, Kind::Other),
        _ => panic!("an opcode the encoder does not write: {opcode:02x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::encode::{Asm, Mem, Size, E, R12, RAX, RBX, RSP};
    use super::selected;

    #[test]
    fn it_reads_back_branches_and_memory_operands() {
        let mut asm = Asm::default();
        asm.jcc(E);
        asm.jcc8(E);
        asm.load(
            Size::B,
            RAX,
            Mem {
                base: RBX,
                index: None,
                disp: 300,
            },
        );
        asm.store(
            Size::Q,
            Mem {
                base: RSP,
                index: None,
                disp: 8,
            },
            R12,
        );
        asm.lea(
            RAX,
            Mem {
                base: RBX,
                index: None,
                disp: 0,
            },
        );
        asm.constant(RAX, -1 << 40);
        let got = selected(&asm.bytes);
        assert_eq!(got.conditional_branches, 2);
        assert_eq!(got.stack_accesses, 1);
        assert_eq!(got.other_accesses, 1);
    }
}
