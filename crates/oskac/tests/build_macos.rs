// SPDX-License-Identifier: Apache-2.0
//! `oskac build --target macos-aarch64` (spec #154, ticket #169) and
//! `--target macos-x86_64` (#173): macOS executables, a Mach-O that `dyld` loads, importing only
//! from `libSystem`, with an ad-hoc code signature `oskac` writes itself.
//!
//! Every host checks the file against Apple's formats (`mach-o/loader.h`,
//! dyld's `mach-o/fixup-chains.h`, the code signature's `CodeDirectory`),
//! byte by byte, so the writer is developed without a Mac. On a Mac the
//! tests also run what they build for it, compare it with the
//! interpreter, and ask macOS's own `codesign` to verify the signature.
//!
//! The cases with golden output compare the exit code, stdout, stderr and
//! the files the build left with `tests/golden/build_macos/<case>.txt`;
//! regenerate it after an intended change with
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test build_macos` and review
//! the diff.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n";

/// A new, empty directory for `case`, holding `files`.
fn workspace(case: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("build_macos")
        .join(case);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the case directory");
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    dir
}

/// Run `oskac args` in `dir`.
fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// A macOS target, and what its executables say about it.
struct Mac {
    platform: &'static str,
    /// The header's `cputype` and `cpusubtype`.
    cputype: u32,
    cpusubtype: u32,
    /// Its page: every segment starts on one.
    page: u64,
    /// The functions its executables import from `libSystem`, in the
    /// order of the import table (ADR 0015: the list is reviewed surface).
    imports: [&'static str; 37],
    /// Whether this host runs its executables.
    is_host: bool,
}

/// Both macOS targets.
const MACS: [Mac; 2] = [
    Mac {
        platform: "macos-aarch64",
        cputype: 0x0100_000c,
        cpusubtype: 0,
        page: 0x4000,
        imports: IMPORTS,
        is_host: cfg!(all(target_os = "macos", target_arch = "aarch64")),
    },
    Mac {
        platform: "macos-x86_64",
        cputype: 0x0100_0007,
        cpusubtype: 3,
        page: 0x1000,
        imports: IMPORTS_X86_64,
        is_host: cfg!(all(target_os = "macos", target_arch = "x86_64")),
    },
];

/// `hello.osk`'s executable for `mac`, built in a fresh directory for
/// `case`.
fn hello(mac: &Mac, case: &str) -> Vec<u8> {
    let dir = workspace(&format!("{}/{case}", mac.platform), &[("hello.osk", HELLO)]);
    let built = oskac(
        &dir,
        &[
            "build",
            "hello.osk",
            "--target",
            mac.platform,
            "-o",
            "hello",
        ],
    );
    assert!(built.status.success(), "{built:?}");
    std::fs::read(dir.join("hello")).expect("the executable should exist")
}

#[test]
fn each_macos_platform_names_a_macos_executable() {
    for mac in &MACS {
        names_a_macos_executable(mac);
    }
}

fn names_a_macos_executable(mac: &Mac) {
    let case = mac.platform.replace('-', "_");
    let dir = workspace(&case, &[("hello.osk", HELLO)]);
    let args = ["build", "hello.osk", "--target", mac.platform];
    let mut before = BTreeSet::new();
    files_in(&dir, &mut before);
    let output = oskac(&dir, &args);
    let mut after = BTreeSet::new();
    files_in(&dir, &mut after);
    let actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}--- written\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        after
            .difference(&before)
            .map(|w| format!("{w}\n"))
            .collect::<String>(),
    );
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/build_macos")
        .join(format!("{case}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a golden directory");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("{} should exist: {e}\n{actual}", golden.display()));
    assert!(
        actual == expected,
        "{} differs; rerun with UPDATE_GOLDEN=1 if the change is intended\n--- actual\n{actual}\n--- expected\n{expected}",
        golden.display()
    );
    let file = std::fs::read(dir.join("hello")).expect("the executable should exist");
    assert_eq!(u32_at(&file, 0), MH_MAGIC_64);
    assert_eq!(u32_at(&file, 4), mac.cputype);
}

/// Every file name in `dir`.
fn files_in(dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).expect("should list the directory") {
        let entry = entry.expect("a directory entry");
        out.insert(entry.file_name().to_string_lossy().into_owned());
    }
}

// ---- Reading the file ------------------------------------------------------------

const MH_MAGIC_64: u32 = 0xfeed_facf;
const MH_EXECUTE: u32 = 2;
const MH_NOUNDEFS: u32 = 0x1;
const MH_DYLDLINK: u32 = 0x4;
const MH_TWOLEVEL: u32 = 0x80;
const MH_PIE: u32 = 0x20_0000;

const LC_SEGMENT_64: u32 = 0x19;
const LC_SYMTAB: u32 = 0x2;
const LC_DYSYMTAB: u32 = 0xb;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_DYLINKER: u32 = 0xe;
const LC_UUID: u32 = 0x1b;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const LC_BUILD_VERSION: u32 = 0x32;
const LC_MAIN: u32 = 0x8000_0028;
const LC_DYLD_CHAINED_FIXUPS: u32 = 0x8000_0034;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

fn u32_be(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn u64_be(bytes: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

/// The NUL-terminated (or field-filling) text at `at`, at most `max`
/// bytes.
fn text_at(bytes: &[u8], at: usize, max: usize) -> String {
    let field = &bytes[at..(at + max).min(bytes.len())];
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// A load command: its type, and where it is in the file.
struct LoadCommand {
    cmd: u32,
    at: usize,
    size: usize,
}

fn commands(file: &[u8]) -> Vec<LoadCommand> {
    let count = u32_at(file, 16) as usize;
    let mut at = 32;
    let mut out = Vec::new();
    for _ in 0..count {
        let size = u32_at(file, at + 4) as usize;
        out.push(LoadCommand {
            cmd: u32_at(file, at),
            at,
            size,
        });
        at += size;
    }
    assert_eq!(at - 32, u32_at(file, 20) as usize, "sizeofcmds");
    out
}

/// A segment: its name, address, sizes and protections, and its
/// sections' names and file offsets and sizes.
#[derive(Debug)]
struct Segment {
    name: String,
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: u32,
    initprot: u32,
    flags: u32,
    sections: Vec<(String, u64, u64, u32)>,
}

fn segments(file: &[u8]) -> Vec<Segment> {
    commands(file)
        .iter()
        .filter(|c| c.cmd == LC_SEGMENT_64)
        .map(|c| {
            let at = c.at;
            let count = u32_at(file, at + 64) as usize;
            let sections = (0..count)
                .map(|i| {
                    let s = at + 72 + 80 * i;
                    (
                        text_at(file, s, 16),
                        u64_at(file, s + 32),
                        u64_at(file, s + 40),
                        u32_at(file, s + 48),
                    )
                })
                .collect();
            assert_eq!(c.size, 72 + 80 * count);
            Segment {
                name: text_at(file, at + 8, 16),
                vmaddr: u64_at(file, at + 24),
                vmsize: u64_at(file, at + 32),
                fileoff: u64_at(file, at + 40),
                filesize: u64_at(file, at + 48),
                maxprot: u32_at(file, at + 56),
                initprot: u32_at(file, at + 60),
                flags: u32_at(file, at + 68),
                sections,
            }
        })
        .collect()
}

/// The one load command of type `cmd`.
fn only(file: &[u8], cmd: u32) -> usize {
    let found: Vec<usize> = commands(file)
        .iter()
        .filter(|c| c.cmd == cmd)
        .map(|c| c.at)
        .collect();
    assert_eq!(found.len(), 1, "load command {cmd:#x}");
    found[0]
}

// ---- The file's format -----------------------------------------------------------

#[test]
fn the_header_says_a_position_independent_executable_dyld_links() {
    for mac in &MACS {
        header(mac);
    }
}

fn header(mac: &Mac) {
    let file = hello(mac, "header");
    assert_eq!(u32_at(&file, 0), MH_MAGIC_64);
    assert_eq!(u32_at(&file, 4), mac.cputype);
    assert_eq!(u32_at(&file, 8), mac.cpusubtype, "the CPU_SUBTYPE_*_ALL");
    assert_eq!(u32_at(&file, 12), MH_EXECUTE);
    assert_eq!(
        u32_at(&file, 24),
        MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE,
        "the flags ld64 writes for an executable"
    );
    let kinds: Vec<u32> = commands(&file).iter().map(|c| c.cmd).collect();
    assert_eq!(
        kinds,
        [
            LC_SEGMENT_64,
            LC_SEGMENT_64,
            LC_SEGMENT_64,
            LC_SEGMENT_64,
            LC_SEGMENT_64,
            LC_DYLD_CHAINED_FIXUPS,
            LC_SYMTAB,
            LC_DYSYMTAB,
            LC_LOAD_DYLINKER,
            LC_UUID,
            LC_BUILD_VERSION,
            LC_MAIN,
            LC_LOAD_DYLIB,
            LC_CODE_SIGNATURE,
        ]
    );
}

#[test]
fn code_is_never_writable_and_data_never_executable() {
    for mac in &MACS {
        segments_of(mac);
    }
}

fn segments_of(mac: &Mac) {
    let page = mac.page;
    let file = hello(mac, "segments");
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
    // Protections: 1 read, 2 write, 4 execute.
    let prots: Vec<(u32, u32)> = segments.iter().map(|s| (s.maxprot, s.initprot)).collect();
    assert_eq!(
        prots,
        [(0, 0), (5, 5), (3, 3), (3, 3), (1, 1)],
        "the zero page nothing, code R+X, the import table and read-only \
         data RW until dyld has bound it, the data RW, the link edit R"
    );
    for segment in &segments {
        assert!(
            segment.maxprot & 6 != 6,
            "{}: writable and executable",
            segment.name
        );
    }
    assert_eq!(
        segments[2].flags, 0x10,
        "__DATA_CONST is SG_READ_ONLY: read-only once bound"
    );
    // A 4 GiB zero page, so that any address made from a null pointer
    // faults; everything else above it, on pages of its own.
    assert_eq!((segments[0].vmaddr, segments[0].vmsize), (0, 1 << 32));
    let text = &segments[1];
    assert_eq!(
        (text.vmaddr, text.fileoff),
        (1 << 32, 0),
        "__TEXT maps the header"
    );
    for pair in segments[1..].windows(2) {
        assert_eq!(
            pair[0].vmaddr + pair[0].vmsize,
            pair[1].vmaddr,
            "{} follows {} in memory",
            pair[1].name,
            pair[0].name
        );
    }
    for segment in &segments[1..] {
        assert_eq!(segment.vmaddr % page, 0, "{}", segment.name);
        assert_eq!(segment.fileoff % page, 0, "{}", segment.name);
        assert_eq!(segment.vmsize % page, 0, "{}", segment.name);
        assert!(segment.filesize <= segment.vmsize, "{}", segment.name);
        for (name, addr, size, _) in &segment.sections {
            assert!(
                segment.vmaddr <= *addr && addr + size <= segment.vmaddr + segment.vmsize,
                "{name} is inside {}",
                segment.name
            );
        }
    }
    let sections: Vec<&str> = segments
        .iter()
        .flat_map(|s| s.sections.iter().map(|section| section.0.as_str()))
        .collect();
    assert_eq!(sections, ["__text", "__got", "__const", "__bss"]);
    assert_eq!(segments[3].filesize, 0, "__DATA is zero-filled");
    let linkedit = &segments[4];
    assert_eq!(
        linkedit.fileoff + linkedit.filesize,
        file.len() as u64,
        "__LINKEDIT ends the file"
    );
    // The entry point is in the code.
    let main = only(&file, LC_MAIN);
    let entry = u64_at(&file, main + 8);
    let (_, code_addr, code_size, code_offset) = &text.sections[0];
    assert_eq!(code_addr - text.vmaddr, u64::from(*code_offset));
    assert!((u64::from(*code_offset)..u64::from(*code_offset) + code_size).contains(&entry));
    // macOS, from macOS 12, whose dyld binds an arm64 executable's
    // chained fixups, on both processors.
    let build = only(&file, LC_BUILD_VERSION);
    assert_eq!(u32_at(&file, build + 8), 1, "PLATFORM_MACOS");
    assert!(u32_at(&file, build + 12) >= 0x000c_0000, "minos");
}

/// The functions every `macos-aarch64` executable imports from
/// `libSystem`, in the order of its import table.
const IMPORTS: [&str; 37] = [
    "___error",
    "_read",
    "_write",
    "_openat",
    "_close",
    "_mkdirat",
    "_fdopendir",
    "_readdir",
    "_closedir",
    "_fstat",
    "_ftruncate",
    "_readlinkat",
    "_unlinkat",
    "_renameat",
    "_getcwd",
    "_fcntl",
    "_mmap",
    "_munmap",
    "_clock_gettime",
    "_ioctl",
    "_getentropy",
    "_sigaction",
    "_sigaltstack",
    "_getpid",
    "_kill",
    "__exit",
    "_socket",
    "_connect",
    "_bind",
    "_listen",
    "_accept",
    "_getsockname",
    "_getsockopt",
    "_setsockopt",
    "_send",
    "_recv",
    "_poll",
];

/// [`IMPORTS`] for `macos-x86_64`: the same, but the functions that take
/// or give a `struct stat` or `struct dirent` are their 64-bit-inode
/// variants, which C's headers select on x86_64 and arm64 has alone.
const IMPORTS_X86_64: [&str; 37] = [
    "___error",
    "_read",
    "_write",
    "_openat",
    "_close",
    "_mkdirat",
    "_fdopendir$INODE64",
    "_readdir$INODE64",
    "_closedir",
    "_fstat$INODE64",
    "_ftruncate",
    "_readlinkat",
    "_unlinkat",
    "_renameat",
    "_getcwd",
    "_fcntl",
    "_mmap",
    "_munmap",
    "_clock_gettime",
    "_ioctl",
    "_getentropy",
    "_sigaction",
    "_sigaltstack",
    "_getpid",
    "_kill",
    "__exit",
    "_socket",
    "_connect",
    "_bind",
    "_listen",
    "_accept",
    "_getsockname",
    "_getsockopt",
    "_setsockopt",
    "_send",
    "_recv",
    "_poll",
];

#[test]
fn it_imports_only_libsystem_bound_by_chained_fixups() {
    for mac in &MACS {
        imports_of(mac);
    }
}

fn imports_of(mac: &Mac) {
    let page = mac.page;
    let file = hello(mac, "imports");
    let dylinker = only(&file, LC_LOAD_DYLINKER);
    let at = u32_at(&file, dylinker + 8) as usize;
    assert_eq!(text_at(&file, dylinker + at, 64), "/usr/lib/dyld");
    // One library: libSystem, ordinal 1.
    let dylib = only(&file, LC_LOAD_DYLIB);
    let at = u32_at(&file, dylib + 8) as usize;
    assert_eq!(text_at(&file, dylib + at, 64), "/usr/lib/libSystem.B.dylib");

    let fixups = only(&file, LC_DYLD_CHAINED_FIXUPS);
    let blob = &file[u32_at(&file, fixups + 8) as usize..][..u32_at(&file, fixups + 12) as usize];
    assert_eq!(u32_at(blob, 0), 0, "fixups_version");
    let starts = u32_at(blob, 4) as usize;
    let imports = u32_at(blob, 8) as usize;
    let symbols = u32_at(blob, 12) as usize;
    let count = u32_at(blob, 16) as usize;
    assert_eq!(u32_at(blob, 20), 1, "DYLD_CHAINED_IMPORT");
    assert_eq!(u32_at(blob, 24), 0, "uncompressed symbol names");
    let names: Vec<String> = (0..count)
        .map(|i| {
            let import = u32_at(blob, imports + 4 * i);
            assert_eq!(import & 0xff, 1, "import {i} is from libSystem");
            assert_eq!(import >> 8 & 1, 0, "import {i} is not weak");
            text_at(blob, symbols + (import >> 9) as usize, 64)
        })
        .collect();
    assert_eq!(names, mac.imports);

    // Only __DATA_CONST, the third of five segments, has a chain: the
    // import table, from its first byte.
    let segments = segments(&file);
    assert_eq!(u32_at(blob, starts) as usize, segments.len());
    let offsets: Vec<u32> = (0..segments.len())
        .map(|i| u32_at(blob, starts + 4 + 4 * i))
        .collect();
    assert!(offsets
        .iter()
        .enumerate()
        .all(|(i, &o)| (i == 2) == (o != 0)));
    let in_segment = starts + offsets[2] as usize;
    let data_const = &segments[2];
    let pages = data_const.vmsize / page;
    assert_eq!(u32_at(blob, in_segment) as u64, 22 + 2 * pages, "size");
    assert_eq!(u64::from(u16_at(blob, in_segment + 4)), page);
    assert_eq!(u16_at(blob, in_segment + 6), 2, "DYLD_CHAINED_PTR_64");
    assert_eq!(
        u64_at(blob, in_segment + 8),
        data_const.vmaddr - segments[1].vmaddr,
        "segment_offset"
    );
    assert_eq!(u16_at(blob, in_segment + 20) as u64, pages);
    assert_eq!(
        u16_at(blob, in_segment + 22),
        0,
        "the chain starts the page"
    );

    // The import table: one slot per import, each a bind of its import,
    // chained to the next (two 4-byte strides on), the last ending it.
    let (name, _, size, offset) = &data_const.sections[0];
    assert_eq!(name, "__got");
    assert_eq!(*size, 8 * mac.imports.len() as u64);
    for i in 0..mac.imports.len() {
        let slot = u64_at(&file, *offset as usize + 8 * i);
        assert_eq!(slot >> 63, 1, "slot {i} binds");
        assert_eq!(slot & 0xff_ffff, i as u64, "slot {i}'s ordinal");
        assert_eq!(slot >> 24 & 0xff, 0, "slot {i}'s addend");
        let next = slot >> 51 & 0xfff;
        assert_eq!(
            next,
            if i + 1 == mac.imports.len() { 0 } else { 2 },
            "slot {i}"
        );
    }
}

#[test]
fn the_ad_hoc_signature_hashes_every_page_before_it() {
    for mac in &MACS {
        signature_of(mac);
    }
}

fn signature_of(mac: &Mac) {
    let file = hello(mac, "signature");
    let command = only(&file, LC_CODE_SIGNATURE);
    let at = u32_at(&file, command + 8) as usize;
    let size = u32_at(&file, command + 12) as usize;
    assert_eq!(at % 16, 0);
    assert_eq!(at + size, file.len(), "the signature ends the file");
    let blob = &file[at..];
    assert_eq!(u32_be(blob, 0), 0xfade_0cc0, "CSMAGIC_EMBEDDED_SIGNATURE");
    let count = u32_be(blob, 8) as usize;
    let index: Vec<(u32, usize)> = (0..count)
        .map(|i| (u32_be(blob, 12 + 8 * i), u32_be(blob, 16 + 8 * i) as usize))
        .collect();
    let slots: Vec<u32> = index.iter().map(|(slot, _)| *slot).collect();
    assert_eq!(
        slots,
        [0, 2, 0x1_0000],
        "a code directory, requirements and a signature slot, as codesign -s - writes"
    );
    let requirements = &blob[index[1].1..];
    assert_eq!(u32_be(requirements, 0), 0xfade_0c01);
    let requirements = &requirements[..u32_be(requirements, 4) as usize];
    assert_eq!(requirements.len(), 12, "an empty requirement set");
    let wrapper = &blob[index[2].1..];
    assert_eq!(
        (u32_be(wrapper, 0), u32_be(wrapper, 4)),
        (0xfade_0b01, 8),
        "an empty CMS blob: no certificate"
    );

    let cd = &blob[index[0].1..];
    assert_eq!(u32_be(cd, 0), 0xfade_0c02, "CSMAGIC_CODEDIRECTORY");
    assert!(
        u32_be(cd, 8) >= 0x2_0400,
        "a version with the executable segment"
    );
    assert_eq!(u32_be(cd, 12) & 0x2, 0x2, "CS_ADHOC");
    let hashes = u32_be(cd, 16) as usize;
    let special = u32_be(cd, 24) as usize;
    let code_slots = u32_be(cd, 28) as usize;
    let code_limit = u32_be(cd, 32) as usize;
    assert_eq!(code_limit, at, "it hashes everything before it");
    assert_eq!(cd[36], 32, "hashSize");
    assert_eq!(cd[37], 2, "CS_HASHTYPE_SHA256");
    let page = 1usize << cd[39];
    assert_eq!(code_slots, code_limit.div_ceil(page));
    for (i, chunk) in file[..code_limit].chunks(page).enumerate() {
        assert_eq!(
            &cd[hashes + 32 * i..hashes + 32 * (i + 1)],
            &sha256(chunk),
            "page {i}"
        );
    }
    assert_eq!(special, 2);
    assert_eq!(
        &cd[hashes - 64..hashes - 32],
        &sha256(requirements),
        "slot -2: the requirements"
    );
    assert_eq!(&cd[hashes - 32..hashes], &[0; 32], "slot -1: no Info.plist");
    // The executable segment: __TEXT, of the main executable.
    let text = &segments(&file)[1];
    assert_eq!(u64_be(cd, 64), text.fileoff, "execSegBase");
    assert_eq!(u64_be(cd, 72), text.filesize, "execSegLimit");
    assert_eq!(u64_be(cd, 80), 1, "CS_EXECSEG_MAIN_BINARY");
}

#[test]
fn the_same_program_builds_to_the_same_bytes_uuid_and_signature_included() {
    for mac in &MACS {
        same_bytes(mac);
    }
}

fn same_bytes(mac: &Mac) {
    let a = hello(mac, "same_a");
    let b = hello(mac, "same_b");
    assert!(a == b, "two builds of one program differ");
    let uuid = only(&a, LC_UUID);
    assert_ne!(&a[uuid + 8..uuid + 24], &[0; 16], "the UUID is set");
    let other = workspace(
        &format!("{}/other", mac.platform),
        &[(
            "hello.osk",
            "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"bye\")\n}\n",
        )],
    );
    let built = oskac(
        &other,
        &[
            "build",
            "hello.osk",
            "--target",
            mac.platform,
            "-o",
            "hello",
        ],
    );
    assert!(built.status.success());
    let c = std::fs::read(other.join("hello")).expect("the executable");
    let uuid_c = only(&c, LC_UUID);
    assert_ne!(
        a[uuid + 8..uuid + 24],
        c[uuid_c + 8..uuid_c + 24],
        "another program has another UUID"
    );
}

/// The SHA-256 of `data` (FIPS 180-4): this test's own, so it checks the
/// backend's hashes rather than repeating them.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bits = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (state, value) in h.iter_mut().zip(v) {
            *state = state.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (bytes, word) in out.chunks_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[test]
fn the_test_s_own_sha256_is_right() {
    let hex: String = sha256(b"abc").iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

// ---- On a Mac --------------------------------------------------------------------

/// Small programs: integers, calls, text, exit codes and stdout, a
/// program's `Err`, and a panic with its location.
const PROGRAMS: [(&str, &str); 5] = [
    ("hello", HELLO),
    (
        "integers",
        "fn fib(n: Int) -> Int {\n    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }\n}\n\n\
         fn main(io: Stdout) -> Result<(), IoError> {\n    var i = 0\n    while i < 20 {\n        \
         let f = fib(i)\n        io.println(\"fib({i}) = {f}\")?\n        i = i + 1\n    }\n    \
         io.println((-9223372036854775807 - 1).to_string())\n}\n",
    ),
    (
        "args",
        "fn main(io: Stdout, args: Args) -> Result<(), IoError> {\n    match args.get(1) {\n        \
         Some(a) => io.println(\"argument: {a}\")\n        None => io.println(\"no argument\")\n    \
         }\n}\n",
    ),
    (
        "error",
        "fn main(io: Stdout) -> Result<(), String> {\n    \
         let _ = io.println(\"before\")\n    Err(\"it failed\")\n}\n",
    ),
    (
        "panic",
        "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"before\")?\n    \
         let big = 9223372036854775807\n    io.println((big + 1).to_string())\n}\n",
    ),
];

/// Each program, built for each macOS target and run on a Mac of its
/// kind, does what the interpreter does, byte for byte, and macOS accepts
/// its signature.
#[test]
fn small_programs_run_on_a_mac_as_on_the_interpreter() {
    for mac in &MACS {
        small_programs(mac);
    }
}

fn small_programs(mac: &Mac) {
    for (name, src) in PROGRAMS {
        let dir = workspace(
            &format!("{}/run_{name}", mac.platform),
            &[("main.osk", src)],
        );
        let interpreted = oskac(&dir, &["run", "--interpret", "main.osk"]);
        assert!(
            interpreted.status.code().is_some(),
            "{name}: the interpreter: {interpreted:?}"
        );
        let built = oskac(
            &dir,
            &["build", "main.osk", "--target", mac.platform, "-o", "main"],
        );
        assert!(built.status.success(), "{name}: {built:?}");
        if !mac.is_host {
            continue;
        }
        let verify = Command::new("codesign")
            .args(["--verify", "--strict", "--verbose=2", "main"])
            .current_dir(&dir)
            .output()
            .expect("codesign should run");
        assert!(verify.status.success(), "{name}: codesign: {verify:?}");
        let native = Command::new(dir.join("main"))
            .current_dir(&dir)
            .output()
            .expect("the executable should start");
        assert_eq!(
            native.status.code(),
            interpreted.status.code(),
            "{name}: exit code; native stderr: {}",
            String::from_utf8_lossy(&native.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&native.stdout),
            String::from_utf8_lossy(&interpreted.stdout),
            "{name}: stdout"
        );
        assert_eq!(
            String::from_utf8_lossy(&native.stderr),
            String::from_utf8_lossy(&interpreted.stderr),
            "{name}: stderr"
        );
    }
}
