//! The executable file for macOS: a 64-bit Mach-O `MH_EXECUTE` that the
//! dynamic loader, `dyld`, loads, imports functions from `libSystem` only,
//! and carries an ad-hoc code signature `oskac` writes itself (ADR 0011,
//! ADR 0015). The formats are Apple's: `mach-o/loader.h` (xnu),
//! `mach-o/fixup-chains.h` (dyld) and the code signature's
//! `CodeDirectory`, as `docs/research/2026-11-macos-libsystem-calls.md`
//! cites them.
//!
//! - Position-independent (`MH_PIE`), with a 4 GiB `__PAGEZERO` below
//!   everything, so a null pointer and any address made from one fault.
//! - `__TEXT` (read and execute): the header, the load commands and the
//!   code. `__DATA_CONST` (read-only once `dyld` has bound it,
//!   `SG_READ_ONLY`): the import table, `__got`, then the read-only data.
//!   `__DATA` (read and write): the zero-initialised writable data, which
//!   takes no bytes in the file. `__LINKEDIT` (read-only): the import
//!   table's fixups, an empty string table and the code signature. Code
//!   is never writable and data never executable. Each segment starts on
//!   a boundary of the target's page in the file and in memory.
//! - The import table holds one slot per function of the platform's
//!   import list, always all of them, bound at load through
//!   `LC_DYLD_CHAINED_FIXUPS`: no stubs, no lazy binding, and nothing
//!   the program fixes up itself. Every import comes from
//!   `/usr/lib/libSystem.B.dylib`, the one library loaded.
//! - `LC_MAIN` names the entry point, which `dyld` calls as `main(argc,
//!   argv)`.
//! - `LC_UUID` is a hash of the rest of the file, and the code signature
//!   hashes every page before it, so the file depends on the program and
//!   the target alone, as on Linux: every host writes the same bytes.

use crate::sha256;
use crate::target::{Code, Layout};

/// What the file says about its target's processor.
pub struct Machine {
    /// The header's `cputype` and `cpusubtype`.
    pub cputype: u32,
    pub cpusubtype: u32,
    /// The alignment of segments, in the file and in memory: the target's
    /// page size.
    pub page: u64,
    /// The oldest macOS the file runs on, as `LC_BUILD_VERSION` encodes
    /// it: `0xXXXX_YYZZ` for X.Y.Z.
    pub minos: u32,
}

/// `CPU_TYPE_ARM64` and `CPU_SUBTYPE_ARM64_ALL`.
pub const CPU_TYPE_ARM64: u32 = 0x0100_000c;
pub const CPU_SUBTYPE_ARM64_ALL: u32 = 0;

const MH_MAGIC_64: u32 = 0xfeed_facf;
const MH_EXECUTE: u32 = 2;
/// `MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE`, as `ld64` writes
/// them for an executable.
const MH_FLAGS: u32 = 0x0020_0085;

const LC_SEGMENT_64: u32 = 0x19;
const LC_SYMTAB: u32 = 0x2;
const LC_DYSYMTAB: u32 = 0xb;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_DYLINKER: u32 = 0xe;
const LC_UUID: u32 = 0x1b;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const LC_BUILD_VERSION: u32 = 0x32;
const LC_REQ_DYLD: u32 = 0x8000_0000;
const LC_MAIN: u32 = 0x28 | LC_REQ_DYLD;
const LC_DYLD_CHAINED_FIXUPS: u32 = 0x34 | LC_REQ_DYLD;

const VM_PROT_READ: u32 = 1;
const VM_PROT_WRITE: u32 = 2;
const VM_PROT_EXECUTE: u32 = 4;
/// The segment is made read-only once `dyld` has applied its fixups.
const SG_READ_ONLY: u32 = 0x10;
/// `S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS`.
const S_CODE: u32 = 0x8000_0400;
const S_ZEROFILL: u32 = 0x1;
const PLATFORM_MACOS: u32 = 1;

/// `__PAGEZERO`'s size, and so where `__TEXT` is meant to load.
const PAGE_ZERO: u64 = 0x1_0000_0000;

const DYLINKER: &str = "/usr/lib/dyld";
const LIBSYSTEM: &str = "/usr/lib/libSystem.B.dylib";

/// `DYLD_CHAINED_PTR_64`: 8-byte slots, chained in 4-byte strides.
const DYLD_CHAINED_PTR_64: u16 = 2;
/// `DYLD_CHAINED_IMPORT`: 4-byte imports.
const DYLD_CHAINED_IMPORT: u32 = 1;
/// A page of a segment with no fixups in it.
const DYLD_CHAINED_PTR_START_NONE: u16 = 0xffff;

/// The code signature's magic numbers, slots and constants.
const CSMAGIC_EMBEDDED_SIGNATURE: u32 = 0xfade_0cc0;
const CSMAGIC_CODEDIRECTORY: u32 = 0xfade_0c02;
const CSMAGIC_REQUIREMENTS: u32 = 0xfade_0c01;
const CSMAGIC_BLOBWRAPPER: u32 = 0xfade_0b01;
const CSSLOT_CODEDIRECTORY: u32 = 0;
const CSSLOT_REQUIREMENTS: u32 = 2;
const CSSLOT_SIGNATURESLOT: u32 = 0x1_0000;
/// The `CodeDirectory` version with the executable segment's fields.
const CS_SUPPORTSEXECSEG: u32 = 0x2_0400;
const CS_ADHOC: u32 = 0x2;
const CS_HASHTYPE_SHA256: u8 = 2;
const CS_EXECSEG_MAIN_BINARY: u64 = 1;
/// The signature hashes the file in pages of 4 KiB (`2^12`).
const CS_PAGE_BITS: u8 = 12;
const CS_PAGE: u64 = 1 << CS_PAGE_BITS;
/// The size of a `CodeDirectory` of version [`CS_SUPPORTSEXECSEG`],
/// before its identifier.
const CODE_DIRECTORY: u64 = 88;
/// The signature's identifier: the same for every executable, so it
/// depends on nothing but the file.
const IDENTIFIER: &str = "oska-program";
/// The special slots hashed before the code's: the requirements (-2) and
/// the `Info.plist` (-1, none).
const SPECIAL_SLOTS: u64 = 2;

/// A segment's place in the file and in memory, from the start of
/// `__TEXT`.
#[derive(Debug, Clone, Copy)]
struct Segment {
    name: &'static str,
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    prot: u32,
    flags: u32,
}

/// A section, in a segment.
struct Section {
    name: &'static str,
    segment: &'static str,
    addr: u64,
    size: u64,
    offset: u32,
    align: u32,
    flags: u32,
}

/// The executable for `machine` holding `text`, `rodata`, `data_size`
/// bytes of zeroed writable data, and an import table of `imports`, the
/// platform's library functions by symbol.
pub fn write(
    machine: &Machine,
    text: &dyn Code,
    rodata: &[u8],
    data_size: u32,
    imports: &[&str],
) -> Vec<u8> {
    let page = machine.page;
    let commands = commands_size(imports.is_empty());
    let header = 32 + commands;
    // The code, after the headers.
    let code_at = header.next_multiple_of(16);
    let text_segment = Segment {
        name: "__TEXT",
        vmaddr: 0,
        vmsize: (code_at + text.size()).next_multiple_of(page),
        fileoff: 0,
        filesize: (code_at + text.size()).next_multiple_of(page),
        prot: VM_PROT_READ | VM_PROT_EXECUTE,
        flags: 0,
    };
    // The import table, then the read-only data.
    let got_size = 8 * imports.len() as u64;
    let rodata_at = got_size.next_multiple_of(16);
    let const_size = (rodata_at + rodata.len() as u64)
        .next_multiple_of(page)
        .max(page);
    let data_const = Segment {
        name: "__DATA_CONST",
        vmaddr: text_segment.vmaddr + text_segment.vmsize,
        vmsize: const_size,
        fileoff: text_segment.fileoff + text_segment.filesize,
        filesize: const_size,
        prot: VM_PROT_READ | VM_PROT_WRITE,
        flags: SG_READ_ONLY,
    };
    let linkedit_at = data_const.fileoff + data_const.filesize;
    let data = Segment {
        name: "__DATA",
        vmaddr: data_const.vmaddr + data_const.vmsize,
        vmsize: u64::from(data_size).next_multiple_of(page).max(page),
        fileoff: linkedit_at,
        filesize: 0,
        prot: VM_PROT_READ | VM_PROT_WRITE,
        flags: 0,
    };
    let layout = Layout {
        rodata: data_const.vmaddr + rodata_at,
        code: code_at,
        data: data.vmaddr,
        got: data_const.vmaddr,
    };
    let linked = text.link(&layout);

    // `__LINKEDIT`: the fixups, the string table, then the signature.
    let segments_before = [&text_segment, &data_const, &data];
    let fixups = chained_fixups(imports, &data_const, page, segments_before.len() + 2);
    let fixups_at = linkedit_at;
    let strings_at = (fixups_at + fixups.len() as u64).next_multiple_of(8);
    let strings_size = 8;
    let signature_at = (strings_at + strings_size).next_multiple_of(16);
    let signature_size = signature_size(signature_at);
    let linkedit = Segment {
        name: "__LINKEDIT",
        vmaddr: data.vmaddr + data.vmsize,
        vmsize: (signature_at + signature_size - linkedit_at).next_multiple_of(page),
        fileoff: linkedit_at,
        filesize: signature_at + signature_size - linkedit_at,
        prot: VM_PROT_READ,
        flags: 0,
    };
    let page_zero = Segment {
        name: "__PAGEZERO",
        vmaddr: 0,
        vmsize: PAGE_ZERO,
        fileoff: 0,
        filesize: 0,
        prot: 0,
        flags: 0,
    };

    let mut file = Vec::new();
    put32(&mut file, MH_MAGIC_64);
    put32(&mut file, machine.cputype);
    put32(&mut file, machine.cpusubtype);
    put32(&mut file, MH_EXECUTE);
    put32(&mut file, command_count(imports.is_empty()));
    put32(&mut file, commands as u32);
    put32(&mut file, MH_FLAGS);
    put32(&mut file, 0);

    // Addresses as the file says them: from `__PAGEZERO`'s bottom.
    let at = |segment: &Segment| Segment {
        vmaddr: if segment.name == "__PAGEZERO" {
            0
        } else {
            PAGE_ZERO + segment.vmaddr
        },
        ..*segment
    };
    segment_command(&mut file, &at(&page_zero), &[]);
    segment_command(
        &mut file,
        &at(&text_segment),
        &[Section {
            name: "__text",
            segment: "__TEXT",
            addr: PAGE_ZERO + code_at,
            size: text.size(),
            offset: code_at as u32,
            align: 4,
            flags: S_CODE,
        }],
    );
    segment_command(
        &mut file,
        &at(&data_const),
        &[
            Section {
                name: "__got",
                segment: "__DATA_CONST",
                addr: PAGE_ZERO + data_const.vmaddr,
                size: got_size,
                offset: data_const.fileoff as u32,
                align: 3,
                flags: 0,
            },
            Section {
                name: "__const",
                segment: "__DATA_CONST",
                addr: PAGE_ZERO + layout.rodata,
                size: rodata.len() as u64,
                offset: (data_const.fileoff + rodata_at) as u32,
                align: 4,
                flags: 0,
            },
        ],
    );
    segment_command(
        &mut file,
        &at(&data),
        &[Section {
            name: "__bss",
            segment: "__DATA",
            addr: PAGE_ZERO + data.vmaddr,
            size: u64::from(data_size),
            offset: 0,
            align: 4,
            flags: S_ZEROFILL,
        }],
    );
    segment_command(&mut file, &at(&linkedit), &[]);
    // LC_DYLD_CHAINED_FIXUPS.
    put32(&mut file, LC_DYLD_CHAINED_FIXUPS);
    put32(&mut file, 16);
    put32(&mut file, fixups_at as u32);
    put32(&mut file, fixups.len() as u32);
    // LC_SYMTAB: no symbols, and an empty string table.
    put32(&mut file, LC_SYMTAB);
    put32(&mut file, 24);
    put32(&mut file, strings_at as u32);
    put32(&mut file, 0);
    put32(&mut file, strings_at as u32);
    put32(&mut file, strings_size as u32);
    // LC_DYSYMTAB: empty.
    put32(&mut file, LC_DYSYMTAB);
    put32(&mut file, 80);
    file.resize(file.len() + 72, 0);
    // LC_LOAD_DYLINKER.
    put32(&mut file, LC_LOAD_DYLINKER);
    put32(&mut file, path_command_size(12, DYLINKER));
    put32(&mut file, 12);
    put_path(&mut file, 12, DYLINKER);
    // LC_UUID: filled in below.
    put32(&mut file, LC_UUID);
    put32(&mut file, 24);
    let uuid_at = file.len();
    file.resize(file.len() + 16, 0);
    // LC_BUILD_VERSION: macOS, the oldest release it runs on, and the
    // SDK it was built for, the same; no tools.
    put32(&mut file, LC_BUILD_VERSION);
    put32(&mut file, 24);
    put32(&mut file, PLATFORM_MACOS);
    put32(&mut file, machine.minos);
    put32(&mut file, machine.minos);
    put32(&mut file, 0);
    // LC_MAIN: the entry point, as an offset in the file.
    put32(&mut file, LC_MAIN);
    put32(&mut file, 24);
    put64(&mut file, code_at + text.entry());
    put64(&mut file, 0);
    // LC_LOAD_DYLIB: libSystem, library ordinal 1. Its timestamp, and
    // the versions it asks for: any `libSystem` since 1.0.
    if !imports.is_empty() {
        put32(&mut file, LC_LOAD_DYLIB);
        put32(&mut file, path_command_size(24, LIBSYSTEM));
        put32(&mut file, 24);
        put32(&mut file, 2);
        put32(&mut file, 0x1_0000);
        put32(&mut file, 0x1_0000);
        put_path(&mut file, 24, LIBSYSTEM);
    }
    // LC_CODE_SIGNATURE.
    put32(&mut file, LC_CODE_SIGNATURE);
    put32(&mut file, 16);
    put32(&mut file, signature_at as u32);
    put32(&mut file, signature_size as u32);
    debug_assert_eq!(file.len() as u64, header);

    // The segments' contents.
    pad_to(&mut file, code_at);
    file.extend_from_slice(&linked);
    pad_to(&mut file, data_const.fileoff);
    for (index, _) in imports.iter().enumerate() {
        put64(&mut file, bind(index, index + 1 == imports.len()));
    }
    pad_to(&mut file, data_const.fileoff + rodata_at);
    file.extend_from_slice(rodata);
    pad_to(&mut file, fixups_at);
    file.extend_from_slice(&fixups);
    pad_to(&mut file, strings_at + strings_size);
    pad_to(&mut file, signature_at);

    // The UUID: a hash of every other byte, as a version 4 (random) UUID.
    let hash = sha256::digest(&file);
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&hash[..16]);
    uuid[6] = (uuid[6] & 0x0f) | 0x40;
    uuid[8] = (uuid[8] & 0x3f) | 0x80;
    file[uuid_at..uuid_at + 16].copy_from_slice(&uuid);

    let signature = signature(&file, text_segment.filesize);
    debug_assert_eq!(signature.len() as u64, signature_size);
    file.extend_from_slice(&signature);
    file
}

/// How many load commands the file has: five segments, the fixups, the
/// symbol tables, the loader, the UUID, the build version, the entry
/// point, `libSystem` (when anything is imported) and the signature.
fn command_count(no_imports: bool) -> u32 {
    if no_imports {
        13
    } else {
        14
    }
}

/// The size in bytes of the load commands.
fn commands_size(no_imports: bool) -> u64 {
    let segments = 5 * 72 + 80 * (1 + 2 + 1);
    let dylib = if no_imports {
        0
    } else {
        u64::from(path_command_size(24, LIBSYSTEM))
    };
    segments + 16 + 24 + 80 + u64::from(path_command_size(12, DYLINKER)) + 24 + 24 + 24 + dylib + 16
}

/// A load command of `fixed` bytes followed by `path` and its NUL,
/// padded to a multiple of 8.
fn path_command_size(fixed: u32, path: &str) -> u32 {
    (fixed + path.len() as u32 + 1).next_multiple_of(8)
}

fn put_path(file: &mut Vec<u8>, fixed: u32, path: &str) {
    let start = file.len();
    file.extend_from_slice(path.as_bytes());
    file.resize(start + (path_command_size(fixed, path) - fixed) as usize, 0);
}

fn segment_command(file: &mut Vec<u8>, segment: &Segment, sections: &[Section]) {
    put32(file, LC_SEGMENT_64);
    put32(file, 72 + 80 * sections.len() as u32);
    put_name(file, segment.name);
    put64(file, segment.vmaddr);
    put64(file, segment.vmsize);
    put64(file, segment.fileoff);
    put64(file, segment.filesize);
    put32(file, segment.prot);
    put32(file, segment.prot);
    put32(file, sections.len() as u32);
    put32(file, segment.flags);
    for section in sections {
        put_name(file, section.name);
        put_name(file, section.segment);
        put64(file, section.addr);
        put64(file, section.size);
        put32(file, section.offset);
        put32(file, section.align);
        // No relocations: `reloff`, `nreloc`.
        put32(file, 0);
        put32(file, 0);
        put32(file, section.flags);
        file.resize(file.len() + 12, 0);
    }
}

/// `name` in a 16-byte field, NUL-padded.
fn put_name(file: &mut Vec<u8>, name: &str) {
    let mut field = [0u8; 16];
    field[..name.len()].copy_from_slice(name.as_bytes());
    file.extend_from_slice(&field);
}

/// The import slot `index`'s bytes in the file: a
/// `dyld_chained_ptr_64_bind` of import `index`, chained to the next slot
/// (8 bytes, two strides on) unless it is the `last`.
fn bind(index: usize, last: bool) -> u64 {
    let next: u64 = if last { 0 } else { 2 };
    (1 << 63) | (next << 51) | index as u64
}

/// The `LC_DYLD_CHAINED_FIXUPS` blob: its header, where each of the
/// file's `segments` has its chain (only `__DATA_CONST`, the third, has
/// one: the import table, from its first byte), the imports, all from
/// library 1, and their names.
fn chained_fixups(imports: &[&str], data_const: &Segment, page: u64, segments: usize) -> Vec<u8> {
    const HEADER: u32 = 28;
    let starts_at = HEADER.next_multiple_of(8);
    // `dyld_chained_starts_in_image`: the count, then an offset per
    // segment, from its own start.
    let image = 4 + 4 * segments as u32;
    let in_segment_at = image.next_multiple_of(8);
    let pages = (data_const.vmsize / page) as u16;
    // `dyld_chained_starts_in_segment`, up to `page_start`, and one
    // `page_start` per page.
    let in_segment = 22 + 2 * u32::from(pages);
    let imports_at = (starts_at + in_segment_at + in_segment).next_multiple_of(4);
    let symbols_at = imports_at + 4 * imports.len() as u32;

    let mut blob = Vec::new();
    put32(&mut blob, 0);
    put32(&mut blob, starts_at);
    put32(&mut blob, imports_at);
    put32(&mut blob, symbols_at);
    put32(&mut blob, imports.len() as u32);
    put32(&mut blob, DYLD_CHAINED_IMPORT);
    put32(&mut blob, 0);
    blob.resize(starts_at as usize, 0);
    put32(&mut blob, segments as u32);
    for segment in 0..segments {
        let has_chain = segment == 2 && !imports.is_empty();
        put32(&mut blob, if has_chain { in_segment_at } else { 0 });
    }
    blob.resize((starts_at + in_segment_at) as usize, 0);
    put32(&mut blob, in_segment);
    put16(&mut blob, page as u16);
    put16(&mut blob, DYLD_CHAINED_PTR_64);
    put64(&mut blob, data_const.vmaddr);
    put32(&mut blob, 0);
    put16(&mut blob, pages);
    for index in 0..pages {
        put16(
            &mut blob,
            if index == 0 {
                0
            } else {
                DYLD_CHAINED_PTR_START_NONE
            },
        );
    }
    blob.resize(imports_at as usize, 0);
    let mut name_at = 0u32;
    for symbol in imports {
        put32(&mut blob, 1 | name_at << 9);
        name_at += symbol.len() as u32 + 1;
    }
    for symbol in imports {
        blob.extend_from_slice(symbol.as_bytes());
        blob.push(0);
    }
    blob.resize(blob.len().next_multiple_of(8), 0);
    blob
}

/// How many 4 KiB pages the signature hashes in a file whose signature
/// starts at `code_limit`.
fn code_slots(code_limit: u64) -> u64 {
    code_limit.div_ceil(CS_PAGE)
}

/// The size of the signature [`signature`] writes for a file whose
/// signature starts at `code_limit`, a multiple of 16.
fn signature_size(code_limit: u64) -> u64 {
    let super_blob = 12 + 8 * 3;
    let identifier = IDENTIFIER.len() as u64 + 1;
    let code_directory =
        CODE_DIRECTORY + identifier + 32 * (SPECIAL_SLOTS + code_slots(code_limit));
    let requirements = 12;
    let wrapper = 8;
    (super_blob + code_directory + requirements + wrapper).next_multiple_of(16)
}

/// The ad-hoc code signature of `file`, every byte before it: a
/// `CodeDirectory` of SHA-256 hashes of its 4 KiB pages, with no
/// certificate, as `codesign -s -` writes one: an empty requirement set
/// and an empty signature (CMS) blob. `exec_limit` is the end of
/// `__TEXT`, the executable segment. Multi-byte fields are big-endian.
fn signature(file: &[u8], exec_limit: u64) -> Vec<u8> {
    let code_limit = file.len() as u64;
    let slots = code_slots(code_limit);
    let mut requirements = Vec::new();
    put32be(&mut requirements, CSMAGIC_REQUIREMENTS);
    put32be(&mut requirements, 12);
    put32be(&mut requirements, 0);

    let identifier_at = CODE_DIRECTORY;
    let hashes_at = identifier_at + IDENTIFIER.len() as u64 + 1 + 32 * SPECIAL_SLOTS;
    let directory_size = hashes_at + 32 * slots;
    let mut directory = Vec::new();
    put32be(&mut directory, CSMAGIC_CODEDIRECTORY);
    put32be(&mut directory, directory_size as u32);
    put32be(&mut directory, CS_SUPPORTSEXECSEG);
    put32be(&mut directory, CS_ADHOC);
    put32be(&mut directory, hashes_at as u32);
    put32be(&mut directory, identifier_at as u32);
    put32be(&mut directory, SPECIAL_SLOTS as u32);
    put32be(&mut directory, slots as u32);
    put32be(&mut directory, code_limit as u32);
    // hashSize, hashType, platform, pageSize.
    directory.extend_from_slice(&[32, CS_HASHTYPE_SHA256, 0, CS_PAGE_BITS]);
    // spare2, scatterOffset, teamOffset, spare3.
    put32be(&mut directory, 0);
    put32be(&mut directory, 0);
    put32be(&mut directory, 0);
    put32be(&mut directory, 0);
    // codeLimit64 (unused below 4 GiB), and the executable segment.
    put64be(&mut directory, 0);
    put64be(&mut directory, 0);
    put64be(&mut directory, exec_limit);
    put64be(&mut directory, CS_EXECSEG_MAIN_BINARY);
    debug_assert_eq!(directory.len() as u64, CODE_DIRECTORY);
    directory.extend_from_slice(IDENTIFIER.as_bytes());
    directory.push(0);
    // Slot -2, the requirements; slot -1, the `Info.plist`: none.
    directory.extend_from_slice(&sha256::digest(&requirements));
    directory.extend_from_slice(&[0; 32]);
    for page in file.chunks(CS_PAGE as usize) {
        directory.extend_from_slice(&sha256::digest(page));
    }

    let mut wrapper = Vec::new();
    put32be(&mut wrapper, CSMAGIC_BLOBWRAPPER);
    put32be(&mut wrapper, 8);

    let blobs = [
        (CSSLOT_CODEDIRECTORY, directory),
        (CSSLOT_REQUIREMENTS, requirements),
        (CSSLOT_SIGNATURESLOT, wrapper),
    ];
    let mut at = 12 + 8 * blobs.len() as u32;
    let length: usize = at as usize + blobs.iter().map(|(_, blob)| blob.len()).sum::<usize>();
    let mut signature = Vec::new();
    put32be(&mut signature, CSMAGIC_EMBEDDED_SIGNATURE);
    put32be(&mut signature, length as u32);
    put32be(&mut signature, blobs.len() as u32);
    for (slot, blob) in &blobs {
        put32be(&mut signature, *slot);
        put32be(&mut signature, at);
        at += blob.len() as u32;
    }
    for (_, blob) in &blobs {
        signature.extend_from_slice(blob);
    }
    signature.resize(signature.len().next_multiple_of(16), 0);
    signature
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

fn put32be(file: &mut Vec<u8>, value: u32) {
    file.extend_from_slice(&value.to_be_bytes());
}

fn put64be(file: &mut Vec<u8>, value: u64) {
    file.extend_from_slice(&value.to_be_bytes());
}
