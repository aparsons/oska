//! The executable file: a static, position-independent ELF for aarch64
//! Linux.
//!
//! - `ET_DYN` with no `PT_INTERP`, no dynamic section and no relocations:
//!   every address in the code is PC-relative, so the kernel loads it at a
//!   random address and it runs as it is.
//! - Three `PT_LOAD` segments: the headers and read-only data (R), the
//!   code (R+X), and the writable data (RW); and a `PT_GNU_STACK` that
//!   makes the stack non-executable.
//! - Each segment starts on a new 64 KiB boundary (the largest aarch64
//!   page size) both in the file and in memory. The kernel maps whole
//!   pages of the file, so a segment sharing a file page with its
//!   neighbour would map the neighbour's bytes with its own permissions
//!   (the headers and text literals executable, code writable): with
//!   every boundary on a 64 KiB one, no page has two segments'
//!   permissions whatever the kernel's page size. The padding costs up
//!   to 128 KiB of zeros in the file.
//!
//! Nothing in the file depends on the host, so every host writes the same
//! bytes for the same program.

use crate::aarch64::{self, Text};
use crate::ir::Symbol;

/// The alignment of segments in memory.
const PAGE: u64 = 0x1_0000;
const ELF_HEADER: u64 = 64;
const PROGRAM_HEADER: u64 = 56;
const PROGRAM_HEADERS: u64 = 4;
/// Where the read-only data starts: after the headers.
const RODATA: u64 = ELF_HEADER + PROGRAM_HEADER * PROGRAM_HEADERS;

const PT_LOAD: u32 = 1;
const PT_GNU_STACK: u32 = 0x6474_e551;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const ET_DYN: u16 = 3;
const EM_AARCH64: u16 = 183;

/// A segment's place in the file and in memory.
struct Segment {
    offset: u64,
    vaddr: u64,
    size: u64,
    flags: u32,
}

/// The executable holding `text`, `rodata` and `data_size` bytes of
/// zeroed writable data.
pub fn write(text: &Text, rodata: &[u8], data_size: u32) -> Vec<u8> {
    let read_only = Segment {
        offset: 0,
        vaddr: 0,
        size: RODATA + rodata.len() as u64,
        flags: PF_R,
    };
    let code = after(&read_only, 4 * text.words.len() as u64, PF_R | PF_X);
    let data = after(&code, u64::from(data_size), PF_R | PF_W);

    let mut words = text.words.clone();
    for fixup in &text.fixups {
        let target = match fixup.symbol {
            Symbol::Rodata(at) => RODATA + u64::from(at),
            Symbol::Data(at) => data.vaddr + u64::from(at),
            Symbol::Function(start) => code.vaddr + 4 * start as u64,
        };
        let pc = code.vaddr + 4 * fixup.at as u64;
        let adrp = &mut words[fixup.at];
        *adrp = aarch64::adrp(*adrp & 0x1f, pc, target);
        let add = &mut words[fixup.at + 1];
        let rd = *add & 0x1f;
        *add = aarch64::add_imm(rd, rd, (target & 0xfff) as u32, false);
    }

    let mut file = Vec::new();
    // The ELF header.
    file.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]);
    file.extend_from_slice(&[0; 8]);
    put16(&mut file, ET_DYN);
    put16(&mut file, EM_AARCH64);
    put32(&mut file, 1);
    put64(&mut file, code.vaddr + 4 * text.entry as u64);
    put64(&mut file, ELF_HEADER);
    put64(&mut file, 0);
    put32(&mut file, 0);
    put16(&mut file, ELF_HEADER as u16);
    put16(&mut file, PROGRAM_HEADER as u16);
    put16(&mut file, PROGRAM_HEADERS as u16);
    put16(&mut file, 64);
    put16(&mut file, 0);
    put16(&mut file, 0);
    // The program headers.
    for segment in [&read_only, &code, &data] {
        put32(&mut file, PT_LOAD);
        put32(&mut file, segment.flags);
        put64(&mut file, segment.offset);
        put64(&mut file, segment.vaddr);
        put64(&mut file, segment.vaddr);
        put64(&mut file, segment.size);
        put64(&mut file, segment.size);
        put64(&mut file, PAGE);
    }
    put32(&mut file, PT_GNU_STACK);
    put32(&mut file, PF_R | PF_W);
    for _ in 0..5 {
        put64(&mut file, 0);
    }
    put64(&mut file, 16);
    // The segments' contents.
    file.extend_from_slice(rodata);
    pad_to(&mut file, code.offset);
    for word in words {
        put32(&mut file, word);
    }
    pad_to(&mut file, data.offset);
    file.resize(file.len() + data_size as usize, 0);
    file
}

/// The segment of `size` bytes that follows `previous`, on pages of its
/// own in the file and in memory.
fn after(previous: &Segment, size: u64, flags: u32) -> Segment {
    let offset = (previous.offset + previous.size).next_multiple_of(PAGE);
    let vaddr = (previous.vaddr + previous.size).next_multiple_of(PAGE);
    Segment {
        offset,
        vaddr,
        size,
        flags,
    }
}

fn pad_to(file: &mut Vec<u8>, offset: u64) {
    file.resize(offset as usize, 0);
}

fn put16(file: &mut Vec<u8>, value: u16) {
    file.extend_from_slice(&value.to_le_bytes());
}

fn put32(file: &mut Vec<u8>, value: u32) {
    file.extend_from_slice(&value.to_le_bytes());
}

fn put64(file: &mut Vec<u8>, value: u64) {
    file.extend_from_slice(&value.to_le_bytes());
}
