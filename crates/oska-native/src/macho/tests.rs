//! The Mach-O a `macos-aarch64` build writes, read back as Apple's
//! formats document it, on any host (spec #154: "Mach-O output checked
//! byte by byte on the Pi"). Every number here is from the documents,
//! not from `macho.rs`: `mach-o/loader.h` (xnu), `mach-o/fixup-chains.h`
//! (dyld) and `kern/cs_blobs.h` (xnu). The macOS runner runs the files
//! and has macOS verify their signatures (`.github/workflows/ci.yml`).

use crate::{build, Target};

const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n";

/// `src`'s executable for `macos-aarch64`.
fn executable(src: &str) -> Vec<u8> {
    let report = oska_check::check_module(src);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let program = report.program.expect("the program should check");
    build(&program, Target::MacosAarch64, &|d| d).expect("the program should build")
}

fn le(bytes: &[u8], at: usize, size: usize) -> u64 {
    bytes[at..at + size]
        .iter()
        .rev()
        .fold(0, |acc, &b| acc << 8 | u64::from(b))
}

fn be(bytes: &[u8], at: usize, size: usize) -> u64 {
    bytes[at..at + size]
        .iter()
        .fold(0, |acc, &b| acc << 8 | u64::from(b))
}

/// A NUL-terminated or NUL-padded name at `at`.
fn name(bytes: &[u8], at: usize, max: usize) -> String {
    let field = &bytes[at..(at + max).min(bytes.len())];
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// A load command: its `cmd` and where it starts.
struct Command {
    cmd: u64,
    at: usize,
    size: usize,
}

/// A `segment_command_64`.
#[derive(Debug)]
struct Segment {
    name: String,
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: u64,
    initprot: u64,
    flags: u64,
    sections: Vec<Section>,
}

/// A `section_64`.
#[derive(Debug)]
struct Section {
    name: String,
    segment: String,
    addr: u64,
    size: u64,
    offset: u64,
    flags: u64,
}

fn commands(file: &[u8]) -> Vec<Command> {
    let count = le(file, 16, 4) as usize;
    let mut at = 32;
    let mut out = Vec::new();
    for _ in 0..count {
        let size = le(file, at + 4, 4) as usize;
        out.push(Command {
            cmd: le(file, at, 4),
            at,
            size,
        });
        at += size;
    }
    assert_eq!(at, 32 + le(file, 20, 4) as usize, "sizeofcmds");
    out
}

const LC_SEGMENT_64: u64 = 0x19;
const LC_LOAD_DYLIB: u64 = 0xc;
const LC_LOAD_DYLINKER: u64 = 0xe;
const LC_UUID: u64 = 0x1b;
const LC_CODE_SIGNATURE: u64 = 0x1d;
const LC_BUILD_VERSION: u64 = 0x32;
const LC_MAIN: u64 = 0x28 | 0x8000_0000;
const LC_DYLD_CHAINED_FIXUPS: u64 = 0x34 | 0x8000_0000;

fn segments(file: &[u8]) -> Vec<Segment> {
    commands(file)
        .iter()
        .filter(|c| c.cmd == LC_SEGMENT_64)
        .map(|c| {
            let at = c.at;
            let count = le(file, at + 64, 4) as usize;
            assert_eq!(c.size, 72 + 80 * count);
            Segment {
                name: name(file, at + 8, 16),
                vmaddr: le(file, at + 24, 8),
                vmsize: le(file, at + 32, 8),
                fileoff: le(file, at + 40, 8),
                filesize: le(file, at + 48, 8),
                maxprot: le(file, at + 56, 4),
                initprot: le(file, at + 60, 4),
                flags: le(file, at + 68, 4),
                sections: (0..count)
                    .map(|i| {
                        let s = at + 72 + 80 * i;
                        Section {
                            name: name(file, s, 16),
                            segment: name(file, s + 16, 16),
                            addr: le(file, s + 32, 8),
                            size: le(file, s + 40, 8),
                            offset: le(file, s + 48, 4),
                            flags: le(file, s + 64, 4),
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}

fn one(file: &[u8], cmd: u64) -> Command {
    let mut found = commands(file).into_iter().filter(|c| c.cmd == cmd);
    let command = found.next().expect("the load command");
    assert!(found.next().is_none(), "one load command {cmd:#x}");
    command
}

fn segment<'a>(segments: &'a [Segment], name: &str) -> &'a Segment {
    segments
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn the_header_is_a_64_bit_arm64_executable_loaded_by_dyld() {
    let file = executable(HELLO);
    // MH_MAGIC_64, CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64_ALL, MH_EXECUTE.
    assert_eq!(le(&file, 0, 4), 0xfeed_facf);
    assert_eq!(le(&file, 4, 4), 0x0100_000c);
    assert_eq!(le(&file, 8, 4), 0);
    assert_eq!(le(&file, 12, 4), 2);
    // MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE, as ld64 writes.
    assert_eq!(le(&file, 24, 4), 0x0020_0085);
    let dyld = one(&file, LC_LOAD_DYLINKER);
    assert_eq!(
        name(&file, dyld.at + le(&file, dyld.at + 8, 4) as usize, 32),
        "/usr/lib/dyld"
    );
    // LC_BUILD_VERSION: PLATFORM_MACOS, from macOS 12, which loads
    // chained fixups.
    let build = one(&file, LC_BUILD_VERSION);
    assert_eq!(le(&file, build.at + 8, 4), 1);
    assert!(le(&file, build.at + 12, 4) >= 0x000c_0000);
    // Every load command is a multiple of 8 bytes.
    assert!(commands(&file).iter().all(|c| c.size % 8 == 0));
}

#[test]
fn segments_are_page_aligned_and_no_page_is_both_writable_and_executable() {
    let file = executable(HELLO);
    let segments = segments(&file);
    let names: Vec<&str> = segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "__PAGEZERO",
            "__TEXT",
            "__DATA_CONST",
            "__DATA",
            "__LINKEDIT"
        ]
    );
    // The first 4 GiB, no access: null and truncated pointers fault.
    let zero = segment(&segments, "__PAGEZERO");
    assert_eq!((zero.vmaddr, zero.vmsize, zero.filesize), (0, 1 << 32, 0));
    assert_eq!((zero.maxprot, zero.initprot), (0, 0));
    const R: u64 = 1;
    const W: u64 = 2;
    const X: u64 = 4;
    let expected = [
        ("__TEXT", R | X, 0),
        // SG_READ_ONLY: read-only once the loader has bound its slots.
        ("__DATA_CONST", R | W, 0x10),
        ("__DATA", R | W, 0),
        ("__LINKEDIT", R, 0),
    ];
    let mut end = 1 << 32;
    for (name, prot, flags) in expected {
        let s = segment(&segments, name);
        assert_eq!(
            (s.maxprot, s.initprot, s.flags),
            (prot, prot, flags),
            "{name}"
        );
        // On 16 KiB pages, in the file and in memory, one after another,
        // mapped at their file offsets above 4 GiB.
        assert_eq!(s.vmaddr % 0x4000, 0, "{name}");
        assert_eq!(s.fileoff % 0x4000, 0, "{name}");
        assert_eq!(s.vmsize % 0x4000, 0, "{name}");
        assert_eq!(s.vmaddr, end, "{name}");
        assert_eq!(s.vmaddr - (1 << 32), s.fileoff, "{name}");
        assert!(s.filesize <= s.vmsize, "{name}");
        end = s.vmaddr + s.vmsize;
        for section in &s.sections {
            assert_eq!(section.segment, s.name);
            assert!(section.addr >= s.vmaddr && section.addr + section.size <= s.vmaddr + s.vmsize);
            assert_eq!(section.addr - (1 << 32), section.offset, "{}", section.name);
        }
    }
    // `__TEXT` starts the file, with the headers; `__LINKEDIT` ends it.
    assert_eq!(segment(&segments, "__TEXT").fileoff, 0);
    let linkedit = segment(&segments, "__LINKEDIT");
    assert_eq!(linkedit.fileoff + linkedit.filesize, file.len() as u64);
    let text = &segment(&segments, "__TEXT").sections;
    assert_eq!(text.len(), 1);
    // S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS.
    assert_eq!(
        (text[0].name.as_str(), text[0].flags),
        ("__text", 0x8000_0400)
    );
}

#[test]
fn the_entry_point_is_in_the_code() {
    let file = executable(HELLO);
    let main = one(&file, LC_MAIN);
    let entry = le(&file, main.at + 8, 8);
    let segments = segments(&file);
    let text = &segment(&segments, "__TEXT").sections[0];
    assert!(text.offset <= entry && entry < text.offset + text.size);
    assert_eq!(entry % 4, 0);
}

/// The import slots and the chained fixups that bind them, walked as
/// dyld walks them: every slot binds a libSystem function, and nothing
/// else is fixed up.
#[test]
fn every_import_is_bound_from_lib_system_alone_through_one_chain() {
    let file = executable(HELLO);
    let dylibs: Vec<Command> = commands(&file)
        .into_iter()
        .filter(|c| c.cmd == LC_LOAD_DYLIB)
        .collect();
    assert_eq!(dylibs.len(), 1, "exactly one library");
    let lib = &dylibs[0];
    assert_eq!(
        name(&file, lib.at + le(&file, lib.at + 8, 4) as usize, 40),
        "/usr/lib/libSystem.B.dylib"
    );

    let command = one(&file, LC_DYLD_CHAINED_FIXUPS);
    let blob_at = le(&file, command.at + 8, 4) as usize;
    let blob = &file[blob_at..blob_at + le(&file, command.at + 12, 4) as usize];
    // dyld_chained_fixups_header: version 0, DYLD_CHAINED_IMPORT,
    // uncompressed names.
    assert_eq!(le(blob, 0, 4), 0);
    let starts = le(blob, 4, 4) as usize;
    let imports_at = le(blob, 8, 4) as usize;
    let symbols_at = le(blob, 12, 4) as usize;
    let count = le(blob, 16, 4) as usize;
    assert_eq!((le(blob, 20, 4), le(blob, 24, 4)), (1, 0));
    let imports: Vec<(u64, String)> = (0..count)
        .map(|i| {
            let import = le(blob, imports_at + 4 * i, 4);
            let name_at = (import >> 9) as usize;
            (import & 0xff, name(blob, symbols_at + name_at, 64))
        })
        .collect();
    assert!(
        imports.iter().all(|(ordinal, _)| *ordinal == 1),
        "{imports:?}"
    );
    let names: Vec<&str> = imports.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, crate::intrinsics::MACOS_AARCH64.imports);

    // dyld_chained_starts_in_image: one entry per segment.
    let segments = segments(&file);
    assert_eq!(le(blob, starts, 4) as usize, segments.len());
    let mut bound = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let info = le(blob, starts + 4 + 4 * index, 4) as usize;
        if info == 0 {
            continue;
        }
        assert_eq!(segment.name, "__DATA_CONST");
        let s = starts + info;
        let page_size = le(blob, s + 4, 2);
        // DYLD_CHAINED_PTR_64 or DYLD_CHAINED_PTR_64_OFFSET: their binds
        // are the same.
        assert!(matches!(le(blob, s + 6, 2), 2 | 6));
        assert_eq!(le(blob, s + 8, 8), segment.vmaddr - (1 << 32));
        assert_eq!(page_size, 0x4000);
        let pages = le(blob, s + 20, 2) as usize;
        assert_eq!(pages as u64, segment.vmsize / page_size);
        for page in 0..pages {
            let start = le(blob, s + 22 + 2 * page, 2);
            if start == 0xffff {
                continue;
            }
            let mut at = segment.fileoff as usize + page * page_size as usize + start as usize;
            loop {
                let word = le(&file, at, 8);
                // dyld_chained_ptr_64_bind: bind 1, ordinal, addend 0.
                assert_eq!(word >> 63, 1, "a bind at {at:#x}");
                assert_eq!((word >> 24) & 0xff, 0, "no addend");
                bound.push((at, word & 0xff_ffff));
                let next = (word >> 51) & 0xfff;
                if next == 0 {
                    break;
                }
                at += 4 * next as usize;
            }
        }
    }
    // Each slot of `__got`, in order, binds the import of its index.
    let got = segment(&segments, "__DATA_CONST")
        .sections
        .iter()
        .find(|s| s.name == "__got")
        .expect("a __got");
    assert_eq!(got.size, 8 * count as u64);
    let expected: Vec<(usize, u64)> = (0..count)
        .map(|i| (got.offset as usize + 8 * i, i as u64))
        .collect();
    assert_eq!(bound, expected);
}

/// The ad-hoc signature, checked as the kernel checks it: each 4 KiB
/// page of the file before it hashes to its slot.
#[test]
fn the_ad_hoc_signature_hashes_every_page_of_the_file() {
    let file = executable(HELLO);
    let command = one(&file, LC_CODE_SIGNATURE);
    let at = le(&file, command.at + 8, 4) as usize;
    let size = le(&file, command.at + 12, 4) as usize;
    assert_eq!(at % 16, 0);
    assert_eq!(at + size, file.len(), "the signature ends the file");
    let sig = &file[at..];
    // CSMAGIC_EMBEDDED_SIGNATURE, big-endian like every field.
    assert_eq!(be(sig, 0, 4), 0xfade_0cc0);
    assert_eq!(be(sig, 4, 4) as usize, size);
    let blobs: Vec<(u64, usize)> = (0..be(sig, 8, 4) as usize)
        .map(|i| (be(sig, 12 + 8 * i, 4), be(sig, 16 + 8 * i, 4) as usize))
        .collect();
    let slot = |kind: u64| blobs.iter().find(|b| b.0 == kind).map(|b| b.1);
    // CSSLOT_REQUIREMENTS: an empty set (CSMAGIC_REQUIREMENTS, 12 bytes,
    // no requirement).
    let requirements_at = slot(2).expect("requirements");
    let requirements = &sig[requirements_at..requirements_at + 12];
    assert_eq!(be(requirements, 0, 4), 0xfade_0c01);
    assert_eq!((be(requirements, 4, 4), be(requirements, 8, 4)), (12, 0));
    // CSSLOT_SIGNATURESLOT: an empty CMS wrapper, no certificate.
    let cms = slot(0x1_0000).expect("a signature slot");
    assert_eq!((be(sig, cms, 4), be(sig, cms + 4, 4)), (0xfade_0b01, 8));

    let cd_at = slot(0).expect("a code directory");
    let cd = &sig[cd_at..];
    assert_eq!(be(cd, 0, 4), 0xfade_0c02);
    // Version 0x20400, with the executable segment; CS_ADHOC.
    assert!(be(cd, 8, 4) >= 0x2_0400);
    assert_eq!(be(cd, 12, 4), 0x2);
    let hash_offset = be(cd, 16, 4) as usize;
    let ident_offset = be(cd, 20, 4) as usize;
    let special = be(cd, 24, 4) as usize;
    let code_slots = be(cd, 28, 4) as usize;
    let code_limit = be(cd, 32, 4) as usize;
    // SHA-256 hashes of 4 KiB pages.
    assert_eq!((cd[36], cd[37], cd[39]), (32, 2, 12));
    assert_eq!(code_limit, at, "everything before the signature is signed");
    assert_eq!(code_slots, code_limit.div_ceil(4096));
    for (page, bytes) in file[..code_limit].chunks(4096).enumerate() {
        let hash = &cd[hash_offset + 32 * page..hash_offset + 32 * (page + 1)];
        assert_eq!(hash, crate::sha256::digest(bytes), "page {page}");
    }
    // Slot -2 is the requirements' hash; -1, the absent Info.plist.
    assert!(special >= 2);
    assert_eq!(
        &cd[hash_offset - 64..hash_offset - 32],
        crate::sha256::digest(requirements)
    );
    assert_eq!(&cd[hash_offset - 32..hash_offset], [0; 32]);
    assert!(name(cd, ident_offset, 64).starts_with("oska-"));
    // The executable segment, `__TEXT`, of a main executable.
    let segments = segments(&file);
    let text = segment(&segments, "__TEXT");
    assert_eq!(be(cd, 64, 8), text.fileoff);
    assert_eq!(be(cd, 72, 8), text.filesize);
    assert_eq!(be(cd, 80, 8), 1);
}

/// The same program gives the same bytes, its UUID and signature
/// included; another program, another UUID.
#[test]
fn the_uuid_and_signature_are_the_program_s_alone() {
    let first = executable(HELLO);
    assert!(first == executable(HELLO));
    let uuid = |file: &[u8]| {
        let command = one(file, LC_UUID);
        assert_eq!(command.size, 24);
        file[command.at + 8..command.at + 24].to_vec()
    };
    assert_ne!(uuid(&first), [0; 16]);
    let other = executable(&HELLO.replace("hello", "other"));
    assert_ne!(uuid(&first), uuid(&other));
}

/// macOS supports calling the kernel only through libSystem (ADR 0011,
/// ADR 0015): the code has no `svc` of its own.
#[test]
fn the_code_makes_no_system_call_of_its_own() {
    let file = executable(HELLO);
    let segments = segments(&file);
    let text = &segment(&segments, "__TEXT").sections[0];
    let code = &file[text.offset as usize..(text.offset + text.size) as usize];
    // `svc #imm16`: 0xd4000001 with the immediate at bit 5.
    let svc = code
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .filter(|w| w & 0xffe0_001f == 0xd400_0001)
        .count();
    assert_eq!(svc, 0);
}
