//! The executable file for macOS: a position-independent Mach-O that the
//! dynamic loader (`dyld`) starts, which imports only from libSystem,
//! with an ad-hoc code signature (ADR 0011, ADR 0014, ADR 0015).
//!
//! The formats are Apple's: `mach-o/loader.h` (xnu), `mach-o/
//! fixup-chains.h` (dyld) and the code-signing blobs of `cs_blobs.h`
//! (xnu). The layout:
//!
//! - `__PAGEZERO`: the first 4 GiB, mapped with no access, so a null or
//!   truncated pointer faults.
//! - `__TEXT` (read and execute): the header, the load commands and the
//!   code (`__text`).
//! - `__DATA_CONST` (read and write while the loader binds the imports,
//!   then read-only, `SG_READ_ONLY`): the import slots (`__got`), then
//!   the read-only data (`__const`), so no data is executable.
//! - `__DATA` (read and write): the zeroed writable data.
//! - `__LINKEDIT` (read-only): the chained fixups that bind each import
//!   slot to its libSystem function, then the code signature.
//!
//! Each segment starts on a 16 KiB page, Apple silicon's, in the file and
//! in memory. Every address in the code is PC-relative, and the import
//! slots are the only words the loader writes: the file has no
//! relocations of its own.
//!
//! The `LC_UUID` is the SHA-256 of the file before it is signed, and the
//! signature's identifier is made from it: nothing in the file depends on
//! the host or the output's name, so every host writes the same bytes.

use crate::sha256;
use crate::target::{Code, Layout};

/// What the file says about its target's processor.
pub struct Machine {
    /// The Mach header's `cputype`.
    pub cputype: u32,
}

/// `CPU_TYPE_ARM64`: `CPU_TYPE_ARM | CPU_ARCH_ABI64`.
pub const CPU_TYPE_ARM64: u32 = 0x0100_000c;

/// Apple silicon's page, which segments are aligned to.
const PAGE: u64 = 0x4000;
/// The page code signatures hash: 4 KiB, on every processor (`1 << 12`).
const SIGNED_PAGE: usize = 0x1000;
/// Where `__TEXT` is mapped: above `__PAGEZERO`.
const BASE: u64 = 0x1_0000_0000;

const MH_MAGIC_64: u32 = 0xfeed_facf;
const MH_EXECUTE: u32 = 2;
/// `MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE`, as ld64 writes
/// them for an executable: every symbol is bound to a library.
const MH_FLAGS: u32 = 0x0020_0085;
const HEADER: u64 = 32;

const LC_SEGMENT_64: u32 = 0x19;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_DYLINKER: u32 = 0xe;
const LC_UUID: u32 = 0x1b;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const LC_BUILD_VERSION: u32 = 0x32;
/// `LC_REQ_DYLD` is set: a loader that does not know these must refuse.
const LC_MAIN: u32 = 0x8000_0028;
const LC_DYLD_CHAINED_FIXUPS: u32 = 0x8000_0034;

const VM_PROT_READ: u32 = 1;
const VM_PROT_WRITE: u32 = 2;
const VM_PROT_EXECUTE: u32 = 4;
/// The segment is made read-only once its fixups are applied.
const SG_READ_ONLY: u32 = 0x10;

const S_REGULAR: u32 = 0;
const S_NON_LAZY_SYMBOL_POINTERS: u32 = 6;
/// `S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS`.
const S_CODE: u32 = 0x8000_0400;

const PLATFORM_MACOS: u32 = 1;
/// macOS 12.0, as `xxxx.yy.zz` nibbles: the first with chained fixups
/// on arm64 (ld64's default from it), declared as both the minimum and
/// the SDK.
const MACOS_12: u32 = 0x000c_0000;

const DYLD: &str = "/usr/lib/dyld";
const LIBSYSTEM: &str = "/usr/lib/libSystem.B.dylib";

/// `DYLD_CHAINED_PTR_64_OFFSET`: 64-bit slots, as ld64 writes them for
/// arm64 executables from macOS 12. A bind slot is the same in it as in
/// `DYLD_CHAINED_PTR_64`.
const DYLD_CHAINED_PTR_64_OFFSET: u16 = 6;
const DYLD_CHAINED_IMPORT: u32 = 1;
const DYLD_CHAINED_PTR_START_NONE: u16 = 0xffff;

const CSMAGIC_EMBEDDED_SIGNATURE: u32 = 0xfade_0cc0;
const CSMAGIC_CODEDIRECTORY: u32 = 0xfade_0c02;
const CSMAGIC_BLOBWRAPPER: u32 = 0xfade_0b01;
const CSSLOT_CODEDIRECTORY: u32 = 0;
const CSSLOT_REQUIREMENTS: u32 = 2;
const CSSLOT_SIGNATURESLOT: u32 = 0x1_0000;
/// The code directory's version with the executable segment's fields.
const CS_SUPPORTSEXECSEG: u32 = 0x2_0400;
const CS_ADHOC: u32 = 2;
const CS_HASHTYPE_SHA256: u8 = 2;
const CS_EXECSEG_MAIN_BINARY: u64 = 1;
/// The code directory's fixed fields, up to `execSegFlags`.
const CODE_DIRECTORY: usize = 88;
/// The special slots hashed: the requirements (-2), and the absent
/// `Info.plist` (-1, zeros).
const SPECIAL_SLOTS: usize = 2;
/// An empty requirements set: the blob's magic (`CSMAGIC_REQUIREMENTS`),
/// its length and a count of 0.
const EMPTY_REQUIREMENTS: [u8; 12] = [0xfa, 0xde, 0x0c, 0x01, 0, 0, 0, 12, 0, 0, 0, 0];
const HASH: usize = 32;

/// The segments, in the order of their load commands: the chained
/// fixups name each by its index.
const SEGMENTS: usize = 5;
/// `__DATA_CONST`'s index: the one segment with fixups.
const DATA_CONST: u32 = 2;

/// Where each part of the file is, as offsets from its start, which are
/// also the parts' addresses from `__TEXT`'s.
struct Plan {
    commands: u64,
    text: u64,
    data_const: u64,
    got: u64,
    rodata: u64,
    data: u64,
    data_end: u64,
    linkedit: u64,
    fixups: u64,
    signature: u64,
    end: u64,
}

/// The executable for `machine` holding `text`, `rodata`, `data_size`
/// bytes of zeroed writable data, and a slot bound to each of `imports`,
/// libSystem's functions, in order.
pub fn write(
    machine: &Machine,
    text: &dyn Code,
    rodata: &[u8],
    data_size: u32,
    imports: &[&str],
) -> Vec<u8> {
    let plan = plan(text, rodata, data_size, imports);
    let fixups = fixups(plan.data_const, plan.data_const_pages(), imports);
    let linked = text.link(&Layout {
        rodata: BASE + plan.rodata,
        code: BASE + plan.text,
        data: BASE + plan.data,
        got: BASE + plan.got,
    });

    let mut file = Vec::new();
    put32(&mut file, MH_MAGIC_64);
    put32(&mut file, machine.cputype);
    put32(&mut file, 0);
    put32(&mut file, MH_EXECUTE);
    put32(&mut file, COMMANDS);
    put32(&mut file, plan.commands as u32);
    put32(&mut file, MH_FLAGS);
    put32(&mut file, 0);

    segment(&mut file, "__PAGEZERO", 0, BASE, 0, 0, 0, 0, &[]);
    segment(
        &mut file,
        "__TEXT",
        BASE,
        plan.data_const,
        0,
        plan.data_const,
        VM_PROT_READ | VM_PROT_EXECUTE,
        0,
        &[Section {
            name: "__text",
            at: plan.text,
            size: text.size(),
            align: 2,
            flags: S_CODE,
        }],
    );
    segment(
        &mut file,
        "__DATA_CONST",
        BASE + plan.data_const,
        plan.data - plan.data_const,
        plan.data_const,
        plan.data - plan.data_const,
        VM_PROT_READ | VM_PROT_WRITE,
        SG_READ_ONLY,
        &[
            Section {
                name: "__got",
                at: plan.got,
                size: 8 * imports.len() as u64,
                align: 3,
                flags: S_NON_LAZY_SYMBOL_POINTERS,
            },
            Section {
                name: "__const",
                at: plan.rodata,
                size: rodata.len() as u64,
                align: 4,
                flags: S_REGULAR,
            },
        ],
    );
    segment(
        &mut file,
        "__DATA",
        BASE + plan.data,
        plan.linkedit - plan.data,
        plan.data,
        plan.linkedit - plan.data,
        VM_PROT_READ | VM_PROT_WRITE,
        0,
        &[Section {
            name: "__data",
            at: plan.data,
            size: plan.data_end - plan.data,
            align: 3,
            flags: S_REGULAR,
        }],
    );
    segment(
        &mut file,
        "__LINKEDIT",
        BASE + plan.linkedit,
        (plan.end - plan.linkedit).next_multiple_of(PAGE),
        plan.linkedit,
        plan.end - plan.linkedit,
        VM_PROT_READ,
        0,
        &[],
    );
    linkedit_data(
        &mut file,
        LC_DYLD_CHAINED_FIXUPS,
        plan.fixups,
        fixups.len() as u64,
    );
    // The dynamic loader, its name at offset 12.
    put32(&mut file, LC_LOAD_DYLINKER);
    put32(&mut file, DYLINKER_COMMAND);
    put32(&mut file, 12);
    put_name(&mut file, DYLD, DYLINKER_COMMAND as usize - 12);
    // The UUID, filled in once the rest of the file is written.
    put32(&mut file, LC_UUID);
    put32(&mut file, 24);
    let uuid_at = file.len();
    file.extend_from_slice(&[0; 16]);
    put32(&mut file, LC_BUILD_VERSION);
    put32(&mut file, 24);
    put32(&mut file, PLATFORM_MACOS);
    put32(&mut file, MACOS_12);
    put32(&mut file, MACOS_12);
    put32(&mut file, 0);
    // Where the process starts, from the start of `__TEXT`, and the
    // main thread's default stack size.
    put32(&mut file, LC_MAIN);
    put32(&mut file, 24);
    put64(&mut file, plan.text + text.entry());
    put64(&mut file, 0);
    // libSystem: its name at offset 24, a timestamp, and its current and
    // compatibility versions, 1.0.0, which every libSystem meets.
    put32(&mut file, LC_LOAD_DYLIB);
    put32(&mut file, DYLIB_COMMAND);
    put32(&mut file, 24);
    put32(&mut file, 2);
    put32(&mut file, 0x1_0000);
    put32(&mut file, 0x1_0000);
    put_name(&mut file, LIBSYSTEM, DYLIB_COMMAND as usize - 24);
    linkedit_data(
        &mut file,
        LC_CODE_SIGNATURE,
        plan.signature,
        plan.end - plan.signature,
    );
    debug_assert_eq!(file.len() as u64, HEADER + plan.commands);

    pad_to(&mut file, plan.text);
    file.extend_from_slice(&linked);
    pad_to(&mut file, plan.got);
    for index in 0..imports.len() {
        let last = index + 1 == imports.len();
        put64(&mut file, bind(index as u64, if last { 0 } else { 2 }));
    }
    pad_to(&mut file, plan.rodata);
    file.extend_from_slice(rodata);
    pad_to(&mut file, plan.linkedit);
    file.extend_from_slice(&fixups);
    pad_to(&mut file, plan.signature);

    let uuid = sha256::digest(&file);
    file[uuid_at..uuid_at + 16].copy_from_slice(&uuid[..16]);
    let identifier = identifier(&uuid);
    let signature = signature(&file, &identifier, plan.data_const);
    file.extend_from_slice(&signature);
    debug_assert_eq!(file.len() as u64, plan.end);
    file
}

/// How many load commands the file has.
const COMMANDS: u32 = 12;
/// `dylinker_command` and its name, padded to 8 bytes.
const DYLINKER_COMMAND: u32 = 32;
/// `dylib_command` and its name, padded to 8 bytes.
const DYLIB_COMMAND: u32 = 56;

/// Lay the file out.
fn plan(text: &dyn Code, rodata: &[u8], data_size: u32, imports: &[&str]) -> Plan {
    let commands = 72
        + (72 + 80)
        + (72 + 2 * 80)
        + (72 + 80)
        + 72
        + 16
        + u64::from(DYLINKER_COMMAND)
        + 24
        + 24
        + 24
        + u64::from(DYLIB_COMMAND)
        + 16;
    let text_at = (HEADER + commands).next_multiple_of(16);
    let text_end = text_at + text.size();
    let data_const = text_end.next_multiple_of(PAGE);
    let got = data_const;
    let rodata_at = (got + 8 * imports.len() as u64).next_multiple_of(16);
    let data = (rodata_at + rodata.len() as u64).next_multiple_of(PAGE);
    let data_end = data + u64::from(data_size);
    let linkedit = data_end.max(data + 1).next_multiple_of(PAGE);
    let fixups = fixups(data_const, (data - data_const) / PAGE, imports).len() as u64;
    let signature = (linkedit + fixups).next_multiple_of(16);
    let code_limit = signature as usize;
    Plan {
        commands,
        text: text_at,
        data_const,
        got,
        rodata: rodata_at,
        data,
        data_end,
        linkedit,
        fixups: linkedit,
        signature,
        end: signature + signature_size(code_limit) as u64,
    }
}

impl Plan {
    /// How many pages `__DATA_CONST` has.
    fn data_const_pages(&self) -> u64 {
        (self.data - self.data_const) / PAGE
    }
}

/// A section of a segment.
struct Section {
    name: &'static str,
    at: u64,
    size: u64,
    /// As a power of two.
    align: u32,
    flags: u32,
}

/// A `segment_command_64` and its sections, which are mapped at their
/// file offsets above `BASE`.
#[allow(clippy::too_many_arguments)]
fn segment(
    file: &mut Vec<u8>,
    name: &str,
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    prot: u32,
    flags: u32,
    sections: &[Section],
) {
    put32(file, LC_SEGMENT_64);
    put32(file, 72 + 80 * sections.len() as u32);
    put_fixed(file, name);
    put64(file, vmaddr);
    put64(file, vmsize);
    put64(file, fileoff);
    put64(file, filesize);
    // The most protection it may ever have, and what it starts with.
    put32(file, prot);
    put32(file, prot);
    put32(file, sections.len() as u32);
    put32(file, flags);
    for section in sections {
        put_fixed(file, section.name);
        put_fixed(file, name);
        put64(file, BASE + section.at);
        put64(file, section.size);
        put32(file, section.at as u32);
        put32(file, section.align);
        // No relocations.
        put32(file, 0);
        put32(file, 0);
        put32(file, section.flags);
        for _ in 0..3 {
            put32(file, 0);
        }
    }
}

/// A `linkedit_data_command`: the part of `__LINKEDIT` at `offset` in the
/// file, `size` bytes.
fn linkedit_data(file: &mut Vec<u8>, cmd: u32, offset: u64, size: u64) {
    put32(file, cmd);
    put32(file, 16);
    put32(file, offset as u32);
    put32(file, size as u32);
}

/// The import slot `ordinal`'s word: a `dyld_chained_ptr_64_bind`
/// binding it to import `ordinal`, `next` 4-byte strides before the next
/// slot of the chain (0 for the last).
fn bind(ordinal: u64, next: u64) -> u64 {
    1 << 63 | next << 51 | ordinal
}

/// The chained fixups (`LC_DYLD_CHAINED_FIXUPS`): one chain, through the
/// import slots at the start of `__DATA_CONST`, which starts at
/// `data_const` from `__TEXT` and is `pages` pages long; each import from
/// libSystem, the first and only library.
fn fixups(data_const: u64, pages: u64, imports: &[&str]) -> Vec<u8> {
    // `dyld_chained_fixups_header`, then the starts at 32.
    let starts = 32u32;
    let image = 4 + 4 * SEGMENTS as u32;
    let segment_starts = image.next_multiple_of(8);
    let segment_size = 22 + 2 * pages as u32;
    let imports_at = (starts + segment_starts + segment_size).next_multiple_of(4);
    let symbols_at = imports_at + 4 * imports.len() as u32;
    let mut blob = Vec::new();
    put32(&mut blob, 0);
    put32(&mut blob, starts);
    put32(&mut blob, imports_at);
    put32(&mut blob, symbols_at);
    put32(&mut blob, imports.len() as u32);
    put32(&mut blob, DYLD_CHAINED_IMPORT);
    // Uncompressed symbol names.
    put32(&mut blob, 0);
    pad_to(&mut blob, u64::from(starts));
    // `dyld_chained_starts_in_image`: every segment, with the offset of
    // its starts from here, or 0 for none.
    put32(&mut blob, SEGMENTS as u32);
    for index in 0..SEGMENTS as u32 {
        put32(
            &mut blob,
            if index == DATA_CONST {
                segment_starts
            } else {
                0
            },
        );
    }
    pad_to(&mut blob, u64::from(starts + segment_starts));
    // `dyld_chained_starts_in_segment`: the chain starts at the first
    // page's first byte; no other page has one.
    put32(&mut blob, segment_size);
    put16(&mut blob, PAGE as u16);
    put16(&mut blob, DYLD_CHAINED_PTR_64_OFFSET);
    put64(&mut blob, data_const);
    put32(&mut blob, 0);
    put16(&mut blob, pages as u16);
    for page in 0..pages {
        put16(
            &mut blob,
            if page == 0 {
                0
            } else {
                DYLD_CHAINED_PTR_START_NONE
            },
        );
    }
    pad_to(&mut blob, u64::from(imports_at));
    // `dyld_chained_import`: library ordinal 1, not weak, the name's
    // offset in the symbols.
    let mut name_at = 0u32;
    for name in imports {
        put32(&mut blob, 1 | name_at << 9);
        name_at += name.len() as u32 + 1;
    }
    for name in imports {
        blob.extend_from_slice(name.as_bytes());
        blob.push(0);
    }
    let end = (blob.len() as u64).next_multiple_of(8);
    pad_to(&mut blob, end);
    blob
}

/// The signature's identifier: `oska-` and the UUID in hex, the same
/// length for every program.
fn identifier(uuid: &[u8; 32]) -> String {
    let hex: String = uuid[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("oska-{hex}")
}

/// The identifier's length, with its NUL.
const IDENTIFIER: usize = 5 + 32 + 1;

/// The size of the signature of a file whose first `code_limit` bytes
/// are signed.
fn signature_size(code_limit: usize) -> usize {
    12 + 3 * 8 + code_directory_size(code_limit) + EMPTY_REQUIREMENTS.len() + 8
}

fn code_directory_size(code_limit: usize) -> usize {
    CODE_DIRECTORY + IDENTIFIER + HASH * (SPECIAL_SLOTS + code_limit.div_ceil(SIGNED_PAGE))
}

/// The ad-hoc code signature of `file`, all of which is signed: an
/// embedded signature holding a code directory of SHA-256 page hashes
/// with no certificate, an empty requirements set, and an empty CMS
/// signature. `exec_limit` is `__TEXT`'s size, which starts the file.
/// Every field is big-endian.
fn signature(file: &[u8], identifier: &str, exec_limit: u64) -> Vec<u8> {
    let code_limit = file.len();
    let directory_size = code_directory_size(code_limit);
    let directory_at = 12 + 3 * 8;
    let requirements_at = directory_at + directory_size;
    let wrapper_at = requirements_at + EMPTY_REQUIREMENTS.len();
    let total = signature_size(code_limit);

    let mut blob = Vec::new();
    be32(&mut blob, CSMAGIC_EMBEDDED_SIGNATURE);
    be32(&mut blob, total as u32);
    be32(&mut blob, 3);
    for (slot, at) in [
        (CSSLOT_CODEDIRECTORY, directory_at),
        (CSSLOT_REQUIREMENTS, requirements_at),
        (CSSLOT_SIGNATURESLOT, wrapper_at),
    ] {
        be32(&mut blob, slot);
        be32(&mut blob, at as u32);
    }

    let ident_at = CODE_DIRECTORY;
    let hashes_at = ident_at + IDENTIFIER + HASH * SPECIAL_SLOTS;
    be32(&mut blob, CSMAGIC_CODEDIRECTORY);
    be32(&mut blob, directory_size as u32);
    be32(&mut blob, CS_SUPPORTSEXECSEG);
    be32(&mut blob, CS_ADHOC);
    be32(&mut blob, hashes_at as u32);
    be32(&mut blob, ident_at as u32);
    be32(&mut blob, SPECIAL_SLOTS as u32);
    be32(&mut blob, code_limit.div_ceil(SIGNED_PAGE) as u32);
    be32(&mut blob, code_limit as u32);
    blob.push(HASH as u8);
    blob.push(CS_HASHTYPE_SHA256);
    // No platform; pages of `1 << 12` bytes.
    blob.push(0);
    blob.push(12);
    // `spare2`, `scatterOffset`, `teamOffset`, `spare3` and
    // `codeLimit64`: none.
    for _ in 0..4 {
        be32(&mut blob, 0);
    }
    blob.extend_from_slice(&0u64.to_be_bytes());
    // The executable segment: `__TEXT`, of the main executable.
    blob.extend_from_slice(&0u64.to_be_bytes());
    blob.extend_from_slice(&exec_limit.to_be_bytes());
    blob.extend_from_slice(&CS_EXECSEG_MAIN_BINARY.to_be_bytes());
    blob.extend_from_slice(identifier.as_bytes());
    blob.push(0);
    // Slot -2, the requirements; slot -1, the absent `Info.plist`.
    blob.extend_from_slice(&sha256::digest(&EMPTY_REQUIREMENTS));
    blob.extend_from_slice(&[0; HASH]);
    for page in file.chunks(SIGNED_PAGE) {
        blob.extend_from_slice(&sha256::digest(page));
    }

    blob.extend_from_slice(&EMPTY_REQUIREMENTS);
    be32(&mut blob, CSMAGIC_BLOBWRAPPER);
    be32(&mut blob, 8);
    debug_assert_eq!(blob.len(), total);
    blob
}

/// `name`, NUL-padded to 16 bytes.
fn put_fixed(file: &mut Vec<u8>, name: &str) {
    let mut field = [0u8; 16];
    field[..name.len()].copy_from_slice(name.as_bytes());
    file.extend_from_slice(&field);
}

/// `name` and a NUL, padded with NULs to `size` bytes.
fn put_name(file: &mut Vec<u8>, name: &str, size: usize) {
    let end = file.len() + size;
    file.extend_from_slice(name.as_bytes());
    file.resize(end, 0);
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

fn be32(file: &mut Vec<u8>, value: u32) {
    file.extend_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod tests;
